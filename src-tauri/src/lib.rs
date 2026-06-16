pub mod analysis_thread;
pub mod camera_quality;
pub mod camera_thread;
pub mod commands;
pub mod config_paths;
pub mod error;
pub mod events;
pub mod export;
pub mod logging;
pub mod messages;
pub mod monitor;
pub mod params;
pub mod render;
pub mod sample_data;
pub mod session;
pub mod state;
pub mod stimulus_thread;
pub mod timing;

use std::sync::Arc;

use error::{AppError, AppResult};
use state::AppState;

/// Raise the Windows multimedia timer resolution to 1 ms for the process so
/// `std::thread::sleep` and scheduler-quantum waits aren't silently rounded up
/// to the default ~15.6 ms — which would coarsen the camera poll interval and
/// the idle sleeps below what their millisecond config values claim. No-op off
/// Windows; paired with [`end_realtime_timer`] (the OS also resets it on exit).
pub fn begin_realtime_timer() {
    #[cfg(windows)]
    {
        // SAFETY: a valid timer period; matched by `end_realtime_timer`.
        let r = unsafe { windows::Win32::Media::timeBeginPeriod(1) };
        if r == 0 {
            tracing::debug!("process timer resolution raised to 1 ms");
        } else {
            tracing::warn!(
                code = r,
                "timeBeginPeriod(1) failed — timer resolution not raised"
            );
        }
    }
}

/// Release the 1 ms timer resolution requested by [`begin_realtime_timer`].
pub fn end_realtime_timer() {
    #[cfg(windows)]
    {
        // SAFETY: matches the earlier `timeBeginPeriod(1)`.
        let _ = unsafe { windows::Win32::Media::timeEndPeriod(1) };
    }
}

/// Public entry point. Thin wrapper around `try_run()` that prints any
/// startup error in a single readable line and exits non-zero. No panic,
/// no stack trace — the user is a scientist, not a developer.
pub fn run() {
    if let Err(e) = try_run() {
        eprintln!("openisi: {e}");
        std::process::exit(1);
    }
}

/// All startup logic. Returns `Result` so config / setup failures
/// surface cleanly through `?`-propagation instead of `eprintln!` +
/// `process::exit(1)` sprinkled through the code path.
fn try_run() -> AppResult<()> {
    // Config-store construction + config-dir resolution happen inside the Tauri
    // `setup()` callback, where `app.path()` (the platform-standard resolver
    // for the bundle resource dir and the per-user app-config dir) is
    // available. The dev-mode repo `config` is anchored to CARGO_MANIFEST_DIR
    // there, so no exe-relative search is needed up front.
    start_tauri()
}

