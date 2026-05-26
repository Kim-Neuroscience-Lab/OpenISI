//! OpenISI headless CLI — runs acquisitions without the GUI.
//!
//! Uses the same backend code as the Tauri app: same config, same threads,
//! same export pipeline. For testing, validation, and scripted acquisition.
//!
//! Analysis commands (analyze, inspect, import, import-session) work on all
//! platforms. Hardware commands (info, validate-display, validate-timing,
//! acquire) require Windows (DXGI, QPC, PCO SDK).

use std::path::PathBuf;

use openisi_lib::error::{AppError, AppResult};
use openisi_lib::params::Registry;

// Windows-only imports for hardware commands.
#[cfg(windows)]
use std::time::{Duration, Instant};
#[cfg(windows)]
use openisi_lib::export::SweepSchedule;
#[cfg(windows)]
use openisi_lib::messages::*;
#[cfg(windows)]
use openisi_lib::monitor;

/// Public entry point. Thin wrapper around `try_main()` that prints any
/// error in a single readable line and exits non-zero. No panic, no
/// stack trace — same shape as the Tauri `run()` wrapper.
fn main() {
    if let Err(e) = try_main() {
        eprintln!("openisi: {e}");
        std::process::exit(1);
    }
}

fn try_main() -> AppResult<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    match args[1].as_str() {
        // Hardware commands — Windows only.
        "info" => cmd_info(),
        "validate-display" => cmd_validate_display(&args[2..]),
        "validate-timing" => cmd_validate_timing(&args[2..]),
        "acquire" => cmd_acquire(&args[2..]),

        // Software commands — all platforms.
        "analyze" => cmd_analyze(&args[2..]),
        "migrate" => cmd_migrate(&args[2..]),
        "inspect" => cmd_inspect(&args[2..]),
        "import" => cmd_import(&args[2..]),
        "import-samples" => cmd_import_samples(&args[2..]),
        "import-session" => cmd_import_session(&args[2..]),
        "test-read" => cmd_test_read(&args[2..]),
        "dump-h5" => cmd_dump_h5(&args[2..]),
        other => {
            print_usage();
            Err(AppError::Validation(format!("unknown command: {other}")))
        }
    }
}

fn print_usage() {
    eprintln!("OpenISI Headless CLI");
    eprintln!();
    eprintln!("Usage: openisi-headless <command> [options]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  info                    Show detected hardware and config");
    eprintln!("  validate-display [idx]  Validate display timing (default: monitor 1)");
    eprintln!("  validate-timing [secs]  Measure camera+stimulus rates simultaneously (default: 3s)");
    eprintln!("  acquire [seconds]       Run acquisition for N seconds (default: 10)");
    eprintln!("  analyze <file.oisi>     Run analysis on an .oisi file");
    eprintln!("                          Flags: --figures [dir]    standard figure export");
    eprintln!("                                 --threshold-sweep  also emit VFS-threshold sweep grids");
    eprintln!("                                 --compare-methods  also re-run pipeline per method-variant");
    eprintln!("                                                    per multi-variant stage and stitch grids");
    eprintln!("  migrate <file.oisi>     Upgrade a pre-2026 .oisi's /analysis_params");
    eprintln!("                          attribute to the current registry-tree schema.");
    eprintln!("                          Idempotent — re-runs on current-schema files");
    eprintln!("                          are a no-op with a clear message.");
    eprintln!("  inspect <file.oisi>     Inspect .oisi file contents");
    eprintln!("  import <dir>            Import SNLC .mat directory to .oisi");
    eprintln!("  import-samples          Download SNLC sample bundle and import each subject");
    eprintln!("                          into the configured data directory");
}

// ═══════════════════════════════════════════════════════════════════════
// Config loading
// ═══════════════════════════════════════════════════════════════════════

fn load_registry() -> AppResult<Registry> {
    let exe_dir = std::env::current_exe()
        .map_err(|e| AppError::Config(format!("locate current executable: {e}")))?
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| AppError::Config("current executable has no parent directory".into()))?;

    let candidates = vec![
        exe_dir.join("config"),
        exe_dir.join("../config"),
        exe_dir.join("../../config"),
    ];
    let candidate_paths: Vec<String> = candidates.iter().map(|p| p.display().to_string()).collect();

    let config_dir = candidates.into_iter()
        .find(|p| p.join("rig.toml").exists())
        .ok_or_else(|| AppError::Config(format!(
            "cannot find config directory with rig.toml. Searched: {}",
            candidate_paths.join(", ")
        )))?;

    // Behavior-preserving placeholder: shipped == user == config_dir,
    // pending proper dev/prod path resolution.
    let mut registry = Registry::new(&config_dir, &config_dir);
    registry.load_rig().map_err(|e| AppError::Config(
        format!("load rig config from {}: {e}", config_dir.display())
    ))?;
    registry.load_analysis().map_err(|e| AppError::Config(
        format!("load analysis config from {}: {e}", config_dir.display())
    ))?;
    registry.load_experiment().map_err(|e| AppError::Config(
        format!("load experiment from {}: {e}", config_dir.display())
    ))?;

    Ok(registry)
}

// ═══════════════════════════════════════════════════════════════════════
// Hardware commands — Windows only
// ═══════════════════════════════════════════════════════════════════════

#[cfg(not(windows))]
fn cmd_info() -> AppResult<()> {
    Err(AppError::NotAvailable(
        "'info' requires Windows (DXGI, QPC, PCO SDK)".into(),
    ))
}

#[cfg(not(windows))]
fn cmd_validate_display(_args: &[String]) -> AppResult<()> {
    Err(AppError::NotAvailable(
        "'validate-display' requires Windows (DXGI WaitForVBlank)".into(),
    ))
}

#[cfg(not(windows))]
fn cmd_validate_timing(_args: &[String]) -> AppResult<()> {
    Err(AppError::NotAvailable(
        "'validate-timing' requires Windows (DXGI, QPC, PCO SDK)".into(),
    ))
}

#[cfg(not(windows))]
fn cmd_acquire(_args: &[String]) -> AppResult<()> {
    Err(AppError::NotAvailable(
        "'acquire' requires Windows (DXGI, QPC, PCO SDK)".into(),
    ))
}

