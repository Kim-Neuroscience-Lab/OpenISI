//! Hardware configuration commands: monitors, display, camera, exposure.

use tauri::State;

use crate::error::{AppError, AppResult};
use crate::messages::CameraCmd;
use crate::session::MonitorInfo;

use super::SharedState;

/// Get list of detected monitors.
#[tauri::command]
pub fn get_monitors(state: State<'_, SharedState>) -> AppResult<Vec<MonitorInfo>> {
    Ok((*state.monitors).clone())
}

/// Select a display for stimulus presentation. Spawns the stimulus thread.
#[tauri::command]
pub fn select_display(
    state: State<'_, SharedState>,
    monitor_index: usize,
) -> AppResult<MonitorInfo> {
    let monitor = state
        .monitors
        .get(monitor_index)
        .ok_or_else(|| {
            AppError::Validation(format!(
                "Monitor index {} out of range (have {} monitors)",
                monitor_index,
                state.monitors.len()
            ))
        })?
        .clone();

    state.session.lock().set_selected_display(monitor.clone());

    // Spawn stimulus thread if not already running. `spawn_stimulus_thread`
    // is a no-op if already spawned.
    state.spawn_stimulus_thread(&monitor);

    Ok(monitor)
}

/// Validate display timing via WaitForVBlank measurement (~2.5s).
/// This blocks the calling thread but the frontend can await it.
#[tauri::command]
pub fn validate_display(
    state: State<'_, SharedState>,
) -> AppResult<crate::session::DisplayValidation> {
    #[cfg(not(windows))]
    {
        let _ = state;
        return Err(AppError::NotAvailable(
            "Display validation requires Windows (DXGI WaitForVBlank)".into(),
        ));
    }

    #[cfg(windows)]
    {
        let monitor = state
            .session
            .lock()
            .selected_display
            .clone()
            .ok_or(AppError::Validation("No display selected".into()))?;

        let monitor_index = monitor.index;
        let expected_refresh = monitor.refresh_hz as f64;
        let sample_count = state.config.lock().rig().system.display_validation_sample_count;

        let dxgi_output = crate::monitor::find_dxgi_output(monitor_index)?;

        let mut qpc_freq = 0i64;
        unsafe {
            let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut qpc_freq);
        }
        if qpc_freq == 0 {
            return Err(AppError::Hardware(
                "QueryPerformanceFrequency failed".into(),
            ));
        }

        // Collect raw timestamps (including warmup).
        let warmup_count = 30u32;
        let total_samples = sample_count + warmup_count;
        let mut timestamps = Vec::with_capacity(total_samples as usize);

        for _ in 0..total_samples {
            unsafe {
                dxgi_output
                    .WaitForVBlank()
                    .map_err(|e| AppError::Hardware(format!("WaitForVBlank: {e}")))?;
            }
            let mut qpc = 0i64;
            unsafe {
                let _ = windows::Win32::System::Performance::QueryPerformanceCounter(&mut qpc);
            }
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
        let variance = deltas_us
            .iter()
            .map(|d| (d - mean_delta_us).powi(2))
            .sum::<f64>()
            / n;
        let jitter_us = variance.sqrt();

        // 95% confidence interval: mean +/- z * (std / sqrt(n))
        let z_score = 1.96;
        let ci_delta_us = z_score * jitter_us / n.sqrt();
        let ci_hz_low = 1_000_000.0 / (mean_delta_us + ci_delta_us);
        let ci_hz_high = 1_000_000.0 / (mean_delta_us - ci_delta_us);
        let ci95_hz = (ci_hz_high - ci_hz_low) / 2.0;

        // Mismatch detection: does measured match reported within 5%?
        let tolerance = 0.05;
        let matches_reported =
            (measured_refresh_hz - expected_refresh).abs() / expected_refresh < tolerance;

        let mut warnings = Vec::new();

        if ci95_hz / measured_refresh_hz > 0.02 {
            warnings.push(format!(
                "High measurement uncertainty: 95% CI is ±{:.2}Hz ({:.1}% of {:.1}Hz)",
                ci95_hz,
                ci95_hz / measured_refresh_hz * 100.0,
                measured_refresh_hz
            ));
        }

        if !matches_reported {
            warnings.push(format!(
                "Measured {:.2}Hz differs from reported {:.0}Hz by {:.1}%",
                measured_refresh_hz,
                expected_refresh,
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

        tracing::info!(
            measured_hz = measured_refresh_hz, reported_hz = expected_refresh,
            jitter_us, ci95_hz, samples = sample_count, warmup = warmup_count,
            warnings = %if warnings.is_empty() { String::new() } else { warnings.join("; ") },
            "display validation",
        );

        state
            .session
            .lock()
            .set_display_validation(validation.clone());

        Ok(validation)
    }
}

/// Validate timing relationship between camera and stimulus clocks.
#[tauri::command]
pub fn validate_timing(
    state: State<'_, SharedState>,
) -> AppResult<crate::timing::TimingCharacterization> {
    #[cfg(not(windows))]
    {
        let _ = state;
        return Err(AppError::NotAvailable(
            "Timing validation requires Windows (DXGI WaitForVBlank)".into(),
        ));
    }

    #[cfg(windows)]
    {
        // Prerequisites — read session, copy out, drop the guard.
        let (monitor_index, monitor_width_px, monitor_height_px) = {
            let session = state.session.lock();
            let monitor = session
                .selected_display
                .as_ref()
                .ok_or(AppError::Validation("No display selected".into()))?;
            if !session.camera_connected {
                return Err(AppError::Validation(
                    "Camera not connected — connect camera before timing validation".into(),
                ));
            }
            if session.display_validation.is_none() {
                return Err(AppError::Validation(
                    "Display not validated — validate display before timing validation".into(),
                ));
            }
            (monitor.index, monitor.width_px, monitor.height_px)
        };

        // Grab camera timestamps from ring buffer.
        let (cam_hw_ts, cam_sys_ts) = {
            let capture = state.capture.lock();
            (capture.timing.hw.clone(), capture.timing.sys.clone())
        };

        // Read experiment + rig params from the typed config snapshot.
        let snap = state.config.lock().snapshot();

        // Need at least 30 camera frames for meaningful statistics.
        if cam_hw_ts.len() < 30 {
            return Err(AppError::Validation(format!(
                "Not enough camera frames for timing validation ({} frames, need >=30). \
             Let the camera run for a few seconds first.",
                cam_hw_ts.len()
            )));
        }

        // Camera deltas from hardware timestamps.
        let cam_deltas: Vec<f64> = cam_hw_ts.windows(2).map(|w| (w[1] - w[0]) as f64).collect();

        // Clock offset uncertainty: std dev of (sys - hw) across recent frames.
        let offsets: Vec<f64> = cam_sys_ts
            .iter()
            .zip(cam_hw_ts.iter())
            .map(|(&sys, &hw)| (sys - hw) as f64)
            .collect();
        let offset_mean = offsets.iter().sum::<f64>() / offsets.len() as f64;
        let offset_variance = offsets
            .iter()
            .map(|o| (o - offset_mean).powi(2))
            .sum::<f64>()
            / offsets.len() as f64;
        let clock_offset_uncertainty_us = offset_variance.sqrt();

        // Stimulus rate: measure WaitForVBlank (~200 samples, ~3s).
        let dxgi_output = crate::monitor::find_dxgi_output(monitor_index)?;
        let mut qpc_freq = 0i64;
        unsafe {
            let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut qpc_freq);
        }
        if qpc_freq == 0 {
            return Err(AppError::Hardware(
                "QueryPerformanceFrequency failed".into(),
            ));
        }

        let warmup = 30;
        let sample_count = 150;
        let mut stim_timestamps = Vec::with_capacity(warmup + sample_count);
        for _ in 0..(warmup + sample_count) {
            unsafe {
                dxgi_output
                    .WaitForVBlank()
                    .map_err(|e| AppError::Hardware(format!("WaitForVBlank: {e}")))?;
            }
            let mut qpc = 0i64;
            unsafe {
                let _ = windows::Win32::System::Performance::QueryPerformanceCounter(&mut qpc);
            }
            stim_timestamps.push(((qpc as i128 * 1_000_000) / qpc_freq as i128) as i64);
        }

        let valid_stim = &stim_timestamps[warmup..];
        let stim_deltas: Vec<f64> = valid_stim
            .windows(2)
            .map(|w| (w[1] - w[0]) as f64)
            .collect();

        // Compute session parameters from snapshot + geometry.
        use openisi_stimulus::geometry::DisplayGeometry;

        let geo = &snap.experiment.geometry;
        let rig_geo = &snap.rig.geometry;
        let geometry = DisplayGeometry::new(
            geo.projection,
            rig_geo.viewing_distance_cm,
            geo.horizontal_offset_deg,
            geo.vertical_offset_deg,
            rig_geo.bisector_x_cm,
            rig_geo.bisector_y_cm,
            rig_geo.monitor_width_cm,
            rig_geo.monitor_height_cm,
            monitor_width_px,
            monitor_height_px,
        );

        let stim = &snap.experiment.stimulus;
        let sp = &stim.params;
        let sweep_sec = match stim.envelope {
            crate::params::Envelope::Bar => {
                let total_travel = geometry.visual_field_width_deg() + sp.stimulus_width_deg;
                total_travel / sp.sweep_speed_deg_per_sec
            }
            crate::params::Envelope::Wedge => 360.0 / sp.rotation_speed_deg_per_sec,
            crate::params::Envelope::Ring => {
                let total_travel = geometry.get_max_eccentricity_deg() + sp.stimulus_width_deg;
                total_travel / sp.expansion_speed_deg_per_sec
            }
            crate::params::Envelope::Fullfield => 0.0,
        };

        let timing = &snap.experiment.timing;
        let n_conditions = snap.experiment.presentation.conditions.len();
        let n_reps = snap.experiment.presentation.repetitions as usize;
        let n_trials = n_conditions * n_reps;
        let inter_trial_sec = sweep_sec + timing.inter_stimulus_sec;

        let total_sweep_time = n_trials as f64 * sweep_sec;
        let total_inter_stim = if n_trials > 1 {
            (n_trials - 1) as f64 * timing.inter_stimulus_sec
        } else {
            0.0
        };
        let total_inter_dir = if n_conditions > 1 {
            (n_conditions - 1) as f64 * timing.inter_direction_sec * n_reps as f64
        } else {
            0.0
        };
        let session_sec = timing.baseline_start_sec
            + total_sweep_time
            + total_inter_stim
            + total_inter_dir
            + timing.baseline_end_sec;

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

        tracing::info!("{tc}");

        // Store in session.
        state.session.lock().timing_characterization = Some(tc.clone());

        Ok(tc)
    } // #[cfg(windows)]
}

/// Set the physical rotation of the stimulus monitor.
#[tauri::command]
pub fn set_monitor_rotation(state: State<'_, SharedState>, rotation_deg: f64) -> AppResult<()> {
    let mut cfg = state.config.lock();
    cfg.merge_rig(&serde_json::json!({ "display": { "monitor_rotation_deg": rotation_deg } }))?;
    if let Err(e) = cfg.save_all() {
        tracing::error!(error = %e, "failed to save monitor rotation");
    }
    tracing::info!(rotation_deg, "monitor rotation set");
    Ok(())
}

/// Get the rig geometry (viewing distance).
#[tauri::command]
pub fn get_rig_geometry(state: State<'_, SharedState>) -> AppResult<serde_json::Value> {
    let cfg = state.config.lock();
    Ok(serde_json::json!({
        "viewing_distance_cm": cfg.rig().geometry.viewing_distance_cm,
    }))
}

/// Set the viewing distance. Persists to rig.toml.
#[tauri::command]
pub fn set_viewing_distance(state: State<'_, SharedState>, distance_cm: f64) -> AppResult<()> {
    let mut cfg = state.config.lock();
    cfg.merge_rig(&serde_json::json!({ "geometry": { "viewing_distance_cm": distance_cm } }))?;
    if let Err(e) = cfg.save_all() {
        tracing::error!(error = %e, "failed to save viewing distance");
    }
    Ok(())
}

/// Override physical dimensions of the selected display.
#[tauri::command]
pub fn set_display_dimensions(
    state: State<'_, SharedState>,
    width_cm: f64,
    height_cm: f64,
) -> AppResult<()> {
    let mut session = state.session.lock();
    let display = session
        .selected_display
        .as_mut()
        .ok_or(AppError::Validation("No display selected".into()))?;
    display.width_cm = width_cm;
    display.height_cm = height_cm;
    display.physical_source = "user_override".into();
    tracing::info!(
        width_cm,
        height_cm,
        "display dimensions set (user override)"
    );
    Ok(())
}

/// Get ring overlay config.
#[tauri::command]
pub fn get_ring_overlay(state: State<'_, SharedState>) -> AppResult<serde_json::Value> {
    let cfg = state.config.lock();
    let r = &cfg.rig().ring_overlay;
    Ok(serde_json::json!({
        "enabled": r.enabled,
        "radius_px": r.radius_px,
        "center_x_px": r.center_x_px,
        "center_y_px": r.center_y_px,
        "diameter_mm": r.diameter_mm,
    }))
}

/// Update ring overlay config. Persists to rig.json. `diameter_mm` is optional
/// (omitted ⇒ the stored value is kept) so older callers are unaffected.
#[tauri::command]
pub fn set_ring_overlay(
    state: State<'_, SharedState>,
    enabled: bool,
    radius_px: u32,
    center_x_px: u32,
    center_y_px: u32,
    diameter_mm: Option<f64>,
) -> AppResult<()> {
    let mut cfg = state.config.lock();
    let mut overlay = serde_json::json!({ "ring_overlay": {
        "enabled": enabled,
        "radius_px": radius_px,
        "center_x_px": center_x_px,
        "center_y_px": center_y_px,
    } });
    if let Some(d) = diameter_mm {
        overlay["ring_overlay"]["diameter_mm"] = serde_json::json!(d);
    }
    cfg.merge_rig(&overlay)?;
    if let Err(e) = cfg.save_all() {
        tracing::error!(error = %e, "failed to save ring overlay");
    }
    Ok(())
}

/// Calibrate `camera.um_per_pixel` from the head-ring overlay (its known physical
/// diameter spanning its pixel radius). Writes the measured value into the live
/// config and returns it (µm/pixel). Errors if the ring can't define a scale
/// (disabled / zero radius / non-positive diameter) rather than calibrating to a
/// meaningless number.
#[tauri::command]
pub fn calibrate_um_per_pixel_from_ring(state: State<'_, SharedState>) -> AppResult<f64> {
    let mut cfg = state.config.lock();
    let um_per_pixel = cfg.calibrate_um_per_pixel_from_ring()?;
    if let Err(e) = cfg.save_all() {
        tracing::error!(error = %e, "failed to save um_per_pixel after ring calibration");
    }
    Ok(um_per_pixel)
}

/// Enumerate available cameras — results arrive via camera:enumerated event.
#[tauri::command]
pub fn enumerate_cameras(state: State<'_, SharedState>) -> AppResult<()> {
    state
        .threads
        .camera_tx
        .send(CameraCmd::Enumerate)
        .map_err(|e| AppError::Hardware(format!("Failed to send enumerate command: {e}")))?;
    Ok(())
}

/// Connect to a specific camera by index.
#[tauri::command]
pub fn connect_camera(state: State<'_, SharedState>, camera_index: u16) -> AppResult<()> {
    let (exposure_us, binning) = {
        let cfg = state.config.lock();
        (cfg.rig().camera.exposure_us, cfg.rig().camera.binning)
    };
    state
        .threads
        .camera_tx
        .send(CameraCmd::Connect {
            index: camera_index,
            exposure_us,
            binning,
        })
        .map_err(|e| AppError::Hardware(format!("Failed to send connect command: {e}")))?;
    Ok(())
}

/// Disconnect from the camera.
#[tauri::command]
pub fn disconnect_camera(state: State<'_, SharedState>) -> AppResult<()> {
    state
        .threads
        .camera_tx
        .send(CameraCmd::Disconnect)
        .map_err(|e| AppError::Hardware(format!("Failed to send disconnect command: {e}")))?;
    Ok(())
}

/// Capture the current camera frame as a 16-bit PNG anatomical reference.
#[tauri::command]
pub fn capture_anatomical(state: State<'_, SharedState>, path: String) -> AppResult<String> {
    // Lock capture, clone the frame out, drop the guard before any compute/IO.
    let (width, height, pixels) = {
        let capture = state.capture.lock();
        let cache = capture
            .latest_frame
            .as_ref()
            .ok_or(AppError::NotAvailable("No camera frame available".into()))?;
        (cache.width, cache.height, cache.pixels.clone())
    };

    // Store as u8 ndarray for embedding in .oisi later.
    let min_val = pixels.iter().copied().min().unwrap_or(0);
    let max_val = pixels.iter().copied().max().unwrap_or(0);
    let range = (max_val - min_val).max(1) as f64;
    let u8_pixels: Vec<u8> = pixels
        .iter()
        .map(|&p| ((p - min_val) as f64 / range * 255.0) as u8)
        .collect();
    let anat_array = ndarray::Array2::from_shape_vec((height as usize, width as usize), u8_pixels)
        .map_err(|e| AppError::Hardware(format!("Camera frame shape error: {e}")))?;
    state.handoff.lock().anatomical = Some(anat_array);

    // Encode as 16-bit grayscale PNG for external file.
    let mut png_data = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_data, width, height);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Sixteen);
        let mut writer = encoder
            .write_header()
            .map_err(|e| AppError::Io(std::io::Error::other(format!("PNG header error: {e}"))))?;
        let bytes: Vec<u8> = pixels.iter().flat_map(|&p: &u16| p.to_be_bytes()).collect();
        writer
            .write_image_data(&bytes)
            .map_err(|e| AppError::Io(std::io::Error::other(format!("PNG write error: {e}"))))?;
    }

    std::fs::write(&path, &png_data)?;

    tracing::info!(width, height, %path, "anatomical saved");
    Ok(path)
}

/// Set camera exposure in microseconds. Persists to rig.toml.
#[tauri::command]
pub fn set_exposure(state: State<'_, SharedState>, exposure_us: u32) -> AppResult<()> {
    state
        .threads
        .camera_tx
        .send(CameraCmd::SetExposure(exposure_us))
        .map_err(|e| AppError::Hardware(format!("Failed to send exposure command: {e}")))?;
    // Persist to config (config-scoped, brief: merge + save while holding the lock).
    {
        let mut cfg = state.config.lock();
        cfg.merge_rig(&serde_json::json!({ "camera": { "exposure_us": exposure_us } }))?;
        if let Err(e) = cfg.save_all() {
            tracing::error!(error = %e, "failed to save exposure");
        }
    }
    // Keep session camera info in sync.
    if let Some(ref mut cam) = state.session.lock().camera {
        cam.exposure_us = exposure_us;
    }
    Ok(())
}
