//! Tauri IPC commands — frontend calls these via `invoke()`.
//!
//! Organized by workspace tool. Each tool's commands are independent.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::State;

use crate::config::Experiment;
use crate::messages::{CameraCmd, PreviewCommand, AcquisitionCommand, StimulusCmd};
use crate::session::MonitorInfo;
use crate::state::AppState;

type SharedState = Arc<Mutex<AppState>>;

// ════════════════════════════════════════════════════════════════════════
// Hardware tool
// ════════════════════════════════════════════════════════════════════════

// ════════════════════════════════════════════════════════════════════════
// Library
// ════════════════════════════════════════════════════════════════════════

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
pub fn list_oisi_files(state: State<'_, SharedState>) -> Result<Vec<OisiFileInfo>, String> {
    let app = state.lock().unwrap();
    let cfg = app.config.lock().unwrap();
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
pub fn get_data_directory(state: State<'_, SharedState>) -> String {
    let app = state.lock().unwrap();
    app.config.lock().unwrap().rig.paths.data_directory.clone()
}

/// Set the data directory path. Persists to rig.toml.
#[tauri::command]
pub fn set_data_directory(state: State<'_, SharedState>, path: String) -> Result<(), String> {
    let app = state.lock().unwrap();
    let mut cfg = app.config.lock().unwrap();
    cfg.rig.paths.data_directory = path;
    if let Err(e) = cfg.save() {
        eprintln!("[config] Failed to save data directory: {e}");
    }
    Ok(())
}

// ════════════════════════════════════════════════════════════════════════
// Import
// ════════════════════════════════════════════════════════════════════════

/// Import SNLC .mat files from a directory into a new .oisi file.
/// Expects: 2 data .mat files (horizontal + vertical) and optionally a grab_*.mat anatomical.
/// Returns the output .oisi file path.
#[tauri::command]
pub fn import_snlc(state: State<'_, SharedState>, dir_path: String) -> Result<String, String> {
    let src_dir = std::path::Path::new(&dir_path);
    let folder_name = src_dir.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "import".into());

    let out_dir = {
        let app = state.lock().unwrap();
        let cfg = app.config.lock().unwrap();
        let data_dir = &cfg.rig.paths.data_directory;
        if data_dir.is_empty() {
            src_dir.parent().unwrap_or(src_dir).to_path_buf()
        } else {
            std::path::PathBuf::from(data_dir)
        }
    };
    let _ = std::fs::create_dir_all(&out_dir);
    let output_path = out_dir.join(format!("{folder_name}.oisi"));

    isi_analysis::io::import_snlc_directory(src_dir, &output_path)
        .map_err(|e| format!("Import failed: {e}"))?;

    let path_str = output_path.to_string_lossy().to_string();
    eprintln!("[commands] imported SNLC data to {path_str}");
    Ok(path_str)
}

const SNLC_SAMPLE_DATA_URL: &str =
    "https://github.com/SNLC/ISI/raw/master/Sample%20Data.zip";

/// Download SNLC sample data from GitHub, extract, and import each subject.
/// Returns the list of created .oisi file paths.
#[tauri::command]
pub fn import_snlc_sample_data(state: State<'_, SharedState>) -> Result<Vec<String>, String> {
    // Determine output directory (same logic as import_snlc).
    let out_dir = {
        let app = state.lock().unwrap();
        let cfg = app.config.lock().unwrap();
        let data_dir = &cfg.rig.paths.data_directory;
        if data_dir.is_empty() {
            return Err("Set a data directory before downloading sample data.".into());
        }
        std::path::PathBuf::from(data_dir)
    };
    let _ = std::fs::create_dir_all(&out_dir);

    // Create temp directory for extraction (cleaned up on all paths).
    let temp_dir = std::env::temp_dir().join("openisi_sample_data");
    let _ = std::fs::remove_dir_all(&temp_dir); // clean any previous attempt
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| format!("Failed to create temp directory: {e}"))?;

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
        .map_err(|e| format!("Download failed: {e}"))?;

    let mut zip_file = std::fs::File::create(&zip_path)
        .map_err(|e| format!("Failed to create temp zip: {e}"))?;
    std::io::copy(&mut response.into_body().as_reader(), &mut zip_file)
        .map_err(|e| format!("Failed to write zip: {e}"))?;
    drop(zip_file);
    eprintln!("[commands] download complete, extracting...");

    // Extract the zip.
    let extract_dir = temp_dir.join("extracted");
    {
        let file = std::fs::File::open(&zip_path)
            .map_err(|e| format!("Failed to open zip: {e}"))?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| format!("Failed to read zip: {e}"))?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)
                .map_err(|e| format!("Zip entry error: {e}"))?;
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
                let mut out = std::fs::File::create(&entry_path)
                    .map_err(|e| format!("Failed to extract {}: {e}", entry.name()))?;
                std::io::copy(&mut entry, &mut out)
                    .map_err(|e| format!("Failed to write {}: {e}", entry.name()))?;
            }
        }
    }
    // Remove the zip now that it's extracted.
    let _ = std::fs::remove_file(&zip_path);

    // Find subject directories — any directory that contains .mat files.
    let mut subject_dirs = Vec::new();
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
    find_mat_dirs(&extract_dir, &mut subject_dirs);
    subject_dirs.sort();

    if subject_dirs.is_empty() {
        return Err("No subject directories with .mat files found in the sample data.".into());
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
        return Err(format!("All subjects failed to import:\n{}", errors.join("\n")));
    }
    if !errors.is_empty() {
        eprintln!("[commands] {} subjects failed: {}", errors.len(), errors.join("; "));
    }

    // Cleanup is handled by the CleanupGuard drop.
    eprintln!("[commands] sample data import complete: {} imported, {} failed",
        imported.len(), errors.len());
    Ok(imported)
}

// ════════════════════════════════════════════════════════════════════════
// Analysis
// ════════════════════════════════════════════════════════════════════════

/// Inspect a .oisi file — what data is present.
#[tauri::command]
pub fn inspect_oisi(path: String) -> Result<serde_json::Value, String> {
    let caps = isi_analysis::io::inspect(std::path::Path::new(&path))
        .map_err(|e| format!("Failed to inspect: {e}"))?;
    Ok(serde_json::json!({
        "has_anatomical": caps.has_anatomical,
        "has_acquisition": caps.has_acquisition,
        "has_complex_maps": caps.has_complex_maps,
        "has_results": caps.has_results,
        "dimensions": caps.dimensions,
        "acquisition_cycles": caps.acquisition_cycles,
        "results": caps.results,
    }))
}

