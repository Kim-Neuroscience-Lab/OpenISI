//! OpenISI headless CLI — runs acquisitions without the GUI.
//!
//! Uses the same backend code as the Tauri app: same config, same threads,
//! same export pipeline. For testing, validation, and scripted acquisition.
//!
//! Analysis commands (analyze, inspect, import, import-session) work on all
//! platforms. Hardware commands (info, validate-display, validate-timing,
//! acquire) require Windows (DXGI, QPC, PCO SDK).

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use openisi_lib::error::{AppError, AppResult};
use openisi_params::config::ConfigStore;

mod figures;
use figures::{
    compare_method_variants, default_figures_dir, export_all_figures, export_threshold_sweep_grids,
    repo_root, write_meta_json,
};

// Windows-only imports for hardware commands.
#[cfg(windows)]
use openisi_lib::export::SweepSchedule;
#[cfg(windows)]
use openisi_lib::messages::*;
#[cfg(windows)]
use openisi_lib::monitor;
#[cfg(windows)]
use std::time::{Duration, Instant};

/// Public entry point. Thin wrapper around `try_main()` that prints any
/// error in a single readable line and exits non-zero. No panic, no
/// stack trace — same shape as the Tauri `run()` wrapper.
fn main() {
    if let Err(e) = try_main() {
        eprintln!("openisi: {e}");
        std::process::exit(1);
    }
}

/// OpenISI headless CLI. Subcommands and their args are defined declaratively
/// via `clap` derive — `--help`, usage, version, and arg validation come for
/// free, and the command list has a single source of truth (this enum), no
/// hand-maintained `match` + usage string.
#[derive(Parser)]
#[command(name = "openisi-headless", about = "OpenISI headless CLI", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show detected hardware and config (Windows only).
    Info,
    /// Validate the stimulus display timing (Windows only).
    ValidateDisplay {
        /// Monitor index (default: monitor 1 if multiple, else 0).
        monitor: Option<usize>,
    },
    /// Measure camera + stimulus rates simultaneously (Windows only).
    ValidateTiming {
        /// Measurement window in seconds (default: 3).
        seconds: Option<f64>,
    },
    /// Run an acquisition: stimulus + camera (Windows only).
    Acquire {
        /// Acquisition duration in seconds (default: 10).
        duration_sec: Option<f64>,
        /// Render the stimulus on the main monitor when only one display exists.
        #[arg(long, short)]
        primary: bool,
        /// Gate pass/fail on drops/rate (stimulus-timing auto-skips over RDP).
        #[arg(long)]
        validate: bool,
    },
    /// Camera-only quality test (Windows only).
    CameraCheck {
        /// Capture duration in seconds (default: 10).
        duration_sec: Option<f64>,
    },
    /// Run the analysis pipeline on an `.oisi` file.
    Analyze {
        /// Path to the `.oisi` file.
        file: PathBuf,
        /// Export figures. With no value, auto-tags into `dev_figures/`;
        /// with a directory, writes there verbatim.
        #[arg(long, num_args = 0..=1, value_name = "DIR")]
        figures: Option<Option<PathBuf>>,
        /// Also emit VFS-threshold sweep grids (requires `--figures`).
        #[arg(long)]
        threshold_sweep: bool,
        /// Also re-run the pipeline per method-variant and stitch grids
        /// (requires `--figures`).
        #[arg(long)]
        compare_methods: bool,
    },
    /// DFT / phase diagnostic on an `.oisi` file.
    DftCheck {
        /// Path to the `.oisi` file.
        file: PathBuf,
    },
    /// Upgrade a pre-2026 `.oisi`'s `/analysis_params` to the current schema.
    Migrate {
        /// Path to the `.oisi` file.
        file: PathBuf,
    },
    /// Inspect an `.oisi` file's contents.
    Inspect {
        /// Path to the `.oisi` file.
        file: PathBuf,
    },
    /// Export an `.oisi` file to a reference-valid NWB file (DANDI-submittable).
    ///
    /// A pure transformation via the `tools/export_nwb` Python bridge (pynwb +
    /// the `ndx-openisi` extension) — the native `.oisi` is unchanged. Requires
    /// Python with `pynwb` installed at export time only (see
    /// `tools/export_nwb/requirements.txt`). Conformance is guaranteed by the
    /// reference implementation, not by this CLI.
    ExportNwb {
        /// Path to the `.oisi` file.
        file: PathBuf,
        /// Output `.nwb` path (default: same stem, `.nwb` extension).
        output: Option<PathBuf>,
        /// Optional JSON sidecar of DANDI-required metadata the `.oisi` does not
        /// capture (subject age/sex/species, experimenter, …).
        #[arg(long)]
        metadata: Option<PathBuf>,
    },
    /// Import an SNLC `.mat` directory into a new `.oisi`.
    Import {
        /// Directory of SNLC `.mat` files.
        mat_dir: PathBuf,
        /// Output `.oisi` path (default: derived from the directory).
        output: Option<PathBuf>,
    },
    /// Download the SNLC sample bundle and import each subject.
    ImportSamples,
    /// Import a legacy session directory into an `.oisi`.
    ImportSession {
        /// Session directory.
        session_dir: PathBuf,
        /// Output `.oisi` path (default: derived from the directory).
        output: Option<PathBuf>,
    },
    /// Dump an `.oisi` `/results` group.
    TestRead {
        /// Path to the `.oisi` file.
        file: PathBuf,
    },
    /// Dump an HDF5 file's group/dataset structure.
    DumpH5 {
        /// Path to the HDF5 file.
        file: PathBuf,
    },
}

