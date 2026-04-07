//! OpenISI headless CLI — runs acquisitions without the GUI.
//!
//! Uses the same backend code as the Tauri app: same config, same threads,
//! same export pipeline. For testing, validation, and scripted acquisition.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use openisi_lib::config::{ConfigManager, Experiment};
use openisi_lib::export::SweepSchedule;
use openisi_lib::messages::*;
use openisi_lib::monitor;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        return;
    }

    match args[1].as_str() {
        "info" => cmd_info(),
        "validate-display" => cmd_validate_display(&args[2..]),
        "validate-timing" => cmd_validate_timing(&args[2..]),
        "acquire" => cmd_acquire(&args[2..]),
        "analyze" => cmd_analyze(&args[2..]),
        "inspect" => cmd_inspect(&args[2..]),
        "import" => cmd_import(&args[2..]),
        "import-session" => cmd_import_session(&args[2..]),
        "test-read" => cmd_test_read(&args[2..]),
        "dump-h5" => cmd_dump_h5(&args[2..]),
        _ => {
            eprintln!("Unknown command: {}", args[1]);
            print_usage();
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
    eprintln!("  inspect <file.oisi>     Inspect .oisi file contents");
    eprintln!("  import <dir>            Import SNLC .mat directory to .oisi");
}

// ═══════════════════════════════════════════════════════════════════════
// Config loading
// ═══════════════════════════════════════════════════════════════════════

fn load_config() -> (ConfigManager, Experiment) {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    let candidates = vec![
        exe_dir.join("config"),
        exe_dir.join("../config"),
        exe_dir.join("../../config"),
    ];

    let config_dir = candidates.into_iter()
        .find(|p| p.join("rig.toml").exists())
        .unwrap_or_else(|| {
            panic!("Cannot find config directory with rig.toml");
        });

    let config = ConfigManager::load(&config_dir)
        .unwrap_or_else(|e| panic!("Failed to load rig config: {e}"));

    let exp_path = config.experiment_path();
    let experiment = Experiment::load(&exp_path)
        .unwrap_or_else(|e| panic!("Failed to load experiment: {e}"));

    (config, experiment)
}

// ═══════════════════════════════════════════════════════════════════════
// info
// ═══════════════════════════════════════════════════════════════════════

fn cmd_info() {
    let (config, experiment) = load_config();

    println!("=== Rig Config ===");
    println!("Camera: exposure={}µs gain={} target_fps={}",
        config.rig.camera.exposure_us, config.rig.camera.gain, config.rig.camera.target_fps);
    println!("Geometry: viewing_distance={}cm", config.rig.geometry.viewing_distance_cm);
    println!("Display: target_fps={} rotation={}°",
        config.rig.display.target_stimulus_fps, config.rig.display.monitor_rotation_deg);

    println!();
    println!("=== Experiment ===");
    println!("Envelope: {:?}", experiment.stimulus.envelope);
    println!("Carrier: {:?}", experiment.stimulus.carrier);
    println!("Conditions: {:?}", experiment.presentation.conditions);
    println!("Repetitions: {}", experiment.presentation.repetitions);
    println!("Baselines: {}/{}s", experiment.timing.baseline_start_sec, experiment.timing.baseline_end_sec);

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
            return;
        }
    };
    let cameras = sdk.enumerate_cameras(10);
    if cameras.is_empty() {
        println!("  No cameras found");
    } else {
        for c in &cameras {
            println!("  [{}] {} {}x{} {:.1}fps", c.index, c.name, c.width, c.height, c.max_fps);
        }
        // Open first camera to query capabilities.
        if let Ok(cam) = sdk.open_camera(cameras[0].index) {
            let info = cam.info();
            println!("  Pixel rates: {:?}", info.pixel_rates);
            println!("  Exposure range: {}ns .. {}ms", info.min_exposure_ns, info.max_exposure_ms);
            let (max_h, step_h, max_v, step_v) = cam.available_binning();
            println!("  Binning: max {}x{}, stepping h={} v={}", max_h, max_v, step_h, step_v);
            // Drop cam — closes camera properly.
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// validate-display
// ═══════════════════════════════════════════════════════════════════════

fn cmd_validate_display(args: &[String]) {
    let (config, _) = load_config();

    let monitors = monitor::detect_monitors();
    let idx: usize = args.first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(if monitors.len() > 1 { 1 } else { 0 });

    if idx >= monitors.len() {
        eprintln!("Monitor index {} out of range (have {})", idx, monitors.len());
        return;
    }

    let m = &monitors[idx];
    println!("Validating monitor [{}] {} @{}Hz...", idx, m.name, m.refresh_hz);

    let dxgi_output = match monitor::find_dxgi_output(idx) {
        Ok(o) => o,
        Err(e) => { eprintln!("Failed to find DXGI output: {e}"); return; }
    };

    let mut qpc_freq = 0i64;
    unsafe { let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut qpc_freq); }

    let sample_count = config.rig.system.display_validation_sample_count;
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
}

// ═══════════════════════════════════════════════════════════════════════
// validate-timing
// ═══════════════════════════════════════════════════════════════════════

fn cmd_validate_timing(args: &[String]) {
    let measure_sec: f64 = args.first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3.0);

    let (config, experiment) = load_config();

    let monitors = monitor::detect_monitors();
    let stim_idx = if monitors.len() > 1 { 1 } else { 0 };
    let mon = &monitors[stim_idx];

    println!("Measuring timing for {:.1}s...", measure_sec);
    println!("Camera: connecting...");

    // Open camera and start recording.
    let sdk = pco_sdk::Sdk::load().expect("PCO SDK required");
    let cameras = sdk.enumerate_cameras(10);
    if cameras.is_empty() {
        eprintln!("No cameras found");
        return;
    }
    let mut camera = sdk.open_camera(cameras[0].index).expect("Failed to open camera");
    let _rate = camera.set_max_pixel_rate().expect("Failed to set pixel rate");
    let binning = config.rig.camera.binning;
    if binning > 1 {
        camera.set_binning(binning, binning).expect("Failed to set binning");
    }
    camera.set_timestamp_binary().expect("Failed to set timestamp mode");
    camera.set_exposure_us(config.rig.camera.exposure_us).expect("Failed to set exposure");
    if let Err(e) = camera.arm() {
        eprintln!("Failed to arm camera: {e}");
        return;
    }
    println!("Camera: {}x{}", camera.width, camera.height);

    let mut recorder = camera.create_recorder(10).expect("Failed to create recorder");
    recorder.start().expect("Failed to start recording");

    // Wait for first frame.
    let deadline = std::time::Instant::now() + Duration::from_millis(
        config.rig.system.camera_first_frame_timeout_ms as u64
    );
    loop {
        if std::time::Instant::now() > deadline {
            eprintln!("Timed out waiting for first camera frame");
            return;
        }
        match recorder.get_latest_frame() {
            Ok(Some(_)) => break,
            Ok(None) => std::thread::sleep(Duration::from_millis(
                config.rig.system.camera_first_frame_poll_ms as u64
            )),
            Err(e) => { eprintln!("Frame error: {e}"); return; }
        }
    }

    // Start stimulus thread.
    let (stim_cmd_tx, stim_cmd_rx) = crossbeam_channel::unbounded();
    let (stim_evt_tx, stim_evt_rx) = crossbeam_channel::unbounded();
    let sys_cfg = config.rig.system.clone();
    let bg_lum = experiment.stimulus.params.background_luminance;
    let mon_idx = mon.index;
    let mon_w = mon.width_px;
    let mon_h = mon.height_px;
    let mon_pos = mon.position;

    std::thread::Builder::new()
        .name("stimulus".into())
        .spawn(move || {
            openisi_lib::stimulus_thread::run(
                stim_cmd_rx, stim_evt_tx, mon_idx, mon_w, mon_h, mon_pos, sys_cfg, bg_lum,
            );
        })
        .expect("Failed to spawn stimulus thread");

    // Wait for ready.
    loop {
        match stim_evt_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(StimulusEvt::Ready) => break,
            Ok(_) => {}
            Err(_) => { eprintln!("Stimulus thread timeout"); return; }
        }
    }

    // Start a preview to get stimulus vsync running.
    stim_cmd_tx.send(StimulusCmd::Preview(PreviewCommand {
        experiment: experiment.clone(),
        geometry: config.rig.geometry.clone(),
        monitor: openisi_lib::session::MonitorInfo {
            index: mon.index, name: mon.name.clone(),
            width_px: mon.width_px, height_px: mon.height_px,
            width_cm: mon.width_cm, height_cm: mon.height_cm,
            refresh_hz: mon.refresh_hz, position: mon.position,
            physical_source: mon.physical_source.clone(),
        },
    })).expect("Failed to start preview");

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
        std::thread::sleep(Duration::from_millis(config.rig.system.camera_poll_interval_ms as u64));
    }

    // Stop.
    let _ = recorder.stop();
    stim_cmd_tx.send(StimulusCmd::StopPreview).ok();
    stim_cmd_tx.send(StimulusCmd::Shutdown).ok();

    // Collect stimulus timestamps from vsync events that were emitted during preview.
    // The stimulus thread sent PreviewFrame events at ~10fps — not per-vsync.
    // For the stimulus rate, use the display validation measurement instead.
    // Run a quick WaitForVBlank measurement for stimulus rate.
    let dxgi_output = match monitor::find_dxgi_output(stim_idx) {
        Ok(o) => o,
        Err(e) => { eprintln!("DXGI: {e}"); return; }
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
        return;
    }

    // Clock offset uncertainty: std dev of (sys_ts - hw_ts) across frames.
    let offsets: Vec<f64> = cam_sys_timestamps.iter().zip(cam_hw_timestamps.iter())
        .map(|(&sys, &hw)| (sys - hw) as f64)
        .collect();
    let offset_mean = offsets.iter().sum::<f64>() / offsets.len() as f64;
    let offset_variance = offsets.iter()
        .map(|o| (o - offset_mean).powi(2))
        .sum::<f64>() / offsets.len() as f64;
    let clock_offset_uncertainty_us = offset_variance.sqrt();

    // Compute trial parameters from experiment + actual geometry.
    use openisi_stimulus::geometry::{DisplayGeometry, ProjectionType};

    let projection = match experiment.geometry.projection {
        openisi_lib::config::Projection::Cartesian => ProjectionType::Cartesian,
        openisi_lib::config::Projection::Spherical => ProjectionType::Spherical,
        openisi_lib::config::Projection::Cylindrical => ProjectionType::Cylindrical,
    };
    let geometry = DisplayGeometry::new(
        projection,
        config.rig.geometry.viewing_distance_cm,
        experiment.geometry.horizontal_offset_deg,
        experiment.geometry.vertical_offset_deg,
        mon.width_cm, mon.height_cm,
        mon.width_px, mon.height_px,
    );

    let p = &experiment.stimulus.params;
    let sweep_sec = match experiment.stimulus.envelope {
        openisi_lib::config::Envelope::Bar => {
            let total_travel = geometry.visual_field_width_deg() + p.stimulus_width_deg;
            total_travel / p.sweep_speed_deg_per_sec
        }
        openisi_lib::config::Envelope::Wedge => {
            360.0 / p.rotation_speed_deg_per_sec
        }
        openisi_lib::config::Envelope::Ring => {
            let total_travel = geometry.get_max_eccentricity_deg() + p.stimulus_width_deg;
            total_travel / p.expansion_speed_deg_per_sec
        }
        openisi_lib::config::Envelope::Fullfield => 0.0,
    };

    let n_conditions = experiment.presentation.conditions.len();
    let n_reps = experiment.presentation.repetitions as usize;
    let n_trials = n_conditions * n_reps;
    // Inter-trial interval: time between consecutive sweep onsets.
    let inter_trial_sec = sweep_sec + experiment.timing.inter_stimulus_sec;

    // Session duration from baselines + sweeps + intervals.
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

    println!("Sweep duration: {:.3}s ({:?} envelope)", sweep_sec, experiment.stimulus.envelope);
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
}