/// Run analysis on a .oisi file.
#[tauri::command]
pub fn run_analysis(state: State<'_, SharedState>, path: String) -> Result<String, String> {
    let app = state.lock().unwrap();
    let analysis_cfg = app.config.lock().unwrap().rig.analysis.clone();
    drop(app); // Release lock during analysis

    let seg_params = analysis_cfg.segmentation.as_ref().map(|s| {
        isi_analysis::params::SegmentationParams {
            sign_map_filter_sigma: s.sign_map_filter_sigma,
            sign_map_threshold: s.sign_map_threshold,
            open_radius: s.open_radius,
            close_radius: s.close_radius,
            dilate_radius: s.dilate_radius,
            pad_border: s.pad_border,
            spur_iterations: s.spur_iterations,
            split_overlap_threshold: s.split_overlap_threshold,
            merge_overlap_threshold: s.merge_overlap_threshold,
            merge_dilate_radius: s.merge_dilate_radius,
            merge_close_radius: s.merge_close_radius,
            eccentricity_radius: s.eccentricity_radius,
        }
    });

    let params = isi_analysis::AnalysisParams {
        smoothing_sigma: analysis_cfg.smoothing_sigma,
        rotation_k: analysis_cfg.rotation_k,
        azi_angular_range: analysis_cfg.azi_angular_range,
        alt_angular_range: analysis_cfg.alt_angular_range,
        offset_azi: analysis_cfg.offset_azi,
        offset_alt: analysis_cfg.offset_alt,
        epsilon: analysis_cfg.epsilon,
        segmentation: seg_params,
    };

    let progress = isi_analysis::SilentProgress;
    let cancel = std::sync::atomic::AtomicBool::new(false);

    isi_analysis::analyze(
        std::path::Path::new(&path),
        &params,
        &progress,
        &cancel,
    ).map_err(|e| format!("Analysis failed: {e}"))?;

    Ok("Analysis complete".into())
}

/// Get analysis parameters (from rig.toml [analysis]).
#[tauri::command]
pub fn get_analysis_params(state: State<'_, SharedState>) -> serde_json::Value {
    let app = state.lock().unwrap();
    let a = app.config.lock().unwrap().rig.analysis.clone();
    serde_json::json!({
        "smoothing_sigma": a.smoothing_sigma,
        "rotation_k": a.rotation_k,
        "azi_angular_range": a.azi_angular_range,
        "alt_angular_range": a.alt_angular_range,
        "offset_azi": a.offset_azi,
        "offset_alt": a.offset_alt,
        "epsilon": a.epsilon,
        "segmentation": a.segmentation,
    })
}

/// Update analysis parameters. Persists to rig.toml.
#[tauri::command]
pub fn set_analysis_params(
    state: State<'_, SharedState>,
    smoothing_sigma: f64,
    rotation_k: i32,
    azi_angular_range: f64,
    alt_angular_range: f64,
    offset_azi: f64,
    offset_alt: f64,
    sign_map_filter_sigma: Option<f64>,
    sign_map_threshold: Option<f64>,
    eccentricity_radius: Option<f64>,
) -> Result<(), String> {
    let app = state.lock().unwrap();
    let mut cfg = app.config.lock().unwrap();
    cfg.rig.analysis.smoothing_sigma = smoothing_sigma;
    cfg.rig.analysis.rotation_k = rotation_k;
    cfg.rig.analysis.azi_angular_range = azi_angular_range;
    cfg.rig.analysis.alt_angular_range = alt_angular_range;
    cfg.rig.analysis.offset_azi = offset_azi;
    cfg.rig.analysis.offset_alt = offset_alt;
    // Update segmentation params if provided.
    if let Some(ref mut seg) = cfg.rig.analysis.segmentation {
        if let Some(v) = sign_map_filter_sigma { seg.sign_map_filter_sigma = v; }
        if let Some(v) = sign_map_threshold { seg.sign_map_threshold = v; }
        if let Some(v) = eccentricity_radius { seg.eccentricity_radius = v; }
    }
    if let Err(e) = cfg.save() {
        eprintln!("[config] Failed to save analysis params: {e}");
    }
    Ok(())
}

/// Read any result dataset from a .oisi file. Returns typed data.
#[tauri::command]
pub fn read_result(path: String, name: String) -> Result<serde_json::Value, String> {
    let file = hdf5::File::open(&path)
        .map_err(|e| format!("Failed to open: {e}"))?;
    let ds = file.dataset(&format!("results/{name}"))
        .map_err(|e| format!("Failed to open results/{name}: {e}"))?;
    let shape = ds.shape();

    // 1D array (e.g., area_signs).
    if shape.len() == 1 {
        // Read as i32 for broadest HDF5 compatibility (i8 may not be directly supported).
        let data: Vec<i32> = ds.read_1d()
            .map_err(|e| format!("reading 1D {name}: {e}"))?
            .to_vec();
        return Ok(serde_json::json!({
            "type": "sign_array",
            "data": data,
        }));
    }

    // 2D dataset — determine type from name.
    let (h, w) = (shape[0], shape[1]);

    if name == "area_labels" {
        let data: ndarray::Array2<i32> = ds.read()
            .map_err(|e| format!("reading {name}: {e}"))?;
        let flat: Vec<i32> = data.into_raw_vec_and_offset().0;
        return Ok(serde_json::json!({
            "type": "label_map",
            "width": w, "height": h,
            "data": flat,
        }));
    }

    if name == "area_borders" || name == "contours_azi" || name == "contours_alt" {
        let data: ndarray::Array2<u8> = ds.read()
            .map_err(|e| format!("reading {name}: {e}"))?;
        let flat: Vec<u8> = data.into_raw_vec_and_offset().0;
        return Ok(serde_json::json!({
            "type": "bool_mask",
            "width": w, "height": h,
            "data": flat,
        }));
    }

    // Default: f64 scalar map.
    let data: ndarray::Array2<f64> = ds.read()
        .map_err(|e| format!("reading {name}: {e}"))?;
    let flat: Vec<f64> = data.into_raw_vec_and_offset().0;
    Ok(serde_json::json!({
        "type": "scalar_map",
        "width": w, "height": h,
        "data": flat,
    }))
}

/// Read the anatomical image from a .oisi file.
#[tauri::command]
pub fn read_anatomical(path: String) -> Result<serde_json::Value, String> {
    let data = isi_analysis::io::read_anatomical(std::path::Path::new(&path))
        .map_err(|e| format!("Failed to read anatomical: {e}"))?;
    let (h, w) = data.dim();
    let flat: Vec<u8> = data.into_raw_vec_and_offset().0;
    Ok(serde_json::json!({
        "width": w,
        "height": h,
        "data": flat,
    }))
}