#[cfg(windows)]
fn cmd_info() -> AppResult<()> {
    let reg = load_registry()?;
    let snap = reg.snapshot();

    println!("=== Rig Config ===");
    println!("Camera: exposure={}µs binning={}",
        snap.camera_exposure_us(), snap.camera_binning());
    println!("Geometry: viewing_distance={}cm", snap.viewing_distance_cm());
    println!("Display: target_fps={} rotation={}°",
        snap.target_stimulus_fps(), snap.monitor_rotation_deg());

    println!();
    println!("=== Experiment ===");
    println!("Envelope: {:?}", snap.stimulus_envelope());
    println!("Carrier: {:?}", snap.stimulus_carrier());
    println!("Conditions: {:?}", snap.conditions());
    println!("Repetitions: {}", snap.repetitions());
    println!("Baselines: {}/{}s", snap.baseline_start_sec(), snap.baseline_end_sec());

    println!();
    println!("=== Monitors ===");
    let monitors = monitor::detect_monitors();
    for m in &monitors {
        println!("  [{}] {} {}x{} @{}Hz {:.1}x{:.1}cm at ({},{})",
            m.index, m.name, m.width_px, m.height_px, m.refresh_hz,
            m.width_cm, m.height_cm, m.position.0, m.position.1);
    }

    println!();
    println!("=== Camera ===");
    let sdk = match pco_sdk::Sdk::load() {
        Ok(sdk) => {
            println!("PCO SDK loaded");
            sdk
        }
        Err(e) => {
            println!("PCO SDK not available: {e}");
            return Ok(());
        }
    };
    let cameras = sdk.enumerate_cameras(10);
    if cameras.is_empty() {
        println!("  No cameras found");
    } else {
        for c in &cameras {
            println!("  [{}] {} {}x{} {:.1}fps", c.index, c.name, c.width, c.height, c.max_fps);
        }
        if let Ok(cam) = sdk.open_camera(cameras[0].index) {
            let info = cam.info();
            println!("  Pixel rates: {:?}", info.pixel_rates);
            println!("  Exposure range: {}ns .. {}ms", info.min_exposure_ns, info.max_exposure_ms);
            let (max_h, step_h, max_v, step_v) = cam.available_binning();
            println!("  Binning: max {}x{}, stepping h={} v={}", max_h, max_v, step_h, step_v);
        }
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// validate-display
// ═══════════════════════════════════════════════════════════════════════

#[cfg(windows)]
fn cmd_validate_display(args: &[String]) -> AppResult<()> {
    let reg = load_registry()?;
    let snap = reg.snapshot();

    let monitors = monitor::detect_monitors();
    let idx: usize = args.first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(if monitors.len() > 1 { 1 } else { 0 });

    if idx >= monitors.len() {
        eprintln!("Monitor index {} out of range (have {})", idx, monitors.len());
        return Ok(());
    }

    let m = &monitors[idx];
    println!("Validating monitor [{}] {} @{}Hz...", idx, m.name, m.refresh_hz);

    let dxgi_output = match monitor::find_dxgi_output(idx) {
        Ok(o) => o,
        Err(e) => { eprintln!("Failed to find DXGI output: {e}"); return Ok(()); }
    };

    let mut qpc_freq = 0i64;
    unsafe { let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut qpc_freq); }

    let sample_count = snap.display_validation_sample_count();
    let warmup = 30u32;
    let total = sample_count + warmup;
    let mut timestamps = Vec::with_capacity(total as usize);

    for _ in 0..total {
        unsafe { let _ = dxgi_output.WaitForVBlank(); }
        let mut qpc = 0i64;
        unsafe { let _ = windows::Win32::System::Performance::QueryPerformanceCounter(&mut qpc); }
        timestamps.push(qpc);
    }

    let valid = &timestamps[warmup as usize..];
    let deltas_us: Vec<f64> = valid.windows(2)
        .map(|w| (w[1] - w[0]) as f64 * 1_000_000.0 / qpc_freq as f64)
        .collect();

    let n = deltas_us.len() as f64;
    let mean = deltas_us.iter().sum::<f64>() / n;
    let hz = 1_000_000.0 / mean;
    let variance = deltas_us.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / n;
    let jitter = variance.sqrt();
    let ci95 = 1.96 * jitter / n.sqrt();
    let ci95_hz = (1_000_000.0 / (mean - ci95) - 1_000_000.0 / (mean + ci95)) / 2.0;

    let mismatch = (hz - m.refresh_hz as f64).abs() / m.refresh_hz as f64;

    println!("Measured: {:.2} Hz (reported: {} Hz)", hz, m.refresh_hz);
    println!("Jitter:   {:.1} µs", jitter);
    println!("95% CI:   ±{:.3} Hz", ci95_hz);
    println!("Samples:  {} (+ {} warmup)", sample_count, warmup);
    if mismatch > 0.05 {
        println!("WARNING: Measured differs from reported by {:.1}%", mismatch * 100.0);
    } else {
        println!("Match:    OK ({:.1}% difference)", mismatch * 100.0);
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// validate-timing
// ═══════════════════════════════════════════════════════════════════════

#[cfg(windows)]
fn cmd_validate_timing(args: &[String]) -> AppResult<()> {
    let measure_sec: f64 = args.first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3.0);

    let reg = load_registry()?;
    let snap = reg.snapshot();

    let monitors = monitor::detect_monitors();
    let stim_idx = if monitors.len() > 1 { 1 } else { 0 };
    let mon = &monitors[stim_idx];

    println!("Measuring timing for {:.1}s...", measure_sec);
    println!("Camera: connecting...");

    // Open camera and start recording.
    let sdk = pco_sdk::Sdk::load()
        .map_err(|e| AppError::Hardware(format!("PCO SDK required: {e}")))?;
    let cameras = sdk.enumerate_cameras(10);
    if cameras.is_empty() {
        eprintln!("No cameras found");
        return Ok(());
    }
    let mut camera = sdk.open_camera(cameras[0].index)
        .map_err(|e| AppError::Hardware(format!("Failed to open camera: {e}")))?;
    let _rate = camera.set_max_pixel_rate()
        .map_err(|e| AppError::Hardware(format!("Failed to set pixel rate: {e}")))?;
    let binning = snap.camera_binning();
    if binning > 1 {
        camera.set_binning(binning, binning)
            .map_err(|e| AppError::Hardware(format!("Failed to set binning: {e}")))?;
    }
    camera.set_timestamp_binary()
        .map_err(|e| AppError::Hardware(format!("Failed to set timestamp mode: {e}")))?;
    camera.set_exposure_us(snap.camera_exposure_us())
        .map_err(|e| AppError::Hardware(format!("Failed to set exposure: {e}")))?;
    if let Err(e) = camera.arm() {
        eprintln!("Failed to arm camera: {e}");
        return Ok(());
    }
    println!("Camera: {}x{}", camera.width, camera.height);

    let mut recorder = camera.create_recorder(10)
        .map_err(|e| AppError::Hardware(format!("Failed to create recorder: {e}")))?;
    recorder.start()
        .map_err(|e| AppError::Hardware(format!("Failed to start recording: {e}")))?;

    // Wait for first frame.
    let deadline = std::time::Instant::now() + Duration::from_millis(
        snap.camera_first_frame_timeout_ms() as u64
    );
    loop {
        if std::time::Instant::now() > deadline {
            eprintln!("Timed out waiting for first camera frame");
            return Ok(());
        }
        match recorder.get_latest_frame() {
            Ok(Some(_)) => break,
            Ok(None) => std::thread::sleep(Duration::from_millis(
                snap.camera_first_frame_poll_ms() as u64
            )),
            Err(e) => { eprintln!("Frame error: {e}"); return Ok(()); }
        }
    }

    // Start stimulus thread.
    let (stim_cmd_tx, stim_cmd_rx) = crossbeam_channel::unbounded();
    let (stim_evt_tx, stim_evt_rx) = crossbeam_channel::unbounded();
    let bg_lum = snap.background_luminance();
    let preview_width_px = snap.preview_width_px();
    let preview_interval_ms = snap.preview_interval_ms();
    let preview_cycle_sec = snap.preview_cycle_sec();
    let idle_sleep_ms = snap.idle_sleep_ms();
    let fps_window_frames = snap.fps_window_frames();
    let drop_detection_warmup_frames = snap.drop_detection_warmup_frames();
    let mon_idx = mon.index;
    let mon_w = mon.width_px;
    let mon_h = mon.height_px;
    let mon_pos = mon.position;

    std::thread::Builder::new()
        .name("stimulus".into())
        .spawn(move || {
            openisi_lib::stimulus_thread::run(
                stim_cmd_rx, stim_evt_tx, mon_idx, mon_w, mon_h, mon_pos,
                preview_width_px, preview_interval_ms, preview_cycle_sec,
                idle_sleep_ms, fps_window_frames, drop_detection_warmup_frames,
                bg_lum,
            );
        })
        .map_err(|e| AppError::Hardware(format!("Failed to spawn stimulus thread: {e}")))?;

    // Wait for ready.
    loop {
        match stim_evt_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(StimulusEvt::Ready) => break,
            Ok(_) => {}
            Err(_) => { eprintln!("Stimulus thread timeout"); return Ok(()); }
        }
    }

    // Start a preview to get stimulus vsync running.
    stim_cmd_tx.send(StimulusCmd::Preview(PreviewCommand {
        snapshot: snap.clone(),
        monitor: openisi_lib::session::MonitorInfo {
            index: mon.index, name: mon.name.clone(),
            width_px: mon.width_px, height_px: mon.height_px,
            width_cm: mon.width_cm, height_cm: mon.height_cm,
            refresh_hz: mon.refresh_hz, position: mon.position,
            physical_source: mon.physical_source.clone(),
        },
    })).map_err(|e| AppError::Hardware(format!("Failed to start preview: {e}")))?;

    println!("Collecting timestamps...");

    let qpc_freq = {
        let mut f = 0i64;
        unsafe { let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut f); }
        f
    };

    // Collect camera hardware timestamps.
    let mut cam_hw_timestamps: Vec<i64> = Vec::new();
    let mut cam_sys_timestamps: Vec<i64> = Vec::new();
    let start = std::time::Instant::now();

    while start.elapsed() < Duration::from_secs_f64(measure_sec) {
        match recorder.get_latest_frame() {
            Ok(Some(frame)) => {
                cam_hw_timestamps.push(frame.timestamp.to_us_since_midnight());
                let mut qpc = 0i64;
                unsafe { let _ = windows::Win32::System::Performance::QueryPerformanceCounter(&mut qpc); }
                cam_sys_timestamps.push(((qpc as i128 * 1_000_000) / qpc_freq as i128) as i64);
            }
            Ok(None) => {}
            Err(e) => { eprintln!("Frame error: {e}"); break; }
        }
        std::thread::sleep(Duration::from_millis(snap.camera_poll_interval_ms() as u64));
    }

    // Stop.
    let _ = recorder.stop();
    stim_cmd_tx.send(StimulusCmd::StopPreview).ok();
    stim_cmd_tx.send(StimulusCmd::Shutdown).ok();

    let dxgi_output = match monitor::find_dxgi_output(stim_idx) {
        Ok(o) => o,
        Err(e) => { eprintln!("DXGI: {e}"); return Ok(()); }
    };
    let mut stim_timestamps: Vec<i64> = Vec::new();
    for _ in 0..200 {
        unsafe { let _ = dxgi_output.WaitForVBlank(); }
        let mut qpc = 0i64;
        unsafe { let _ = windows::Win32::System::Performance::QueryPerformanceCounter(&mut qpc); }
        stim_timestamps.push(((qpc as i128 * 1_000_000) / qpc_freq as i128) as i64);
    }

    // Compute deltas.
    let cam_deltas: Vec<f64> = cam_hw_timestamps.windows(2)
        .map(|w| (w[1] - w[0]) as f64)
        .collect();
    let stim_deltas: Vec<f64> = stim_timestamps.windows(2)
        .map(|w| (w[1] - w[0]) as f64)
        .collect();

    if cam_deltas.is_empty() || stim_deltas.is_empty() {
        eprintln!("Not enough samples collected");
        return Ok(());
    }

    let offsets: Vec<f64> = cam_sys_timestamps.iter().zip(cam_hw_timestamps.iter())
        .map(|(&sys, &hw)| (sys - hw) as f64)
        .collect();
    let offset_mean = offsets.iter().sum::<f64>() / offsets.len() as f64;
    let offset_variance = offsets.iter()
        .map(|o| (o - offset_mean).powi(2))
        .sum::<f64>() / offsets.len() as f64;
    let clock_offset_uncertainty_us = offset_variance.sqrt();

    use openisi_stimulus::geometry::DisplayGeometry;

    let geometry = DisplayGeometry::new(
        snap.experiment_projection(),
        snap.viewing_distance_cm(),
        snap.horizontal_offset_deg(),
        snap.vertical_offset_deg(),
        mon.width_cm, mon.height_cm,
        mon.width_px, mon.height_px,
    );

    let envelope = snap.stimulus_envelope();
    let sweep_sec = match envelope {
        openisi_lib::params::Envelope::Bar => {
            let total_travel = geometry.visual_field_width_deg() + snap.stimulus_width_deg();
            total_travel / snap.sweep_speed_deg_per_sec()
        }
        openisi_lib::params::Envelope::Wedge => {
            360.0 / snap.rotation_speed_deg_per_sec()
        }
        openisi_lib::params::Envelope::Ring => {
            let total_travel = geometry.get_max_eccentricity_deg() + snap.stimulus_width_deg();
            total_travel / snap.expansion_speed_deg_per_sec()
        }
        openisi_lib::params::Envelope::Fullfield => 0.0,
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

    println!("Sweep duration: {:.3}s ({:?} envelope)", sweep_sec, envelope);
    println!("Session duration: {:.1}s ({} trials)", session_sec, n_trials);

    let params = openisi_lib::timing::TimingParams {
        n_trials,
        inter_trial_sec,
        session_duration_sec: session_sec,
    };

    let tc = openisi_lib::timing::characterize_timing(
        &cam_deltas,
        &stim_deltas,
        clock_offset_uncertainty_us,
        &params,
    );

    println!();
    println!("=== Timing Characterization ===");
    print!("{tc}");
    println!("Clock offset: mean={:.1}µs, uncertainty={:.1}µs",
        offset_mean, clock_offset_uncertainty_us);
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// acquire
// ═══════════════════════════════════════════════════════════════════════

#[cfg(windows)]
fn cmd_acquire(args: &[String]) -> AppResult<()> {
    let duration_sec: f64 = args.first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10.0);

    let reg = load_registry()?;
    let snap = reg.snapshot();

    let monitors = monitor::detect_monitors();
    let stim_idx = if monitors.len() > 1 { 1 } else { 0 };
    let monitor = &monitors[stim_idx];

    println!("Acquiring for {:.1}s on monitor [{}] {}", duration_sec, stim_idx, monitor.name);
    println!("Experiment: {:?} {:?}", snap.stimulus_envelope(), snap.stimulus_carrier());

    // Camera setup.
    let sdk = pco_sdk::Sdk::load()
        .map_err(|e| AppError::Hardware(format!("PCO SDK required: {e}")))?;
    let cameras = sdk.enumerate_cameras(10);
    if cameras.is_empty() {
        eprintln!("No cameras found");
        return Ok(());
    }
    println!("Camera: {} {}x{}", cameras[0].name, cameras[0].width, cameras[0].height);

    let mut camera = sdk.open_camera(cameras[0].index)
        .map_err(|e| AppError::Hardware(format!("Failed to open camera: {e}")))?;
    let _rate = camera.set_max_pixel_rate()
        .map_err(|e| AppError::Hardware(format!("Failed to set pixel rate: {e}")))?;
    let binning = snap.camera_binning();
    if binning > 1 {
        if !camera.is_valid_binning(binning) {
            let (max_h, step_h, max_v, step_v) = camera.available_binning();
            eprintln!("Binning {}x{} not supported. Camera supports max {}x{} (stepping h={} v={})",
                binning, binning, max_h, max_v, step_h, step_v);
            return Ok(());
        }
        camera.set_binning(binning, binning)
            .map_err(|e| AppError::Hardware(format!("Failed to set binning: {e}")))?;
    }
    camera.set_timestamp_binary()
        .map_err(|e| AppError::Hardware(format!("Failed to set timestamp mode: {e}")))?;
    camera.set_exposure_us(snap.camera_exposure_us())
        .map_err(|e| AppError::Hardware(format!("Failed to set exposure: {e}")))?;
    if let Err(e) = camera.arm() {
        eprintln!("Failed to arm camera: {e}");
        eprintln!("This may indicate the binning/pixel rate/exposure combination is not supported by the USB interface.");
        return Ok(());
    }

    let cam_w = camera.width;
    let cam_h = camera.height;
    println!("Camera armed: {}x{}, exposure {}µs", cam_w, cam_h, snap.camera_exposure_us());

    // Stimulus thread.
    let (stim_cmd_tx, stim_cmd_rx) = crossbeam_channel::unbounded();
    let (stim_evt_tx, stim_evt_rx) = crossbeam_channel::unbounded();

    let bg_lum = snap.background_luminance();
    let preview_width_px = snap.preview_width_px();
    let preview_interval_ms = snap.preview_interval_ms();
    let preview_cycle_sec = snap.preview_cycle_sec();
    let idle_sleep_ms = snap.idle_sleep_ms();
    let fps_window_frames = snap.fps_window_frames();
    let drop_detection_warmup_frames = snap.drop_detection_warmup_frames();
    let mon_idx = monitor.index;
    let mon_w = monitor.width_px;
    let mon_h = monitor.height_px;
    let mon_pos = monitor.position;

    std::thread::Builder::new()
        .name("stimulus".into())
        .spawn(move || {
            openisi_lib::stimulus_thread::run(
                stim_cmd_rx, stim_evt_tx, mon_idx, mon_w, mon_h, mon_pos,
                preview_width_px, preview_interval_ms, preview_cycle_sec,
                idle_sleep_ms, fps_window_frames, drop_detection_warmup_frames,
                bg_lum,
            );
        })
        .map_err(|e| AppError::Hardware(format!("Failed to spawn stimulus thread: {e}")))?;

    // Wait for stimulus ready.
    loop {
        match stim_evt_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(StimulusEvt::Ready) => { println!("Stimulus thread ready"); break; }
            Ok(_) => {}
            Err(_) => { eprintln!("Stimulus thread did not become ready in 10s"); return Ok(()); }
        }
    }

    // Start acquisition.
    let acq_cmd = AcquisitionCommand {
        snapshot: snap.clone(),
        monitor: openisi_lib::session::MonitorInfo {
            index: monitor.index,
            name: monitor.name.clone(),
            width_px: monitor.width_px,
            height_px: monitor.height_px,
            width_cm: monitor.width_cm,
            height_cm: monitor.height_cm,
            refresh_hz: monitor.refresh_hz,
            position: monitor.position,
            physical_source: monitor.physical_source.clone(),
        },
        measured_refresh_hz: monitor.refresh_hz as f64,
    };

    stim_cmd_tx.send(StimulusCmd::StartAcquisition(acq_cmd))
        .map_err(|e| AppError::Hardware(format!("Failed to start acquisition: {e}")))?;
    println!("Acquisition started");

    // Camera recording.
    let mut recorder = camera.create_recorder(10)
        .map_err(|e| AppError::Hardware(format!("Failed to create recorder: {e}")))?;
    recorder.start()
        .map_err(|e| AppError::Hardware(format!("Failed to start recording: {e}")))?;

    // Wait for first frame.
    let deadline = Instant::now() + Duration::from_millis(snap.camera_first_frame_timeout_ms() as u64);
    loop {
        if Instant::now() > deadline {
            eprintln!("Timed out waiting for first camera frame");
            return Ok(());
        }
        match recorder.get_latest_frame() {
            Ok(Some(_)) => break,
            Ok(None) => std::thread::sleep(Duration::from_millis(snap.camera_first_frame_poll_ms() as u64)),
            Err(e) => { eprintln!("Frame read error: {e}"); return Ok(()); }
        }
    }
    println!("Camera streaming");

    // Accumulate frames.
    let mut accumulator = openisi_lib::export::AcquisitionAccumulator::new();
    accumulator.start(cam_w, cam_h);

    let qpc_freq = {
        let mut f = 0i64;
        unsafe { let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut f); }
        f
    };

    let start = Instant::now();
    let mut frame_count = 0u64;

    while start.elapsed() < Duration::from_secs_f64(duration_sec) {
        match recorder.get_latest_frame() {
            Ok(Some(frame)) => {
                let sys_us = {
                    let mut qpc = 0i64;
                    unsafe { let _ = windows::Win32::System::Performance::QueryPerformanceCounter(&mut qpc); }
                    ((qpc as i128 * 1_000_000) / qpc_freq as i128) as i64
                };
                accumulator.add_frame(
                    frame.pixels.clone(),
                    frame.timestamp.to_us_since_midnight(),
                    sys_us,
                    frame.image_number as u64,
                );
                frame_count += 1;
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("Frame error: {e}");
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(snap.camera_poll_interval_ms() as u64));
    }

    // Stop.
    let _ = recorder.stop();
    stim_cmd_tx.send(StimulusCmd::Stop)
        .map_err(|e| AppError::Hardware(format!("Failed to stop: {e}")))?;

    // Drain stimulus events to get the dataset.
    let mut stim_dataset = None;
    let mut sweep_schedule = SweepSchedule {
        sweep_sequence: Vec::new(),
        sweep_start_us: Vec::new(),
        sweep_end_us: Vec::new(),
    };

    loop {
        match stim_evt_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(StimulusEvt::Complete(result)) => {
                sweep_schedule = SweepSchedule {
                    sweep_sequence: result.sweep_sequence,
                    sweep_start_us: result.sweep_start_us,
                    sweep_end_us: result.sweep_end_us,
                };
                stim_dataset = Some(result.dataset);
                break;
            }
            Ok(StimulusEvt::Stopped) => break,
            Ok(_) => {}
            Err(_) => { eprintln!("Timeout waiting for stimulus completion"); break; }
        }
    }

    stim_cmd_tx.send(StimulusCmd::Shutdown).ok();
    let elapsed = start.elapsed();
    println!("Captured {} camera frames in {:.1}s ({:.1} fps)",
        frame_count, elapsed.as_secs_f64(), frame_count as f64 / elapsed.as_secs_f64());

    // Save.
    let camera_data = accumulator.finish();
    let data_dir = snap.data_directory().to_string();
    let output_dir = if data_dir.is_empty() {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    } else {
        PathBuf::from(data_dir)
    };
    let _ = std::fs::create_dir_all(&output_dir);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let output_path = output_dir.join(format!("acquisition_{ts}.oisi"));

    if let Some(ds) = &stim_dataset {
        match openisi_lib::export::write_oisi(
            &output_path,
            ds,
            camera_data,
            &snap,
            None,
            &sweep_schedule,
            None,
            None,
            None,
            false,
        ) {
            Ok(summary) => println!("{summary}"),
            Err(e) => eprintln!("Export failed: {e}"),
        }
    } else {
        eprintln!("No stimulus dataset — skipping export");
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Software commands — all platforms
// ═══════════════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════════════
// analyze
// ═══════════════════════════════════════════════════════════════════════

fn cmd_analyze(args: &[String]) -> AppResult<()> {
    if args.is_empty() {
        eprintln!("Usage: openisi-headless analyze <file.oisi>");
        return Ok(());
    }

    let registry = load_registry()?;
    let path = std::path::Path::new(&args[0]);

    // Pre-2026 schema files are refused with a clear migration message;
    // there is no implicit conversion.
    if isi_analysis::io::is_pre_2026_analysis_params(path)? {
        return Err(AppError::Validation(format!(
            "{} has pre-2026 /analysis_params schema. Run `oisi migrate {}` first.",
            path.display(),
            path.display(),
        )));
    }

    let snapshot = registry.snapshot();
    let params = isi_analysis::bridge::analysis_params_from_snapshot(&snapshot);
    let params_tree = snapshot.to_json_for_target(openisi_params::PersistTarget::Analysis);

    let progress = isi_analysis::SilentProgress;
    let cancel = std::sync::atomic::AtomicBool::new(false);

    println!("Analyzing {}...", path.display());
    isi_analysis::analyze(path, &params, &progress, &cancel)?;
    // Stamp the registry tree into /analysis_params for provenance.
    isi_analysis::io::write_analysis_params_attr(path, &params_tree)?;
    println!("Analysis complete");
    // Export figures: `analyze <file> --figures [dir]`.
    //
    // No arg: auto-tag into <repo_root>/dev_figures/<stem>/<tag>/.
    // Explicit <dir>: use it verbatim (one-off comparison, no auto-tag).
    {
            let figures_dir = if let Some(flag_pos) = args.iter().position(|a| a == "--figures") {
                let custom_dir = args
                    .get(flag_pos + 1)
                    .filter(|s| !s.starts_with("-"))
                    .map(|s| std::path::PathBuf::from(s));
                Some(custom_dir.unwrap_or_else(|| default_figures_dir(path, &params)))
            } else {
                None
            };
            if let Some(ref dir) = figures_dir {
                export_all_figures(path, &dir.to_string_lossy());
                write_meta_json(dir, path, &params);
                if args.iter().any(|a| a == "--threshold-sweep") {
                    export_threshold_sweep_grids(path, dir, &params);
                }
                if args.iter().any(|a| a == "--compare-methods") {
                    compare_method_variants(path, &params, dir);
                }
            }
    }
    Ok(())
}

/// Per-stage method-variant comparison. For each pipeline stage that has
/// multiple implemented variants in the current codebase, re-run the
/// pipeline once per variant (defaults for all other stages), render
/// the affected figures into a per-variant subdirectory, and composite
/// a grid PNG per figure showing the variants side-by-side.
///
/// Variants that fail at resolve time (e.g. `CortexSource::Reliability`
/// on cycle-averaged imports without per-cycle data) are skipped with
/// a log line. `unimplemented!()` stubs panic — those variants are
/// excluded from the iteration list, not retried.
///
/// Outputs land in `<figures_dir>/compare/<stage>/<variant>/` (per-
/// variant figures) plus `<figures_dir>/compare/<stage>/grid_<figure>.png`
/// (composite grids).
fn compare_method_variants(
    oisi_path: &std::path::Path,
    base_params: &isi_analysis::AnalysisParams,
    figures_dir: &std::path::Path,
) {
    use isi_analysis::methods::CortexSource;

    let compare_dir = figures_dir.join("compare");
    if let Err(e) = std::fs::create_dir_all(&compare_dir) {
        eprintln!("[compare] Failed to create compare dir: {e}");
        return;
    }

    println!("Comparing method variants per stage...");

    // Build all CortexSource variants using SSoT-sourced tunables from
    // the current Registry. The tunable values for each variant come
    // from PARAM_DEFS via the typed snapshot accessor; the method-
    // choice itself is enumerated locally to drive the comparison.
    let reg = match load_registry() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[compare] cannot load registry: {e}");
            return;
        }
    };
    let snap = reg.snapshot();
    let cortex_variants = vec![
        CortexSource::allen_zhuang2017_full_frame(),
        CortexSource::reliability(
            snap.typed::<openisi_params::CortexSourceReliabilityThreshold>(),
        ),
        CortexSource::user_polygon(),
        CortexSource::snlc_garrett2014_im_bound(
            snap.typed::<openisi_params::CortexSourceSnlcK>(),
            snap.typed::<openisi_params::CortexSourceSnlcClose>(),
            snap.typed::<openisi_params::CortexSourceSnlcDilate>(),
        ),
    ];

    compare_stage_variants(
        "cortex_source",
        oisi_path,
        base_params,
        &compare_dir,
        cortex_variants,
        |params, variant| { params.cortex_source = variant; },
        |variant| variant.short_label(),
        // Figures affected by cortex_source choice — gates segmentation,
        // so essentially all downstream patch-derived maps shift.
        &[
            "cortex_mask",
            "area_labels",
            "area_borders",
            "eccentricity",
            "magnification",
            "contours_azi",
            "contours_alt",
            "vfs_smoothed_thresholded",
        ],
    );

    // Future: when additional method variants land in patch_threshold,
    // patch_refinement, or sign_map_smoothing, add comparable calls
    // here. Each per-stage `all_variants()` controls what's tried.
}