fn start_tauri() -> AppResult<()> {
    use tauri::Manager;

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            // Install the tracing subscriber before any diagnostics fire.
            crate::logging::init();
            // Fine-grained timer for the camera poll + idle sleeps (see fn docs).
            crate::begin_realtime_timer();

            // ── Resolve config directories properly via Tauri's path API ──
            // The config store is constructed HERE (not before the app) because
            // `app.path()` — the platform-standard resolver for the bundle
            // resource dir and the per-user app-config dir — only exists once
            // the app does. Policy lives in `config_paths` (pure, tested);
            // here we supply the base dirs and surface the resolved choice.
            let is_dev = tauri::is_dev();
            let profile = config_paths::Profile::resolve(is_dev)
                .map_err(|e| Box::<dyn std::error::Error>::from(format!("config profile: {e}")))?;
            // Dev shipped baseline + dev-overlay parent: the repo `config/`,
            // anchored to the compile-time source path (CARGO_MANIFEST_DIR =
            // <repo>/src-tauri) so it is robust against stray `target/` copies
            // that an exe-relative search would otherwise pick up first. Only
            // consulted in the dev branch; prod uses resource_dir below.
            let repo_config = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .map(|root| root.join("config"));
            let resource_config = app.path().resource_dir().ok().map(|p| p.join("config"));
            let app_config = app.path().app_config_dir().ok();
            let layout = config_paths::resolve_layout(
                is_dev,
                profile,
                repo_config.as_deref(),
                resource_config.as_deref(),
                app_config.as_deref(),
            )
            .map_err(|e| Box::<dyn std::error::Error>::from(format!("resolve config dirs: {e}")))?;
            tracing::info!(
                profile = profile.as_str(),
                shipped = %layout.shipped_dir.display(),
                user = %layout.user_dir.display(),
                "config resolved",
            );

            // ── Build + load the typed config store ──
            // GUI loads rig + experiment; analysis params come from the file's
            // `/analysis_params` or default.
            let mut config =
                openisi_params::config::ConfigStore::new(&layout.shipped_dir, &layout.user_dir);
            config.load_rig().map_err(|e| {
                Box::<dyn std::error::Error>::from(format!(
                    "load rig params from {}: {e}",
                    layout.shipped_dir.display()
                ))
            })?;
            config.load_experiment().map_err(|e| {
                Box::<dyn std::error::Error>::from(format!(
                    "load experiment params from {}: {e}",
                    layout.shipped_dir.display()
                ))
            })?;

            // ── First-run default data directory ──
            // A deliberate, visible default (<Documents>/OpenISI) persisted
            // explicitly into the user layer and surfaced in the UI — not an
            // ongoing silent fallback.
            if config.rig().paths.data_directory.is_empty()
                && let Some(default_dir) =
                    config_paths::default_data_dir(app.path().document_dir().ok().as_deref())
            {
                std::fs::create_dir_all(&default_dir).map_err(|e| {
                    Box::<dyn std::error::Error>::from(format!(
                        "create default data dir {}: {e}",
                        default_dir.display()
                    ))
                })?;
                config
                    .merge_rig(&serde_json::json!({
                        "paths": { "data_directory": default_dir.to_string_lossy().into_owned() }
                    }))
                    .map_err(|e| {
                        Box::<dyn std::error::Error>::from(format!("set default data dir: {e}"))
                    })?;
                config.save_all().map_err(|e| {
                    Box::<dyn std::error::Error>::from(format!(
                        "persist default data dir to {}: {e}",
                        layout.user_dir.display()
                    ))
                })?;
                tracing::info!(dir = %default_dir.display(), "data directory defaulted");
            }

            // ── Param-change observer for IPC ──
            config.set_observer(Box::new(crate::params::observer::TauriParamObserver::new(
                app.handle().clone(),
            )));

            // ── Channels ──
            let (stim_cmd_tx, stim_cmd_rx) = crossbeam_channel::unbounded();
            let (stim_evt_tx, stim_evt_rx) = crossbeam_channel::unbounded();
            let (cam_cmd_tx, cam_cmd_rx) = crossbeam_channel::unbounded();
            let (cam_evt_tx, cam_evt_rx) = crossbeam_channel::unbounded();
            let (analysis_cmd_tx, analysis_cmd_rx) = crossbeam_channel::unbounded();
            let (analysis_evt_tx, analysis_evt_rx) = crossbeam_channel::unbounded();

            // ── Detect monitors at startup ──
            let monitors = monitor::detect_monitors();
            tracing::info!(count = monitors.len(), "detected monitors");
            for m in &monitors {
                tracing::debug!(
                    index = m.index, name = %m.name,
                    width_px = m.width_px, height_px = m.height_px, refresh_hz = m.refresh_hz,
                    width_cm = m.width_cm, height_cm = m.height_cm,
                    x = m.position.0, y = m.position.1,
                    "monitor",
                );
            }

            // ── Thread handles (immutable after startup) ──
            // Senders/receivers are stored directly; the one-time stimulus
            // spawn handles live behind their own small mutex.
            let threads = crate::state::ThreadHandles {
                stimulus_tx: stim_cmd_tx,
                stimulus_rx: stim_evt_rx,
                camera_tx: cam_cmd_tx,
                camera_rx: cam_evt_rx,
                analysis_tx: analysis_cmd_tx,
                analysis_rx: analysis_evt_rx,
                stimulus_spawn: parking_lot::Mutex::new(crate::state::StimulusSpawn {
                    cmd_rx: Some(stim_cmd_rx),
                    evt_tx: Some(stim_evt_tx),
                    spawned: false,
                }),
            };

            // ── App state ──
            let app_state = AppState::new(config, threads, monitors);

            // ── Spawn analysis worker thread ──
            // Runs `isi_analysis::analyze` off the IPC command thread,
            // with preemptive cancellation between Run requests. UI
            // listens to `analysis-*` Tauri events for completion.
            let _analysis_handle = std::thread::Builder::new()
                .name("analysis_worker".into())
                .spawn(move || {
                    crate::analysis_thread::run(analysis_cmd_rx, analysis_evt_tx);
                })
                .map_err(|e| {
                    Box::<dyn std::error::Error>::from(format!("spawn analysis worker: {e}"))
                })?;

            // Push the EDID-detected stimulus monitor into the config store's
            // HardwareContext so `effective_monitor_width_cm()` etc. resolve
            // to the live panel dims (auto-detected). The user can still
            // override via the Rig Calibration UI; user overrides win over
            // hardware per `ConfigSnapshot::effective_monitor_width_cm`.
            //
            // The "stimulus monitor" is whichever was previously selected,
            // else the highest-refresh-rate one (typical convention for a
            // dedicated stimulus display).
            let stimulus_monitor = {
                let prev_name = app_state
                    .session
                    .lock()
                    .selected_display
                    .as_ref()
                    .map(|d| d.name.clone());
                let by_prev = prev_name
                    .as_ref()
                    .and_then(|n| app_state.monitors.iter().find(|m| &m.name == n).cloned());
                by_prev.or_else(|| {
                    app_state
                        .monitors
                        .iter()
                        .max_by_key(|m| m.refresh_hz)
                        .cloned()
                })
            };
            if let Some(m) = &stimulus_monitor {
                let mut cfg = app_state.config.lock();
                let mut hw = cfg.hardware().clone();
                hw.monitor_width_cm = Some(m.width_cm);
                hw.monitor_height_cm = Some(m.height_cm);
                hw.monitor_width_px = Some(m.width_px);
                hw.monitor_height_px = Some(m.height_px);
                hw.monitor_refresh_hz = Some(m.refresh_hz);
                cfg.inject_hardware(hw);
                tracing::info!(
                    monitor = %m.name,
                    width_px = m.width_px, height_px = m.height_px,
                    width_cm = m.width_cm, height_cm = m.height_cm, refresh_hz = m.refresh_hz,
                    "hardware context",
                );
            }

            // ── Restore the last session (best-effort, from user_dir) ──
            // Durable user intent only: animal_id/notes always; the selected
            // display only if a matching monitor is still present; the active
            // .oisi only if it still exists. Validation / camera / timing are
            // deliberately NOT restored — they must be re-established. A
            // missing/corrupt session file simply starts fresh (UI state, not
            // provenance), so this is best-effort with transparent logging.
            if let Some(saved) = crate::session::PersistedSession::load(&layout.user_dir) {
                {
                    let mut session = app_state.session.lock();
                    session.animal_id = saved.animal_id;
                    session.notes = saved.notes;
                    if let Some(disp) = saved.selected_display {
                        if crate::session::PersistedSession::display_still_present(
                            &disp,
                            &app_state.monitors,
                        ) {
                            session.selected_display = Some(disp);
                            // display_validation intentionally left None — re-validate.
                        } else {
                            tracing::warn!(
                                display = %disp.name,
                                "saved display no longer present — not restored",
                            );
                        }
                    }
                }
                if let Some(p) = saved.active_oisi_path {
                    if p.exists() {
                        *app_state.active_oisi.lock() = Some(p);
                    } else {
                        tracing::warn!(
                            path = %p.display(),
                            "last active file no longer exists — not restored",
                        );
                    }
                }
                tracing::info!("restored previous session");
            }

            // Spawn camera thread (direct PCO SDK via FFI). System-tuning
            // snapshots are read once; no runtime command mutates them.
            let (
                cam_first_frame_timeout_ms,
                cam_first_frame_poll_ms,
                cam_frame_send_interval_ms,
                cam_poll_interval_ms,
            ) = {
                let cfg = app_state.config.lock();
                let s = &cfg.rig().system;
                (
                    s.camera_first_frame_timeout_ms,
                    s.camera_first_frame_poll_ms,
                    s.camera_frame_send_interval_ms,
                    s.camera_poll_interval_ms,
                )
            };
            std::thread::Builder::new()
                .name("camera".into())
                .spawn(move || {
                    camera_thread::run(
                        cam_cmd_rx,
                        cam_evt_tx,
                        cam_first_frame_timeout_ms,
                        cam_first_frame_poll_ms,
                        cam_frame_send_interval_ms,
                        cam_poll_interval_ms,
                    );
                })
                .map_err(|e| {
                    Box::<dyn std::error::Error>::from(format!("spawn camera thread: {e}"))
                })?;

            // Stimulus thread is spawned on-demand when a display is selected;
            // its spawn handles are held in `threads.stimulus_spawn`.

            // ── Manage state ──
            let shared_state = Arc::new(app_state);
            app.manage(shared_state.clone());

            // ── Event forwarder: bridges crossbeam channels to Tauri events ──
            let handle = app.handle().clone();
            std::thread::Builder::new()
                .name("event_forwarder".into())
                .spawn(move || {
                    events::run_event_forwarder(handle, shared_state);
                })
                .map_err(|e| {
                    Box::<dyn std::error::Error>::from(format!("spawn event forwarder thread: {e}"))
                })?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Library
            commands::library::list_oisi_files,
            commands::library::delete_oisi_files,
            commands::library::get_data_directory,
            commands::library::set_data_directory,
            // Import
            commands::library::import_snlc,
            commands::library::import_snlc_sample_data,
            // Analysis
            commands::analysis::get_analysis_backend,
            commands::analysis::inspect_oisi,
            commands::analysis::run_analysis,
            commands::analysis::get_analysis_params,
            commands::analysis::set_active_oisi,
            commands::analysis::migrate_oisi,
            commands::analysis::read_result,
            commands::analysis::read_anatomical,
            commands::analysis::export_map_png,
            // Hardware
            commands::hardware::get_monitors,
            commands::hardware::select_display,
            commands::hardware::validate_display,
            commands::hardware::validate_timing,
            commands::hardware::set_monitor_rotation,
            commands::hardware::set_display_dimensions,
            commands::hardware::get_rig_geometry,
            commands::hardware::set_viewing_distance,
            commands::hardware::get_ring_overlay,
            commands::hardware::set_ring_overlay,
            commands::hardware::calibrate_um_per_pixel_from_ring,
            commands::hardware::enumerate_cameras,
            commands::hardware::connect_camera,
            commands::hardware::disconnect_camera,
            // Camera tool
            commands::hardware::capture_anatomical,
            commands::hardware::set_exposure,
            // Experiment tool
            commands::experiment::get_experiment,
            commands::experiment::update_experiment,
            commands::experiment::load_experiment,
            commands::experiment::save_experiment_as,
            commands::experiment::list_experiments,
            commands::experiment::get_duration_summary,
            commands::acquire::start_preview,
            commands::acquire::stop_preview,
            // Acquire tool
            commands::acquire::set_session_metadata,
            commands::acquire::start_acquisition,
            commands::acquire::stop_acquisition,
            commands::acquire::save_acquisition,
            commands::acquire::discard_acquisition,
            // Workspace state
            commands::acquire::get_session,
            commands::acquire::get_workspace_status,
            // Parameters (typed config store)
            crate::params::commands::get_param_descriptors,
            crate::params::commands::get_analysis_stages,
            crate::params::commands::set_params,
        ])
        .build(tauri::generate_context!())
        .map_err(|e| AppError::Config(format!("build Tauri application: {e}")))?
        .run(move |app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                // Send shutdown commands to background threads so they clean up
                // hardware. State is managed inside setup(), so fetch it from the
                // app handle. Lock/send failures during shutdown are expected
                // (threads may have already exited) and intentionally ignored.
                tracing::info!("shutting down");
                crate::end_realtime_timer();
                if let Some(state) = app_handle.try_state::<crate::commands::SharedState>() {
                    let s = state.inner();
                    // Persist the durable session for next-launch resume.
                    // user_dir comes from the config store; lock briefly and release.
                    let user_dir = s.config.lock().user_dir().to_path_buf();
                    {
                        let dir = user_dir;
                        // Capture the durable session snapshot. Copy the active
                        // path out first, then lock session — one group at a time.
                        let active = s.active_oisi.lock().clone();
                        let persisted = {
                            let session = s.session.lock();
                            crate::session::PersistedSession::capture(&session, active.as_deref())
                        };
                        if let Err(e) = persisted.save(&dir) {
                            tracing::error!(error = %e, "failed to save session");
                        } else {
                            tracing::info!(dir = %dir.display(), "session saved");
                        }

                        // Persist the typed config user layer (rig/experiment/
                        // analysis JSON). Without this, UI edits to params would
                        // not survive a restart. At shutdown there is no
                        // contention, so holding the config lock across the
                        // writes is fine.
                        {
                            let cfg = s.config.lock();
                            if let Err(e) = cfg.save_all() {
                                tracing::error!(error = %e, "failed to save param overlays");
                            } else {
                                tracing::info!(dir = %dir.display(), "param overlays saved");
                            }
                        }
                    }
                    let _ = s
                        .threads
                        .camera_tx
                        .send(crate::messages::CameraCmd::Shutdown);
                    let _ = s
                        .threads
                        .stimulus_tx
                        .send(crate::messages::StimulusCmd::Shutdown);
                }
                // Give threads time to close hardware handles.
                // Camera needs to stop recording, disarm, and call PCO_CloseCamera.
                std::thread::sleep(std::time::Duration::from_millis(1000));
                tracing::info!("shutdown complete");
            }
        });
    Ok(())
}

// Path-resolution policy is unit-tested in `config_paths`; the exe-relative
// `find_config_dir` search (and its tests) was removed in favor of
// CARGO_MANIFEST_DIR (dev) + resource_dir (prod), which don't get shadowed
// by stray `target/` copies.