/// Export a result map as a PNG file.
#[tauri::command]
pub fn export_map_png(path: String, map_name: String, output_path: String) -> Result<(), String> {
    let data = isi_analysis::io::read_result_map(std::path::Path::new(&path), &map_name)
        .map_err(|e| format!("Failed to read map: {e}"))?;
    let (h, w) = data.dim();

    // Normalize to 0-255 for PNG.
    let mut min_val = f64::INFINITY;
    let mut max_val = f64::NEG_INFINITY;
    for &v in data.iter() {
        if v.is_finite() {
            if v < min_val { min_val = v; }
            if v > max_val { max_val = v; }
        }
    }
    let range = (max_val - min_val).max(1e-10);

    // Encode as RGB PNG with jet colormap.
    let mut rgb = Vec::with_capacity(h * w * 3);
    for &v in data.iter() {
        let t = ((v - min_val) / range).clamp(0.0, 1.0);
        let (r, g, b) = jet_colormap(t);
        rgb.push(r);
        rgb.push(g);
        rgb.push(b);
    }

    let mut png_data = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_data, w as u32, h as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()
            .map_err(|e| format!("PNG header: {e}"))?;
        writer.write_image_data(&rgb)
            .map_err(|e| format!("PNG write: {e}"))?;
    }

    std::fs::write(&output_path, &png_data)
        .map_err(|e| format!("Failed to write {output_path}: {e}"))?;
    Ok(())
}

fn jet_colormap(t: f64) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let (r, g, b) = if t < 0.125 {
        (0.0, 0.0, 0.5 + t / 0.125 * 0.5)
    } else if t < 0.375 {
        (0.0, (t - 0.125) / 0.25, 1.0)
    } else if t < 0.625 {
        ((t - 0.375) / 0.25, 1.0, 1.0 - (t - 0.375) / 0.25)
    } else if t < 0.875 {
        (1.0, 1.0 - (t - 0.625) / 0.25, 0.0)
    } else {
        (1.0 - (t - 0.875) / 0.125 * 0.5, 0.0, 0.0)
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

// ════════════════════════════════════════════════════════════════════════
// Hardware tool
// ════════════════════════════════════════════════════════════════════════

/// Get list of detected monitors.
#[tauri::command]
pub fn get_monitors(state: State<'_, SharedState>) -> Vec<MonitorInfo> {
    let app = state.lock().unwrap();
    app.monitors.clone()
}

/// Select a display for stimulus presentation. Spawns the stimulus thread.
#[tauri::command]
pub fn select_display(state: State<'_, SharedState>, monitor_index: usize) -> Result<MonitorInfo, String> {
    let mut app = state.lock().unwrap();

    let monitor = app.monitors.get(monitor_index)
        .ok_or_else(|| format!("Monitor index {} out of range", monitor_index))?
        .clone();

    app.session.set_selected_display(monitor.clone());

    // Spawn stimulus thread if not already running.
    if !app.threads.stimulus_thread_spawned {
        app.spawn_stimulus_thread(&monitor);
    }

    Ok(monitor)
}

/// Validate display timing via WaitForVBlank measurement (~2.5s).
/// This blocks the calling thread but the frontend can await it.
#[tauri::command]
pub fn validate_display(state: State<'_, SharedState>) -> Result<crate::session::DisplayValidation, String> {
    #[cfg(not(windows))]
    {
        let _ = state;
        return Err("Display validation requires Windows (DXGI WaitForVBlank)".into());
    }

    #[cfg(windows)]
    {
        let app = state.lock().unwrap();
        let monitor = app.session.selected_display.as_ref()
            .ok_or("No display selected")?;

        let monitor_index = monitor.index;
        let expected_refresh = monitor.refresh_hz as f64;
        let sample_count = app.config.lock().unwrap().rig.system.display_validation_sample_count;
        drop(app); // Release lock during measurement

        let dxgi_output = crate::monitor::find_dxgi_output(monitor_index)?;

        let mut qpc_freq = 0i64;
        unsafe {
            let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut qpc_freq);
        }
        if qpc_freq == 0 {
            return Err("QueryPerformanceFrequency failed".into());
        }

        // Collect raw timestamps (including warmup).
        let warmup_count = 30u32;
        let total_samples = sample_count + warmup_count;
        let mut timestamps = Vec::with_capacity(total_samples as usize);

        for _ in 0..total_samples {
            unsafe {
                dxgi_output.WaitForVBlank().map_err(|e| format!("WaitForVBlank: {e}"))?;
            }
            let mut qpc = 0i64;
            unsafe { let _ = windows::Win32::System::Performance::QueryPerformanceCounter(&mut qpc); }
            timestamps.push(qpc);
        }

        // Skip warmup samples, compute deltas on the rest.
        let valid_timestamps = &timestamps[warmup_count as usize..];
        let deltas_us: Vec<f64> = valid_timestamps
            .windows(2)
            .map(|w| (w[1] - w[0]) as f64 * 1_000_000.0 / qpc_freq as f64)
            .collect();

        let n = deltas_us.len() as f64;
        let mean_delta_us = deltas_us.iter().sum::<f64>() / n;
        let measured_refresh_hz = 1_000_000.0 / mean_delta_us;
        let variance = deltas_us.iter()
            .map(|d| (d - mean_delta_us).powi(2))
            .sum::<f64>() / n;
        let jitter_us = variance.sqrt();

        // 95% confidence interval: mean ± z * (std / sqrt(n))
        let z_score = 1.96;
        let ci_delta_us = z_score * jitter_us / n.sqrt();
        let ci_hz_low = 1_000_000.0 / (mean_delta_us + ci_delta_us);
        let ci_hz_high = 1_000_000.0 / (mean_delta_us - ci_delta_us);
        let ci95_hz = (ci_hz_high - ci_hz_low) / 2.0;

        // Mismatch detection: does measured match reported within 5%?
        let tolerance = 0.05;
        let matches_reported = (measured_refresh_hz - expected_refresh).abs() / expected_refresh < tolerance;

        let mut warnings = Vec::new();

        // CI width > 2% of mean → measurement is noisy.
        if ci95_hz / measured_refresh_hz > 0.02 {
            warnings.push(format!(
                "High measurement uncertainty: 95% CI is ±{:.2}Hz ({:.1}% of {:.1}Hz)",
                ci95_hz, ci95_hz / measured_refresh_hz * 100.0, measured_refresh_hz
            ));
        }

        if !matches_reported {
            warnings.push(format!(
                "Measured {:.2}Hz differs from reported {:.0}Hz by {:.1}%",
                measured_refresh_hz, expected_refresh,
                (measured_refresh_hz - expected_refresh).abs() / expected_refresh * 100.0
            ));
        }

        let validation = crate::session::DisplayValidation {
            measured_refresh_hz,
            sample_count,
            jitter_us,
            ci95_hz,
            matches_reported,
            reported_refresh_hz: expected_refresh,
            warnings: warnings.clone(),
        };

        eprintln!(
            "[validate] measured {:.2}Hz (reported {:.0}Hz), jitter={:.1}µs, CI95=±{:.2}Hz, {} samples ({}warmup skipped){}",
            measured_refresh_hz, expected_refresh, jitter_us, ci95_hz, sample_count, warmup_count,
            if warnings.is_empty() { String::new() } else { format!(" WARNINGS: {}", warnings.join("; ")) }
        );

        let mut app = state.lock().unwrap();
        app.session.set_display_validation(validation.clone());

        Ok(validation)
    }
}