fn try_main() -> AppResult<()> {
    openisi_lib::logging::init();
    openisi_lib::begin_realtime_timer();
    let cli = Cli::parse();

    match cli.command {
        Commands::Info => cmd_info(),
        Commands::ValidateDisplay { monitor } => cmd_validate_display(monitor),
        Commands::ValidateTiming { seconds } => cmd_validate_timing(seconds),
        Commands::Acquire {
            duration_sec,
            primary,
            validate,
        } => cmd_acquire(duration_sec, primary, validate),
        Commands::CameraCheck { duration_sec } => cmd_camera_check(duration_sec),
        Commands::Analyze {
            file,
            figures,
            threshold_sweep,
            compare_methods,
        } => cmd_analyze(&file, figures, threshold_sweep, compare_methods),
        Commands::DftCheck { file } => cmd_dft_check(&file),
        Commands::Migrate { file } => cmd_migrate(&file),
        Commands::Inspect { file } => cmd_inspect(&file),
        Commands::ExportNwb {
            file,
            output,
            metadata,
        } => cmd_export_nwb(&file, output.as_deref(), metadata.as_deref()),
        Commands::Import { mat_dir, output } => cmd_import(&mat_dir, output.as_deref()),
        Commands::ImportSamples => cmd_import_samples(),
        Commands::ImportSession {
            session_dir,
            output,
        } => cmd_import_session(&session_dir, output.as_deref()),
        Commands::TestRead { file } => cmd_test_read(&file),
        Commands::DumpH5 { file } => cmd_dump_h5(&file),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Config loading
// ═══════════════════════════════════════════════════════════════════════

fn load_config_store() -> AppResult<ConfigStore> {
    let exe_dir = std::env::current_exe()
        .map_err(|e| AppError::Config(format!("locate current executable: {e}")))?
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| AppError::Config("current executable has no parent directory".into()))?;

    // Prefer the repo `config/` anchored to the compile-time source path
    // (`CARGO_MANIFEST_DIR`, via `repo_root()`) FIRST. In a dev run that is the
    // source-tree config the user actually edits, and — unlike the exe-relative
    // search — it cannot be shadowed by a stale `target/<profile>/config` build
    // copy (the bug that silently ignored config edits). Shipped builds, where
    // the compile-time manifest path no longer exists, fall through to the
    // exe-relative candidates.
    let candidates = vec![
        repo_root().join("config"),
        exe_dir.join("config"),
        exe_dir.join("../config"),
        exe_dir.join("../../config"),
    ];
    let candidate_paths: Vec<String> = candidates.iter().map(|p| p.display().to_string()).collect();

    let config_dir = candidates
        .into_iter()
        .find(|p| p.join("rig.json").exists())
        .ok_or_else(|| {
            AppError::Config(format!(
                "cannot find config directory with rig.json. Searched: {}",
                candidate_paths.join(", ")
            ))
        })?;

    // Behavior-preserving placeholder: shipped == user == config_dir,
    // pending proper dev/prod path resolution.
    let mut store = ConfigStore::new(&config_dir, &config_dir);
    store.load_rig().map_err(|e| {
        AppError::Config(format!(
            "load rig config from {}: {e}",
            config_dir.display()
        ))
    })?;
    store.load_analysis().map_err(|e| {
        AppError::Config(format!(
            "load analysis config from {}: {e}",
            config_dir.display()
        ))
    })?;
    store.load_experiment().map_err(|e| {
        AppError::Config(format!(
            "load experiment from {}: {e}",
            config_dir.display()
        ))
    })?;

    Ok(store)
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
fn cmd_validate_display(_monitor: Option<usize>) -> AppResult<()> {
    Err(AppError::NotAvailable(
        "'validate-display' requires Windows (DXGI WaitForVBlank)".into(),
    ))
}

#[cfg(not(windows))]
fn cmd_validate_timing(_seconds: Option<f64>) -> AppResult<()> {
    Err(AppError::NotAvailable(
        "'validate-timing' requires Windows (DXGI, QPC, PCO SDK)".into(),
    ))
}

#[cfg(not(windows))]
fn cmd_acquire(_duration_sec: Option<f64>, _primary: bool, _validate: bool) -> AppResult<()> {
    Err(AppError::NotAvailable(
        "'acquire' requires Windows (DXGI, QPC, PCO SDK)".into(),
    ))
}

#[cfg(not(windows))]
fn cmd_camera_check(_duration_sec: Option<f64>) -> AppResult<()> {
    Err(AppError::NotAvailable(
        "'camera-check' requires Windows (PCO SDK)".into(),
    ))
}

#[cfg(windows)]
fn cmd_info() -> AppResult<()> {
    let reg = load_config_store()?;
    let snap = reg.snapshot();

    println!("=== Rig Config ===");
    println!(
        "Camera: exposure={}µs binning={}",
        snap.rig.camera.exposure_us,
        snap.rig.camera.binning
    );
    println!(
        "Geometry: viewing_distance={}cm",
        snap.rig.geometry.viewing_distance_cm
    );
    println!(
        "Display: target_fps={} rotation={}°",
        snap.rig.display.target_stimulus_fps,
        snap.rig.display.monitor_rotation_deg
    );

    println!();
    println!("=== Experiment ===");
    println!("Envelope: {:?}", snap.experiment.stimulus.envelope);
    println!("Carrier: {:?}", snap.experiment.stimulus.carrier);
    println!("Conditions: {:?}", snap.experiment.presentation.conditions);
    println!("Repetitions: {}", snap.experiment.presentation.repetitions);
    println!(
        "Baselines: {}/{}s",
        snap.experiment.timing.baseline_start_sec,
        snap.experiment.timing.baseline_end_sec
    );

    println!();
    println!("=== Monitors ===");
    let monitors = monitor::detect_monitors();
    for m in &monitors {
        println!(
            "  [{}] {} {}x{} @{}Hz {:.1}x{:.1}cm at ({},{})",
            m.index,
            m.name,
            m.width_px,
            m.height_px,
            m.refresh_hz,
            m.width_cm,
            m.height_cm,
            m.position.0,
            m.position.1
        );
    }

    println!();
    println!("=== Camera ===");
    let sdk = match pco_sdk::Sdk::load() {
        Ok(sdk) => {
            println!("PCO SDK loaded");
            sdk
        }
        Err(e) => {
            return Err(AppError::Hardware(format!("PCO SDK not available: {e}")));
        }
    };
    let cameras = sdk.enumerate_cameras(10);
    if cameras.is_empty() {
        println!("  No cameras found");
    } else {
        for c in &cameras {
            println!(
                "  [{}] {} {}x{} {:.1}fps",
                c.index, c.name, c.width, c.height, c.max_fps
            );
        }
        if let Ok(cam) = sdk.open_camera(cameras[0].index) {
            let info = cam.info();
            println!("  Pixel rates: {:?}", info.pixel_rates);
            println!(
                "  Exposure range: {}ns .. {}ms",
                info.min_exposure_ns, info.max_exposure_ms
            );
            let (max_h, step_h, max_v, step_v) = cam.available_binning();
            println!(
                "  Binning: max {}x{}, stepping h={} v={}",
                max_h, max_v, step_h, step_v
            );
        }
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// validate-display
// ═══════════════════════════════════════════════════════════════════════

#[cfg(windows)]
fn cmd_validate_display(monitor: Option<usize>) -> AppResult<()> {
    let reg = load_config_store()?;
    let snap = reg.snapshot();

    let monitors = monitor::detect_monitors();
    let idx: usize = monitor.unwrap_or(if monitors.len() > 1 { 1 } else { 0 });

    if idx >= monitors.len() {
        return Err(AppError::Validation(format!(
            "Monitor index {} out of range (have {})",
            idx,
            monitors.len()
        )));
    }

    let m = &monitors[idx];
    println!(
        "Validating monitor [{}] {} @{}Hz...",
        idx, m.name, m.refresh_hz
    );

    let dxgi_output = match monitor::find_dxgi_output(idx) {
        Ok(o) => o,
        Err(e) => {
            return Err(AppError::Hardware(format!(
                "Failed to find DXGI output: {e}"
            )));
        }
    };

    let mut qpc_freq = 0i64;
    unsafe {
        let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut qpc_freq);
    }
    if qpc_freq == 0 {
        return Err(AppError::Hardware(
            "QueryPerformanceFrequency returned 0".into(),
        ));
    }

    let sample_count = snap.rig.system.display_validation_sample_count;
    let warmup = 30u32;
    let total = sample_count + warmup;
    let mut timestamps = Vec::with_capacity(total as usize);

    for _ in 0..total {
        unsafe {
            let _ = dxgi_output.WaitForVBlank();
        }
        let mut qpc = 0i64;
        unsafe {
            let _ = windows::Win32::System::Performance::QueryPerformanceCounter(&mut qpc);
        }
        timestamps.push(qpc);
    }

    let valid = &timestamps[warmup as usize..];
    let deltas_us: Vec<f64> = valid
        .windows(2)
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
        println!(
            "WARNING: Measured differs from reported by {:.1}%",
            mismatch * 100.0
        );
    } else {
        println!("Match:    OK ({:.1}% difference)", mismatch * 100.0);
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// validate-timing
// ═══════════════════════════════════════════════════════════════════════

#[cfg(windows)]
fn cmd_validate_timing(seconds: Option<f64>) -> AppResult<()> {
    let measure_sec: f64 = seconds.unwrap_or(3.0);

    let reg = load_config_store()?;
    let snap = reg.snapshot();

    let monitors = monitor::detect_monitors();
    if monitors.is_empty() {
        return Err(AppError::Hardware("no monitors detected".into()));
    }
    let stim_idx = if monitors.len() > 1 { 1 } else { 0 };
    let mon = &monitors[stim_idx];

    println!("Measuring timing for {:.1}s...", measure_sec);
    println!("Camera: connecting...");

    // Open camera and start recording.
    let sdk =
        pco_sdk::Sdk::load().map_err(|e| AppError::Hardware(format!("PCO SDK required: {e}")))?;
    let cameras = sdk.enumerate_cameras(10);
    if cameras.is_empty() {
        return Err(AppError::Hardware("No cameras found".into()));
    }
    let mut camera = sdk
        .open_camera(cameras[0].index)
        .map_err(|e| AppError::Hardware(format!("Failed to open camera: {e}")))?;
    let _rate = camera
        .set_max_pixel_rate()
        .map_err(|e| AppError::Hardware(format!("Failed to set pixel rate: {e}")))?;
    let binning = snap.rig.camera.binning;
    if binning > 1 {
        camera
            .set_binning(binning, binning)
            .map_err(|e| AppError::Hardware(format!("Failed to set binning: {e}")))?;
    }
    camera
        .set_timestamp_binary()
        .map_err(|e| AppError::Hardware(format!("Failed to set timestamp mode: {e}")))?;
    camera
        .set_exposure_us(snap.rig.camera.exposure_us)
        .map_err(|e| AppError::Hardware(format!("Failed to set exposure: {e}")))?;
    if let Err(e) = camera.arm() {
        return Err(AppError::Hardware(format!("Failed to arm camera: {e}")));
    }
    println!("Camera: {}x{}", camera.width, camera.height);

    let mut recorder = camera
        .create_recorder(10)
        .map_err(|e| AppError::Hardware(format!("Failed to create recorder: {e}")))?;
    recorder
        .start()
        .map_err(|e| AppError::Hardware(format!("Failed to start recording: {e}")))?;

    // Wait for first frame.
    let deadline = std::time::Instant::now()
        + Duration::from_millis(snap.rig.system.camera_first_frame_timeout_ms as u64);
    loop {
        if std::time::Instant::now() > deadline {
            return Err(AppError::Hardware(
                "Timed out waiting for first camera frame".into(),
            ));
        }
        match recorder.get_latest_frame() {
            Ok(Some(_)) => break,
            Ok(None) => std::thread::sleep(Duration::from_millis(
                snap.rig.system.camera_first_frame_poll_ms as u64,
            )),
            Err(e) => {
                return Err(AppError::Hardware(format!("Frame error: {e}")));
            }
        }
    }

    // Start stimulus thread.
    let (stim_cmd_tx, stim_cmd_rx) = crossbeam_channel::unbounded();
    let (stim_evt_tx, stim_evt_rx) = crossbeam_channel::unbounded();
    let bg_lum = snap.experiment.stimulus.params.background_luminance;
    let preview_width_px = snap.rig.system.preview_width_px;
    let preview_interval_ms = snap.rig.system.preview_interval_ms;
    let preview_cycle_sec = snap.rig.system.preview_cycle_sec;
    let idle_sleep_ms = snap.rig.system.idle_sleep_ms;
    let drop_detection_warmup_frames = snap.rig.system.drop_detection_warmup_frames;
    let mon_idx = mon.index;
    let mon_w = mon.width_px;
    let mon_h = mon.height_px;
    let mon_pos = mon.position;

    std::thread::Builder::new()
        .name("stimulus".into())
        .spawn(move || {
            let config = openisi_lib::stimulus_thread::StimulusConfig {
                monitor_index: mon_idx,
                monitor_width_px: mon_w,
                monitor_height_px: mon_h,
                monitor_position: mon_pos,
                preview_width_px,
                preview_interval_ms,
                preview_cycle_sec,
                idle_sleep_ms,
                drop_detection_warmup_frames,
                initial_bg_luminance: bg_lum,
            };
            openisi_lib::stimulus_thread::run(stim_cmd_rx, stim_evt_tx, config);
        })
        .map_err(|e| AppError::Hardware(format!("Failed to spawn stimulus thread: {e}")))?;

    // Wait for ready.
    loop {
        match stim_evt_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(StimulusEvt::Ready) => break,
            Ok(_) => {}
            Err(_) => {
                return Err(AppError::Hardware("Stimulus thread timeout".into()));
            }
        }
    }

    // Start a preview to get stimulus vsync running.
    stim_cmd_tx
        .send(StimulusCmd::Preview(PreviewCommand {
            snapshot: snap.clone(),
            monitor: openisi_lib::session::MonitorInfo {
                index: mon.index,
                name: mon.name.clone(),
                width_px: mon.width_px,
                height_px: mon.height_px,
                width_cm: mon.width_cm,
                height_cm: mon.height_cm,
                refresh_hz: mon.refresh_hz,
                position: mon.position,
                physical_source: mon.physical_source.clone(),
            },
        }))
        .map_err(|e| AppError::Hardware(format!("Failed to start preview: {e}")))?;

    println!("Collecting timestamps...");

    let qpc_freq = {
        let mut f = 0i64;
        unsafe {
            let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut f);
        }
        f
    };
    if qpc_freq == 0 {
        return Err(AppError::Hardware(
            "QueryPerformanceFrequency returned 0".into(),
        ));
    }

    // Collect camera hardware timestamps.
    let mut cam_hw_timestamps: Vec<i64> = Vec::new();
    let mut cam_sys_timestamps: Vec<i64> = Vec::new();
    let start = std::time::Instant::now();

    while start.elapsed() < Duration::from_secs_f64(measure_sec) {
        match recorder.get_latest_frame() {
            Ok(Some(frame)) => {
                cam_hw_timestamps.push(frame.timestamp.to_us_since_midnight());
                let mut qpc = 0i64;
                unsafe {
                    let _ = windows::Win32::System::Performance::QueryPerformanceCounter(&mut qpc);
                }
                cam_sys_timestamps.push(((qpc as i128 * 1_000_000) / qpc_freq as i128) as i64);
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("Frame error: {e}");
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(snap.rig.system.camera_poll_interval_ms as u64));
    }

    // Stop.
    let _ = recorder.stop();
    stim_cmd_tx.send(StimulusCmd::StopPreview).ok();
    stim_cmd_tx.send(StimulusCmd::Shutdown).ok();

    let dxgi_output = match monitor::find_dxgi_output(stim_idx) {
        Ok(o) => o,
        Err(e) => {
            return Err(AppError::Hardware(format!("DXGI: {e}")));
        }
    };
    let mut stim_timestamps: Vec<i64> = Vec::new();
    for _ in 0..200 {
        unsafe {
            let _ = dxgi_output.WaitForVBlank();
        }
        let mut qpc = 0i64;
        unsafe {
            let _ = windows::Win32::System::Performance::QueryPerformanceCounter(&mut qpc);
        }
        stim_timestamps.push(((qpc as i128 * 1_000_000) / qpc_freq as i128) as i64);
    }

    // Compute deltas.
    let cam_deltas: Vec<f64> = cam_hw_timestamps
        .windows(2)
        .map(|w| (w[1] - w[0]) as f64)
        .collect();
    let stim_deltas: Vec<f64> = stim_timestamps
        .windows(2)
        .map(|w| (w[1] - w[0]) as f64)
        .collect();

    if cam_deltas.is_empty() || stim_deltas.is_empty() {
        return Err(AppError::Validation("Not enough samples collected".into()));
    }

    let offsets: Vec<f64> = cam_sys_timestamps
        .iter()
        .zip(cam_hw_timestamps.iter())
        .map(|(&sys, &hw)| (sys - hw) as f64)
        .collect();
    let offset_mean = offsets.iter().sum::<f64>() / offsets.len() as f64;
    let offset_variance = offsets
        .iter()
        .map(|o| (o - offset_mean).powi(2))
        .sum::<f64>()
        / offsets.len() as f64;
    let clock_offset_uncertainty_us = offset_variance.sqrt();

    use openisi_stimulus::geometry::DisplayGeometry;

    let geometry = DisplayGeometry::new(
        snap.experiment.geometry.projection,
        snap.rig.geometry.viewing_distance_cm,
        snap.experiment.geometry.horizontal_offset_deg,
        snap.experiment.geometry.vertical_offset_deg,
        snap.rig.geometry.bisector_x_cm,
        snap.rig.geometry.bisector_y_cm,
        snap.rig.geometry.monitor_width_cm,
        snap.rig.geometry.monitor_height_cm,
        mon.width_px,
        mon.height_px,
    );

    let envelope = snap.experiment.stimulus.envelope;
    let sweep_sec = match envelope {
        openisi_lib::params::Envelope::Bar => {
            let total_travel = geometry.visual_field_width_deg() + snap.experiment.stimulus.params.stimulus_width_deg;
            total_travel / snap.experiment.stimulus.params.sweep_speed_deg_per_sec
        }
        openisi_lib::params::Envelope::Wedge => 360.0 / snap.experiment.stimulus.params.rotation_speed_deg_per_sec,
        openisi_lib::params::Envelope::Ring => {
            let total_travel = geometry.get_max_eccentricity_deg() + snap.experiment.stimulus.params.stimulus_width_deg;
            total_travel / snap.experiment.stimulus.params.expansion_speed_deg_per_sec
        }
        openisi_lib::params::Envelope::Fullfield => 0.0,
    };

    let n_conditions = snap.experiment.presentation.conditions.len();
    let n_reps = snap.experiment.presentation.repetitions as usize;
    let n_trials = n_conditions * n_reps;
    let inter_trial_sec = sweep_sec + snap.experiment.timing.inter_stimulus_sec;

    let total_sweep_time = n_trials as f64 * sweep_sec;
    let total_inter_stim = if n_trials > 1 {
        (n_trials - 1) as f64 * snap.experiment.timing.inter_stimulus_sec
    } else {
        0.0
    };
    let total_inter_dir = if n_conditions > 1 {
        (n_conditions - 1) as f64 * snap.experiment.timing.inter_direction_sec * n_reps as f64
    } else {
        0.0
    };
    let session_sec = snap.experiment.timing.baseline_start_sec
        + total_sweep_time
        + total_inter_stim
        + total_inter_dir
        + snap.experiment.timing.baseline_end_sec;

    println!(
        "Sweep duration: {:.3}s ({:?} envelope)",
        sweep_sec, envelope
    );
    println!(
        "Session duration: {:.1}s ({} trials)",
        session_sec, n_trials
    );

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
    println!(
        "Clock offset: mean={:.1}µs, uncertainty={:.1}µs",
        offset_mean, clock_offset_uncertainty_us
    );
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// acquire
// ═══════════════════════════════════════════════════════════════════════

/// Choose which monitor the stimulus renders on — explicitly, with no silent
/// fallback.
///
/// A real experiment needs a *dedicated* stimulus display (the correct size,
/// refresh rate, and gamma for the animal); silently rendering on the operator's
/// main monitor would produce invalid data with no warning. So the policy is:
/// use a secondary monitor when one is present, and otherwise **refuse** —
/// unless `allow_primary` is set, the deliberate test-only escape hatch for
/// single-display machines (e.g. an RDP session) where the goal is to exercise
/// the acquisition pipeline, not to run a real experiment.
#[cfg(windows)]
fn select_stimulus_monitor(
    monitors: &[openisi_lib::session::MonitorInfo],
    allow_primary: bool,
) -> AppResult<usize> {
    if monitors.is_empty() {
        return Err(AppError::Hardware("no monitors detected".into()));
    }
    if monitors.len() > 1 {
        return Ok(1);
    }
    if allow_primary {
        println!(
            "TEST MODE: only one monitor detected — rendering the stimulus on the PRIMARY \
             display. For pipeline testing only; real acquisition needs a dedicated stimulus \
             monitor."
        );
        Ok(0)
    } else {
        Err(AppError::Hardware(
            "only one monitor detected; acquisition requires a dedicated stimulus monitor. Pass \
             --primary to render on the main display for testing on a single-monitor machine."
                .into(),
        ))
    }
}

/// Open the first PCO camera, apply the config-snapshot configuration (max
/// pixel rate, binning, hardware binary timestamps, exposure) and arm it.
///
/// This is the single shared camera-setup path for every headless hardware
/// command, so the acquisition run and the `camera-check` test configure the
/// camera identically — no divergent copies. Every step returns a typed `Err`
/// on failure; the camera is never reported as ready when a setup step failed.
#[cfg(windows)]
fn open_armed_camera<'a>(
    sdk: &'a pco_sdk::Sdk,
    snap: &openisi_params::config::ConfigSnapshot,
) -> AppResult<pco_sdk::Camera<'a>> {
    let cameras = sdk.enumerate_cameras(10);
    if cameras.is_empty() {
        return Err(AppError::Hardware("No cameras found".into()));
    }
    println!(
        "Camera: {} {}x{}",
        cameras[0].name, cameras[0].width, cameras[0].height
    );

    let mut camera = sdk
        .open_camera(cameras[0].index)
        .map_err(|e| AppError::Hardware(format!("Failed to open camera: {e}")))?;
    camera
        .set_max_pixel_rate()
        .map_err(|e| AppError::Hardware(format!("Failed to set pixel rate: {e}")))?;
    let binning = snap.rig.camera.binning;
    if binning > 1 {
        if !camera.is_valid_binning(binning) {
            let (max_h, step_h, max_v, step_v) = camera.available_binning();
            return Err(AppError::Validation(format!(
                "Binning {}x{} not supported. Camera supports max {}x{} (stepping h={} v={})",
                binning, binning, max_h, max_v, step_h, step_v
            )));
        }
        camera
            .set_binning(binning, binning)
            .map_err(|e| AppError::Hardware(format!("Failed to set binning: {e}")))?;
    }
    camera
        .set_timestamp_binary()
        .map_err(|e| AppError::Hardware(format!("Failed to set timestamp mode: {e}")))?;
    camera
        .set_exposure_us(snap.rig.camera.exposure_us)
        .map_err(|e| AppError::Hardware(format!("Failed to set exposure: {e}")))?;
    camera.arm().map_err(|e| {
        AppError::Hardware(format!(
            "Failed to arm camera: {e}. This may indicate the binning/pixel rate/exposure \
             combination is not supported by the USB interface."
        ))
    })?;
    Ok(camera)
}

/// `camera-check [duration_sec]` — camera-only acquisition-quality test.
///
/// Captures every frame the camera produces for `duration_sec` (default 10),
/// recording each frame's hardware sequence number and timestamp, then asserts
/// the capture met the rig's requirements — no dropped frames, a steady rate,
/// bounded jitter — via [`openisi_lib::camera_quality::assess_capture`]. Prints
/// the report and returns `Err` (process exit 1) if it fails, so it is usable
/// as a real hardware-in-the-loop gate watchable over RDP. It drives only the
/// camera, not the stimulus monitor, to isolate camera health from end-to-end.
#[cfg(windows)]
fn cmd_camera_check(duration_sec: Option<f64>) -> AppResult<()> {
    use openisi_lib::camera_quality::{FrameStamp, QualityThresholds, assess_capture};

    let duration_sec: f64 = duration_sec.unwrap_or(10.0);
    let reg = load_config_store()?;
    let snap = reg.snapshot();

    println!("Camera quality check: capturing for {duration_sec:.1}s (camera only, no stimulus)");

    let sdk =
        pco_sdk::Sdk::load().map_err(|e| AppError::Hardware(format!("PCO SDK required: {e}")))?;
    let mut camera = open_armed_camera(&sdk, &snap)?;
    println!(
        "Camera armed: {}x{}, exposure {}µs, binning {}",
        camera.width,
        camera.height,
        snap.rig.camera.exposure_us,
        snap.rig.camera.binning
    );

    let mut recorder = camera
        .create_recorder(16)
        .map_err(|e| AppError::Hardware(format!("Failed to create recorder: {e}")))?;
    recorder
        .start()
        .map_err(|e| AppError::Hardware(format!("Failed to start recording: {e}")))?;

    // Read EVERY frame (no decimation): poll fast — 1ms, accurate now that
    // timeBeginPeriod(1) is active — and record each new hardware image number
    // and timestamp. `has_new_frame()` lets us skip the multi-megabyte frame
    // copy except when a genuinely new frame is ready, so the reader keeps up
    // with the sensor and a gap in the hardware sequence counter means a real
    // dropped frame rather than a reader that fell behind.
    let mut stamps: Vec<FrameStamp> = Vec::new();
    let mut last_image: Option<u32> = None;
    let start = Instant::now();
    while start.elapsed().as_secs_f64() < duration_sec {
        let has_new = recorder
            .has_new_frame()
            .map_err(|e| AppError::Hardware(format!("Recorder status failed: {e}")))?;
        if has_new
            && let Some(frame) = recorder
                .get_latest_frame()
                .map_err(|e| AppError::Hardware(format!("Frame read failed: {e}")))?
            && last_image != Some(frame.image_number)
        {
            stamps.push(FrameStamp {
                sequence_number: frame.image_number as u64,
                hardware_timestamp_us: frame.timestamp.to_us_since_midnight(),
            });
            last_image = Some(frame.image_number);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let _ = recorder.stop();

    // Assess against the rig's drop-detection policy (config SSoT), so the
    // test and the live acquisition path agree on what "dropped" and "too long
    // an interval" mean.
    let thr = QualityThresholds {
        timing_anomaly_factor: snap.rig.system.drop_detection_threshold,
        warmup_frames: snap.rig.system.drop_detection_warmup_frames,
        max_jitter_fraction: QualityThresholds::default().max_jitter_fraction,
    };
    let report = assess_capture(&stamps, &thr);

    println!("\n-- Camera quality report --");
    println!("  frames captured : {}", report.n_frames);
    println!("  duration        : {:.2}s", report.duration_sec);
    println!(
        "  mean rate       : {:.1} fps (median period {:.0}us)",
        report.mean_fps, report.median_period_us
    );
    println!(
        "  jitter          : {:.0}us ({:.1}% of period)",
        report.jitter_us,
        report.jitter_fraction * 100.0
    );
    println!("  sequence drops  : {}", report.sequence_drops);
    println!(
        "  timing anomalies: {} (max interval {}us)",
        report.timing_anomalies, report.max_delta_us
    );

    if report.passed {
        println!("  VERDICT         : PASS");
        Ok(())
    } else {
        println!("  VERDICT         : FAIL");
        for f in &report.failures {
            println!("    - {f}");
        }
        Err(AppError::Validation(format!(
            "camera quality check FAILED: {}",
            report.failures.join("; ")
        )))
    }
}

#[cfg(windows)]
fn cmd_acquire(duration_sec: Option<f64>, primary: bool, validate: bool) -> AppResult<()> {
    let allow_primary = primary;
    let duration_sec: f64 = duration_sec.unwrap_or(10.0);

    let reg = load_config_store()?;
    let snap = reg.snapshot();

    let monitors = monitor::detect_monitors();
    let stim_idx = select_stimulus_monitor(&monitors, allow_primary)?;
    let monitor = &monitors[stim_idx];

    // Is the stimulus presented on a real hardware scanout? Over RDP the desktop
    // is a virtual display with no hardware vblank, so stimulus present-timing
    // (dropped-frame counts) is not physically measurable. The camera is
    // unaffected (its own hardware clock). This flag is recorded as provenance
    // in the .oisi and gates the stimulus-timing verdict below.
    let stimulus_timing_validatable = !monitor::is_remote_session();
    if !stimulus_timing_validatable {
        println!(
            "Display: REMOTE session (virtual display) — stimulus render is real, but present \
             timing has no hardware vsync; stimulus-timing checks will be SKIPPED. Run at the \
             physical console to validate timing."
        );
    }

    println!(
        "Acquiring for {:.1}s on monitor [{}] {}",
        duration_sec, stim_idx, monitor.name
    );
    println!(
        "Experiment: {:?} {:?}",
        snap.experiment.stimulus.envelope,
        snap.experiment.stimulus.carrier
    );

    // Camera setup (shared path — identical config to `camera-check`).
    let sdk =
        pco_sdk::Sdk::load().map_err(|e| AppError::Hardware(format!("PCO SDK required: {e}")))?;
    let mut camera = open_armed_camera(&sdk, &snap)?;

    let cam_w = camera.width;
    let cam_h = camera.height;
    println!(
        "Camera armed: {}x{}, exposure {}µs",
        cam_w,
        cam_h,
        snap.rig.camera.exposure_us
    );

    // Stimulus thread.
    let (stim_cmd_tx, stim_cmd_rx) = crossbeam_channel::unbounded();
    let (stim_evt_tx, stim_evt_rx) = crossbeam_channel::unbounded();

    let bg_lum = snap.experiment.stimulus.params.background_luminance;
    let preview_width_px = snap.rig.system.preview_width_px;
    let preview_interval_ms = snap.rig.system.preview_interval_ms;
    let preview_cycle_sec = snap.rig.system.preview_cycle_sec;
    let idle_sleep_ms = snap.rig.system.idle_sleep_ms;
    let drop_detection_warmup_frames = snap.rig.system.drop_detection_warmup_frames;
    let mon_idx = monitor.index;
    let mon_w = monitor.width_px;
    let mon_h = monitor.height_px;
    let mon_pos = monitor.position;

    std::thread::Builder::new()
        .name("stimulus".into())
        .spawn(move || {
            let config = openisi_lib::stimulus_thread::StimulusConfig {
                monitor_index: mon_idx,
                monitor_width_px: mon_w,
                monitor_height_px: mon_h,
                monitor_position: mon_pos,
                preview_width_px,
                preview_interval_ms,
                preview_cycle_sec,
                idle_sleep_ms,
                drop_detection_warmup_frames,
                initial_bg_luminance: bg_lum,
            };
            openisi_lib::stimulus_thread::run(stim_cmd_rx, stim_evt_tx, config);
        })
        .map_err(|e| AppError::Hardware(format!("Failed to spawn stimulus thread: {e}")))?;

    // Wait for stimulus ready.
    loop {
        match stim_evt_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(StimulusEvt::Ready) => {
                println!("Stimulus thread ready");
                break;
            }
            Ok(_) => {}
            Err(_) => {
                return Err(AppError::Hardware(
                    "Stimulus thread did not become ready in 10s".into(),
                ));
            }
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

    stim_cmd_tx
        .send(StimulusCmd::StartAcquisition(acq_cmd))
        .map_err(|e| AppError::Hardware(format!("Failed to start acquisition: {e}")))?;
    println!("Acquisition started");

    // Camera recording.
    let mut recorder = camera
        .create_recorder(10)
        .map_err(|e| AppError::Hardware(format!("Failed to create recorder: {e}")))?;
    recorder
        .start()
        .map_err(|e| AppError::Hardware(format!("Failed to start recording: {e}")))?;

    // Wait for first frame.
    let deadline =
        Instant::now() + Duration::from_millis(snap.rig.system.camera_first_frame_timeout_ms as u64);
    loop {
        if Instant::now() > deadline {
            return Err(AppError::Hardware(
                "Timed out waiting for first camera frame".into(),
            ));
        }
        match recorder.get_latest_frame() {
            Ok(Some(_)) => break,
            Ok(None) => std::thread::sleep(Duration::from_millis(
                snap.rig.system.camera_first_frame_poll_ms as u64,
            )),
            Err(e) => {
                return Err(AppError::Hardware(format!("Frame read error: {e}")));
            }
        }
    }
    println!("Camera streaming");

    // Accumulate frames.
    let mut accumulator = openisi_lib::export::AcquisitionAccumulator::new();
    accumulator.start(cam_w, cam_h);

    let qpc_freq = {
        let mut f = 0i64;
        unsafe {
            let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut f);
        }
        f
    };
    if qpc_freq == 0 {
        return Err(AppError::Hardware(
            "QueryPerformanceFrequency returned 0".into(),
        ));
    }

    let start = Instant::now();
    let mut frame_count = 0u64;

    while start.elapsed() < Duration::from_secs_f64(duration_sec) {
        match recorder.get_latest_frame() {
            Ok(Some(frame)) => {
                let sys_us = {
                    let mut qpc = 0i64;
                    unsafe {
                        let _ =
                            windows::Win32::System::Performance::QueryPerformanceCounter(&mut qpc);
                    }
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
        std::thread::sleep(Duration::from_millis(snap.rig.system.camera_poll_interval_ms as u64));
    }

    // Stop.
    let _ = recorder.stop();
    stim_cmd_tx
        .send(StimulusCmd::Stop)
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
                let result = *result;
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
            Err(_) => {
                eprintln!("Timeout waiting for stimulus completion");
                break;
            }
        }
    }

    stim_cmd_tx.send(StimulusCmd::Shutdown).ok();
    let elapsed = start.elapsed();
    println!(
        "Captured {} camera frames in {:.1}s ({:.1} fps)",
        frame_count,
        elapsed.as_secs_f64(),
        frame_count as f64 / elapsed.as_secs_f64()
    );

    // Save.
    let camera_data = accumulator.finish();

    // Metrics for the optional `--validate` verdict, captured before
    // `camera_data` is moved into the exporter below.
    let verdict_cam_frames = camera_data.frames.len();
    let verdict_cam_drops = camera_data
        .sequence_numbers
        .windows(2)
        .filter(|w| w[1] != w[0] + 1)
        .count();
    let verdict_fps = if elapsed.as_secs_f64() > 0.0 {
        frame_count as f64 / elapsed.as_secs_f64()
    } else {
        0.0
    };
    let verdict_stim_drops = stim_dataset
        .as_ref()
        .map(|d| d.dropped_frame_indices.len())
        .unwrap_or(0);
    let verdict_sweeps = sweep_schedule.sweep_sequence.len();

    // Typed config snapshot for provenance + the data-dir lookup.
    let cfg_snap = snap.clone();
    let data_dir = cfg_snap.rig.paths.data_directory.clone();
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
        let summary = openisi_lib::export::write_oisi(
            &output_path,
            openisi_lib::export::OisiBundle {
                stimulus_dataset: ds,
                camera_data,
                snapshot: &cfg_snap,
                hardware: None,
                schedule: &sweep_schedule,
                timing: None,
                session_meta: None,
                anatomical: None,
                acquisition_complete: false,
                stimulus_timing_validatable,
            },
        )?;
        println!("{summary}");
    } else {
        return Err(AppError::Validation(
            "no stimulus dataset produced — nothing to export".into(),
        ));
    }

    // Honest pass/fail verdict (opt-in via --validate). Each check that is
    // physically valid in the current environment is enforced; the
    // stimulus-timing check is enforced only on a real hardware scanout and
    // otherwise SKIPPED with the reason, so a remote run never false-fails and a
    // console run validates fully.
    if validate {
        println!("\n-- Acquisition validation verdict --");
        let mut failures: Vec<String> = Vec::new();

        // Camera capture — always physically valid (its own hardware clock).
        println!("  camera frames   : {verdict_cam_frames}");
        println!("  camera rate     : {verdict_fps:.1} fps");
        if verdict_cam_frames < 2 {
            failures.push(format!(
                "captured {verdict_cam_frames} camera frame(s); need at least 2"
            ));
        }
        if verdict_cam_drops > 0 {
            println!("  camera drops    : {verdict_cam_drops}  [FAIL]");
            failures.push(format!(
                "{verdict_cam_drops} camera frame(s) dropped (gaps in the hardware sequence counter)"
            ));
        } else {
            println!("  camera drops    : 0  [PASS]");
        }

        // Stimulus present-timing — only enforceable on a real hardware scanout.
        if stimulus_timing_validatable {
            if verdict_stim_drops > 0 {
                println!("  stimulus timing : {verdict_stim_drops} drops  [FAIL]");
                failures.push(format!(
                    "{verdict_stim_drops} stimulus frame(s) dropped at the display vsync"
                ));
            } else {
                println!("  stimulus timing : 0 drops  [PASS]");
            }
        } else {
            println!(
                "  stimulus timing : SKIPPED — remote virtual display (no hardware vsync); \
                 re-run at the physical console to validate. ({verdict_stim_drops} present-gaps \
                 observed, not a real measurement)"
            );
        }

        // Analysis round-trip — pure compute, valid anywhere (RDP included).
        // Re-reads the .oisi we just wrote and runs the full pipeline, checking
        // the acquire→analyze plumbing and that outputs are finite. It does NOT
        // gate reliability *high* — that needs a live biological signal — and a
        // short validation capture without a full multi-cycle schedule
        // legitimately has too little data, which is SKIPPED, not failed. The
        // correctness of phase recovery + reliability itself is proven
        // hardware-independently by isi-analysis `compute::ops::tests`.
        if verdict_sweeps < 2 {
            println!(
                "  analysis        : SKIPPED — only {verdict_sweeps} stimulus sweep(s) captured; a \
                 full retinotopy schedule (≥2 cycles × directions) is needed. Run a full-length \
                 acquisition at the console to exercise analysis."
            );
        } else {
            let params = isi_analysis::AnalysisParams::from(&snap.analysis);
            let cancel = std::sync::atomic::AtomicBool::new(false);
            match isi_analysis::analyze(
                &output_path,
                &params,
                &isi_analysis::SilentProgress,
                &cancel,
            ) {
                Ok(()) => match isi_analysis::io::read_reliability_maps(&output_path) {
                    Ok(rel_opt) => {
                        let (finite, mean) = match &rel_opt {
                            Some(rel) => {
                                let v = &rel.rel_azi_fwd;
                                let finite =
                                    v.iter().all(|x| x.is_finite() && (0.0..=1.0).contains(x));
                                let mean = v.iter().sum::<f64>() / v.len().max(1) as f64;
                                (finite, Some(mean))
                            }
                            None => (true, None),
                        };
                        if finite {
                            println!(
                                "  analysis        : pipeline completed, outputs finite  [PASS]"
                            );
                            match mean {
                                Some(m) => println!(
                                    "                    reliability mean {m:.3} (observed; high \
                                     values require a live biological signal — not gated here)"
                                ),
                                None => println!(
                                    "                    reliability not computed (single-cycle \
                                     data)"
                                ),
                            }
                        } else {
                            println!(
                                "  analysis        : non-finite / out-of-range reliability  [FAIL]"
                            );
                            failures.push(
                                "analysis produced non-finite or out-of-range reliability".into(),
                            );
                        }
                    }
                    Err(e) => {
                        println!("  analysis        : results unreadable  [FAIL] ({e})");
                        failures.push(format!("could not read analysis results: {e}"));
                    }
                },
                Err(
                    e @ (isi_analysis::AnalysisError::MissingData(_)
                    | isi_analysis::AnalysisError::Validation(_)),
                ) => {
                    println!("  analysis        : SKIPPED — insufficient data ({e})");
                }
                Err(e) => {
                    println!("  analysis        : FAILED — {e}");
                    failures.push(format!("analysis pipeline error: {e}"));
                }
            }
        }

        if failures.is_empty() {
            println!("  VERDICT         : PASS");
        } else {
            println!("  VERDICT         : FAIL");
            for f in &failures {
                println!("    - {f}");
            }
            return Err(AppError::Validation(format!(
                "acquisition validation FAILED: {}",
                failures.join("; ")
            )));
        }
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// Software commands — all platforms
// ═══════════════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════════════
// dft-check — diagnostic: current DFT-from-raw vs cached complex maps
// ═══════════════════════════════════════════════════════════════════════

/// Recompute the per-direction complex maps FROM RAW with the current code
/// (`compute_complex_maps_from_raw` reads the raw frames directly, ignoring any
/// cached `/complex_maps`), and compare their per-direction phase spread against
/// the file's CACHED maps (the known-good reference). The metric is the
/// amplitude-weighted circular std of phase over the signal region:
/// ~0 ⇒ uniform phase (no retinotopic gradient); >1 ⇒ a real position gradient.
/// If the cached maps show a gradient but the from-raw maps are uniform, the
/// current DFT-from-raw path is the regression.
fn cmd_dft_check(file: &std::path::Path) -> AppResult<()> {
    let path = file.to_path_buf();

    // Amplitude-weighted circular std of phase over the signal region (pixels
    // with amplitude > 20% of max). r = |Σ a·e^{iφ}| / Σ a ∈ [0,1]: r≈1 means
    // every signal pixel shares one phase (uniform); r≈0 means phases are spread
    // across the cortex (a gradient). circ_std = sqrt(-2·ln r).
    // Amplitude-weighted circular std of a set of complex phasors.
    fn circ_std(sig: &[isi_analysis::Complex64]) -> f64 {
        let (mut sw, mut sx, mut sy) = (0.0_f64, 0.0_f64, 0.0_f64);
        for c in sig {
            let a = c.norm();
            let ph = c.arg();
            sw += a;
            sx += a * ph.cos();
            sy += a * ph.sin();
        }
        let r = if sw > 0.0 {
            ((sx * sx + sy * sy).sqrt() / sw).clamp(1e-12, 1.0)
        } else {
            1.0
        };
        (-2.0 * r.ln()).sqrt()
    }

    // Spatial test for a smooth retinotopic gradient. Subtract the global
    // (spatial-mean) phasor, then fit the residual phase to a plane
    // φ ≈ a + b·col + c·row by amplitude-weighted least squares. Returns
    // (circular-spread, plane R², linear phase range in rad over the signal
    // bbox, n). High R² ⇒ a SMOOTH spatial gradient (real retinotopy, even if
    // small-range); low R² ⇒ noise. The global subtraction keeps the residual
    // range small so phase-wrapping doesn't corrupt the fit.
    fn analyze_map(m: &ndarray::Array2<isi_analysis::Complex64>) -> (f64, f64, f64, usize) {
        let (h, w) = m.dim();
        let amax = m.iter().map(|c| c.norm()).fold(0.0_f64, f64::max);
        let thr = amax * 0.2;
        let mut sig = Vec::new();
        for ((r, c), z) in m.indexed_iter() {
            if z.norm() > thr {
                sig.push((c as f64, r as f64, *z));
            }
        }
        let n = sig.len();
        if n < 8 {
            return (0.0, 0.0, 0.0, n);
        }
        let phasors: Vec<isi_analysis::Complex64> = sig.iter().map(|t| t.2).collect();
        let spread = circ_std(&phasors);
        // Remove global phasor, take residual phase (small range → no wrap).
        let g = phasors.iter().sum::<isi_analysis::Complex64>() / n as f64;
        let pts: Vec<(f64, f64, f64, f64)> = sig
            .iter()
            .map(|&(x, y, z)| (x, y, (z - g).arg(), z.norm()))
            .collect();
        // Weighted least squares plane φ = a + b·x + c·y. Normal equations.
        let (mut sw, mut sx, mut sy, mut sxx, mut sxy, mut syy) =
            (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        let (mut sp, mut sxp, mut syp) = (0.0, 0.0, 0.0);
        for &(x, y, p, wt) in &pts {
            sw += wt; sx += wt*x; sy += wt*y; sxx += wt*x*x; sxy += wt*x*y; syy += wt*y*y;
            sp += wt*p; sxp += wt*x*p; syp += wt*y*p;
        }
        // Solve 3x3 [sw sx sy; sx sxx sxy; sy sxy syy] [a b c] = [sp sxp syp].
        let m3 = [[sw, sx, sy], [sx, sxx, sxy], [sy, sxy, syy]];
        let rhs = [sp, sxp, syp];
        let det = m3[0][0]*(m3[1][1]*m3[2][2]-m3[1][2]*m3[2][1])
                - m3[0][1]*(m3[1][0]*m3[2][2]-m3[1][2]*m3[2][0])
                + m3[0][2]*(m3[1][0]*m3[2][1]-m3[1][1]*m3[2][0]);
        if det.abs() < 1e-9 { return (spread, 0.0, 0.0, n); }
        let solve = |col: usize| {
            let mut mm = m3;
            for r in 0..3 { mm[r][col] = rhs[r]; }
            (mm[0][0]*(mm[1][1]*mm[2][2]-mm[1][2]*mm[2][1])
             - mm[0][1]*(mm[1][0]*mm[2][2]-mm[1][2]*mm[2][0])
             + mm[0][2]*(mm[1][0]*mm[2][1]-mm[1][1]*mm[2][0])) / det
        };
        let (a, b, c) = (solve(0), solve(1), solve(2));
        // R² (weighted).
        let pbar = sp / sw;
        let (mut ssr, mut sst) = (0.0, 0.0);
        for &(x, y, p, wt) in &pts {
            let fit = a + b*x + c*y;
            ssr += wt*(p - fit).powi(2);
            sst += wt*(p - pbar).powi(2);
        }
        let r2 = if sst > 0.0 { 1.0 - ssr/sst } else { 0.0 };
        let range = b.abs()*(w as f64 - 1.0) + c.abs()*(h as f64 - 1.0);
        (spread, r2.max(0.0), range, n)
    }

    let store = load_config_store()?;
    let snapshot = store.snapshot();
    let params = isi_analysis::AnalysisParams::from(store.analysis());
    let cancel = std::sync::atomic::AtomicBool::new(false);

    println!("DFT-from-raw vs cached complex maps — {}", path.display());
    println!("phase spread (amp-weighted circular std): ~0 = UNIFORM (no gradient), >1 = GRADIENT\n");

    let pick = |c: &isi_analysis::ComplexMaps, name: &str| -> ndarray::Array2<isi_analysis::Complex64> {
        match name {
            "azi_fwd" => c.azi_fwd.clone(),
            "azi_rev" => c.azi_rev.clone(),
            "alt_fwd" => c.alt_fwd.clone(),
            _ => c.alt_rev.clone(),
        }
    };

    // Schedule diagnostics — does the per-cycle DFT window match the bar sweep?
    // If the recorded sweep window is much longer than the bar's actual sweep
    // time, the position phase fills only that fraction of 2π → compressed range
    // → uniform-looking phase even with good data.
    if let Ok(f) = hdf5::File::open(&path) {
        let rd = |n: &str| f.dataset(n).and_then(|d| d.read_1d::<f64>()).ok();
        if let (Some(ss), Some(se), Some(ct)) = (
            rd("acquisition/schedule/sweep_start_sec"),
            rd("acquisition/schedule/sweep_end_sec"),
            rd("acquisition/camera/timestamps_sec"),
        ) {
            let durs: Vec<f64> = ss.iter().zip(se.iter()).map(|(a, b)| b - a).collect();
            let mean_dur = durs.iter().sum::<f64>() / durs.len().max(1) as f64;
            let cam_dur = ct[ct.len() - 1] - ct[0];
            let fps = (ct.len() - 1) as f64 / cam_dur;
            let speed = snapshot.experiment.stimulus.params.sweep_speed_deg_per_sec;
            let azi = snapshot.experiment.stimulus_geometry.azi_angular_range;
            let alt = snapshot.experiment.stimulus_geometry.alt_angular_range;
            let barw = snapshot.experiment.stimulus.params.stimulus_width_deg;
            println!(
                "Schedule: {} sweeps, mean recorded sweep window = {:.2}s, camera {:.1} fps, total {:.0}s",
                durs.len(), mean_dur, fps, cam_dur
            );
            println!(
                "Expected bar-sweep time @ {:.0}°/s: azi ({:.0}°+{:.0}° bar)/speed = {:.2}s,  alt = {:.2}s",
                speed, azi, barw, (azi + barw) / speed, (alt + barw) / speed
            );
            let ratio = mean_dur / (((azi + barw) / speed).max(1e-6));
            println!(
                "  → DFT window / azi-sweep ratio ≈ {:.2}  (≈1 is correct; ≫1 ⇒ phase compressed ~{:.0}×)",
                ratio, ratio.max(1.0)
            );
        }
    }

    println!("\nComputing complex maps FROM RAW with current code (this reads all frames)...");
    let from_raw =
        match isi_analysis::io::compute_complex_maps_from_raw(&path, &params, &isi_analysis::SilentProgress, &cancel) {
            Ok(raw) => Some(raw.complex_maps),
            Err(e) => {
                println!("  (no raw frames / from-raw unavailable: {e})");
                None
            }
        };
    let cached = isi_analysis::io::read_complex_maps(&path).ok();

    // Prefer from-raw (current code); fall back to cached for import-only files.
    let src = from_raw.as_ref().or(cached.as_ref());
    println!("\n  direction    spread   plane-R²   grad-range(rad)   signal_px");
    match src {
        Some(maps) => {
            for name in ["azi_fwd", "azi_rev", "alt_fwd", "alt_rev"] {
                let (spread, r2, range, n) = analyze_map(&pick(maps, name));
                println!("  {name:<10}  {spread:>6.3}   {r2:>6.3}     {range:>10.3}        n={n}");
            }
        }
        None => println!("  (no complex maps available)"),
    }
    println!(
        "\nInterpretation: plane-R² is the key test. HIGH R² (≳0.5) ⇒ the phase is a SMOOTH \
         spatial gradient = real retinotopy (even when 'spread' is small, i.e. the gradient \
         is just low-range / offset-dominated). LOW R² ⇒ no coherent gradient (noise). \
         grad-range is the linear phase swing across the field in radians."
    );
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// analyze
// ═══════════════════════════════════════════════════════════════════════

fn cmd_analyze(
    file: &std::path::Path,
    figures: Option<Option<PathBuf>>,
    threshold_sweep: bool,
    compare_methods: bool,
) -> AppResult<()> {
    let store = load_config_store()?;
    let path = file;

    // Pre-2026 schema files are refused with a clear migration message;
    // there is no implicit conversion.
    if isi_analysis::io::is_pre_2026_analysis_params(path)? {
        return Err(AppError::Validation(format!(
            "{} has pre-2026 /analysis_params schema. Run `oisi migrate {}` first.",
            path.display(),
            path.display(),
        )));
    }

    let analysis_config = store.analysis().clone();
    let params = isi_analysis::AnalysisParams::from(&analysis_config);
    // Provenance: tagged `AnalysisConfig` (serde), the canonical schema.
    let params_tree = serde_json::to_value(&analysis_config)
        .map_err(|e| AppError::Validation(format!("serialize analysis params: {e}")))?;

    let progress = isi_analysis::SilentProgress;
    let cancel = std::sync::atomic::AtomicBool::new(false);

    println!("Analyzing {}...", path.display());
    isi_analysis::analyze(path, &params, &progress, &cancel)?;
    // Stamp the config tree into /analysis_params for provenance.
    isi_analysis::io::write_analysis_params_attr(path, &params_tree)?;
    println!("Analysis complete");
    // Export figures (`--figures [dir]`). `Some(None)` = flag with no value →
    // auto-tag into `<repo_root>/dev_figures/<stem>/<tag>/`; `Some(Some(dir))` =
    // explicit dir used verbatim (one-off comparison); `None` = no figures.
    if let Some(maybe_dir) = figures {
        let dir = maybe_dir.unwrap_or_else(|| default_figures_dir(path, &params));
        export_all_figures(path, &dir.to_string_lossy());
        write_meta_json(&dir, path, &params);
        if threshold_sweep {
            export_threshold_sweep_grids(path, &dir, &params);
        }
        if compare_methods {
            compare_method_variants(path, &params, &dir);
        }
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// migrate — upgrade pre-2026 /analysis_params to the current tagged schema
// ═══════════════════════════════════════════════════════════════════════

/// Migrate a `.oisi` file's `/analysis_params` attribute from the pre-2026
/// shape to the current **tagged `AnalysisConfig`** shape.
///
/// The current schema is the serde form of [`openisi_params::config::AnalysisConfig`]:
/// `{"<stage>": {"method": "x", <x's tunables flat at the stage level>}}`. The
/// full translation (legacy flat/nested tunables, method renames, dropped
/// root-level fields, fill-in defaults for unset tunables) lives in
/// [`isi_analysis::migrate::translate_pre_2026_analysis_params`] — the ONLY place
/// the old schema's field names appear post-refactor.
fn cmd_migrate(file: &std::path::Path) -> AppResult<()> {
    let path = file.to_path_buf();
    if !path.exists() {
        return Err(AppError::NotAvailable(format!(
            "migrate: file does not exist: {}",
            path.display()
        )));
    }

    let Some(old_tree) = isi_analysis::io::read_analysis_params_attr(&path)? else {
        println!(
            "{}: no /analysis_params attribute — nothing to migrate.",
            path.display()
        );
        return Ok(());
    };

    if !isi_analysis::io::is_pre_2026_analysis_params(&path)? {
        println!(
            "{}: /analysis_params already in current tagged-AnalysisConfig schema. No migration needed.",
            path.display()
        );
        return Ok(());
    }

    let new_tree = isi_analysis::migrate::translate_pre_2026_analysis_params(&old_tree)?;
    isi_analysis::io::write_analysis_params_attr(&path, &new_tree)?;

    println!("Migrated /analysis_params on {}", path.display());
    println!("  new shape: tagged AnalysisConfig (active variant's tunables flat at the stage level)");
    println!("  defaults for unset tunables sourced from the canonical analysis defaults");
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// export-nwb — transform an .oisi into a reference-valid NWB file
// ═══════════════════════════════════════════════════════════════════════

/// Export an `.oisi` to NWB by invoking the `tools/export_nwb` Python bridge.
///
/// This is the no-Python-in-the-core design: acquisition + analysis are pure
/// Rust; only *export* shells out to pynwb (the reference NWB implementation, the
/// only way to guarantee a valid file without re-implementing the spec). The
/// native `.oisi` is read-only here — nothing about the analysis pipeline changes.
fn cmd_export_nwb(
    file: &std::path::Path,
    output: Option<&std::path::Path>,
    metadata: Option<&std::path::Path>,
) -> AppResult<()> {
    if !file.exists() {
        return Err(AppError::NotAvailable(format!(
            "export-nwb: file does not exist: {}",
            file.display()
        )));
    }
    let out_path = match output {
        Some(p) => p.to_path_buf(),
        None => file.with_extension("nwb"),
    };

    let script = repo_root().join("tools/export_nwb/export_oisi_to_nwb.py");
    if !script.exists() {
        return Err(AppError::NotAvailable(format!(
            "export-nwb: bridge script not found at {}. Run from a source checkout.",
            script.display()
        )));
    }

    let python = if cfg!(windows) { "python" } else { "python3" };
    let mut cmd = std::process::Command::new(python);
    cmd.arg(&script).arg(file).arg(&out_path);
    if let Some(m) = metadata {
        cmd.arg("--metadata").arg(m);
    }

    println!("Exporting {} -> {} ...", file.display(), out_path.display());
    let status = cmd.status().map_err(|e| {
        AppError::NotAvailable(format!(
            "export-nwb: failed to launch `{python}` ({e}). Install Python + the \
             export deps: pip install -r tools/export_nwb/requirements.txt"
        ))
    })?;
    if !status.success() {
        return Err(AppError::Validation(format!(
            "export-nwb: the Python bridge exited with status {status}. \
             Ensure `pip install -r tools/export_nwb/requirements.txt` has been run."
        )));
    }
    println!("NWB export complete: {}", out_path.display());
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// inspect
// ═══════════════════════════════════════════════════════════════════════

fn cmd_inspect(file: &std::path::Path) -> AppResult<()> {
    let path = file;

    let caps = isi_analysis::io::inspect(path)?;
    println!("File: {}", path.display());
    println!(
        "Anatomical:   {}",
        if caps.has_anatomical { "yes" } else { "no" }
    );
    println!(
        "Acquisition:  {}",
        if caps.has_acquisition {
            match &caps.acquisition_schedule {
                Some(s) => format!(
                    "yes ({} cycles × {} directions, {} sweeps)",
                    s.cycles_per_direction, s.directions, s.total_sweeps
                ),
                None => "yes (no readable stimulus schedule)".into(),
            }
        } else {
            "no".into()
        }
    );
    println!(
        "Complex maps: {}",
        if caps.has_complex_maps { "yes" } else { "no" }
    );
    println!(
        "Results:      {}",
        if caps.has_results {
            format!(
                "yes ({})",
                caps.results
                    .iter()
                    .map(|r| r.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            "no".into()
        }
    );
    if let Some((h, w)) = caps.dimensions {
        println!("Dimensions:   {}x{}", w, h);
    }

    if let Ok(file) = hdf5::File::open(path) {
        if let Ok(ds) = file.dataset("acquisition/camera/timestamps_sec")
            && let Ok(ts) = ds.read_1d::<f64>()
        {
            let n = ts.len();
            if n > 0 {
                println!("\nUnified timeline (seconds from t=0):");
                println!(
                    "  Camera frames:  {} (t=[{:.6} .. {:.6}]s)",
                    n,
                    ts[0],
                    ts[n - 1]
                );
            }
        }
        if let Ok(ds) = file.dataset("acquisition/stimulus/timestamps_sec")
            && let Ok(ts) = ds.read_1d::<f64>()
        {
            let n = ts.len();
            if n > 0 {
                println!(
                    "  Stimulus frames: {} (t=[{:.6} .. {:.6}]s)",
                    n,
                    ts[0],
                    ts[n - 1]
                );
            }
        }
        if let Ok(ds) = file.dataset("acquisition/schedule/sweep_start_sec")
            && let Ok(starts) = ds.read_1d::<f64>()
            && let Ok(ends_ds) = file.dataset("acquisition/schedule/sweep_end_sec")
            && let Ok(ends) = ends_ds.read_1d::<f64>()
        {
            println!("  Sweeps: {}", starts.len());
            for i in 0..starts.len() {
                println!("    [{i}] {:.6}s .. {:.6}s", starts[i], ends[i]);
            }
        }
        if let Ok(tg) = file.group("acquisition/timing") {
            println!("\nTiming characterization:");
            if let Ok(attr) = tg.attr("regime")
                && let Ok(val) = attr.read_scalar::<hdf5::types::VarLenUnicode>()
            {
                println!("  Regime:        {val}");
            }
            if let Ok(attr) = tg.attr("f_cam_hz")
                && let Ok(val) = attr.read_scalar::<f64>()
            {
                println!("  Camera:        {:.3} Hz", val);
            }
            if let Ok(attr) = tg.attr("f_stim_hz")
                && let Ok(val) = attr.read_scalar::<f64>()
            {
                println!("  Stimulus:      {:.3} Hz", val);
            }
            if let Ok(attr) = tg.attr("beat_period_sec")
                && let Ok(val) = attr.read_scalar::<f64>()
            {
                println!("  Beat period:   {:.3}s", val);
            }
            if let Ok(attr) = tg.attr("phase_coverage")
                && let Ok(val) = attr.read_scalar::<f64>()
            {
                println!("  Phase coverage: {:.1}%", val * 100.0);
            }
        }
        if let Ok(cs) = file.group("acquisition/clock_sync") {
            println!("\nClock sync:");
            if let Ok(attr) = cs.attr("cam_hw_minus_sys_start_us")
                && let Ok(val) = attr.read_scalar::<f64>()
            {
                println!("  HW-SYS offset (start): {:.1}µs", val);
            }
            if let Ok(attr) = cs.attr("cam_hw_minus_sys_end_us")
                && let Ok(val) = attr.read_scalar::<f64>()
            {
                println!("  HW-SYS offset (end):   {:.1}µs", val);
            }
            if let Ok(attr) = cs.attr("drift_us")
                && let Ok(val) = attr.read_scalar::<f64>()
            {
                println!("  Drift:                 {:.1}µs", val);
            }
        }
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// import
// ═══════════════════════════════════════════════════════════════════════

fn cmd_import(mat_dir: &std::path::Path, output_arg: Option<&std::path::Path>) -> AppResult<()> {
    let dir = mat_dir;
    if !dir.is_dir() {
        eprintln!("Not a directory: {}", dir.display());
        return Ok(());
    }

    let output = if let Some(o) = output_arg {
        o.to_path_buf()
    } else {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "import".into());
        dir.parent().unwrap_or(dir).join(format!("{name}.oisi"))
    };

    println!("Importing {} -> {}", dir.display(), output.display());

    isi_analysis::io::import_snlc_directory(dir, &output)?;
    println!("Import complete: {}", output.display());
    let caps = isi_analysis::io::inspect(&output)?;
    println!(
        "  Complex maps: {}",
        if caps.has_complex_maps { "yes" } else { "no" }
    );
    println!(
        "  Anatomical:   {}",
        if caps.has_anatomical { "yes" } else { "no" }
    );
    if let Some((h, w)) = caps.dimensions {
        println!("  Dimensions:   {}x{}", w, h);
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// import-samples
// ═══════════════════════════════════════════════════════════════════════

fn cmd_import_samples() -> AppResult<()> {
    let reg = load_config_store()?;
    let data_dir = reg.snapshot().rig.paths.data_directory.clone();
    if data_dir.is_empty() {
        eprintln!("Set paths.data_directory in rig.json before downloading samples.");
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

fn cmd_test_read(file: &std::path::Path) -> AppResult<()> {
    let path = file;
    let file = hdf5::File::open(path).map_err(|e| {
        AppError::Analysis(isi_analysis::AnalysisError::Hdf5(format!(
            "open {}: {e}", path.display()
        )))
    })?;

    let group = file.group("results").map_err(|e| {
        AppError::Analysis(isi_analysis::AnalysisError::MissingData(format!(
            "no results group: {e}"
        )))
    })?;

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

fn cmd_import_session(
    session_dir: &std::path::Path,
    output_arg: Option<&std::path::Path>,
) -> AppResult<()> {
    let results_path = session_dir.join("analysis/retinotopy_results.h5");
    let anat_path = session_dir.join("anatomical.png");

    if !results_path.exists() {
        eprintln!(
            "No retinotopy_results.h5 found in {}",
            session_dir.display()
        );
        return Ok(());
    }

    let output = if let Some(o) = output_arg {
        o.to_path_buf()
    } else {
        let name = session_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or("session".into());
        session_dir
            .parent()
            .unwrap_or(session_dir)
            .join(format!("{name}.oisi"))
    };

    println!(
        "Importing session {} -> {}",
        session_dir.display(),
        output.display()
    );

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

    let azi_fwd = read_complex("LR").ok_or_else(|| {
        AppError::Analysis(isi_analysis::AnalysisError::MissingData(
            "missing LR direction in results".into(),
        ))
    })?;
    let azi_rev = read_complex("RL").ok_or_else(|| {
        AppError::Analysis(isi_analysis::AnalysisError::MissingData(
            "missing RL direction in results".into(),
        ))
    })?;
    let alt_fwd = read_complex("TB").ok_or_else(|| {
        AppError::Analysis(isi_analysis::AnalysisError::MissingData(
            "missing TB direction in results".into(),
        ))
    })?;
    let alt_rev = read_complex("BT").ok_or_else(|| {
        AppError::Analysis(isi_analysis::AnalysisError::MissingData(
            "missing BT direction in results".into(),
        ))
    })?;

    let complex_maps = isi_analysis::ComplexMaps {
        azi_fwd,
        azi_rev,
        alt_fwd,
        alt_rev,
    };

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
                        png::ColorType::GrayscaleAlpha => {
                            buf[..w * h * 2].chunks(2).map(|c| c[0]).collect()
                        }
                        png::ColorType::Rgb => buf[..w * h * 3]
                            .chunks(3)
                            .map(|c| ((c[0] as u16 + c[1] as u16 + c[2] as u16) / 3) as u8)
                            .collect(),
                        png::ColorType::Rgba => buf[..w * h * 4]
                            .chunks(4)
                            .map(|c| ((c[0] as u16 + c[1] as u16 + c[2] as u16) / 3) as u8)
                            .collect(),
                        _ => {
                            eprintln!("  Unsupported PNG color type for anatomical");
                            Vec::new()
                        }
                    };
                    if !gray.is_empty() {
                        let arr = ndarray::Array2::from_shape_vec((h, w), gray)
                            .map_err(|e| AppError::Hardware(format!("Shape error: {e}")))?;
                        let file = hdf5::File::open_rw(&output).map_err(|e| {
                            AppError::Hardware(format!("Failed to open .oisi for anatomical: {e}"))
                        })?;
                        file.new_dataset_builder()
                            .with_data(&arr)
                            .create("anatomical")
                            .map_err(|e| {
                                AppError::Hardware(format!("Failed to write anatomical: {e}"))
                            })?;
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

fn cmd_dump_h5(file: &std::path::Path) -> AppResult<()> {
    let path = file;
    let file = hdf5::File::open(path).map_err(|e| {
        AppError::Analysis(isi_analysis::AnalysisError::Hdf5(format!(
            "open {}: {e}", path.display()
        )))
    })?;

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

    let root = file.as_group().map_err(|e| {
        AppError::Analysis(isi_analysis::AnalysisError::Hdf5(format!(
            "open root group: {e}"
        )))
    })?;
    dump_group(&root, "");
    Ok(())
}
