//! Hardware configuration commands: monitors, display, camera, exposure.

use tauri::State;

use crate::error::{lock_state, AppError, AppResult};
use crate::messages::CameraCmd;
use crate::session::MonitorInfo;

use super::SharedState;

/// Get list of detected monitors.
#[tauri::command]
pub fn get_monitors(state: State<'_, SharedState>) -> AppResult<Vec<MonitorInfo>> {
    let app = lock_state(&state, "get_monitors")?;
    Ok(app.monitors.clone())
}

/// Select a display for stimulus presentation. Spawns the stimulus thread.
#[tauri::command]
pub fn select_display(state: State<'_, SharedState>, monitor_index: usize) -> AppResult<MonitorInfo> {
    let mut app = lock_state(&state, "select_display")?;

    let monitor = app.monitors.get(monitor_index)
        .ok_or_else(|| AppError::Validation(
            format!("Monitor index {} out of range (have {} monitors)", monitor_index, app.monitors.len()),
        ))?
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
pub fn validate_display(state: State<'_, SharedState>) -> AppResult<crate::session::DisplayValidation> {
    #[cfg(not(windows))]
    {
        let _ = state;
        return Err(AppError::NotAvailable(
            "Display validation requires Windows (DXGI WaitForVBlank)".into(),
        ));
    }

    #[cfg(windows)]
    {
        let app = lock_state(&state, "validate_display")?;
        let monitor = app.session.selected_display.as_ref()
            .ok_or(AppError::Validation("No display selected".into()))?;

        let monitor_index = monitor.index;
        let expected_refresh = monitor.refresh_hz as f64;
        let sample_count = lock_state(&app.registry, "validate_display registry")?.display_validation_sample_count();
        drop(app); // Release lock during measurement

        let dxgi_output = crate::monitor::find_dxgi_output(monitor_index)
                .map_err(|e| AppError::Hardware(e))?;

        let mut qpc_freq = 0i64;
        unsafe {
            let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut qpc_freq);
        }
        if qpc_freq == 0 {
            return Err(AppError::Hardware("QueryPerformanceFrequency failed".into()));
        }

        // Collect raw timestamps (including warmup).
        let warmup_count = 30u32;
        let total_samples = sample_count + warmup_count;
        let mut timestamps = Vec::with_capacity(total_samples as usize);

        for _ in 0..total_samples {
            unsafe {
                dxgi_output.WaitForVBlank()
                    .map_err(|e| AppError::Hardware(format!("WaitForVBlank: {e}")))?;
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

        // 95% confidence interval: mean +/- z * (std / sqrt(n))
        let z_score = 1.96;
        let ci_delta_us = z_score * jitter_us / n.sqrt();
        let ci_hz_low = 1_000_000.0 / (mean_delta_us + ci_delta_us);
        let ci_hz_high = 1_000_000.0 / (mean_delta_us - ci_delta_us);
        let ci95_hz = (ci_hz_high - ci_hz_low) / 2.0;

        // Mismatch detection: does measured match reported within 5%?
        let tolerance = 0.05;
        let matches_reported = (measured_refresh_hz - expected_refresh).abs() / expected_refresh < tolerance;

        let mut warnings = Vec::new();

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
            "[validate] measured {:.2}Hz (reported {:.0}Hz), jitter={:.1}us, CI95=±{:.2}Hz, {} samples ({}warmup skipped){}",
            measured_refresh_hz, expected_refresh, jitter_us, ci95_hz, sample_count, warmup_count,
            if warnings.is_empty() { String::new() } else { format!(" WARNINGS: {}", warnings.join("; ")) }
        );

        let mut app = lock_state(&state, "validate_display store result")?;
        app.session.set_display_validation(validation.clone());

        Ok(validation)
    }
}

/// Validate timing relationship between camera and stimulus clocks.
#[tauri::command]
pub fn validate_timing(state: State<'_, SharedState>) -> AppResult<crate::timing::TimingCharacterization> {
    #[cfg(not(windows))]
    {
        let _ = state;
        return Err(AppError::NotAvailable(
            "Timing validation requires Windows (DXGI WaitForVBlank)".into(),
        ));
    }

    #[cfg(windows)]
    {
    let app = lock_state(&state, "validate_timing")?;

    // Prerequisites.
    let monitor = app.session.selected_display.as_ref()
        .ok_or(AppError::Validation("No display selected".into()))?;
    if !app.session.camera_connected {
        return Err(AppError::Validation(
            "Camera not connected — connect camera before timing validation".into(),
        ));
    }
    let _display_validation = app.session.display_validation.as_ref()
        .ok_or(AppError::Validation(
            "Display not validated — validate display before timing validation".into(),
        ))?;

    let monitor_index = monitor.index;
    let monitor_width_cm = monitor.width_cm;
    let monitor_height_cm = monitor.height_cm;
    let monitor_width_px = monitor.width_px;
    let monitor_height_px = monitor.height_px;

    // Grab camera timestamps from ring buffer.
    let cam_hw_ts = app.camera_hw_timestamps_ring.clone();
    let cam_sys_ts = app.camera_sys_timestamps_ring.clone();

    // Read experiment + rig params from registry.
    let reg = lock_state(&app.registry, "validate_timing registry")?;
    let snap = reg.snapshot();
    drop(reg);
    drop(app); // Release lock during measurement.

    // Need at least 30 camera frames for meaningful statistics.
    if cam_hw_ts.len() < 30 {
        return Err(AppError::Validation(format!(
            "Not enough camera frames for timing validation ({} frames, need >=30). \
             Let the camera run for a few seconds first.",
            cam_hw_ts.len()
        )));
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
    let dxgi_output = crate::monitor::find_dxgi_output(monitor_index)
                .map_err(|e| AppError::Hardware(e))?;
    let mut qpc_freq = 0i64;
    unsafe {
        let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut qpc_freq);
    }
    if qpc_freq == 0 {
        return Err(AppError::Hardware("QueryPerformanceFrequency failed".into()));
    }

    let warmup = 30;
    let sample_count = 150;
    let mut stim_timestamps = Vec::with_capacity(warmup + sample_count);
    for _ in 0..(warmup + sample_count) {
        unsafe {
            dxgi_output.WaitForVBlank()
                .map_err(|e| AppError::Hardware(format!("WaitForVBlank: {e}")))?;
        }
        let mut qpc = 0i64;
        unsafe { let _ = windows::Win32::System::Performance::QueryPerformanceCounter(&mut qpc); }
        stim_timestamps.push(((qpc as i128 * 1_000_000) / qpc_freq as i128) as i64);
    }

    let valid_stim = &stim_timestamps[warmup..];
    let stim_deltas: Vec<f64> = valid_stim.windows(2)
        .map(|w| (w[1] - w[0]) as f64)
        .collect();

    // Compute session parameters from snapshot + geometry.
    use openisi_stimulus::geometry::DisplayGeometry;

    let geometry = DisplayGeometry::new(
        snap.experiment_projection(),
        snap.viewing_distance_cm(),
        snap.horizontal_offset_deg(),
        snap.vertical_offset_deg(),
        monitor_width_cm, monitor_height_cm,
        monitor_width_px, monitor_height_px,
    );

    let envelope = snap.stimulus_envelope();
    let sweep_sec = match envelope {
        crate::params::Envelope::Bar => {
            let total_travel = geometry.visual_field_width_deg() + snap.stimulus_width_deg();
            total_travel / snap.sweep_speed_deg_per_sec()
        }
        crate::params::Envelope::Wedge => {
            360.0 / snap.rotation_speed_deg_per_sec()
        }
        crate::params::Envelope::Ring => {
            let total_travel = geometry.get_max_eccentricity_deg() + snap.stimulus_width_deg();
            total_travel / snap.expansion_speed_deg_per_sec()
        }
        crate::params::Envelope::Fullfield => 0.0,
    };

    let n_conditions = snap.conditions().len();
    let n_reps = snap.repetitions() as usize;
    let n_trials = n_conditions * n_reps;
    let inter_trial_sec = sweep_sec + snap.inter_stimulus_sec();

    let total_sweep_time = n_trials as f64 * sweep_sec;
    let total_inter_stim = if n_trials > 1 {
        (n_trials - 1) as f64 * snap.inter_stimulus_sec()
    } else { 0.0 };
    let total_inter_dir = if n_conditions > 1 {
        (n_conditions - 1) as f64 * snap.inter_direction_sec() * n_reps as f64
    } else { 0.0 };
    let session_sec = snap.baseline_start_sec()
        + total_sweep_time
        + total_inter_stim
        + total_inter_dir
        + snap.baseline_end_sec();

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
    let mut app = lock_state(&state, "validate_timing store result")?;
    app.session.timing_characterization = Some(tc.clone());

    Ok(tc)
    } // #[cfg(windows)]
}

/// Set the physical rotation of the stimulus monitor.
#[tauri::command]
pub fn set_monitor_rotation(state: State<'_, SharedState>, rotation_deg: f64) -> AppResult<()> {
    let app = lock_state(&state, "set_monitor_rotation")?;
    let mut reg = lock_state(&app.registry, "set_monitor_rotation registry")?;
    reg.set(crate::params::ParamId::MonitorRotationDeg, crate::params::ParamValue::F64(rotation_deg))
        .map_err(|e| AppError::Validation(e))?;
    if let Err(e) = reg.save_rig() {
        eprintln!("[params] Failed to save monitor rotation: {e}");
    }
    eprintln!("[params] monitor rotation set to {rotation_deg}°");
    Ok(())
}

/// Get the rig geometry (viewing distance).
#[tauri::command]
pub fn get_rig_geometry(state: State<'_, SharedState>) -> AppResult<serde_json::Value> {
    let app = lock_state(&state, "get_rig_geometry")?;
    let reg = lock_state(&app.registry, "get_rig_geometry registry")?;
    Ok(serde_json::json!({
        "viewing_distance_cm": reg.viewing_distance_cm(),
    }))
}

/// Set the viewing distance. Persists to rig.toml.
#[tauri::command]
pub fn set_viewing_distance(state: State<'_, SharedState>, distance_cm: f64) -> AppResult<()> {
    let app = lock_state(&state, "set_viewing_distance")?;
    let mut reg = lock_state(&app.registry, "set_viewing_distance registry")?;
    reg.set(crate::params::ParamId::ViewingDistanceCm, crate::params::ParamValue::F64(distance_cm))
        .map_err(|e| AppError::Validation(e))?;
    if let Err(e) = reg.save_rig() {
        eprintln!("[params] Failed to save viewing distance: {e}");
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
    let mut app = lock_state(&state, "set_display_dimensions")?;
    let display = app.session.selected_display.as_mut()
        .ok_or(AppError::Validation("No display selected".into()))?;
    display.width_cm = width_cm;
    display.height_cm = height_cm;
    display.physical_source = "user_override".into();
    eprintln!("[config] display dimensions set to {:.1}x{:.1}cm (user override)", width_cm, height_cm);
    Ok(())
}

/// Get ring overlay config.
#[tauri::command]
pub fn get_ring_overlay(state: State<'_, SharedState>) -> AppResult<serde_json::Value> {
    let app = lock_state(&state, "get_ring_overlay")?;
    let reg = lock_state(&app.registry, "get_ring_overlay registry")?;
    Ok(serde_json::json!({
        "enabled": reg.ring_overlay_enabled(),
        "radius_px": reg.ring_overlay_radius_px(),
        "center_x_px": reg.ring_overlay_center_x_px(),
        "center_y_px": reg.ring_overlay_center_y_px(),
    }))
}

/// Update ring overlay config. Persists to rig.toml.
#[tauri::command]
pub fn set_ring_overlay(
    state: State<'_, SharedState>,
    enabled: bool,
    radius_px: u32,
    center_x_px: u32,
    center_y_px: u32,
) -> AppResult<()> {
    use crate::params::{ParamId, ParamValue};
    let app = lock_state(&state, "set_ring_overlay")?;
    let mut reg = lock_state(&app.registry, "set_ring_overlay registry")?;
    reg.batch(|r| -> Result<(), String> {
        r.set(ParamId::RingOverlayEnabled, ParamValue::Bool(enabled))?;
        r.set(ParamId::RingOverlayRadiusPx, ParamValue::U32(radius_px))?;
        r.set(ParamId::RingOverlayCenterXPx, ParamValue::U32(center_x_px))?;
        r.set(ParamId::RingOverlayCenterYPx, ParamValue::U32(center_y_px))?;
        Ok(())
    }).map_err(|e| AppError::Validation(e))?;
    if let Err(e) = reg.save_rig() {
        eprintln!("[params] Failed to save ring overlay: {e}");
    }
    Ok(())
}

/// Enumerate available cameras — results arrive via camera:enumerated event.
#[tauri::command]
pub fn enumerate_cameras(state: State<'_, SharedState>) -> AppResult<()> {
    let app = lock_state(&state, "enumerate_cameras")?;
    let tx = app.threads.camera_tx.as_ref()
        .ok_or(AppError::NotAvailable("Camera thread not running".into()))?;
    tx.send(CameraCmd::Enumerate)
        .map_err(|e| AppError::Hardware(format!("Failed to send enumerate command: {e}")))?;
    Ok(())
}

/// Connect to a specific camera by index.
#[tauri::command]
pub fn connect_camera(state: State<'_, SharedState>, camera_index: u16) -> AppResult<()> {
    let app = lock_state(&state, "connect_camera")?;
    let reg = lock_state(&app.registry, "connect_camera registry")?;
    let exposure_us = reg.camera_exposure_us();
    let binning = reg.camera_binning();
    drop(reg);
    let tx = app.threads.camera_tx.as_ref()
        .ok_or(AppError::NotAvailable("Camera thread not running".into()))?;
    tx.send(CameraCmd::Connect { index: camera_index, exposure_us, binning })
        .map_err(|e| AppError::Hardware(format!("Failed to send connect command: {e}")))?;
    Ok(())
}

/// Disconnect from the camera.
#[tauri::command]
pub fn disconnect_camera(state: State<'_, SharedState>) -> AppResult<()> {
    let app = lock_state(&state, "disconnect_camera")?;
    let tx = app.threads.camera_tx.as_ref()
        .ok_or(AppError::NotAvailable("Camera thread not running".into()))?;
    tx.send(CameraCmd::Disconnect)
        .map_err(|e| AppError::Hardware(format!("Failed to send disconnect command: {e}")))?;
    Ok(())
}

/// Capture the current camera frame as a 16-bit PNG anatomical reference.
#[tauri::command]
pub fn capture_anatomical(state: State<'_, SharedState>, path: String) -> AppResult<String> {
    let mut app = lock_state(&state, "capture_anatomical")?;
    let cache = app.latest_camera_frame.as_ref()
        .ok_or(AppError::NotAvailable("No camera frame available".into()))?;

    let width = cache.width;
    let height = cache.height;
    let pixels = cache.pixels.clone();

    // Store as u8 ndarray for embedding in .oisi later.
    let min_val = pixels.iter().copied().min().unwrap_or(0);
    let max_val = pixels.iter().copied().max().unwrap_or(0);
    let range = (max_val - min_val).max(1) as f64;
    let u8_pixels: Vec<u8> = pixels.iter()
        .map(|&p| ((p - min_val) as f64 / range * 255.0) as u8)
        .collect();
    let anat_array = ndarray::Array2::from_shape_vec(
        (height as usize, width as usize), u8_pixels
    ).map_err(|e| AppError::Hardware(format!("Camera frame shape error: {e}")))?;
    app.anatomical_image = Some(anat_array);

    // Encode as 16-bit grayscale PNG for external file.
    let mut png_data = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_data, width, height);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Sixteen);
        let mut writer = encoder.write_header()
            .map_err(|e| AppError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("PNG header error: {e}"),
            )))?;
        let bytes: Vec<u8> = pixels.iter()
            .flat_map(|&p: &u16| p.to_be_bytes())
            .collect();
        writer.write_image_data(&bytes)
            .map_err(|e| AppError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("PNG write error: {e}"),
            )))?;
    }

    std::fs::write(&path, &png_data)?;

    eprintln!("[commands] anatomical saved: {}x{} to {path}", width, height);
    Ok(path)
}

/// Set camera exposure in microseconds. Persists to rig.toml.
#[tauri::command]
pub fn set_exposure(state: State<'_, SharedState>, exposure_us: u32) -> AppResult<()> {
    let mut app = lock_state(&state, "set_exposure")?;
    let tx = app.threads.camera_tx.as_ref()
        .ok_or(AppError::NotAvailable("Camera thread not running".into()))?;
    tx.send(CameraCmd::SetExposure(exposure_us))
        .map_err(|e| AppError::Hardware(format!("Failed to send exposure command: {e}")))?;
    // Persist to registry.
    {
        let mut reg = lock_state(&app.registry, "set_exposure registry")?;
        reg.set(crate::params::ParamId::CameraExposureUs, crate::params::ParamValue::U32(exposure_us))
            .map_err(|e| AppError::Validation(e))?;
        if let Err(e) = reg.save_rig() {
            eprintln!("[params] Failed to save exposure: {e}");
        }
    }
    // Keep session camera info in sync.
    if let Some(ref mut cam) = app.session.camera {
        cam.exposure_us = exposure_us;
    }
    Ok(())
}
