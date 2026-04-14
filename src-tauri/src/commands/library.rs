//! File browsing, data directory, and import commands.

use serde::Serialize;
use tauri::State;

use crate::error::{lock_state, AppError, AppResult};

use super::SharedState;

/// Info about a .oisi file for the library browser.
#[derive(Serialize)]
pub struct OisiFileInfo {
    pub path: String,
    pub filename: String,
    pub size_bytes: u64,
    /// ISO-8601 local datetime: "2026-03-26 14:30:05"
    pub modified: String,
    /// Unix timestamp (seconds) for sorting.
    pub modified_epoch: u64,
}

/// List .oisi files in the data directory.
#[tauri::command]
pub fn list_oisi_files(state: State<'_, SharedState>) -> AppResult<Vec<OisiFileInfo>> {
    let app = lock_state(&state, "list_oisi_files")?;
    let cfg = lock_state(&app.config, "list_oisi_files config")?;
    let data_dir = &cfg.rig.paths.data_directory;

    if data_dir.is_empty() {
        return Ok(Vec::new());
    }

    let dir = std::path::Path::new(data_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "oisi") {
                let metadata = entry.metadata();
                let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
                let mod_epoch = metadata.ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                let modified = if mod_epoch > 0 {
                    epoch_to_local_datetime(mod_epoch)
                } else {
                    "—".into()
                };

                files.push(OisiFileInfo {
                    path: path.to_string_lossy().to_string(),
                    filename: path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "—".into()),
                    size_bytes: size,
                    modified,
                    modified_epoch: mod_epoch,
                });
            }
        }
    }

    files.sort_by(|a, b| b.modified.cmp(&a.modified)); // newest first
    Ok(files)
}

/// Get the data directory path.
#[tauri::command]
pub fn get_data_directory(state: State<'_, SharedState>) -> AppResult<String> {
    let app = lock_state(&state, "get_data_directory")?;
    let cfg = lock_state(&app.config, "get_data_directory config")?;
    Ok(cfg.rig.paths.data_directory.clone())
}

/// Set the data directory path. Persists to rig.toml.
#[tauri::command]
pub fn set_data_directory(state: State<'_, SharedState>, path: String) -> AppResult<()> {
    let app = lock_state(&state, "set_data_directory")?;
    let mut cfg = lock_state(&app.config, "set_data_directory config")?;
    cfg.rig.paths.data_directory = path;
    if let Err(e) = cfg.save() {
        eprintln!("[config] Failed to save data directory: {e}");
    }
    Ok(())
}

/// Delete one or more .oisi files. Returns the count of files actually deleted.
#[tauri::command]
pub fn delete_oisi_files(paths: Vec<String>) -> AppResult<u32> {
    let mut deleted = 0u32;
    for p in &paths {
        let path = std::path::Path::new(p);
        if path.extension().is_some_and(|ext| ext == "oisi") && path.exists() {
            std::fs::remove_file(path)?;
            deleted += 1;
        }
    }
    eprintln!("[commands] deleted {deleted} file(s)");
    Ok(deleted)
}

/// Import SNLC .mat files from a directory into a new .oisi file.
/// Expects: 2 data .mat files (horizontal + vertical) and optionally a grab_*.mat anatomical.
/// Returns the output .oisi file path.
#[tauri::command]
pub fn import_snlc(state: State<'_, SharedState>, dir_path: String) -> AppResult<String> {
    let src_dir = std::path::Path::new(&dir_path);
    let folder_name = src_dir.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "import".into());

    let out_dir = {
        let app = lock_state(&state, "import_snlc")?;
        let cfg = lock_state(&app.config, "import_snlc config")?;
        let data_dir = &cfg.rig.paths.data_directory;
        if data_dir.is_empty() {
            src_dir.parent().unwrap_or(src_dir).to_path_buf()
        } else {
            std::path::PathBuf::from(data_dir)
        }
    };
    let _ = std::fs::create_dir_all(&out_dir);
    let output_path = out_dir.join(format!("{folder_name}.oisi"));

    isi_analysis::io::import_snlc_directory(src_dir, &output_path)?;

    let path_str = output_path.to_string_lossy().to_string();
    eprintln!("[commands] imported SNLC data to {path_str}");
    Ok(path_str)
}

const SNLC_SAMPLE_DATA_URL: &str =
    "https://github.com/SNLC/ISI/raw/master/Sample%20Data.zip";