/// Run the pipeline once per variant of a single stage and produce a
/// grid composite per affected figure.
fn compare_stage_variants<V, Apply, Label>(
    stage_name: &str,
    oisi_path: &std::path::Path,
    base_params: &isi_analysis::AnalysisParams,
    compare_dir: &std::path::Path,
    variants: Vec<V>,
    apply: Apply,
    label: Label,
    affected_figures: &[&str],
) where
    V: Clone,
    Apply: Fn(&mut isi_analysis::AnalysisParams, V),
    Label: Fn(&V) -> &'static str,
{
    let stage_dir = compare_dir.join(stage_name);
    if let Err(e) = std::fs::create_dir_all(&stage_dir) {
        eprintln!("[compare/{stage_name}] Failed to create dir: {e}");
        return;
    }

    let mut successful: Vec<(String, std::path::PathBuf)> = Vec::new();
    let progress = isi_analysis::SilentProgress;
    let cancel = std::sync::atomic::AtomicBool::new(false);

    for variant in variants {
        let variant_label = label(&variant);
        let variant_dir = stage_dir.join(variant_label);

        // Copy the input to a temp .oisi so we don't trash the primary
        // run's persisted /results. The copy is deleted after rendering.
        let temp_oisi = stage_dir.join(format!(".tmp_{}.oisi", variant_label));
        if let Err(e) = std::fs::copy(oisi_path, &temp_oisi) {
            eprintln!("[compare/{stage_name}/{variant_label}] copy failed: {e}");
            continue;
        }

        // Clear any stored /analysis_params so the temp run uses our params.
        if let Ok(file) = hdf5::File::open_rw(&temp_oisi) {
            let _ = file.delete_attr("analysis_params");
        }

        let mut params = base_params.clone();
        apply(&mut params, variant);

        match isi_analysis::analyze(&temp_oisi, &params, &progress, &cancel) {
            Ok(()) => {
                if let Err(e) = std::fs::create_dir_all(&variant_dir) {
                    eprintln!("[compare/{stage_name}/{variant_label}] mkdir: {e}");
                } else {
                    export_all_figures(&temp_oisi, &variant_dir.to_string_lossy());
                    successful.push((variant_label.to_string(), variant_dir.clone()));
                    println!("  {stage_name}/{variant_label}: ✓");
                }
            }
            Err(e) => {
                println!("  {stage_name}/{variant_label}: skipped — {e}");
            }
        }

        let _ = std::fs::remove_file(&temp_oisi);
    }

    if successful.len() < 2 {
        println!(
            "[compare/{stage_name}] only {} variant(s) succeeded — no grid produced",
            successful.len()
        );
        return;
    }

    // Composite per-figure grids.
    for fig_name in affected_figures {
        composite_variant_grid(stage_name, fig_name, &successful, &stage_dir);
    }
}