/// Validate timing relationship between camera and stimulus clocks.
///
/// Requires: display selected + validated, camera connected (streaming frames).
/// Measures vsync rate via WaitForVBlank, uses recent camera hardware timestamps
/// from the ring buffer, computes TimingCharacterization, stores in session.
#[tauri::command]
pub fn validate_timing(state: State<'_, SharedState>) -> Result<crate::timing::TimingCharacterization, String> {
    #[cfg(not(windows))]
    {
        let _ = state;
        return Err("Timing validation requires Windows (DXGI WaitForVBlank)".into());
    }

    #[cfg(windows)]
    {
    let app = state.lock().unwrap();

    // Prerequisites.
    let monitor = app.session.selected_display.as_ref()
        .ok_or("No display selected")?;
    if !app.session.camera_connected {
        return Err("Camera not connected — connect camera before timing validation".into());
    }
    let _display_validation = app.session.display_validation.as_ref()
        .ok_or("Display not validated — validate display before timing validation")?;

    let monitor_index = monitor.index;
    let monitor_width_cm = monitor.width_cm;
    let monitor_height_cm = monitor.height_cm;
    let monitor_width_px = monitor.width_px;
    let monitor_height_px = monitor.height_px;

    // Grab camera timestamps from ring buffer.
    let cam_hw_ts = app.camera_hw_timestamps_ring.clone();
    let cam_sys_ts = app.camera_sys_timestamps_ring.clone();
    let experiment = app.experiment.clone();
    let rig = app.config.lock().unwrap().rig.clone();

    drop(app); // Release lock during measurement.

    // Need at least 30 camera frames for meaningful statistics.
    if cam_hw_ts.len() < 30 {
        return Err(format!(
            "Not enough camera frames for timing validation ({} frames, need ≥30). \
             Let the camera run for a few seconds first.",
            cam_hw_ts.len()
        ));
    }

    // Camera deltas from hardware timestamps.
    let cam_deltas: Vec<f64> = cam_hw_ts.windows(2)
        .map(|w| (w[1] - w[0]) as f64)
        .collect();

    // Clock offset uncertainty: std dev of (sys - hw) across recent frames.
    let offsets: Vec<f64> = cam_sys_ts.iter().zip(cam_hw_ts.iter())
        .map(|(&sys, &hw)| (sys - hw) as f64)
        .collect();
    let offset_mean = offsets.iter().sum::<f64>() / offsets.len() as f64;
    let offset_variance = offsets.iter()
        .map(|o| (o - offset_mean).powi(2))
        .sum::<f64>() / offsets.len() as f64;
    let clock_offset_uncertainty_us = offset_variance.sqrt();

    // Stimulus rate: measure WaitForVBlank (~200 samples, ~3s).
    let dxgi_output = crate::monitor::find_dxgi_output(monitor_index)?;
    let mut qpc_freq = 0i64;
    unsafe {
        let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut qpc_freq);
    }
    if qpc_freq == 0 {
        return Err("QueryPerformanceFrequency failed".into());
    }

    let warmup = 30;
    let sample_count = 150;
    let mut stim_timestamps = Vec::with_capacity(warmup + sample_count);
    for _ in 0..(warmup + sample_count) {
        unsafe {
            dxgi_output.WaitForVBlank().map_err(|e| format!("WaitForVBlank: {e}"))?;
        }
        let mut qpc = 0i64;
        unsafe { let _ = windows::Win32::System::Performance::QueryPerformanceCounter(&mut qpc); }
        stim_timestamps.push(((qpc as i128 * 1_000_000) / qpc_freq as i128) as i64);
    }

    let valid_stim = &stim_timestamps[warmup..];
    let stim_deltas: Vec<f64> = valid_stim.windows(2)
        .map(|w| (w[1] - w[0]) as f64)
        .collect();

    // Compute session parameters from experiment + geometry.
    use openisi_stimulus::geometry::{DisplayGeometry, ProjectionType};

    let projection = match experiment.geometry.projection {
        crate::config::Projection::Cartesian => ProjectionType::Cartesian,
        crate::config::Projection::Spherical => ProjectionType::Spherical,
        crate::config::Projection::Cylindrical => ProjectionType::Cylindrical,
    };
    let geometry = DisplayGeometry::new(
        projection,
        rig.geometry.viewing_distance_cm,
        experiment.geometry.horizontal_offset_deg,
        experiment.geometry.vertical_offset_deg,
        monitor_width_cm, monitor_height_cm,
        monitor_width_px, monitor_height_px,
    );

    let p = &experiment.stimulus.params;
    let sweep_sec = match experiment.stimulus.envelope {
        crate::config::Envelope::Bar => {
            let total_travel = geometry.visual_field_width_deg() + p.stimulus_width_deg;
            total_travel / p.sweep_speed_deg_per_sec
        }
        crate::config::Envelope::Wedge => {
            360.0 / p.rotation_speed_deg_per_sec
        }
        crate::config::Envelope::Ring => {
            let total_travel = geometry.get_max_eccentricity_deg() + p.stimulus_width_deg;
            total_travel / p.expansion_speed_deg_per_sec
        }
        crate::config::Envelope::Fullfield => 0.0,
    };

    let n_conditions = experiment.presentation.conditions.len();
    let n_reps = experiment.presentation.repetitions as usize;
    let n_trials = n_conditions * n_reps;
    let inter_trial_sec = sweep_sec + experiment.timing.inter_stimulus_sec;

    let total_sweep_time = n_trials as f64 * sweep_sec;
    let total_inter_stim = if n_trials > 1 {
        (n_trials - 1) as f64 * experiment.timing.inter_stimulus_sec
    } else { 0.0 };
    let total_inter_dir = if n_conditions > 1 {
        (n_conditions - 1) as f64 * experiment.timing.inter_direction_sec * n_reps as f64
    } else { 0.0 };
    let session_sec = experiment.timing.baseline_start_sec
        + total_sweep_time
        + total_inter_stim
        + total_inter_dir
        + experiment.timing.baseline_end_sec;

    let timing_params = crate::timing::TimingParams {
        n_trials,
        inter_trial_sec,
        session_duration_sec: session_sec,
    };

    let tc = crate::timing::characterize_timing(
        &cam_deltas,
        &stim_deltas,
        clock_offset_uncertainty_us,
        &timing_params,
    );

    eprintln!("[timing] {tc}");

    // Store in session.
    let mut app = state.lock().unwrap();
    app.session.timing_characterization = Some(tc.clone());

    Ok(tc)
    } // #[cfg(windows)]
}