// ═══════════════════════════════════════════════════════════════════════
// acquire
// ═══════════════════════════════════════════════════════════════════════

fn cmd_acquire(args: &[String]) {
    let duration_sec: f64 = args.first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10.0);

    let (config, experiment) = load_config();

    let monitors = monitor::detect_monitors();
    let stim_idx = if monitors.len() > 1 { 1 } else { 0 };
    let monitor = &monitors[stim_idx];

    println!("Acquiring for {:.1}s on monitor [{}] {}", duration_sec, stim_idx, monitor.name);
    println!("Experiment: {:?} {:?}", experiment.stimulus.envelope, experiment.stimulus.carrier);

    // Camera setup.
    let sdk = pco_sdk::Sdk::load().expect("PCO SDK required");
    let cameras = sdk.enumerate_cameras(10);
    if cameras.is_empty() {
        eprintln!("No cameras found");
        return;
    }
    println!("Camera: {} {}x{}", cameras[0].name, cameras[0].width, cameras[0].height);

    let mut camera = sdk.open_camera(cameras[0].index).expect("Failed to open camera");
    let _rate = camera.set_max_pixel_rate().expect("Failed to set pixel rate");
    let binning = config.rig.camera.binning;
    if binning > 1 {
        if !camera.is_valid_binning(binning) {
            let (max_h, step_h, max_v, step_v) = camera.available_binning();
            eprintln!("Binning {}x{} not supported. Camera supports max {}x{} (stepping h={} v={})",
                binning, binning, max_h, max_v, step_h, step_v);
            return;
        }
        camera.set_binning(binning, binning).expect("Failed to set binning");
    }
    camera.set_timestamp_binary().expect("Failed to set timestamp mode");
    camera.set_exposure_us(config.rig.camera.exposure_us).expect("Failed to set exposure");
    if let Err(e) = camera.arm() {
        eprintln!("Failed to arm camera: {e}");
        eprintln!("This may indicate the binning/pixel rate/exposure combination is not supported by the USB interface.");
        eprintln!("Try binning=1, or reduce pixel rate.");
        return;
    }

    let cam_w = camera.width;
    let cam_h = camera.height;
    println!("Camera armed: {}x{}, exposure {}µs", cam_w, cam_h, config.rig.camera.exposure_us);

    // Stimulus thread.
    let (stim_cmd_tx, stim_cmd_rx) = crossbeam_channel::unbounded();
    let (stim_evt_tx, stim_evt_rx) = crossbeam_channel::unbounded();

    let sys_cfg = config.rig.system.clone();
    let bg_lum = experiment.stimulus.params.background_luminance;
    let mon_idx = monitor.index;
    let mon_w = monitor.width_px;
    let mon_h = monitor.height_px;
    let mon_pos = monitor.position;

    std::thread::Builder::new()
        .name("stimulus".into())
        .spawn(move || {
            openisi_lib::stimulus_thread::run(
                stim_cmd_rx, stim_evt_tx, mon_idx, mon_w, mon_h, mon_pos, sys_cfg, bg_lum,
            );
        })
        .expect("Failed to spawn stimulus thread");

    // Wait for stimulus ready.
    loop {
        match stim_evt_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(StimulusEvt::Ready) => { println!("Stimulus thread ready"); break; }
            Ok(_) => {}
            Err(_) => { eprintln!("Stimulus thread did not become ready in 10s"); return; }
        }
    }

    // Start acquisition.
    let acq_cmd = AcquisitionCommand {
        experiment: experiment.clone(),
        geometry: config.rig.geometry.clone(),
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
        display: config.rig.display.clone(),
        measured_refresh_hz: monitor.refresh_hz as f64,
        system: config.rig.system.clone(),
    };

    stim_cmd_tx.send(StimulusCmd::StartAcquisition(acq_cmd)).expect("Failed to start acquisition");
    println!("Acquisition started");

    // Camera recording.
    let mut recorder = camera.create_recorder(10).expect("Failed to create recorder");
    recorder.start().expect("Failed to start recording");

    // Wait for first frame.
    let deadline = Instant::now() + Duration::from_millis(config.rig.system.camera_first_frame_timeout_ms as u64);
    loop {
        if Instant::now() > deadline {
            eprintln!("Timed out waiting for first camera frame");
            return;
        }
        match recorder.get_latest_frame() {
            Ok(Some(_)) => break,
            Ok(None) => std::thread::sleep(Duration::from_millis(config.rig.system.camera_first_frame_poll_ms as u64)),
            Err(e) => { eprintln!("Frame read error: {e}"); return; }
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
        std::thread::sleep(Duration::from_millis(config.rig.system.camera_poll_interval_ms as u64));
    }

    // Stop.
    let _ = recorder.stop();
    stim_cmd_tx.send(StimulusCmd::Stop).expect("Failed to stop");

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
    let data_dir = &config.rig.paths.data_directory;
    let output_dir = if data_dir.is_empty() {
        std::env::current_dir().unwrap()
    } else {
        PathBuf::from(data_dir)
    };
    let _ = std::fs::create_dir_all(&output_dir);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("System clock before epoch")
        .as_secs();
    let output_path = output_dir.join(format!("acquisition_{ts}.oisi"));

    if let Some(ds) = &stim_dataset {
        match openisi_lib::export::write_oisi(
            &output_path,
            ds,
            camera_data,
            Some(&experiment),
            None,
            &sweep_schedule,
            None, // timing characterization
            None, // session metadata
            None, // anatomical
            false, // stopped early by timeout
        ) {
            Ok(summary) => println!("{summary}"),
            Err(e) => eprintln!("Export failed: {e}"),
        }
    } else {
        eprintln!("No stimulus dataset — skipping export");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// analyze
// ═══════════════════════════════════════════════════════════════════════

fn cmd_analyze(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: openisi-headless analyze <file.oisi>");
        return;
    }

    let (config, _) = load_config();
    let path = std::path::Path::new(&args[0]);

    let seg_params = config.rig.analysis.segmentation.as_ref().map(|s| {
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
        smoothing_sigma: config.rig.analysis.smoothing_sigma,
        rotation_k: config.rig.analysis.rotation_k,
        azi_angular_range: config.rig.analysis.azi_angular_range,
        alt_angular_range: config.rig.analysis.alt_angular_range,
        offset_azi: config.rig.analysis.offset_azi,
        offset_alt: config.rig.analysis.offset_alt,
        epsilon: config.rig.analysis.epsilon,
        segmentation: seg_params,
    };

    let progress = isi_analysis::SilentProgress;
    let cancel = std::sync::atomic::AtomicBool::new(false);

    println!("Analyzing {}...", path.display());
    match isi_analysis::analyze(path, &params, &progress, &cancel) {
        Ok(()) => println!("Analysis complete"),
        Err(e) => eprintln!("Analysis failed: {e}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// inspect
// ═══════════════════════════════════════════════════════════════════════

fn cmd_inspect(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: openisi-headless inspect <file.oisi>");
        return;
    }

    let path = std::path::Path::new(&args[0]);

    match isi_analysis::io::inspect(path) {
        Ok(caps) => {
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
                println!("Dimensions:   {}×{}", w, h);
            }

            // Dump unified timeline info if present.
            if let Ok(file) = hdf5::File::open(path) {
                // Camera unified timestamps.
                if let Ok(ds) = file.dataset("acquisition/camera/timestamps_sec") {
                    if let Ok(ts) = ds.read_1d::<f64>() {
                        let n = ts.len();
                        if n > 0 {
                            println!("\nUnified timeline (seconds from t=0):");
                            println!("  Camera frames:  {} (t=[{:.6} .. {:.6}]s)",
                                n, ts[0], ts[n - 1]);
                        }
                    }
                }
                // Stimulus unified timestamps.
                if let Ok(ds) = file.dataset("acquisition/stimulus/timestamps_sec") {
                    if let Ok(ts) = ds.read_1d::<f64>() {
                        let n = ts.len();
                        if n > 0 {
                            println!("  Stimulus frames: {} (t=[{:.6} .. {:.6}]s)",
                                n, ts[0], ts[n - 1]);
                        }
                    }
                }
                // Schedule unified timestamps.
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
                // Timing characterization.
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
                // Clock sync.
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
        }
        Err(e) => eprintln!("Failed to inspect: {e}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// import
// ═══════════════════════════════════════════════════════════════════════

fn cmd_import(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: openisi-headless import <mat-directory> [output.oisi]");
        return;
    }

    let dir = std::path::Path::new(&args[0]);
    if !dir.is_dir() {
        eprintln!("Not a directory: {}", dir.display());
        return;
    }

    let output = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        let name = dir.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "import".into());
        dir.parent().unwrap_or(dir).join(format!("{name}.oisi"))
    };

    println!("Importing {} → {}", dir.display(), output.display());

    match isi_analysis::io::import_snlc_directory(dir, &output) {
        Ok(()) => {
            println!("Import complete: {}", output.display());
            // Show what was created.
            match isi_analysis::io::inspect(&output) {
                Ok(caps) => {
                    println!("  Complex maps: {}", if caps.has_complex_maps { "yes" } else { "no" });
                    println!("  Anatomical:   {}", if caps.has_anatomical { "yes" } else { "no" });
                    if let Some((h, w)) = caps.dimensions {
                        println!("  Dimensions:   {}×{}", w, h);
                    }
                }
                Err(e) => eprintln!("  (inspect failed: {e})"),
            }
        }
        Err(e) => eprintln!("Import failed: {e}"),
    }
}

fn cmd_test_read(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: headless test-read <file.oisi>");
        return;
    }
    let path = &args[0];
    let file = match hdf5::File::open(path) {
        Ok(f) => f,
        Err(e) => { eprintln!("Failed to open: {e}"); return; }
    };

    // List all datasets in /results/
    let group = match file.group("results") {
        Ok(g) => g,
        Err(e) => { eprintln!("No results group: {e}"); return; }
    };

    let names = group.member_names().unwrap_or_default();
    println!("Results group has {} members:", names.len());

    for name in &names {
        match group.dataset(name) {
            Ok(ds) => {
                let shape = ds.shape();
                let ndim = shape.len();
                if ndim == 2 {
                    // Try reading as f64
                    if let Ok(arr) = ds.read_2d::<f64>() {
                        let (h, w) = arr.dim();
                        let min = arr.iter().cloned().fold(f64::INFINITY, f64::min);
                        let max = arr.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                        println!("  {name}: f64 {h}x{w} [{min:.4} .. {max:.4}]");
                        continue;
                    }
                    // Try as i32
                    if let Ok(arr) = ds.read_2d::<i32>() {
                        let (h, w) = arr.dim();
                        let max = *arr.iter().max().unwrap_or(&0);
                        println!("  {name}: i32 {h}x{w} max={max}");
                        continue;
                    }
                    // Try as u8
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
}

fn cmd_import_session(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: headless import-session <session-dir> [output.oisi]");
        return;
    }
    let session_dir = std::path::Path::new(&args[0]);
    let results_path = session_dir.join("analysis/retinotopy_results.h5");
    let anat_path = session_dir.join("anatomical.png");

    if !results_path.exists() {
        eprintln!("No retinotopy_results.h5 found in {}", session_dir.display());
        return;
    }

    let output = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        let name = session_dir.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or("session".into());
        session_dir.parent().unwrap_or(session_dir).join(format!("{name}.oisi"))
    };

    println!("Importing session {} → {}", session_dir.display(), output.display());

    // Read pre-computed results.
    let results_file = hdf5::File::open(&results_path).expect("Failed to open retinotopy_results.h5");

    // Read per-direction complex maps: magnitude * exp(i * phase)
    let read_complex = |dir: &str| -> Option<ndarray::Array2<isi_analysis::Complex64>> {
        let grp = results_file.group(dir).ok()?;
        let mag: ndarray::Array2<f64> = grp.dataset("magnitude").ok()?.read().ok()?;
        let phase: ndarray::Array2<f64> = grp.dataset("phase_radians").ok()?.read().ok()?;
        let (h, w) = mag.dim();
        Some(ndarray::Array2::from_shape_fn((h, w), |(r, c)| {
            isi_analysis::complex_from_polar(mag[[r, c]], phase[[r, c]])
        }))
    };

    let azi_fwd = read_complex("LR").expect("Missing LR");
    let azi_rev = read_complex("RL").expect("Missing RL");
    let alt_fwd = read_complex("TB").expect("Missing TB");
    let alt_rev = read_complex("BT").expect("Missing BT");

    let complex_maps = isi_analysis::ComplexMaps { azi_fwd, azi_rev, alt_fwd, alt_rev };

    // Create .oisi and write complex maps.
    isi_analysis::io::create(&output, "session_import").expect("Failed to create .oisi");
    isi_analysis::io::write_complex_maps(&output, &complex_maps).expect("Failed to write complex maps");

    // Import anatomical if present.
    if anat_path.exists() {
        if let Ok(img_bytes) = std::fs::read(&anat_path) {
            if let Ok(img) = image_to_gray_array(&img_bytes) {
                isi_analysis::io::write_anatomical(&output, &img).expect("Failed to write anatomical");
                println!("  Anatomical: {}x{}", img.dim().1, img.dim().0);
            }
        }
    }

    println!("Import complete: {}", output.display());

    // Run analysis.
    let (config, _) = load_config();
    let seg_params = config.rig.analysis.segmentation.as_ref().map(|s| {
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
        smoothing_sigma: config.rig.analysis.smoothing_sigma,
        rotation_k: config.rig.analysis.rotation_k,
        azi_angular_range: config.rig.analysis.azi_angular_range,
        alt_angular_range: config.rig.analysis.alt_angular_range,
        offset_azi: config.rig.analysis.offset_azi,
        offset_alt: config.rig.analysis.offset_alt,
        epsilon: config.rig.analysis.epsilon,
        segmentation: seg_params,
    };

    println!("Analyzing...");
    let progress = isi_analysis::SilentProgress;
    let cancel = std::sync::atomic::AtomicBool::new(false);
    match isi_analysis::analyze(&output, &params, &progress, &cancel) {
        Ok(()) => println!("Analysis complete"),
        Err(e) => eprintln!("Analysis failed: {e}"),
    }
}

/// Decode a PNG file to a grayscale Array2<u8>.
fn image_to_gray_array(png_bytes: &[u8]) -> Result<ndarray::Array2<u8>, String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    let mut reader = decoder.read_info().map_err(|e| format!("PNG decode: {e}"))?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("PNG frame: {e}"))?;
    let w = info.width as usize;
    let h = info.height as usize;

    let bytes_per_sample = match info.bit_depth {
        png::BitDepth::Sixteen => 2,
        _ => 1,
    };

    // Convert to grayscale u8 with auto-contrast for 16-bit.
    let gray: Vec<u8> = match (info.color_type, bytes_per_sample) {
        (png::ColorType::Grayscale, 1) => buf[..h * w].to_vec(),
        (png::ColorType::Grayscale, 2) => {
            // 16-bit grayscale: read as big-endian u16, auto-contrast to u8.
            let pixels: Vec<u16> = buf[..h * w * 2].chunks(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            let min = *pixels.iter().min().unwrap_or(&0);
            let max = *pixels.iter().max().unwrap_or(&0);
            let range = (max - min).max(1) as f64;
            pixels.iter().map(|&p| ((p - min) as f64 / range * 255.0) as u8).collect()
        }
        (png::ColorType::GrayscaleAlpha, _) => {
            let step = 2 * bytes_per_sample;
            buf[..h * w * step].chunks(step).map(|c| if bytes_per_sample == 2 { (u16::from_be_bytes([c[0], c[1]]) >> 8) as u8 } else { c[0] }).collect()
        }
        (png::ColorType::Rgb, _) => {
            let step = 3 * bytes_per_sample;
            buf[..h * w * step].chunks(step).map(|c| ((c[0] as u16 + c[1] as u16 + c[2] as u16) / 3) as u8).collect()
        }
        (png::ColorType::Rgba, _) => {
            let step = 4 * bytes_per_sample;
            buf[..h * w * step].chunks(step).map(|c| ((c[0] as u16 + c[1] as u16 + c[2] as u16) / 3) as u8).collect()
        }
        _ => return Err("Unsupported color type".into()),
    };

    ndarray::Array2::from_shape_vec((h, w), gray).map_err(|e| format!("Shape: {e}"))
}

fn cmd_dump_h5(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: headless dump-h5 <file.h5>");
        return;
    }
    let file = match hdf5::File::open(&args[0]) {
        Ok(f) => f,
        Err(e) => { eprintln!("Failed to open: {e}"); return; }
    };
    println!("File: {}", args[0]);
    dump_group(&file, "", 0);
}

fn dump_group(loc: &hdf5::Group, prefix: &str, depth: usize) {
    let indent = "  ".repeat(depth);
    let names = loc.member_names().unwrap_or_default();
    for name in &names {
        let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        if let Ok(ds) = loc.dataset(name) {
            let shape = ds.shape();
            let dtype_str = ds.dtype().map(|d| format!("{:?}", d)).unwrap_or_else(|_| "?".into());
            // Try to show a sample value
            if shape.len() == 0 {
                println!("{indent}{name}: scalar");
            } else if shape.len() == 1 && shape[0] <= 20 {
                if let Ok(arr) = ds.read_1d::<f64>() {
                    println!("{indent}{name}: f64[{}] = {:?}", shape[0], &arr.to_vec()[..shape[0].min(10)]);
                } else if let Ok(arr) = ds.read_1d::<i64>() {
                    println!("{indent}{name}: i64[{}] = {:?}", shape[0], &arr.to_vec()[..shape[0].min(10)]);
                } else {
                    println!("{indent}{name}: {:?} dtype={dtype_str}", shape);
                }
            } else if shape.len() == 2 {
                if let Ok(arr) = ds.read_2d::<f64>() {
                    let min = arr.iter().cloned().fold(f64::INFINITY, f64::min);
                    let max = arr.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                    println!("{indent}{name}: f64 {}x{} [{min:.4} .. {max:.4}]", shape[0], shape[1]);
                } else if let Ok(arr) = ds.read_2d::<u16>() {
                    let min = *arr.iter().min().unwrap_or(&0);
                    let max = *arr.iter().max().unwrap_or(&0);
                    println!("{indent}{name}: u16 {}x{} [{min} .. {max}]", shape[0], shape[1]);
                } else {
                    println!("{indent}{name}: {:?} dtype={dtype_str}", shape);
                }
            } else if shape.len() == 3 {
                println!("{indent}{name}: {:?} dtype={dtype_str}", shape);
            } else {
                println!("{indent}{name}: {:?}", shape);
            }
        } else if let Ok(grp) = loc.group(name) {
            println!("{indent}{name}/");
            dump_group(&grp, &path, depth + 1);
        }
        // Check for attributes
        if let Ok(grp) = loc.group(name) {
            for attr_name in grp.attr_names().unwrap_or_default() {
                if let Ok(attr) = grp.attr(&attr_name) {
                    println!("{indent}  @{attr_name}");
                }
            }
        }
    }
}