/// Read each variant's `<figure>.png` and stitch into a horizontal grid
/// with a label header per cell. Output: `<stage_dir>/grid_<figure>.png`.
fn composite_variant_grid(
    _stage_name: &str,
    fig_name: &str,
    successful: &[(String, std::path::PathBuf)],
    stage_dir: &std::path::Path,
) {
    let png_name = format!("{fig_name}.png");
    let mut cells: Vec<(String, Vec<u8>, u32, u32)> = Vec::new();

    for (label, dir) in successful {
        let path = dir.join(&png_name);
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let decoder = png::Decoder::new(&bytes[..]);
        let mut reader = match decoder.read_info() {
            Ok(r) => r,
            Err(_) => continue,
        };
        let info = reader.info().clone();
        let mut buf = vec![0u8; reader.output_buffer_size()];
        if reader.next_frame(&mut buf).is_err() {
            continue;
        }
        // Force to RGBA for uniform compositing.
        let rgba = match info.color_type {
            png::ColorType::Rgba => buf,
            png::ColorType::Rgb => {
                let mut out = vec![255u8; (info.width * info.height * 4) as usize];
                for i in 0..(info.width * info.height) as usize {
                    out[i * 4]     = buf[i * 3];
                    out[i * 4 + 1] = buf[i * 3 + 1];
                    out[i * 4 + 2] = buf[i * 3 + 2];
                    out[i * 4 + 3] = 255;
                }
                out
            }
            png::ColorType::Grayscale => {
                let mut out = vec![255u8; (info.width * info.height * 4) as usize];
                for i in 0..(info.width * info.height) as usize {
                    out[i * 4]     = buf[i];
                    out[i * 4 + 1] = buf[i];
                    out[i * 4 + 2] = buf[i];
                    out[i * 4 + 3] = 255;
                }
                out
            }
            _ => continue,
        };
        cells.push((label.clone(), rgba, info.width, info.height));
    }

    if cells.len() < 2 {
        return;
    }

    let cell_w = cells[0].2;
    let cell_h = cells[0].3;
    let label_h: u32 = 22;
    let pad: u32 = 6;
    let n = cells.len() as u32;
    let total_w = n * cell_w + (n + 1) * pad;
    let total_h = label_h + cell_h + 2 * pad;
    let mut canvas = vec![240u8; (total_w * total_h * 4) as usize];
    // White background
    for px in 0..(total_w * total_h) as usize {
        canvas[px * 4]     = 245;
        canvas[px * 4 + 1] = 245;
        canvas[px * 4 + 2] = 245;
        canvas[px * 4 + 3] = 255;
    }

    for (i, (label, rgba, w, h)) in cells.iter().enumerate() {
        if *w != cell_w || *h != cell_h {
            // Skip mismatched sizes — shouldn't happen in practice
            // (same input file, same dimensions), but be defensive.
            continue;
        }
        let x0 = pad + (i as u32) * (cell_w + pad);
        let y0 = pad + label_h;
        // Draw label centered at the top of this cell.
        let text_x = x0 as usize + 4;
        let text_y = (pad + 4) as usize;
        draw_text(
            &mut canvas, total_w as usize, total_h as usize,
            text_x, text_y, label, (40, 40, 40), 1,
        );
        // Blit cell pixels.
        for row in 0..(*h as usize) {
            let src_start = row * (*w as usize) * 4;
            let dst_start = ((y0 as usize + row) * total_w as usize + x0 as usize) * 4;
            canvas[dst_start..dst_start + (*w as usize) * 4]
                .copy_from_slice(&rgba[src_start..src_start + (*w as usize) * 4]);
        }
    }

    let out = stage_dir.join(format!("grid_{fig_name}.png"));
    write_rgba_png(&out, total_w, total_h, &canvas);
    println!("  {} ({}x{}, {} variants)",
        out.strip_prefix(stage_dir.parent().unwrap_or(stage_dir))
            .unwrap_or(&out).display(),
        total_w, total_h, cells.len());
}

fn export_all_figures(oisi_path: &std::path::Path, out_dir: &str) {
    let dir = std::path::Path::new(out_dir);
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("Failed to create output dir: {e}");
        return;
    }

    let caps = match isi_analysis::io::inspect(oisi_path) {
        Ok(c) => c,
        Err(e) => { eprintln!("Failed to inspect: {e}"); return; }
    };

    println!("Exporting figures to {}/", out_dir);

    // Read every scalar_map result from HDF5 exactly once. The unified
    // renderer reads from this cache so the same HDF5 dataset isn't fetched
    // multiple times (e.g., when smoothed VFS reuses raw VFS).
    let mut scalar_maps: std::collections::HashMap<String, ndarray::Array2<f64>> =
        std::collections::HashMap::new();
    for result in &caps.results {
        if result.result_type != "scalar_map" { continue; }
        match isi_analysis::io::read_result_map(oisi_path, &result.name) {
            Ok(data) => { scalar_maps.insert(result.name.clone(), data); }
            Err(e) => eprintln!("  {}: read failed: {e}", result.name),
        }
    }

    // Rendering metadata (palette, range, units, NaN/zero semantics)
    // is read per-dataset from HDF5 attrs inside the loop below. The
    // renderer is now pure — it consumes only dataset + `MapMeta` and
    // does no `AnalysisParams` / `AcquisitionProperties` inference.

    // Anatomical grayscale used as the underlay for *masked* figures
    // (Sentinel kind: eccentricity, magnification). Allen and most
    // published mouse-retinotopy figures show vasculature beneath colored
    // patches; this is the same idea.
    let anatomical: Option<Vec<u8>> = isi_analysis::io::read_anatomical(oisi_path)
        .ok()
        .map(|arr| arr.into_iter().collect());

    for result in &caps.results {
        let name = &result.name;
        let rtype = &result.result_type;

        if rtype == "sign_array" { continue; } // metadata, not a map

        let out_path = dir.join(format!("{name}.png"));

        if rtype == "scalar_map" {
            if let Some(data) = scalar_maps.get(name) {
                let (h, w) = data.dim();
                let Some(meta) = isi_analysis::io::read_result_meta(oisi_path, name) else {
                    eprintln!(
                        "  {name}: skipped — MapMeta attrs absent \
                         (file analyzed before 2026-05-23 OR attr corruption); \
                         re-run `analyze` to attach current rendering metadata"
                    );
                    continue;
                };
                let (rgba, label) = render_map(data, &meta, anatomical.as_deref());
                write_rgba_png(&out_path, w as u32, h as u32, &rgba);
                println!("  {name}.png ({w}x{h}, {label})");
            }
        } else if rtype == "bool_mask" {
            // Two binary conventions, distinguished by name:
            // - *_mask, *_labels: area fills — render TRUE=white (highlighted
            //   region) on BLACK background. Matches fluorescence /
            //   anatomical-imaging convention.
            // - Line drawings (area_borders, contours_*): TRUE=black on
            //   WHITE background. Matches print/figure convention for line art.
            match isi_analysis::io::read_result_map(oisi_path, name) {
                Ok(data) => {
                    let (h, w) = data.dim();
                    let is_area_fill = name == "cortex_mask";
                    let (bg, fg): ([u8; 3], [u8; 3]) = if is_area_fill {
                        ([0, 0, 0], [255, 255, 255])
                    } else {
                        ([255, 255, 255], [0, 0, 0])
                    };
                    let mut rgba = vec![255u8; h * w * 4];
                    for (i, &v) in data.iter().enumerate() {
                        let col = if v > 0.5 { fg } else { bg };
                        rgba[i * 4]     = col[0];
                        rgba[i * 4 + 1] = col[1];
                        rgba[i * 4 + 2] = col[2];
                        rgba[i * 4 + 3] = 255;
                    }
                    write_rgba_png(&out_path, w as u32, h as u32, &rgba);
                    println!("  {name}.png ({w}x{h}, {rtype})");
                }
                Err(e) => eprintln!("  {name}: read failed: {e}"),
            }
        } else if rtype == "label_map" {
            // Read as f64, color by label.
            match isi_analysis::io::read_result_map(oisi_path, name) {
                Ok(data) => {
                    let (h, w) = data.dim();
                    // Read area_signs for coloring.
                    let signs: Vec<i32> = match hdf5::File::open(oisi_path) {
                        Ok(f) => f.dataset("results/area_signs")
                            .and_then(|ds| ds.read_1d::<i32>())
                            .map(|a| a.to_vec())
                            .unwrap_or_default(),
                        Err(_) => Vec::new(),
                    };

                    let mut rgba = vec![255u8; h * w * 4]; // white background
                    for (i, &v) in data.iter().enumerate() {
                        let label = v as i32;
                        if label > 0 && label <= signs.len() as i32 {
                            let sign = signs[(label - 1) as usize];
                            if sign > 0 {
                                rgba[i * 4] = 220; rgba[i * 4 + 1] = 50; rgba[i * 4 + 2] = 50;
                            } else {
                                rgba[i * 4] = 50; rgba[i * 4 + 1] = 50; rgba[i * 4 + 2] = 220;
                            }
                        }
                    }
                    write_rgba_png(&out_path, w as u32, h as u32, &rgba);
                    println!("  {name}.png ({w}x{h}, {rtype})");
                }
                Err(e) => eprintln!("  {name}: read failed: {e}"),
            }
        }
    }

    // Also export anatomical if present.
    if caps.has_anatomical {
        match isi_analysis::io::read_anatomical(oisi_path) {
            Ok(anat) => {
                let (h, w) = anat.dim();
                let mut rgba = vec![255u8; h * w * 4];
                for (i, &v) in anat.iter().enumerate() {
                    rgba[i * 4] = v;
                    rgba[i * 4 + 1] = v;
                    rgba[i * 4 + 2] = v;
                }
                let out_path = dir.join("anatomical.png");
                write_rgba_png(&out_path, w as u32, h as u32, &rgba);
                println!("  anatomical.png ({w}x{h})");
            }
            Err(e) => eprintln!("  anatomical: {e}"),
        }
    }

    // Per-direction phase diagnostic figures.
    //
    // The Kalatsky-Stryker / Garrett / Juavinett canonical methods specify
    // that each individual-direction phase map should already show a smooth
    // gradient across the cortex *before* any forward/reverse combination —
    // if each direction's phase is flat across cortex, the data lacks
    // position-tuned response (typically over-anesthesia / poor neurovascular
    // coupling) and no analysis can recover retinotopy.
    //
    // Per Juavinett 2017 Nat Protocols Troubleshooting Step 51 + West 2022:
    // "Reduce isoflurane flow, and wait for the mouse to wake up slightly
    //  such that breathing is >1 breath per s."
    // Per-direction phase diagnostic figures + circular phase statistics.
    // Each direction's complex map → real-valued phase array → unified
    // renderer with RenderKind::Wrapped (HSV, full ±π).
    if let Ok(maps) = isi_analysis::io::read_complex_maps(oisi_path) {
        let amp_azi: Vec<f64> = maps.azi_fwd.iter().zip(maps.azi_rev.iter())
            .map(|(a, b)| 0.5 * (a.norm() + b.norm())).collect();
        let amp_alt: Vec<f64> = maps.alt_fwd.iter().zip(maps.alt_rev.iter())
            .map(|(a, b)| 0.5 * (a.norm() + b.norm())).collect();

        println!("Per-direction phase variation across cortex (amp-weighted):");
        for (name, cm, amp) in [
            ("azi_fwd", &maps.azi_fwd, &amp_azi),
            ("azi_rev", &maps.azi_rev, &amp_azi),
            ("alt_fwd", &maps.alt_fwd, &amp_alt),
            ("alt_rev", &maps.alt_rev, &amp_alt),
        ] {
            let (mean_deg, std_deg) = circular_phase_stats(cm, amp);
            println!("  {name}: mean={mean_deg:>7.2}°  circular_std={std_deg:>6.2}°");
        }

        for (name, cm) in [
            ("azi_fwd_phase", &maps.azi_fwd),
            ("azi_rev_phase", &maps.azi_rev),
            ("alt_fwd_phase", &maps.alt_fwd),
            ("alt_rev_phase", &maps.alt_rev),
        ] {
            let phase = cm.mapv(|z| z.arg());
            let (h, w) = phase.dim();
            // Per-direction phase figures are not stored in `/results`,
            // so they have no `MapMeta` attached. Synthesize one matching
            // the radian-phase convention (HSV over [-π, π], wrap 2π).
            let meta = isi_analysis::MapMeta {
                palette: std::borrow::Cow::Borrowed("hsv_circular"),
                units: std::borrow::Cow::Borrowed("rad"),
                display_min: -std::f64::consts::PI,
                display_max:  std::f64::consts::PI,
                wrap_period:  std::f64::consts::TAU,
                nan_means: std::borrow::Cow::Borrowed(""),
                zero_means: std::borrow::Cow::Borrowed(""),
            };
            let (rgba, label) = render_map(&phase, &meta, None);
            let out_path = dir.join(format!("{name}.png"));
            write_rgba_png(&out_path, w as u32, h as u32, &rgba);
            println!("  {name}.png ({w}x{h}, {label})");
        }
    }

    println!("Done — {} figures exported", caps.results.len());
}

// =============================================================================
// Unified figure renderer — Allen `retinotopic_mapping` conventions
// =============================================================================
//
// Every scalar map goes through `render_map(data, kind)`. RenderKind picks
// the colormap and range exactly as the Allen `retinotopic_mapping` Python
// pipeline does (RetinotopicMapping.py, verified verbatim):
//
//   alt/azi position maps:  cmap='hsv', fixed degree range from the
//                           acquisition's visual-field sweep
//                           (Allen defaults: alt [-40, 60], azi [0, 120])
//   sign map / VFS:         cmap='jet', vmin=-1, vmax=1
//   power / amplitude maps: cmap='hot', vmin=0, vmax=1 after array_nor
//
// No amplitude mask is applied — Allen runs whole-frame. Vessel
// contamination is mitigated *optically* during acquisition by defocusing
// ~400-500 µm below cortex surface (Juavinett 2017 Step 36), not by
// post-hoc thresholding. The amplitude/SNR maps are inspection-only;
// thresholding for segmentation happens on the smoothed sign map at
// |VFS| > signMapThr (Allen default 0.35).