/// Set the physical rotation of the stimulus monitor (degrees around viewing axis).
/// e.g., 180 = mounted upside down. Applied to stimulus output only, not preview.
#[tauri::command]
pub fn set_monitor_rotation(state: State<'_, SharedState>, rotation_deg: f64) -> Result<(), String> {
    let mut app = state.lock().unwrap();
    app.session.monitor_rotation_deg = rotation_deg;
    // Persist to config.
    {
        let mut cfg = app.config.lock().unwrap();
        cfg.rig.display.monitor_rotation_deg = rotation_deg;
        if let Err(e) = cfg.save() {
            eprintln!("[config] Failed to save monitor rotation: {e}");
        }
    }
    eprintln!("[config] monitor rotation set to {rotation_deg}°");
    Ok(())
}

/// Get the rig geometry (viewing distance).
#[tauri::command]
pub fn get_rig_geometry(state: State<'_, SharedState>) -> crate::config::RigGeometry {
    let app = state.lock().unwrap();
    app.config.lock().unwrap().rig.geometry.clone()
}

/// Set the viewing distance. Persists to rig.toml.
#[tauri::command]
pub fn set_viewing_distance(state: State<'_, SharedState>, distance_cm: f64) -> Result<(), String> {
    let app = state.lock().unwrap();
    let mut cfg = app.config.lock().unwrap();
    cfg.rig.geometry.viewing_distance_cm = distance_cm;
    if let Err(e) = cfg.save() {
        eprintln!("[config] Failed to save viewing distance: {e}");
    }
    Ok(())
}

/// Override physical dimensions of the selected display.
#[tauri::command]
pub fn set_display_dimensions(
    state: State<'_, SharedState>,
    width_cm: f64,
    height_cm: f64,
) -> Result<(), String> {
    let mut app = state.lock().unwrap();
    let display = app.session.selected_display.as_mut()
        .ok_or("No display selected")?;
    display.width_cm = width_cm;
    display.height_cm = height_cm;
    display.physical_source = "user_override".into();
    eprintln!("[config] display dimensions set to {:.1}x{:.1}cm (user override)", width_cm, height_cm);
    Ok(())
}

/// Get ring overlay config.
#[tauri::command]
pub fn get_ring_overlay(state: State<'_, SharedState>) -> crate::config::RingOverlay {
    let app = state.lock().unwrap();
    app.config.lock().unwrap().rig.ring_overlay.clone()
}

/// Update ring overlay config. Persists to rig.toml.
#[tauri::command]
pub fn set_ring_overlay(state: State<'_, SharedState>, overlay: crate::config::RingOverlay) -> Result<(), String> {
    let app = state.lock().unwrap();
    let mut cfg = app.config.lock().unwrap();
    cfg.rig.ring_overlay = overlay;
    if let Err(e) = cfg.save() {
        eprintln!("[config] Failed to save ring overlay: {e}");
    }
    Ok(())
}

/// Enumerate available cameras — results arrive via camera:enumerated event.
#[tauri::command]
pub fn enumerate_cameras(state: State<'_, SharedState>) -> Result<(), String> {
    let app = state.lock().unwrap();
    if let Some(ref tx) = app.threads.camera_tx {
        tx.send(CameraCmd::Enumerate).map_err(|e| format!("Send failed: {e}"))?;
        Ok(())
    } else {
        Err("Camera thread not running".into())
    }
}

/// Connect to a specific camera by index.
#[tauri::command]
pub fn connect_camera(state: State<'_, SharedState>, camera_index: u16) -> Result<(), String> {
    let app = state.lock().unwrap();
    let cam = app.config.lock().unwrap().rig.camera.clone();
    if let Some(ref tx) = app.threads.camera_tx {
        tx.send(CameraCmd::Connect { index: camera_index, exposure_us: cam.exposure_us, binning: cam.binning })
            .map_err(|e| format!("Send failed: {e}"))?;
        Ok(())
    } else {
        Err("Camera thread not running".into())
    }
}

/// Disconnect from the camera daemon.
#[tauri::command]
pub fn disconnect_camera(state: State<'_, SharedState>) -> Result<(), String> {
    let app = state.lock().unwrap();
    if let Some(ref tx) = app.threads.camera_tx {
        tx.send(CameraCmd::Disconnect).map_err(|e| format!("Send failed: {e}"))?;
        Ok(())
    } else {
        Err("Camera thread not running".into())
    }
}

// ════════════════════════════════════════════════════════════════════════
// Camera tool
// ════════════════════════════════════════════════════════════════════════

#[derive(Serialize)]
pub struct CameraFrameResponse {
    pub png_bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub sequence_number: u64,
}

/// Get the latest camera frame as PNG bytes (on-demand, for when events aren't enough).
#[tauri::command]
pub fn get_camera_frame(state: State<'_, SharedState>) -> Option<CameraFrameResponse> {
    let app = state.lock().unwrap();
    let cache = app.latest_camera_frame.as_ref()?;
    let png_bytes = crate::events::encode_16bit_to_png_pub(&cache.pixels, cache.width, cache.height)?;
    Some(CameraFrameResponse {
        png_bytes,
        width: cache.width,
        height: cache.height,
        sequence_number: cache.sequence_number,
    })
}

/// Capture the current camera frame as a 16-bit PNG anatomical reference.
#[tauri::command]
pub fn capture_anatomical(state: State<'_, SharedState>, path: String) -> Result<String, String> {
    let mut app = state.lock().unwrap();
    let cache = app.latest_camera_frame.as_ref()
        .ok_or("No camera frame available")?;

    let width = cache.width;
    let height = cache.height;
    let pixels = cache.pixels.clone();

    // Store as u8 ndarray for embedding in .oisi later.
    // Auto-contrast: scale u16 range to u8.
    let min_val = pixels.iter().copied().min().unwrap_or(0);
    let max_val = pixels.iter().copied().max().unwrap_or(0);
    let range = (max_val - min_val).max(1) as f64;
    let u8_pixels: Vec<u8> = pixels.iter()
        .map(|&p| ((p - min_val) as f64 / range * 255.0) as u8)
        .collect();
    let anat_array = ndarray::Array2::from_shape_vec(
        (height as usize, width as usize), u8_pixels
    ).map_err(|e| format!("Shape error: {e}"))?;
    app.anatomical_image = Some(anat_array);

    // Encode as 16-bit grayscale PNG for external file.
    let mut png_data = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_data, width, height);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Sixteen);
        let mut writer = encoder.write_header()
            .map_err(|e| format!("PNG header error: {e}"))?;
        let bytes: Vec<u8> = pixels.iter()
            .flat_map(|&p| p.to_be_bytes())
            .collect();
        writer.write_image_data(&bytes)
            .map_err(|e| format!("PNG write error: {e}"))?;
    }

    std::fs::write(&path, &png_data)
        .map_err(|e| format!("Failed to write {path}: {e}"))?;

    eprintln!("[commands] anatomical saved: {}x{} to {path}", width, height);
    Ok(path)
}