/// Download SNLC sample data from GitHub, extract, and import each subject.
/// Returns the list of created .oisi file paths.
#[tauri::command]
pub fn import_snlc_sample_data(state: State<'_, SharedState>) -> AppResult<Vec<String>> {
    // Determine output directory (same logic as import_snlc).
    let out_dir = {
        let app = lock_state(&state, "import_snlc_sample_data")?;
        let cfg = lock_state(&app.config, "import_snlc_sample_data config")?;
        let data_dir = &cfg.rig.paths.data_directory;
        if data_dir.is_empty() {
            return Err(AppError::Validation(
                "Set a data directory before downloading sample data.".into(),
            ));
        }
        std::path::PathBuf::from(data_dir)
    };
    let _ = std::fs::create_dir_all(&out_dir);

    // Create temp directory for extraction (cleaned up on all paths).
    let temp_dir = std::env::temp_dir().join("openisi_sample_data");
    let _ = std::fs::remove_dir_all(&temp_dir); // clean any previous attempt
    std::fs::create_dir_all(&temp_dir)?;

    // Guard: ensure temp_dir is cleaned up even on early return.
    struct CleanupGuard(std::path::PathBuf);
    impl Drop for CleanupGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = CleanupGuard(temp_dir.clone());

    let zip_path = temp_dir.join("sample_data.zip");

    // Download the zip.
    eprintln!("[commands] downloading SNLC sample data from {SNLC_SAMPLE_DATA_URL}");
    let response = ureq::get(SNLC_SAMPLE_DATA_URL)
        .call()
        .map_err(|e| AppError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Download failed: {e}"),
        )))?;

    let mut zip_file = std::fs::File::create(&zip_path)?;
    std::io::copy(&mut response.into_body().as_reader(), &mut zip_file)?;
    drop(zip_file);
    eprintln!("[commands] download complete, extracting...");

    // Extract the zip.
    let extract_dir = temp_dir.join("extracted");
    {
        let file = std::fs::File::open(&zip_path)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| AppError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to read zip: {e}"),
            )))?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)
                .map_err(|e| AppError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Zip entry error: {e}"),
                )))?;
            let entry_path = match entry.enclosed_name() {
                Some(p) => extract_dir.join(p),
                None => continue,
            };
            if entry.is_dir() {
                let _ = std::fs::create_dir_all(&entry_path);
            } else {
                if let Some(parent) = entry_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let mut out = std::fs::File::create(&entry_path)?;
                std::io::copy(&mut entry, &mut out)?;
            }
        }
    }
    // Remove the zip now that it's extracted.
    let _ = std::fs::remove_file(&zip_path);

    // Find subject directories — any directory that contains .mat files.
    let mut subject_dirs = Vec::new();
    find_mat_dirs(&extract_dir, &mut subject_dirs);
    subject_dirs.sort();

    if subject_dirs.is_empty() {
        return Err(AppError::Validation(
            "No subject directories with .mat files found in the sample data.".into(),
        ));
    }

    eprintln!("[commands] found {} subject directories", subject_dirs.len());

    // Import each subject directory.
    let mut imported: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for dir in &subject_dirs {
        let folder_name = dir.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "import".into());
        let output_path = out_dir.join(format!("{folder_name}.oisi"));

        match isi_analysis::io::import_snlc_directory(dir, &output_path) {
            Ok(()) => {
                let path_str = output_path.to_string_lossy().to_string();
                eprintln!("[commands] imported sample subject {folder_name} to {path_str}");
                imported.push(path_str);
            }
            Err(e) => {
                let msg = format!("{folder_name}: {e}");
                eprintln!("[commands] failed to import sample subject {msg}");
                errors.push(msg);
            }
        }
    }

    if imported.is_empty() {
        return Err(AppError::Analysis(isi_analysis::AnalysisError::MissingData(
            format!("All subjects failed to import:\n{}", errors.join("\n")),
        )));
    }
    if !errors.is_empty() {
        eprintln!("[commands] {} subjects failed: {}", errors.len(), errors.join("; "));
    }

    // Cleanup is handled by the CleanupGuard drop.
    eprintln!("[commands] sample data import complete: {} imported, {} failed",
        imported.len(), errors.len());
    Ok(imported)
}

fn find_mat_dirs(dir: &std::path::Path, results: &mut Vec<std::path::PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut has_mat = false;
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
        } else if path.extension().and_then(|e| e.to_str()) == Some("mat") {
            has_mat = true;
        }
    }
    if has_mat {
        results.push(dir.to_path_buf());
    }
    for sub in subdirs {
        find_mat_dirs(&sub, results);
    }
}

// ════════════════════════════════════════════════════════════════════════
// Helpers
// ════════════════════════════════════════════════════════════════════════

/// Convert Unix epoch seconds to local datetime string "YYYY-MM-DD HH:MM:SS".
/// Uses Windows GetLocalTime via the system's UTC offset.
fn epoch_to_local_datetime(epoch_secs: u64) -> String {
    // Get local offset by comparing SystemTime::now() with a known epoch.
    // Simple approach: compute UTC civil time, then apply a fixed offset.
    // On Windows, use GetTimeZoneInformation for the offset.
    #[cfg(windows)]
    {
        use windows::Win32::System::Time::GetTimeZoneInformation;
        let mut tzi = windows::Win32::System::Time::TIME_ZONE_INFORMATION::default();
        let result = unsafe { GetTimeZoneInformation(&mut tzi) };
        // Bias is in minutes, negative for east of UTC.
        let bias_minutes = match result {
            // TIME_ZONE_ID_DAYLIGHT
            2 => tzi.Bias + tzi.DaylightBias,
            _ => tzi.Bias + tzi.StandardBias,
        };
        let local_secs = epoch_secs as i64 - (bias_minutes as i64 * 60);
        let local_secs = local_secs as u64;
        let days = local_secs / 86400;
        let day_secs = local_secs % 86400;
        let (y, mo, da) = civil_from_days(days as i64);
        let h = day_secs / 3600;
        let m = (day_secs % 3600) / 60;
        let s = day_secs % 60;
        format!("{y:04}-{mo:02}-{da:02} {h:02}:{m:02}:{s:02}")
    }
    #[cfg(not(windows))]
    {
        let days = epoch_secs / 86400;
        let day_secs = epoch_secs % 86400;
        let (y, mo, da) = civil_from_days(days as i64);
        let h = day_secs / 3600;
        let m = (day_secs % 3600) / 60;
        let s = day_secs % 60;
        format!("{y:04}-{mo:02}-{da:02} {h:02}:{m:02}:{s:02}")
    }
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
