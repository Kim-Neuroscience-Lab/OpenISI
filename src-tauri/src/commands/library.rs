//! File browsing, data directory, and import commands.

use serde::Serialize;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::params::ParamValue;

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
    let data_dir = state.registry.lock().data_directory().to_string();

    if data_dir.is_empty() {
        return Ok(Vec::new());
    }

    let dir = std::path::Path::new(&data_dir);
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
                let mod_epoch = metadata
                    .ok()
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
                    filename: path
                        .file_name()
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
    Ok(state.registry.lock().data_directory().to_string())
}

/// Set the data directory path. Persists to rig.toml.
#[tauri::command]
pub fn set_data_directory(state: State<'_, SharedState>, path: String) -> AppResult<()> {
    // Registry-scoped, brief: intentionally save while holding the registry lock.
    let mut reg = state.registry.lock();
    reg.set(
        crate::params::ParamId::DataDirectory,
        ParamValue::String(path),
    )?;
    if let Err(e) = reg.save_rig() {
        tracing::error!(error = %e, "failed to save data directory");
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
    tracing::info!(deleted, "deleted files");
    Ok(deleted)
}

/// Import SNLC .mat files from a directory into a new .oisi file.
/// Expects: 2 data .mat files (horizontal + vertical) and optionally a grab_*.mat anatomical.
/// Returns the output .oisi file path.
#[tauri::command]
pub fn import_snlc(state: State<'_, SharedState>, dir_path: String) -> AppResult<String> {
    let src_dir = std::path::Path::new(&dir_path);
    let folder_name = src_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "import".into());

    let out_dir = {
        let data_dir = state.registry.lock().data_directory().to_string();
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
    tracing::info!(path = %path_str, "imported SNLC data");
    Ok(path_str)
}

/// Download SNLC sample data from GitHub, extract, and import each subject.
/// Returns the list of created .oisi file paths.
#[tauri::command]
pub fn import_snlc_sample_data(state: State<'_, SharedState>) -> AppResult<Vec<String>> {
    let out_dir = {
        let data_dir = state.registry.lock().data_directory().to_string();
        if data_dir.is_empty() {
            return Err(AppError::Validation(
                "Set a data directory before downloading sample data.".into(),
            ));
        }
        std::path::PathBuf::from(data_dir)
    };

    let imported = crate::sample_data::import_snlc_sample_bundle(&out_dir)?;
    Ok(imported
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect())
}

// ════════════════════════════════════════════════════════════════════════
// Helpers
// ════════════════════════════════════════════════════════════════════════

/// Convert Unix epoch seconds to local datetime string "YYYY-MM-DD HH:MM:SS".
fn epoch_to_local_datetime(epoch_secs: u64) -> String {
    #[cfg(windows)]
    {
        use windows::Win32::System::Time::GetTimeZoneInformation;
        let mut tzi = windows::Win32::System::Time::TIME_ZONE_INFORMATION::default();
        let result = unsafe { GetTimeZoneInformation(&mut tzi) };
        let bias_minutes = match result {
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