/// Set camera exposure in microseconds. Persists to rig.toml.
#[tauri::command]
pub fn set_exposure(state: State<'_, SharedState>, exposure_us: u32) -> Result<(), String> {
    let app = state.lock().unwrap();
    if let Some(ref tx) = app.threads.camera_tx {
        tx.send(CameraCmd::SetExposure(exposure_us))
            .map_err(|e| format!("Send failed: {e}"))?;
    } else {
        return Err("Camera thread not running".into());
    }
    // Persist to config.
    {
        let mut cfg = app.config.lock().unwrap();
        cfg.rig.camera.exposure_us = exposure_us;
        if let Err(e) = cfg.save() {
            eprintln!("[config] Failed to save exposure: {e}");
        }
    }
    Ok(())
}

/// Experiment summary for listing saved experiments.
#[derive(Serialize)]
pub struct ExperimentSummary {
    pub path: String,
    pub name: String,
    pub description: String,
    pub envelope: String,
    pub conditions: Vec<String>,
    pub repetitions: u32,
}

/// List available saved experiment files.
#[tauri::command]
pub fn list_experiments(state: State<'_, SharedState>) -> Vec<ExperimentSummary> {
    let app = state.lock().unwrap();
    let paths = app.config.lock().unwrap().list_experiments();

    let mut summaries = Vec::new();
    for path in paths {
        if let Ok(exp) = Experiment::load(&path) {
            summaries.push(ExperimentSummary {
                path: path.to_string_lossy().to_string(),
                name: exp.name.clone().unwrap_or_default(),
                description: exp.description.clone().unwrap_or_default(),
                envelope: format!("{:?}", exp.stimulus.envelope).to_lowercase(),
                conditions: exp.presentation.conditions.clone(),
                repetitions: exp.presentation.repetitions,
            });
        }
    }
    summaries
}

// ════════════════════════════════════════════════════════════════════════
// Stimulus tool
// ════════════════════════════════════════════════════════════════════════

/// Get the current experiment configuration.
#[tauri::command]
pub fn get_experiment(state: State<'_, SharedState>) -> Experiment {
    let app = state.lock().unwrap();
    app.experiment.clone()
}

/// Update experiment configuration. Stores in memory (effective config) and persists to disk.
#[tauri::command]
pub fn update_experiment(state: State<'_, SharedState>, config: Experiment) -> Result<(), String> {
    let mut app = state.lock().unwrap();
    // Store as the effective config — this is what acquisition will use.
    app.experiment = config.clone();
    // Also persist to disk.
    {
        let cfg = app.config.lock().unwrap();
        let exp_path = cfg.experiment_path();
        config.save(&exp_path)
            .map_err(|e| format!("Failed to write experiment.toml: {e}"))?;
    }
    Ok(())
}

/// Load an experiment from a specific file path.
#[tauri::command]
pub fn load_experiment(state: State<'_, SharedState>, path: String) -> Result<Experiment, String> {
    let exp = Experiment::load(std::path::Path::new(&path))
        .map_err(|e| format!("Failed to load experiment: {e}"))?;

    let mut app = state.lock().unwrap();
    app.experiment = exp.clone();

    // Update last_experiment_path in config.
    {
        let mut cfg = app.config.lock().unwrap();
        cfg.rig.paths.last_experiment_path = path;
        if let Err(e) = cfg.save() {
            eprintln!("[config] Failed to save last_experiment_path: {e}");
        }
    }

    Ok(exp)
}

/// Save the current experiment to a new file in the experiments directory.
#[tauri::command]
pub fn save_experiment_as(state: State<'_, SharedState>, name: String) -> Result<String, String> {
    let mut app = state.lock().unwrap();

    // Set name and timestamps.
    app.experiment.name = Some(name.clone());
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let now_str = format!("{now_secs}");
    if app.experiment.created.is_none() {
        app.experiment.created = Some(now_str.clone());
    }
    app.experiment.modified = Some(now_str);

    let exp_dir = app.config.lock().unwrap().experiments_dir();
    let _ = std::fs::create_dir_all(&exp_dir);

    // Sanitize filename.
    let safe_name: String = name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let path = exp_dir.join(format!("{safe_name}.toml"));

    app.experiment.save(&path)
        .map_err(|e| format!("Failed to save experiment: {e}"))?;

    let path_str = path.to_string_lossy().to_string();
    eprintln!("[commands] experiment saved as: {path_str}");
    Ok(path_str)
}

/// Compute duration summary from current experiment config.
#[derive(Serialize)]
pub struct DurationSummary {
    pub total_sec: f64,
    pub sweep_count: usize,
    pub sweep_duration_sec: f64,
    pub formatted: String,
}