/// Semantic classification of a scalar map — picks the rendering policy
/// Single figure renderer — pure function of `(data, meta, anatomical)`.
///
/// All rendering decisions (palette, range, wrap, sentinel handling)
/// come from the `MapMeta` attrs stored on the dataset at write time.
/// The renderer does ZERO name-matching and reads no `AnalysisParams` /
/// `AcquisitionProperties` — adding a new map name only requires
/// extending `meta_for_f64` in the analysis crate.
///
/// `anatomical`: optional grayscale buffer `[h*w]` used as the underlay
/// for pixels where `v == 0` AND the meta declares a sentinel
/// (`zero_means` non-empty). Published mouse retinotopy figures
/// conventionally show vasculature beneath colored patches.
fn render_map(
    data: &ndarray::Array2<f64>,
    meta: &isi_analysis::MapMeta,
    anatomical: Option<&[u8]>,
) -> (Vec<u8>, String) {
    let (h, w) = data.dim();

    let lo = meta.display_min;
    let hi = meta.display_max;
    let range = (hi - lo).max(1e-10);
    let wrap_period = if meta.wrap_period > 0.0 { Some(meta.wrap_period) } else { None };
    let has_zero_sentinel = !meta.zero_means.is_empty();

    let palette: fn(f64) -> (u8, u8, u8) = match meta.palette.as_ref() {
        "hsv_circular" => hsv_circular,
        "hot" => hot,
        "jet" => jet,
        "binary" | "categorical" => jet, // fallbacks; bool/labels handled elsewhere
        _ => jet,
    };

    // Sentinel-zero pixels render as the anatomical underlay (when
    // provided + dimensions match) or stay white.
    let anat_for_meta = has_zero_sentinel
        .then_some(anatomical)
        .flatten()
        .filter(|a| a.len() == h * w);

    let mut rgba = vec![255u8; h * w * 4];
    if let Some(anat) = anat_for_meta {
        for i in 0..h * w {
            let g = anat[i];
            rgba[i * 4]     = g;
            rgba[i * 4 + 1] = g;
            rgba[i * 4 + 2] = g;
            rgba[i * 4 + 3] = 255;
        }
    }
    for (i, &v) in data.iter().enumerate() {
        if has_zero_sentinel && v == 0.0 { continue; }
        if !v.is_finite() { continue; }
        let t = match wrap_period {
            Some(p) => ((v - lo).rem_euclid(p)) / p,
            None => ((v - lo) / range).clamp(0.0, 1.0),
        };
        let (r, g, b) = palette(t);
        rgba[i * 4] = r;
        rgba[i * 4 + 1] = g;
        rgba[i * 4 + 2] = b;
        rgba[i * 4 + 3] = 255;
    }

    // Descriptive label uses the meta's units to pick the formatter.
    let unit_label = |v: f64| -> String {
        match meta.units.as_ref() {
            "rad" => format!("{:+.1}°", v.to_degrees()),
            "deg" => format!("{v:+.1}°"),
            _ => format!("{v:+.3}"),
        }
    };
    let cmap_label = match meta.palette.as_ref() {
        "hsv_circular" => "HSV",
        "hot" if has_zero_sentinel => "hot-sentinel",
        "hot" => "hot (normalized)",
        "jet" if has_zero_sentinel => "jet-sentinel",
        "jet" => "jet",
        other => other,
    };
    let label = format!("{cmap_label} [{}, {}]", unit_label(lo), unit_label(hi));
    (rgba, label)
}

/// Amplitude-weighted circular mean and circular std of a phase map, in
/// degrees. Standard definitions (Mardia 1972):
///   mean φ̄ = arg( Σ w·exp(iφ) / Σ w )
///   R = | Σ w·exp(iφ) / Σ w |     (resultant length, ∈ [0, 1])
///   circular std = sqrt(-2·ln R)   (radians; equals σ for small-spread limit)
fn circular_phase_stats(
    cm: &ndarray::Array2<isi_analysis::Complex64>,
    weights: &[f64],
) -> (f64, f64) {
    let mut sum_re = 0.0_f64;
    let mut sum_im = 0.0_f64;
    let mut sum_w = 0.0_f64;
    for (z, &w) in cm.iter().zip(weights.iter()) {
        if !w.is_finite() || w <= 0.0 { continue; }
        let phi = z.arg();
        sum_re += w * phi.cos();
        sum_im += w * phi.sin();
        sum_w += w;
    }
    if sum_w <= 0.0 { return (0.0, 0.0); }
    let mean_phi = sum_im.atan2(sum_re);
    let r = (sum_re * sum_re + sum_im * sum_im).sqrt() / sum_w;
    let r_clamped = r.clamp(1e-12, 1.0);
    let std_rad = (-2.0 * r_clamped.ln()).sqrt();
    (mean_phi.to_degrees(), std_rad.to_degrees())
}

/// Circular HSV colormap on `t ∈ [0, 1]` (hue 0 → 360°, full saturation and
/// value). Right choice for phase / angular data because there is no
/// discontinuity at the `±π` wrap.
fn hsv_circular(t: f64) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let h = (t * 6.0).rem_euclid(6.0);
    let c = 1.0_f64;
    let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match h as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

fn write_rgba_png(path: &std::path::Path, w: u32, h: u32, rgba: &[u8]) {
    let file = match std::fs::File::create(path) {
        Ok(f) => f,
        Err(e) => { eprintln!("  Failed to create {}: {e}", path.display()); return; }
    };
    let mut encoder = png::Encoder::new(file, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = match encoder.write_header() {
        Ok(w) => w,
        Err(e) => { eprintln!("  PNG header error: {e}"); return; }
    };
    if let Err(e) = writer.write_image_data(rgba) {
        eprintln!("  PNG write error: {e}");
    }
}

/// Matplotlib `hot` colormap: black → red → yellow → white. Allen amplitude
/// convention (`cmap='hot'`). Linear-segmented:
///   r: 0..0.365   → 0..1, then 1
///   g: 0.365..0.746 → 0..1, then 1
///   b: 0.746..1.0 → 0..1
fn hot(t: f64) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let r = if t <= 0.365 { t / 0.365 } else { 1.0 };
    let g = if t <= 0.365 { 0.0 } else if t <= 0.746 { (t - 0.365) / (0.746 - 0.365) } else { 1.0 };
    let b = if t <= 0.746 { 0.0 } else { (t - 0.746) / (1.0 - 0.746) };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

fn jet(t: f64) -> (u8, u8, u8) {
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

// ═══════════════════════════════════════════════════════════════════════
// Tiny 5x7 bitmap font for grid cell labels
//
// Hand-coded for the subset of ASCII chars the threshold-sweep grids need:
//   0-9 . = | > x s g c K A l e n V F S k h r f i o t a m d  space
// Each glyph is 7 rows of 5 bits (LSB is rightmost column).
// ═══════════════════════════════════════════════════════════════════════

const FONT_CHARS: &[(char, [u8; 7])] = &[
    ('0', [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110]),
    ('1', [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110]),
    ('2', [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111]),
    ('3', [0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110]),
    ('4', [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010]),
    ('5', [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110]),
    ('6', [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110]),
    ('7', [0b11111, 0b00001, 0b00010, 0b00100, 0b00100, 0b00100, 0b00100]),
    ('8', [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110]),
    ('9', [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100]),
    ('.', [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00110]),
    ('=', [0b00000, 0b00000, 0b11111, 0b00000, 0b11111, 0b00000, 0b00000]),
    ('|', [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100]),
    ('>', [0b10000, 0b01000, 0b00100, 0b00010, 0b00100, 0b01000, 0b10000]),
    ('<', [0b00001, 0b00010, 0b00100, 0b01000, 0b00100, 0b00010, 0b00001]),
    ('x', [0b00000, 0b00000, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001]),
    ('s', [0b00000, 0b00000, 0b01111, 0b10000, 0b01110, 0b00001, 0b11110]),
    ('g', [0b00000, 0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b01110]),
    ('c', [0b00000, 0b00000, 0b01110, 0b10000, 0b10000, 0b10001, 0b01110]),
    ('K', [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001]),
    ('A', [0b00100, 0b01010, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001]),
    ('l', [0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110]),
    ('e', [0b00000, 0b00000, 0b01110, 0b10001, 0b11111, 0b10000, 0b01110]),
    ('n', [0b00000, 0b00000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001]),
    ('V', [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100]),
    ('F', [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000]),
    ('S', [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110]),
    ('k', [0b10000, 0b10000, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010]),
    ('h', [0b10000, 0b10000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001]),
    ('r', [0b00000, 0b00000, 0b10110, 0b11001, 0b10000, 0b10000, 0b10000]),
    ('f', [0b00110, 0b01001, 0b01000, 0b11110, 0b01000, 0b01000, 0b01000]),
    ('i', [0b00100, 0b00000, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110]),
    ('o', [0b00000, 0b00000, 0b01110, 0b10001, 0b10001, 0b10001, 0b01110]),
    ('t', [0b01000, 0b01000, 0b11110, 0b01000, 0b01000, 0b01001, 0b00110]),
    ('a', [0b00000, 0b00000, 0b01110, 0b00001, 0b01111, 0b10001, 0b01111]),
    ('m', [0b00000, 0b00000, 0b11010, 0b10101, 0b10101, 0b10101, 0b10001]),
    ('d', [0b00001, 0b00001, 0b01111, 0b10001, 0b10001, 0b10001, 0b01111]),
    ('p', [0b00000, 0b00000, 0b11110, 0b10001, 0b11110, 0b10000, 0b10000]),
    (' ', [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000]),
    ('-', [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000]),
    (':', [0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000]),
    (',', [0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00110, 0b00100]),
];

fn glyph_for(ch: char) -> [u8; 7] {
    for &(c, g) in FONT_CHARS { if c == ch { return g; } }
    // Fallback: solid box so missing glyphs are visible.
    [0b11111; 7]
}

/// Draw `text` into the RGBA `buf` at pixel `(x, y)` using the 5×7 bitmap
/// font scaled by `scale`. Pixels off-canvas are silently clipped. The glyph
/// pitch is 6 columns at scale 1 (5px glyph + 1px spacing).
fn draw_text(
    buf: &mut [u8],
    total_w: usize,
    total_h: usize,
    x: usize, y: usize,
    text: &str,
    color: (u8, u8, u8),
    scale: usize,
) {
    let mut cursor_x = x;
    for ch in text.chars() {
        let g = glyph_for(ch);
        for (row, &bits) in g.iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) == 0 { continue; }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = cursor_x + col * scale + dx;
                        let py = y + row * scale + dy;
                        if px >= total_w || py >= total_h { continue; }
                        let i = (py * total_w + px) * 4;
                        buf[i]     = color.0;
                        buf[i + 1] = color.1;
                        buf[i + 2] = color.2;
                        buf[i + 3] = 255;
                    }
                }
            }
        }
        cursor_x += 6 * scale;
    }
}

// ═══════════════════════════════════════════════════════════════════════
// migrate — upgrade pre-2026 /analysis_params to current registry-tree schema
// ═══════════════════════════════════════════════════════════════════════

/// Migrate a `.oisi` file's `/analysis_params` attribute from the
/// pre-2026 serde-derived `AnalysisParams` shape to the current
/// registry-tree shape.
///
/// **Schemas:**
///
/// Pre-2026 was serde-derived JSON of `AnalysisParams`. Per-stage
/// values were tagged enums with tunable fields at the stage level:
///
/// ```json
/// { "phase_smoothing": {"method": "open_isi_amp_weighted_phasor", "sigma_px": 1.5} }
/// ```
///
/// Current schema is the Registry tree (produced by
/// `RegistrySnapshot::to_json_for_target(PersistTarget::Analysis)`).
/// Tunables nest under a subtree named for the active variant, so the
/// same content becomes:
///
/// ```json
/// {
///   "phase_smoothing": {
///     "method": "open_isi_amp_weighted_phasor",
///     "open_isi_amp_weighted_phasor": { "sigma_px": 1.5 }
///   }
/// }
/// ```
///
/// **Translation rules:**
///
/// 1. Start from a default registry tree (PARAM_DEFS defaults for
///    every stage). This populates every variant subtree with the
///    canonical defaults; the migration only overrides what the old
///    file recorded.
/// 2. For each stage in the OLD JSON:
///    - Take the method value from `old[stage]["method"]`, set
///      `new[stage]["method"]`.
///    - For each `(key, value)` in `old[stage]` other than `"method"`,
///      place it into `new[stage][<method>][key]` — the new variant
///      subtree.
/// 3. Root-level moved fields from the very-old `/analysis_params`
///    schema (`azi_angular_range`, `rotation_k`, etc.) are silently
///    dropped — they now live in `/experiment_params` and
///    `/rig_params`, captured at acquisition time.
///
/// **The translation table is hardcoded here** and is the ONLY place
/// the old schema's field names appear in the codebase post-refactor.
/// Defaults for missing tunables come from `PARAM_DEFS` (loaded into a
/// default `Registry`), not from per-method consts.
fn cmd_migrate(args: &[String]) -> AppResult<()> {
    if args.is_empty() {
        eprintln!("Usage: openisi-headless migrate <file.oisi>");
        return Ok(());
    }
    let path = std::path::PathBuf::from(&args[0]);
    if !path.exists() {
        return Err(AppError::NotAvailable(format!(
            "migrate: file does not exist: {}", path.display()
        )));
    }

    let Some(old_tree) = isi_analysis::io::read_analysis_params_attr(&path)? else {
        println!("{}: no /analysis_params attribute — nothing to migrate.", path.display());
        return Ok(());
    };

    if !isi_analysis::io::is_pre_2026_analysis_params(&path)? {
        println!("{}: /analysis_params already in current registry-tree schema. No migration needed.", path.display());
        return Ok(());
    }

    let new_tree = translate_pre_2026_analysis_params(&old_tree)?;
    isi_analysis::io::write_analysis_params_attr(&path, &new_tree)?;

    println!("Migrated /analysis_params on {}", path.display());
    println!("  old shape: serde-derived AnalysisParams (tagged enums, flat tunables)");
    println!("  new shape: Registry tree (tunables nested under variant subtrees)");
    println!("  defaults for unset tunables sourced from PARAM_DEFS");
    Ok(())
}