#[tauri::command]
pub fn get_duration_summary(state: State<'_, SharedState>) -> Result<DurationSummary, String> {
    use openisi_stimulus::geometry::{DisplayGeometry, ProjectionType};

    let app = state.lock().unwrap();
    let exp = &app.experiment;
    let monitor = app.session.selected_display.as_ref()
        .ok_or("No display selected — select a display to compute duration")?;

    let rig_geometry = app.config.lock().unwrap().rig.geometry.clone();

    let n_conditions = exp.presentation.conditions.len();
    let reps = exp.presentation.repetitions as usize;
    let sweep_count = n_conditions * reps;

    let params = &exp.stimulus.params;

    let projection = ProjectionType::from_int(exp.geometry.projection.to_shader_int())
        .ok_or_else(|| format!("Invalid projection type: {:?}", exp.geometry.projection))?;
    let geometry = DisplayGeometry::new(
        projection,
        rig_geometry.viewing_distance_cm,
        exp.geometry.horizontal_offset_deg,
        exp.geometry.vertical_offset_deg,
        monitor.width_cm,
        monitor.height_cm,
        monitor.width_px,
        monitor.height_px,
    );

    let sweep_duration_sec = match exp.stimulus.envelope {
        crate::config::Envelope::Bar => {
            // Bar: (VF width + bar width) / speed
            let total_travel = geometry.visual_field_width_deg() + params.stimulus_width_deg;
            total_travel / params.sweep_speed_deg_per_sec
        }
        crate::config::Envelope::Wedge => {
            // Wedge: 360° / rotation speed
            360.0 / params.rotation_speed_deg_per_sec
        }
        crate::config::Envelope::Ring => {
            // Ring: (max eccentricity + ring width) / expansion speed
            let total_travel = geometry.get_max_eccentricity_deg() + params.stimulus_width_deg;
            total_travel / params.expansion_speed_deg_per_sec
        }
        crate::config::Envelope::Fullfield => {
            // Fullfield: no sweep, duration determined by timing
            0.0
        }
    };

    let total_sweep_time = sweep_count as f64 * sweep_duration_sec;
    let total_baseline = exp.timing.baseline_start_sec + exp.timing.baseline_end_sec;
    let total_inter = if sweep_count > 1 {
        (sweep_count - 1) as f64 * exp.timing.inter_stimulus_sec
    } else {
        0.0
    };
    let total_inter_dir = if n_conditions > 1 {
        (n_conditions - 1) as f64 * exp.timing.inter_direction_sec * reps as f64
    } else {
        0.0
    };

    let total_sec = total_baseline + total_sweep_time + total_inter + total_inter_dir;
    let mins = (total_sec / 60.0).floor() as u32;
    let secs = (total_sec % 60.0).round() as u32;
    let formatted = format!("{}:{:02} ({} sweeps x {:.1}s)", mins, secs, sweep_count, sweep_duration_sec);

    Ok(DurationSummary {
        total_sec,
        sweep_count,
        sweep_duration_sec,
        formatted,
    })
}

/// Validate experiment parameters before acquisition or preview.
fn validate_experiment(exp: &Experiment) -> Result<(), String> {
    let p = &exp.stimulus.params;
    match exp.stimulus.envelope {
        crate::config::Envelope::Bar => {
            if p.sweep_speed_deg_per_sec <= 0.0 {
                return Err("Sweep speed must be greater than zero".into());
            }
        }
        crate::config::Envelope::Wedge => {
            if p.rotation_speed_deg_per_sec <= 0.0 {
                return Err("Rotation speed must be greater than zero".into());
            }
        }
        crate::config::Envelope::Ring => {
            if p.expansion_speed_deg_per_sec <= 0.0 {
                return Err("Expansion speed must be greater than zero".into());
            }
        }
        crate::config::Envelope::Fullfield => {}
    }
    if p.stimulus_width_deg <= 0.0 {
        return Err("Stimulus width must be greater than zero".into());
    }
    if exp.presentation.repetitions == 0 {
        return Err("Repetitions must be at least 1".into());
    }
    if exp.presentation.conditions.is_empty() {
        return Err("No conditions defined".into());
    }
    Ok(())
}

/// Start stimulus preview on the stimulus monitor (no recording).
#[tauri::command]
pub fn start_preview(state: State<'_, SharedState>) -> Result<(), String> {
    let app = state.lock().unwrap();

    let experiment = app.experiment.clone();
    validate_experiment(&experiment)?;
    let geometry = app.config.lock().unwrap().rig.geometry.clone();

    let monitor = app.session.selected_display.clone()
        .ok_or("No display selected — select a display before previewing")?;

    let tx = app.threads.stimulus_tx.as_ref()
        .ok_or("Stimulus thread not running — select a display first")?;

    tx.send(StimulusCmd::Preview(PreviewCommand {
        experiment,
        geometry,
        monitor,
    })).map_err(|e| format!("Send failed: {e}"))?;
    Ok(())
}

/// Stop stimulus preview.
#[tauri::command]
pub fn stop_preview(state: State<'_, SharedState>) -> Result<(), String> {
    let app = state.lock().unwrap();
    if let Some(ref tx) = app.threads.stimulus_tx {
        tx.send(StimulusCmd::StopPreview).map_err(|e| format!("Send failed: {e}"))?;
        Ok(())
    } else {
        Err("Stimulus thread not running".into())
    }
}

// ════════════════════════════════════════════════════════════════════════
// Acquire tool
// ════════════════════════════════════════════════════════════════════════

/// Set the save path for the next acquisition.
/// Call this before start_acquisition — the save-path-first workflow ensures
/// the user chooses where to save before data collection begins.
#[tauri::command]
pub fn set_save_path(state: State<'_, SharedState>, path: String) -> Result<(), String> {
    let mut app = state.lock().unwrap();
    let path = std::path::PathBuf::from(&path);

    // Verify parent directory exists.
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            return Err(format!("Directory does not exist: {}", parent.display()));
        }
    }

    // Verify path ends with .oisi
    if path.extension().and_then(|e| e.to_str()) != Some("oisi") {
        return Err("Save path must end with .oisi".into());
    }

    app.session.set_save_path(path);
    Ok(())
}

/// Get the current save path.
#[tauri::command]
pub fn get_save_path(state: State<'_, SharedState>) -> Option<String> {
    let app = state.lock().unwrap();
    app.session.save_path.as_ref().map(|p| p.to_string_lossy().to_string())
}

/// Set session metadata (animal ID and notes).
#[tauri::command]
pub fn set_session_metadata(state: State<'_, SharedState>, animal_id: String, notes: String) -> Result<(), String> {
    let mut app = state.lock().unwrap();
    app.session.animal_id = animal_id;
    app.session.notes = notes;
    Ok(())
}

/// Start acquisition — ties stimulus + camera together.
#[tauri::command]
pub fn start_acquisition(state: State<'_, SharedState>) -> Result<(), String> {
    let mut app = state.lock().unwrap();

    validate_experiment(&app.experiment)?;

    // Check prerequisites.
    let monitor = app.session.selected_display.as_ref()
        .ok_or("No display selected")?
        .clone();

    if !app.session.camera_connected {
        return Err("Camera not connected".into());
    }

    if app.session.display_validation.is_none() {
        return Err("Display not validated — validate display before acquiring".into());
    }

    // Timing validation is strongly recommended but not a hard block.
    // If present and Systematic, warn the user.
    if let Some(ref tc) = app.session.timing_characterization {
        if tc.regime == crate::timing::TimingRegime::Systematic {
            eprintln!(
                "[acquire] WARNING: Systematic timing regime (beat period {:.1}s). \
                 Every trial sees approximately the same sub-frame onset position.",
                tc.beat_period_sec
            );
        }
    }

    let experiment = app.experiment.clone();
    let measured_refresh_hz = app.session.display_validation.as_ref().unwrap().measured_refresh_hz;

    let rig = app.config.lock().unwrap().rig.clone();

    let acq_cmd = AcquisitionCommand {
        experiment,
        geometry: rig.geometry.clone(),
        monitor: monitor.clone(),
        display: rig.display.clone(),
        measured_refresh_hz,
        system: rig.system.clone(),
    };

    let tx = app.threads.stimulus_tx.as_ref()
        .ok_or("Stimulus thread not running — select a display first")?;
    tx.send(StimulusCmd::StartAcquisition(acq_cmd))
        .map_err(|e| format!("Send failed: {e}"))?;

    // Start camera frame accumulation.
    let (cam_w, cam_h) = {
        let cam = app.session.camera.as_ref()
            .expect("Camera info must be available during acquisition");
        (cam.width_px, cam.height_px)
    };
    app.start_acquisition(cam_w, cam_h);

    Ok(())
}