/// Translate a pre-2026 `/analysis_params` JSON tree into the current
/// registry-tree shape. Pure function; takes the old tree, returns the
/// new tree. Default values for any tunable not present in the old
/// tree come from `PARAM_DEFS` via a fresh `Registry` snapshot.
fn translate_pre_2026_analysis_params(
    old: &serde_json::Value,
) -> AppResult<serde_json::Value> {
    // Base = registry defaults. We overlay the old tree's stage methods
    // and tunables on top of this.
    let migrate_dir = std::path::Path::new("/tmp/migrate");
    let default_registry = openisi_params::Registry::new(migrate_dir, migrate_dir);
    let mut new_tree = default_registry
        .snapshot()
        .to_json_for_target(openisi_params::PersistTarget::Analysis);

    let Some(old_obj) = old.as_object() else {
        return Err(AppError::Validation(
            "/analysis_params is not a JSON object — cannot migrate".into(),
        ));
    };

    // The set of stage names. Root-level keys that aren't stage names
    // (e.g. moved fields like `azi_angular_range`) are silently dropped.
    const STAGES: &[&str] = &[
        "cycle_combine",
        "phase_smoothing",
        "vfs_computation",
        "sign_map_smoothing",
        "cortex_source",
        "patch_threshold",
        "patch_extraction",
        "patch_refinement",
        "quality_gate",
        "eccentricity",
    ];

    let new_obj = new_tree.as_object_mut().expect("registry tree is always an object");

    for stage in STAGES {
        let Some(old_stage) = old_obj.get(*stage).and_then(|v| v.as_object()) else {
            continue; // stage absent → keep the default
        };
        let Some(method) = old_stage.get("method").and_then(|v| v.as_str()) else {
            continue; // malformed; keep the default
        };

        // Build or replace new[stage] entirely so missing-from-old
        // fields fall through to PARAM_DEFS defaults that are already
        // in new_tree.
        let stage_entry = new_obj
            .entry((*stage).to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        let Some(stage_obj) = stage_entry.as_object_mut() else { continue; };

        // Override method.
        stage_obj.insert("method".into(), serde_json::Value::String(method.to_string()));

        // Move tunables into the variant subtree.
        let variant_entry = stage_obj
            .entry(method.to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        let Some(variant_obj) = variant_entry.as_object_mut() else { continue; };

        for (k, v) in old_stage.iter() {
            if k == "method" { continue; }
            variant_obj.insert(k.clone(), v.clone());
        }
    }

    Ok(new_tree)
}

// ═══════════════════════════════════════════════════════════════════════
// inspect
// ═══════════════════════════════════════════════════════════════════════

fn cmd_inspect(args: &[String]) -> AppResult<()> {
    if args.is_empty() {
        eprintln!("Usage: openisi-headless inspect <file.oisi>");
        return Ok(());
    }

    let path = std::path::Path::new(&args[0]);

    let caps = isi_analysis::io::inspect(path)?;
    println!("File: {}", path.display());
    println!("Anatomical:   {}", if caps.has_anatomical { "yes" } else { "no" });
    println!("Acquisition:  {}", if caps.has_acquisition {
        format!("yes ({} cycles)", caps.acquisition_cycles.len())
    } else { "no".into() });
    println!("Complex maps: {}", if caps.has_complex_maps { "yes" } else { "no" });
    println!("Results:      {}", if caps.has_results {
        format!("yes ({})", caps.results.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", "))
    } else { "no".into() });
    if let Some((h, w)) = caps.dimensions {
        println!("Dimensions:   {}x{}", w, h);
    }

    if let Ok(file) = hdf5::File::open(path) {
                if let Ok(ds) = file.dataset("acquisition/camera/timestamps_sec") {
                    if let Ok(ts) = ds.read_1d::<f64>() {
                        let n = ts.len();
                        if n > 0 {
                            println!("\nUnified timeline (seconds from t=0):");
                            println!("  Camera frames:  {} (t=[{:.6} .. {:.6}]s)", n, ts[0], ts[n - 1]);
                        }
                    }
                }
                if let Ok(ds) = file.dataset("acquisition/stimulus/timestamps_sec") {
                    if let Ok(ts) = ds.read_1d::<f64>() {
                        let n = ts.len();
                        if n > 0 {
                            println!("  Stimulus frames: {} (t=[{:.6} .. {:.6}]s)", n, ts[0], ts[n - 1]);
                        }
                    }
                }
                if let Ok(ds) = file.dataset("acquisition/schedule/sweep_start_sec") {
                    if let Ok(starts) = ds.read_1d::<f64>() {
                        if let Ok(ends_ds) = file.dataset("acquisition/schedule/sweep_end_sec") {
                            if let Ok(ends) = ends_ds.read_1d::<f64>() {
                                println!("  Sweeps: {}", starts.len());
                                for i in 0..starts.len() {
                                    println!("    [{i}] {:.6}s .. {:.6}s", starts[i], ends[i]);
                                }
                            }
                        }
                    }
                }
                if let Ok(tg) = file.group("acquisition/timing") {
                    println!("\nTiming characterization:");
                    if let Ok(attr) = tg.attr("regime") {
                        if let Ok(val) = attr.read_scalar::<hdf5::types::VarLenUnicode>() {
                            println!("  Regime:        {val}");
                        }
                    }
                    if let Ok(attr) = tg.attr("f_cam_hz") {
                        if let Ok(val) = attr.read_scalar::<f64>() {
                            println!("  Camera:        {:.3} Hz", val);
                        }
                    }
                    if let Ok(attr) = tg.attr("f_stim_hz") {
                        if let Ok(val) = attr.read_scalar::<f64>() {
                            println!("  Stimulus:      {:.3} Hz", val);
                        }
                    }
                    if let Ok(attr) = tg.attr("beat_period_sec") {
                        if let Ok(val) = attr.read_scalar::<f64>() {
                            println!("  Beat period:   {:.3}s", val);
                        }
                    }
                    if let Ok(attr) = tg.attr("phase_coverage") {
                        if let Ok(val) = attr.read_scalar::<f64>() {
                            println!("  Phase coverage: {:.1}%", val * 100.0);
                        }
                    }
                }
                if let Ok(cs) = file.group("acquisition/clock_sync") {
                    println!("\nClock sync:");
                    if let Ok(attr) = cs.attr("cam_hw_minus_sys_start_us") {
                        if let Ok(val) = attr.read_scalar::<f64>() {
                            println!("  HW-SYS offset (start): {:.1}µs", val);
                        }
                    }
                    if let Ok(attr) = cs.attr("cam_hw_minus_sys_end_us") {
                        if let Ok(val) = attr.read_scalar::<f64>() {
                            println!("  HW-SYS offset (end):   {:.1}µs", val);
                        }
                    }
                    if let Ok(attr) = cs.attr("drift_us") {
                        if let Ok(val) = attr.read_scalar::<f64>() {
                            println!("  Drift:                 {:.1}µs", val);
                        }
                    }
                }
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// import
// ═══════════════════════════════════════════════════════════════════════

fn cmd_import(args: &[String]) -> AppResult<()> {
    if args.is_empty() {
        eprintln!("Usage: openisi-headless import <mat-directory> [output.oisi]");
        return Ok(());
    }

    let dir = std::path::Path::new(&args[0]);
    if !dir.is_dir() {
        eprintln!("Not a directory: {}", dir.display());
        return Ok(());
    }

    let output = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        let name = dir.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "import".into());
        dir.parent().unwrap_or(dir).join(format!("{name}.oisi"))
    };

    println!("Importing {} -> {}", dir.display(), output.display());

    isi_analysis::io::import_snlc_directory(dir, &output)?;
    println!("Import complete: {}", output.display());
    let caps = isi_analysis::io::inspect(&output)?;
    println!("  Complex maps: {}", if caps.has_complex_maps { "yes" } else { "no" });
    println!("  Anatomical:   {}", if caps.has_anatomical { "yes" } else { "no" });
    if let Some((h, w)) = caps.dimensions {
        println!("  Dimensions:   {}x{}", w, h);
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// import-samples
// ═══════════════════════════════════════════════════════════════════════

fn cmd_import_samples(_args: &[String]) -> AppResult<()> {
    let reg = load_registry()?;
    let data_dir = reg.snapshot().data_directory().to_string();
    if data_dir.is_empty() {
        eprintln!("Set [paths] data_directory in rig.toml before downloading samples.");
        return Ok(());
    }
    let out_dir = std::path::Path::new(&data_dir);

    let imported = openisi_lib::sample_data::import_snlc_sample_bundle(out_dir)
        .map_err(|e| AppError::Hardware(format!("sample bundle import: {e}")))?;
    println!("Imported {} subject(s):", imported.len());
    for p in &imported {
        println!("  {}", p.display());
    }
    Ok(())
}

fn cmd_test_read(args: &[String]) -> AppResult<()> {
    if args.is_empty() {
        eprintln!("Usage: headless test-read <file.oisi>");
        return Ok(());
    }
    let path = &args[0];
    let file = hdf5::File::open(path)
        .map_err(|e| AppError::Analysis(isi_analysis::AnalysisError::Hdf5(format!("open {path}: {e}"))))?;

    let group = file.group("results")
        .map_err(|e| AppError::Analysis(isi_analysis::AnalysisError::MissingData(format!("no results group: {e}"))))?;

    let names = group.member_names().unwrap_or_default();
    println!("Results group has {} members:", names.len());

    for name in &names {
        match group.dataset(name) {
            Ok(ds) => {
                let shape = ds.shape();
                let ndim = shape.len();
                if ndim == 2 {
                    if let Ok(arr) = ds.read_2d::<f64>() {
                        let (h, w) = arr.dim();
                        let min = arr.iter().cloned().fold(f64::INFINITY, f64::min);
                        let max = arr.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                        println!("  {name}: f64 {h}x{w} [{min:.4} .. {max:.4}]");
                        continue;
                    }
                    if let Ok(arr) = ds.read_2d::<i32>() {
                        let (h, w) = arr.dim();
                        let max = *arr.iter().max().unwrap_or(&0);
                        println!("  {name}: i32 {h}x{w} max={max}");
                        continue;
                    }
                    if let Ok(arr) = ds.read_2d::<u8>() {
                        let (h, w) = arr.dim();
                        let sum: u64 = arr.iter().map(|&v| v as u64).sum();
                        println!("  {name}: u8 {h}x{w} sum={sum}");
                        continue;
                    }
                    println!("  {name}: 2D {:?} (unreadable)", shape);
                } else if ndim == 1 {
                    if let Ok(arr) = ds.read_1d::<i32>() {
                        println!("  {name}: i32[{}] = {:?}", arr.len(), arr.to_vec());
                        continue;
                    }
                    println!("  {name}: 1D {:?} (unreadable)", shape);
                } else {
                    println!("  {name}: {:?}", shape);
                }
            }
            Err(_) => {
                println!("  {name}: (group, not dataset)");
            }
        }
    }
    Ok(())
}

fn cmd_import_session(args: &[String]) -> AppResult<()> {
    if args.is_empty() {
        eprintln!("Usage: headless import-session <session-dir> [output.oisi]");
        return Ok(());
    }
    let session_dir = std::path::Path::new(&args[0]);
    let results_path = session_dir.join("analysis/retinotopy_results.h5");
    let anat_path = session_dir.join("anatomical.png");

    if !results_path.exists() {
        eprintln!("No retinotopy_results.h5 found in {}", session_dir.display());
        return Ok(());
    }

    let output = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        let name = session_dir.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or("session".into());
        session_dir.parent().unwrap_or(session_dir).join(format!("{name}.oisi"))
    };

    println!("Importing session {} -> {}", session_dir.display(), output.display());

    let results_file = hdf5::File::open(&results_path)
        .map_err(|e| AppError::Hardware(format!("Failed to open retinotopy_results.h5: {e}")))?;

    let read_complex = |dir: &str| -> Option<ndarray::Array2<isi_analysis::Complex64>> {
        let grp = results_file.group(dir).ok()?;
        let mag: ndarray::Array2<f64> = grp.dataset("magnitude").ok()?.read().ok()?;
        let phase: ndarray::Array2<f64> = grp.dataset("phase_radians").ok()?.read().ok()?;
        let (h, w) = mag.dim();
        Some(ndarray::Array2::from_shape_fn((h, w), |(r, c)| {
            isi_analysis::complex_from_polar(mag[[r, c]], phase[[r, c]])
        }))
    };

    let azi_fwd = read_complex("LR").ok_or_else(|| AppError::Analysis(isi_analysis::AnalysisError::MissingData("missing LR direction in results".into())))?;
    let azi_rev = read_complex("RL").ok_or_else(|| AppError::Analysis(isi_analysis::AnalysisError::MissingData("missing RL direction in results".into())))?;
    let alt_fwd = read_complex("TB").ok_or_else(|| AppError::Analysis(isi_analysis::AnalysisError::MissingData("missing TB direction in results".into())))?;
    let alt_rev = read_complex("BT").ok_or_else(|| AppError::Analysis(isi_analysis::AnalysisError::MissingData("missing BT direction in results".into())))?;

    let complex_maps = isi_analysis::ComplexMaps { azi_fwd, azi_rev, alt_fwd, alt_rev };

    isi_analysis::io::create(&output, "session_import")
        .map_err(|e| AppError::Hardware(format!("Failed to create .oisi: {e}")))?;
    isi_analysis::io::write_complex_maps(&output, &complex_maps)
        .map_err(|e| AppError::Hardware(format!("Failed to write complex maps: {e}")))?;

    // Import anatomical if present.
    if anat_path.exists() {
        let file_bytes = std::fs::read(&anat_path)
            .map_err(|e| AppError::Hardware(format!("Failed to read anatomical: {e}")))?;
        let decoder = png::Decoder::new(std::io::Cursor::new(&file_bytes));
        match decoder.read_info() {
            Ok(mut reader) => {
                let mut buf = vec![0u8; reader.output_buffer_size()];
                if let Ok(info) = reader.next_frame(&mut buf) {
                    let (w, h) = (info.width as usize, info.height as usize);
                    // Convert to grayscale u8 if needed.
                    let gray: Vec<u8> = match info.color_type {
                        png::ColorType::Grayscale => buf[..w * h].to_vec(),
                        png::ColorType::GrayscaleAlpha => buf[..w * h * 2].chunks(2).map(|c| c[0]).collect(),
                        png::ColorType::Rgb => buf[..w * h * 3].chunks(3)
                            .map(|c| ((c[0] as u16 + c[1] as u16 + c[2] as u16) / 3) as u8).collect(),
                        png::ColorType::Rgba => buf[..w * h * 4].chunks(4)
                            .map(|c| ((c[0] as u16 + c[1] as u16 + c[2] as u16) / 3) as u8).collect(),
                        _ => { eprintln!("  Unsupported PNG color type for anatomical"); Vec::new() }
                    };
                    if !gray.is_empty() {
                        let arr = ndarray::Array2::from_shape_vec((h, w), gray)
                            .map_err(|e| AppError::Hardware(format!("Shape error: {e}")))?;
                        let file = hdf5::File::open_rw(&output)
                            .map_err(|e| AppError::Hardware(format!("Failed to open .oisi for anatomical: {e}")))?;
                        file.new_dataset_builder()
                            .with_data(&arr)
                            .create("anatomical")
                            .map_err(|e| AppError::Hardware(format!("Failed to write anatomical: {e}")))?;
                        println!("  Anatomical: {}x{}", w, h);
                    }
                }
            }
            Err(e) => eprintln!("  Skipping anatomical ({}): {e}", anat_path.display()),
        }
    }

    println!("Import complete: {}", output.display());
    Ok(())
}

fn cmd_dump_h5(args: &[String]) -> AppResult<()> {
    if args.is_empty() {
        eprintln!("Usage: headless dump-h5 <file.h5>");
        return Ok(());
    }
    let path = &args[0];
    let file = hdf5::File::open(path)
        .map_err(|e| AppError::Analysis(isi_analysis::AnalysisError::Hdf5(format!("open {path}: {e}"))))?;

    fn dump_group(group: &hdf5::Group, prefix: &str) {
        let names = group.member_names().unwrap_or_default();
        for name in &names {
            let full = format!("{prefix}/{name}");
            if let Ok(sub) = group.group(name) {
                println!("{full}/ (group)");
                dump_group(&sub, &full);
            } else if let Ok(ds) = group.dataset(name) {
                println!("{full} {:?}", ds.shape());
            }
        }
    }

    let root = file.as_group()
        .map_err(|e| AppError::Analysis(isi_analysis::AnalysisError::Hdf5(format!("open root group: {e}"))))?;
    dump_group(&root, "");
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// threshold-sweep — VFS threshold strategy comparison
//
// Two grid PNGs side-by-side: (a) discrete patches per threshold, (b) the
// smoothed VFS itself restricted to the passing pixels. Each cell carries a
// small text label with its actual threshold value (and patch count for the
// discrete view).
// ═══════════════════════════════════════════════════════════════════════

#[derive(Copy, Clone)]
enum ThresholdApproach { AllenFixed, SnlcGlobalStd, CortexMaskedStd }

fn export_threshold_sweep_grids(
    oisi_path: &std::path::Path,
    out_dir: &std::path::Path,
    params: &isi_analysis::AnalysisParams,
) {
    use isi_analysis::io::read_result_map;

    // Read the smoothed VFS — the array segmentation thresholds.
    // `/results/vfs` is the raw mathematical VFS; `/results/vfs_smoothed`
    // is the Gaussian-smoothed stage this diagnostic operates on.
    let vfs_smooth = match read_result_map(oisi_path, "vfs_smoothed") {
        Ok(a) => a,
        Err(e) => { eprintln!("threshold-sweep: read vfs_smoothed: {e}"); return; }
    };
    // Read per-direction reliability and derive cortex via the same
    // formula production uses (cortex_from_reliability with the
    // configured threshold). The diagnostic now operates on the exact
    // same cortex as the production pipeline.
    let rel = match (
        read_result_map(oisi_path, "reliability_azi_fwd"),
        read_result_map(oisi_path, "reliability_azi_rev"),
        read_result_map(oisi_path, "reliability_alt_fwd"),
        read_result_map(oisi_path, "reliability_alt_rev"),
    ) {
        (Ok(a), Ok(b), Ok(c), Ok(d)) => (a, b, c, d),
        _ => {
            eprintln!("threshold-sweep: reliability maps missing — run analyze first \
                       on a file with raw per-cycle data");
            return;
        }
    };
    // Pull the reliability threshold from the configured CortexSource
    // method if it's the Reliability variant; otherwise read the
    // registry default via the bridge (consistent SSoT path).
    let reliability_threshold = match &params.cortex_source {
        isi_analysis::methods::CortexSource::Reliability { threshold } => *threshold,
        _ => {
            let def = &openisi_params::PARAM_DEFS[
                openisi_params::ParamId::CortexSourceReliabilityThreshold as usize
            ];
            match &def.default {
                openisi_params::ParamValue::F64(v) => *v,
                _ => unreachable!("CortexSourceReliabilityThreshold is F64"),
            }
        }
    };
    let cortex_mask = isi_analysis::segmentation::cortex_from_reliability(
        &rel.0, &rel.1, &rel.2, &rel.3,
        reliability_threshold,
    );
    let anatomical: Option<Vec<u8>> = isi_analysis::io::read_anatomical(oisi_path)
        .ok()
        .map(|arr| arr.into_iter().collect());

    let (h, w) = vfs_smooth.dim();
    if cortex_mask.dim() != (h, w) {
        eprintln!("threshold-sweep: shape mismatch"); return;
    }

    // Diagnostic-only: Allen `smallPatchThr = 100` default. The actual
    // small-patch threshold lives inside `patch_extraction` method
    // variant params; threshold-sweep uses a fixed value to compare
    // patch counts across threshold values consistently.
    let small_patch_thr: usize = 100;

    let global_std = stddev(vfs_smooth.iter().copied().filter(|v| v.is_finite()));
    let cortex_std = stddev(
        vfs_smooth.iter().zip(cortex_mask.iter())
            .filter_map(|(&v, &m)| if m && v.is_finite() { Some(v) } else { None })
    );

    println!("[threshold-sweep] global σ(vfs_smooth) = {:.4}", global_std);
    println!("[threshold-sweep] cortex σ(vfs_smooth) = {:.4}  ({} pixels)",
        cortex_std, cortex_mask.iter().filter(|&&v| v).count());

    let rows: [(ThresholdApproach, &str, [f64; 5]); 3] = [
        (ThresholdApproach::AllenFixed,      "Allen fixed",   [0.10, 0.15, 0.20, 0.25, 0.35]),
        (ThresholdApproach::SnlcGlobalStd,   "K x global s",  [1.0, 1.5, 2.0, 2.5, 3.0]),
        (ThresholdApproach::CortexMaskedStd, "K x cortex s",  [1.0, 1.5, 2.0, 2.5, 3.0]),
    ];

    render_threshold_grid_patches(
        &vfs_smooth, &cortex_mask, small_patch_thr, global_std, cortex_std, &rows, out_dir,
        anatomical.as_deref(),
    );
    render_threshold_grid_vfs(
        &vfs_smooth, global_std, cortex_std, &rows, out_dir,
        anatomical.as_deref(),
    );
}

fn stddev(values: impl Iterator<Item = f64>) -> f64 {
    let (mut n, mut s, mut sq) = (0u64, 0.0_f64, 0.0_f64);
    for v in values { n += 1; s += v; sq += v * v; }
    if n < 2 { return 0.0; }
    let mean = s / n as f64;
    ((sq / n as f64) - mean * mean).max(0.0).sqrt()
}

fn threshold_for_cell(
    approach: ThresholdApproach,
    p: f64,
    global_std: f64,
    cortex_std: f64,
) -> f64 {
    match approach {
        ThresholdApproach::AllenFixed       => p,
        ThresholdApproach::SnlcGlobalStd    => p * global_std,
        ThresholdApproach::CortexMaskedStd  => p * cortex_std,
    }
}

/// Grid layout: left margin holds row headers, top of each cell holds its
/// per-cell label. Returns `(cell_w, cell_h, label_h, pad, header_w,
/// total_w, total_h, rgba_buf)`.
fn build_grid_canvas(h: usize, w: usize, n_rows: usize, n_cols: usize)
    -> (usize, usize, usize, usize, usize, usize, usize, Vec<u8>)
{
    let cell_h = h / 2;
    let cell_w = w / 2;
    let label_h = 14usize;   // 7px font @ 2× scale
    let header_w = 156usize; // room for "K x cortex s" (12 chars × 12 px + margin) at scale 2
    let pad = 6usize;
    let row_h = label_h + cell_h;
    let total_w = header_w + n_cols * cell_w + (n_cols + 1) * pad;
    let total_h = n_rows * row_h + (n_rows + 1) * pad;
    let mut buf = vec![245u8; total_w * total_h * 4]; // light gray background
    for i in 0..total_w * total_h { buf[i * 4 + 3] = 255; }
    (cell_w, cell_h, label_h, pad, header_w, total_w, total_h, buf)
}

/// Compact per-cell label — just the threshold info, no approach prefix.
fn cell_label_short(approach: ThresholdApproach, p: f64, threshold: f64) -> String {
    match approach {
        ThresholdApproach::AllenFixed      => format!("thr={:.2}", p),
        ThresholdApproach::SnlcGlobalStd   => format!("{:.1}xsg={:.3}", p, threshold),
        ThresholdApproach::CortexMaskedStd => format!("{:.1}xsc={:.3}", p, threshold),
    }
}

fn render_threshold_grid_patches(
    vfs_smooth: &ndarray::Array2<f64>,
    cortex_mask: &ndarray::Array2<bool>,
    small_patch_thr: usize,
    global_std: f64,
    cortex_std: f64,
    rows: &[(ThresholdApproach, &str, [f64; 5]); 3],
    out_dir: &std::path::Path,
    anatomical: Option<&[u8]>,
) {
    let (h, w) = vfs_smooth.dim();
    let n_rows = rows.len();
    let n_cols = 5;
    let (cell_w, cell_h, label_h, pad, header_w, total_w, total_h, mut buf) =
        build_grid_canvas(h, w, n_rows, n_cols);

    for (row_idx, (approach, row_label, params)) in rows.iter().enumerate() {
        // Row header in the left margin, vertically centered against the cell.
        let row_y = pad + row_idx * (label_h + cell_h + pad)
            + label_h + cell_h / 2 - 7;
        draw_text(&mut buf, total_w, total_h, 4, row_y, row_label, (30, 30, 30), 2);

        for (col_idx, &p) in params.iter().enumerate() {
            let threshold = threshold_for_cell(*approach, p, global_std, cortex_std);
            let cell_text = cell_label_short(*approach, p, threshold);

            let (area_labels, area_signs) = isi_analysis::segmentation::segment_threshold_only(
                vfs_smooth, cortex_mask, threshold, small_patch_thr,
            );
            let n_patches = area_signs.len();

            let mut full = vec![255u8; h * w * 4];
            if let Some(anat) = anatomical {
                if anat.len() == h * w {
                    for i in 0..h * w {
                        let g = anat[i];
                        full[i * 4] = g;
                        full[i * 4 + 1] = g;
                        full[i * 4 + 2] = g;
                        full[i * 4 + 3] = 255;
                    }
                }
            } else {
                for i in 0..h * w { full[i * 4 + 3] = 255; }
            }
            for r in 0..h {
                for c in 0..w {
                    let l = area_labels[[r, c]];
                    if l == 0 { continue; }
                    let sign = area_signs[(l - 1) as usize];
                    let (rc, gc, bc) = if sign > 0 { (220, 50, 50) } else { (50, 50, 220) };
                    full[(r * w + c) * 4]     = rc;
                    full[(r * w + c) * 4 + 1] = gc;
                    full[(r * w + c) * 4 + 2] = bc;
                }
            }

            let cell_x = header_w + pad + col_idx * (cell_w + pad);
            let cell_y = pad + row_idx * (label_h + cell_h + pad) + label_h;
            place_downsampled_cell(&full, w, h, &mut buf, total_w,
                cell_x, cell_y, cell_w, cell_h);

            // Per-cell label above the cell.
            let label_str = format!("{}  n={}", cell_text, n_patches);
            draw_text(&mut buf, total_w, total_h,
                cell_x, cell_y - label_h,
                &label_str, (30, 30, 30), 2);
        }
    }

    let path = out_dir.join("threshold_sweep_patches.png");
    write_rgba_png(&path, total_w as u32, total_h as u32, &buf);
    println!("  threshold_sweep_patches.png ({total_w}x{total_h}, {n_rows}x{n_cols} grid)");
}

fn render_threshold_grid_vfs(
    vfs_smooth: &ndarray::Array2<f64>,
    global_std: f64,
    cortex_std: f64,
    rows: &[(ThresholdApproach, &str, [f64; 5]); 3],
    out_dir: &std::path::Path,
    anatomical: Option<&[u8]>,
) {
    let (h, w) = vfs_smooth.dim();
    let n_rows = rows.len();
    let n_cols = 5;
    let (cell_w, cell_h, label_h, pad, header_w, total_w, total_h, mut buf) =
        build_grid_canvas(h, w, n_rows, n_cols);

    for (row_idx, (approach, row_label, params)) in rows.iter().enumerate() {
        let row_y = pad + row_idx * (label_h + cell_h + pad)
            + label_h + cell_h / 2 - 7;
        draw_text(&mut buf, total_w, total_h, 4, row_y, row_label, (30, 30, 30), 2);

        for (col_idx, &p) in params.iter().enumerate() {
            let threshold = threshold_for_cell(*approach, p, global_std, cortex_std);
            let cell_text = cell_label_short(*approach, p, threshold);

            // Render vfs_smooth in jet [-1, +1] only where |VFS| ≥ threshold;
            // pixels below threshold render as background (white).
            let mut full = vec![255u8; h * w * 4];
            if let Some(anat) = anatomical {
                if anat.len() == h * w {
                    for i in 0..h * w {
                        let g = anat[i];
                        full[i * 4] = g;
                        full[i * 4 + 1] = g;
                        full[i * 4 + 2] = g;
                        full[i * 4 + 3] = 255;
                    }
                }
            } else {
                for i in 0..h * w { full[i * 4 + 3] = 255; }
            }
            for r in 0..h {
                for c in 0..w {
                    let v = vfs_smooth[[r, c]];
                    if !v.is_finite() || v.abs() < threshold { continue; }
                    let t = (0.5 + 0.5 * v.clamp(-1.0, 1.0)).clamp(0.0, 1.0);
                    let (rc, gc, bc) = jet(t);
                    full[(r * w + c) * 4]     = rc;
                    full[(r * w + c) * 4 + 1] = gc;
                    full[(r * w + c) * 4 + 2] = bc;
                }
            }

            let cell_x = header_w + pad + col_idx * (cell_w + pad);
            let cell_y = pad + row_idx * (label_h + cell_h + pad) + label_h;
            place_downsampled_cell(&full, w, h, &mut buf, total_w,
                cell_x, cell_y, cell_w, cell_h);

            draw_text(&mut buf, total_w, total_h,
                cell_x, cell_y - label_h,
                &cell_text, (30, 30, 30), 2);
        }
    }

    let path = out_dir.join("threshold_sweep_vfs.png");
    write_rgba_png(&path, total_w as u32, total_h as u32, &buf);
    println!("  threshold_sweep_vfs.png ({total_w}x{total_h}, {n_rows}x{n_cols} grid)");
}

/// 2×2 mean downsample of a full-resolution RGBA cell into the composite.
fn place_downsampled_cell(
    full: &[u8],
    w: usize, h: usize,
    composite: &mut [u8], total_w: usize,
    ox: usize, oy: usize,
    cell_w: usize, cell_h: usize,
) {
    for dr in 0..cell_h {
        for dc in 0..cell_w {
            let mut sr = 0u32; let mut sg = 0u32; let mut sb = 0u32;
            for ddr in 0..2 {
                for ddc in 0..2 {
                    let r = dr * 2 + ddr;
                    let c = dc * 2 + ddc;
                    if r >= h || c >= w { continue; }
                    let i = (r * w + c) * 4;
                    sr += full[i] as u32;
                    sg += full[i + 1] as u32;
                    sb += full[i + 2] as u32;
                }
            }
            let dst = ((oy + dr) * total_w + (ox + dc)) * 4;
            composite[dst]     = (sr / 4) as u8;
            composite[dst + 1] = (sg / 4) as u8;
            composite[dst + 2] = (sb / 4) as u8;
            composite[dst + 3] = 255;
        }
    }
}

// =============================================================================
// dev_figures: default layout, meta.json
//
// See docs/ANALYSIS_COMPUTE.md § "Dev workflow: generated figures".
// =============================================================================

/// `<repo_root>/dev_figures/<oisi_stem>/<device>-<utc>/`
fn default_figures_dir(
    oisi_path: &std::path::Path,
    _params: &isi_analysis::AnalysisParams,
) -> std::path::PathBuf {
    let stem = oisi_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".into());
    let device = isi_analysis::compute::device_tag();
    let unix_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let ts = format_utc_yyyymmddthhmm(unix_secs);

    repo_root()
        .join("dev_figures")
        .join(stem)
        .join(format!("{device}-{ts}"))
}

/// Walk up from this crate's manifest dir to find the workspace root (the
/// directory containing the workspace `Cargo.toml`). Falls back to the parent
/// of `CARGO_MANIFEST_DIR`.
fn repo_root() -> std::path::PathBuf {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().map(|p| p.to_path_buf()).unwrap_or(manifest)
}

/// Format a unix timestamp (seconds) as `YYYYMMDDTHHMM` in UTC, with no
/// external date dependency. Uses Howard Hinnant's `civil_from_days`.
fn format_utc_yyyymmddthhmm(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86400);
    let secs_today = unix_secs.rem_euclid(86400);
    let hour = secs_today / 3600;
    let minute = (secs_today % 3600) / 60;

    // civil_from_days — converts days-since-1970-01-01 to (y, m, d).
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 { y += 1; }
    format!("{y:04}{m:02}{d:02}T{hour:02}{minute:02}")
}

fn git_capture(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() { return None; }
    let s = String::from_utf8(output.stdout).ok()?;
    let s = s.trim();
    if s.is_empty() { None } else { Some(s.to_string()) }
}

/// Write `meta.json` recording the full reproduction context for the figures
/// in `dir`. Uses portable identifiers (animal_id, created_at) so the
/// directory is shareable across machines.
fn write_meta_json(
    dir: &std::path::Path,
    oisi_path: &std::path::Path,
    _params: &isi_analysis::AnalysisParams,
) {
    let filename = oisi_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let identity = isi_analysis::io::read_acquisition_identity(oisi_path).ok();

    let unix_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let stamp = format_utc_yyyymmddthhmm(unix_secs);
    // Pretty ISO-8601 for the meta.json human reader.
    let ts_iso = format!(
        "{}-{}-{}T{}:{}:00Z",
        &stamp[0..4], &stamp[4..6], &stamp[6..8], &stamp[9..11], &stamp[11..13],
    );

    let git_sha = git_capture(&["rev-parse", "--short=7", "HEAD"]);
    let git_branch = git_capture(&["rev-parse", "--abbrev-ref", "HEAD"]);
    let git_dirty = std::process::Command::new("git")
        .args(["diff", "--quiet"])
        .status()
        .map(|s| !s.success())
        .ok();

    // Source the analysis_params tree from the .oisi (the canonical
    // record). If the file hasn't been analyzed yet, fall back to the
    // current registry tree.
    let analysis_params_tree = isi_analysis::io::read_analysis_params_attr(oisi_path)
        .ok()
        .flatten()
        .unwrap_or_else(|| match load_registry() {
            Ok(reg) => reg.snapshot().to_json_for_target(openisi_params::PersistTarget::Analysis),
            Err(_) => serde_json::Value::Null,
        });

    let meta = serde_json::json!({
        "source": {
            "filename": filename,
            "animal_id": identity.as_ref().map(|i| &i.animal_id),
            "created_at": identity.as_ref().map(|i| &i.created_at),
        },
        "device": isi_analysis::compute::backend_info(),
        "git_sha": git_sha,
        "git_branch": git_branch,
        "git_dirty": git_dirty,
        "timestamp_utc": ts_iso,
        "analysis_params": analysis_params_tree,
    });

    let path = dir.join("meta.json");
    match serde_json::to_string_pretty(&meta) {
        Ok(s) => {
            if let Err(e) = std::fs::write(&path, s) {
                eprintln!("  failed to write meta.json: {e}");
            } else {
                println!("  meta.json");
            }
        }
        Err(e) => eprintln!("  meta.json serialize failed: {e}"),
    }
}

#[cfg(test)]
mod migrate_tests {
    use super::translate_pre_2026_analysis_params;
    use serde_json::json;

    #[test]
    fn translates_tagged_enum_with_tunable_to_variant_subtree() {
        // Pre-2026 shape: per-stage tagged enum, tunable at stage level.
        let old = json!({
            "phase_smoothing": {
                "method": "open_isi_amp_weighted_phasor",
                "sigma_px": 2.5
            }
        });
        let new = translate_pre_2026_analysis_params(&old).unwrap();
        // New shape: tunable nested under variant subtree.
        assert_eq!(
            new["phase_smoothing"]["method"],
            json!("open_isi_amp_weighted_phasor")
        );
        assert_eq!(
            new["phase_smoothing"]["open_isi_amp_weighted_phasor"]["sigma_px"],
            json!(2.5)
        );
    }

    #[test]
    fn missing_tunable_falls_back_to_param_defs_default() {
        // Old shape with method present but tunable absent (was Option::None).
        let old = json!({
            "phase_smoothing": { "method": "open_isi_amp_weighted_phasor" }
        });
        let new = translate_pre_2026_analysis_params(&old).unwrap();
        // PARAM_DEFS default for sigma_px is 1.0 (Allen phaseMapFilterSigma).
        assert_eq!(
            new["phase_smoothing"]["open_isi_amp_weighted_phasor"]["sigma_px"],
            json!(1.0)
        );
    }

    #[test]
    fn root_level_moved_fields_are_dropped() {
        // Very-old shape: stimulus-geometry fields at root that have
        // since moved to /experiment_params.
        let old = json!({
            "azi_angular_range": 120.0,
            "rotation_k": 2,
            "cycle_combine": { "method": "marshel_garrett2011_delay_subtraction" }
        });
        let new = translate_pre_2026_analysis_params(&old).unwrap();
        // No azi_angular_range at root of new tree.
        assert!(new.get("azi_angular_range").is_none());
        assert!(new.get("rotation_k").is_none());
        // cycle_combine still migrated.
        assert_eq!(
            new["cycle_combine"]["method"],
            json!("marshel_garrett2011_delay_subtraction")
        );
    }

    #[test]
    fn stage_absent_from_old_keeps_param_defs_defaults() {
        // Old tree contains only one stage; other stages must come
        // from PARAM_DEFS defaults in the new tree.
        let old = json!({
            "sign_map_smoothing": { "method": "gaussian", "sigma_um": 90.0 }
        });
        let new = translate_pre_2026_analysis_params(&old).unwrap();
        // sign_map_smoothing migrated.
        assert_eq!(
            new["sign_map_smoothing"]["gaussian"]["sigma_um"],
            json!(90.0)
        );
        // patch_threshold should be PARAM_DEFS default (Garrett k=1.5).
        assert_eq!(
            new["patch_threshold"]["method"],
            json!("garrett2014_sigma_scaled")
        );
        assert_eq!(
            new["patch_threshold"]["garrett2014_sigma_scaled"]["k"],
            json!(1.5)
        );
    }

    #[test]
    fn multi_field_variant_migrates_all_tunables() {
        // patch_refinement.allen_zhuang2017_split_merge — 8 tunables.
        let old = json!({
            "patch_refinement": {
                "method": "allen_zhuang2017_split_merge",
                "split_overlap_thr": 1.5,
                "merge_overlap_thr": 0.05
            }
        });
        let new = translate_pre_2026_analysis_params(&old).unwrap();
        let subtree = &new["patch_refinement"]["allen_zhuang2017_split_merge"];
        // Overridden values come from old.
        assert_eq!(subtree["split_overlap_thr"], json!(1.5));
        assert_eq!(subtree["merge_overlap_thr"], json!(0.05));
        // Unset fields take PARAM_DEFS defaults.
        assert_eq!(subtree["split_local_min_cut_step"], json!(5.0));
        assert_eq!(subtree["visual_space_close_iter"], json!(15));
        assert_eq!(subtree["small_patch_thr"], json!(100));
    }
}