/// Stop the current acquisition.
#[tauri::command]
pub fn stop_acquisition(state: State<'_, SharedState>) -> Result<(), String> {
    let mut app = state.lock().unwrap();
    if let Some(ref tx) = app.threads.stimulus_tx {
        tx.send(StimulusCmd::Stop).map_err(|e| format!("Send failed: {e}"))?;
        app.session.is_acquiring = false;
        Ok(())
    } else {
        Err("Stimulus thread not running".into())
    }
}

/// Save the pending acquisition to a .oisi file. Called after user confirms.
#[tauri::command]
pub fn save_acquisition(state: State<'_, SharedState>, path: Option<String>) -> Result<String, String> {
    let mut app = state.lock().unwrap();

    let pending = app.pending_save.take()
        .ok_or("No pending acquisition to save")?;

    let experiment = app.experiment.clone();

    // Determine output path.
    let output_path = if let Some(p) = path {
        std::path::PathBuf::from(p)
    } else {
        let cfg = app.config.lock().unwrap();
        let data_dir = &cfg.rig.paths.data_directory;
        let dir = if data_dir.is_empty() {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| std::env::current_dir().unwrap())
        } else {
            std::path::PathBuf::from(data_dir)
        };
        let _ = std::fs::create_dir_all(&dir);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("System clock is before Unix epoch")
            .as_secs();
        // Use animal_id in filename if set, otherwise just timestamp.
        let safe_id: String = pending.animal_id.trim().chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        let filename = if safe_id.is_empty() {
            format!("acquisition_{ts}.oisi")
        } else {
            format!("{safe_id}_{ts}.oisi")
        };
        dir.join(filename)
    };

    let session_meta = crate::export::SessionMetadata {
        animal_id: pending.animal_id.clone(),
        notes: pending.notes.clone(),
    };
    let anatomical = app.anatomical_image.clone();

    drop(app); // Release lock during file write.

    crate::export::write_oisi(
        &output_path,
        &pending.stimulus_dataset,
        pending.camera_data,
        Some(&experiment),
        pending.hardware_snapshot.as_ref(),
        &pending.schedule,
        pending.timing_characterization.as_ref(),
        Some(&session_meta),
        anatomical.as_ref(),
        pending.completed_normally,
    )?;

    // Update summary with file path.
    let mut app = state.lock().unwrap();
    if let Some(ref mut summary) = app.last_acquisition_summary {
        summary.file_path = Some(output_path.to_string_lossy().to_string());
    }

    Ok(output_path.to_string_lossy().to_string())
}

/// Discard the pending acquisition without saving.
#[tauri::command]
pub fn discard_acquisition(state: State<'_, SharedState>) -> Result<(), String> {
    let mut app = state.lock().unwrap();
    let had_pending = app.pending_save.take().is_some();
    if had_pending {
        eprintln!("[commands] acquisition discarded by user");
    }
    Ok(())
}

// ════════════════════════════════════════════════════════════════════════
// Workspace state
// ════════════════════════════════════════════════════════════════════════

/// Get full session state for UI hydration on screen mount.
#[tauri::command]
pub fn get_session(state: State<'_, SharedState>) -> serde_json::Value {
    let app = state.lock().unwrap();
    let exposure_us = app.config.lock().unwrap().rig.camera.exposure_us;
    serde_json::json!({
        "selected_display": app.session.selected_display,
        "display_validation": app.session.display_validation,
        "timing_characterization": app.session.timing_characterization,
        "camera_connected": app.session.camera_connected,
        "camera": app.session.camera,
        "is_acquiring": app.session.is_acquiring,
        "stimulus_thread_ready": app.threads.stimulus_thread_spawned,
        "last_acquisition": app.last_acquisition_summary,
        "save_path": app.session.save_path,
        "monitor_rotation_deg": app.session.monitor_rotation_deg,
        "exposure_us": exposure_us,
        "anatomical_captured": app.anatomical_image.is_some(),
    })
}

/// Get workspace status summary (for status bar).
#[derive(Serialize)]
pub struct WorkspaceStatus {
    pub display: String,
    pub camera: String,
    pub activity: String,
}

#[tauri::command]
pub fn get_workspace_status(state: State<'_, SharedState>) -> WorkspaceStatus {
    let app = state.lock().unwrap();

    let display = if let Some(ref v) = app.session.display_validation {
        if let Some(ref d) = app.session.selected_display {
            format!("{} {:.1}Hz", d.name, v.measured_refresh_hz)
        } else {
            "Validated".into()
        }
    } else if let Some(ref d) = app.session.selected_display {
        format!("{} (not validated)", d.name)
    } else {
        "None".into()
    };

    let camera = if let Some(ref c) = app.session.camera {
        format!("{} {}x{}", c.model, c.width_px, c.height_px)
    } else if app.session.camera_connected {
        "Connected".into()
    } else {
        "Disconnected".into()
    };

    let activity = if app.session.is_acquiring {
        "Acquiring".into()
    } else {
        "Idle".into()
    };

    WorkspaceStatus { display, camera, activity }
}

// ════════════════════════════════════════════════════════════════════════
// Helpers
// ════════════════════════════════════════════════════════════════════════

/// Convert a day count (since Unix epoch) to a civil (year, month, day) date.
/// Algorithm from Howard Hinnant's `chrono`-compatible civil_from_days.
/// Delete one or more .oisi files. Returns the count of files actually deleted.
#[tauri::command]
pub fn delete_oisi_files(paths: Vec<String>) -> Result<u32, String> {
    let mut deleted = 0u32;
    for p in &paths {
        let path = std::path::Path::new(p);
        if path.extension().is_some_and(|ext| ext == "oisi") && path.exists() {
            std::fs::remove_file(path)
                .map_err(|e| format!("Failed to delete {}: {e}", path.display()))?;
            deleted += 1;
        }
    }
    eprintln!("[commands] deleted {deleted} file(s)");
    Ok(deleted)
}

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
