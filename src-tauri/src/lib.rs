pub mod camera_thread;
pub mod commands;
pub mod config_paths;
pub mod error;
pub mod events;
pub mod export;
pub mod messages;
pub mod monitor;
pub mod params;
pub mod sample_data;
pub mod session;
pub mod state;
pub mod stimulus_thread;
pub mod timing;

use std::sync::{Arc, Mutex};

use error::{AppError, AppResult};
use params::Registry;
use state::AppState;

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
    // Registry construction + config-dir resolution happen inside the Tauri
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
            // ── Resolve config directories properly via Tauri's path API ──
            // The registry is constructed HERE (not before the app) because
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
            eprintln!(
                "[openisi] config profile={} shipped={} user={}",
                profile.as_str(),
                layout.shipped_dir.display(),
                layout.user_dir.display()
            );

            // ── Build + load the registry ──
            let mut registry = Registry::new(&layout.shipped_dir, &layout.user_dir);
            registry.load_rig().map_err(|e| Box::<dyn std::error::Error>::from(
                format!("load rig params from {}: {e}", layout.shipped_dir.display())))?;
            registry.load_experiment().map_err(|e| Box::<dyn std::error::Error>::from(
                format!("load experiment params from {}: {e}", layout.shipped_dir.display())))?;

            // ── First-run default data directory ──
            // A deliberate, visible default (<Documents>/OpenISI) persisted
            // explicitly into the user layer and surfaced in the UI — not an
            // ongoing silent fallback.
            if registry.data_directory().is_empty() {
                if let Some(default_dir) =
                    config_paths::default_data_dir(app.path().document_dir().ok().as_deref())
                {
                    std::fs::create_dir_all(&default_dir).map_err(|e| {
                        Box::<dyn std::error::Error>::from(format!(
                            "create default data dir {}: {e}", default_dir.display()))
                    })?;
                    registry
                        .set(
                            crate::params::ParamId::DataDirectory,
                            crate::params::ParamValue::String(default_dir.to_string_lossy().into_owned()),
                        )
                        .map_err(|e| Box::<dyn std::error::Error>::from(
                            format!("set default data dir: {e}")))?;
                    registry.save_rig().map_err(|e| Box::<dyn std::error::Error>::from(
                        format!("persist default data dir to {}: {e}", layout.user_dir.display())))?;
                    eprintln!("[openisi] data directory defaulted to {}", default_dir.display());
                }
            }

            // ── Param-change observer for IPC ──
            registry.set_observer(Box::new(
                crate::params::observer::TauriParamObserver::new(app.handle().clone()),
            ));

            // ── Channels ──
            let (stim_cmd_tx, stim_cmd_rx) = crossbeam_channel::unbounded();
            let (stim_evt_tx, stim_evt_rx) = crossbeam_channel::unbounded();
            let (cam_cmd_tx, cam_cmd_rx) = crossbeam_channel::unbounded();
            let (cam_evt_tx, cam_evt_rx) = crossbeam_channel::unbounded();

            // ── App state ──
            let mut app_state = AppState::new(registry);
            app_state.threads.stimulus_tx = Some(stim_cmd_tx);
            app_state.threads.stimulus_rx = Some(stim_evt_rx);
            app_state.threads.camera_tx = Some(cam_cmd_tx);
            app_state.threads.camera_rx = Some(cam_evt_rx);

            // Detect monitors at startup.
            let monitors = monitor::detect_monitors();
            eprintln!("[openisi] Detected {} monitors", monitors.len());
            for m in &monitors {
                eprintln!(
                    "  [{}] {} {}x{} @{}Hz ({:.1}x{:.1}cm) at ({},{})",
                    m.index, m.name, m.width_px, m.height_px, m.refresh_hz,
                    m.width_cm, m.height_cm, m.position.0, m.position.1
                );
            }
            app_state.monitors = monitors;

            // ── Restore the last session (best-effort, from user_dir) ──
            // Durable user intent only: animal_id/notes always; the selected
            // display only if a matching monitor is still present; the active
            // .oisi only if it still exists. Validation / camera / timing are
            // deliberately NOT restored — they must be re-established. A
            // missing/corrupt session file simply starts fresh (UI state, not
            // provenance), so this is best-effort with transparent logging.
            if let Some(saved) = crate::session::PersistedSession::load(&layout.user_dir) {
                app_state.session.animal_id = saved.animal_id;
                app_state.session.notes = saved.notes;
                if let Some(disp) = saved.selected_display {
                    if crate::session::PersistedSession::display_still_present(
                        &disp,
                        &app_state.monitors,
                    ) {
                        app_state.session.selected_display = Some(disp);
                        // display_validation intentionally left None — re-validate.
                    } else {
                        eprintln!(
                            "[openisi] saved display '{}' no longer present — not restored",
                            disp.name
                        );
                    }
                }
                if let Some(p) = saved.active_oisi_path {
                    if p.exists() {
                        app_state.active_oisi_path = Some(p);
                    } else {
                        eprintln!(
                            "[openisi] last active file {} no longer exists — not restored",
                            p.display()
                        );
                    }
                }
                eprintln!("[openisi] restored previous session");
            }

            // Spawn camera thread (direct PCO SDK via FFI). System-tuning
            // snapshots are read once; no runtime command mutates them.
            let (cam_first_frame_timeout_ms, cam_first_frame_poll_ms,
                 cam_frame_send_interval_ms, cam_poll_interval_ms) = {
                let reg = app_state.registry.lock().map_err(|_| {
                    Box::<dyn std::error::Error>::from("registry lock poisoned at camera init")
                })?;
                (
                    reg.camera_first_frame_timeout_ms(),
                    reg.camera_first_frame_poll_ms(),
                    reg.camera_frame_send_interval_ms(),
                    reg.camera_poll_interval_ms(),
                )
            };
            std::thread::Builder::new()
                .name("camera".into())
                .spawn(move || {
                    camera_thread::run(
                        cam_cmd_rx, cam_evt_tx,
                        cam_first_frame_timeout_ms,
                        cam_first_frame_poll_ms,
                        cam_frame_send_interval_ms,
                        cam_poll_interval_ms,
                    );
                })
                .map_err(|e| Box::<dyn std::error::Error>::from(
                    format!("spawn camera thread: {e}")))?;

            // Stimulus thread is spawned on-demand when a display is selected.
            app_state.threads.stim_cmd_rx = Some(stim_cmd_rx);
            app_state.threads.stim_evt_tx = Some(stim_evt_tx);

            // ── Manage state ──
            let shared_state = Arc::new(Mutex::new(app_state));
            app.manage(shared_state.clone());

            // ── Event forwarder: bridges crossbeam channels to Tauri events ──
            let handle = app.handle().clone();
            std::thread::Builder::new()
                .name("event_forwarder".into())
                .spawn(move || {
                    events::run_event_forwarder(handle, shared_state);
                })
                .map_err(|e| Box::<dyn std::error::Error>::from(
                    format!("spawn event forwarder thread: {e}")))?;

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
            // Parameter registry
            crate::params::commands::get_param_descriptors,
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
                eprintln!("[openisi] shutting down...");
                if let Some(state) = app_handle.try_state::<crate::commands::SharedState>() {
                    if let Ok(s) = state.lock() {
                        // Persist the durable session for next-launch resume.
                        // user_dir comes from the registry; lock briefly and release.
                        let user_dir = s.registry.lock().ok().map(|r| r.user_dir().to_path_buf());
                        if let Some(dir) = user_dir {
                            let persisted = crate::session::PersistedSession::capture(
                                &s.session,
                                s.active_oisi_path.as_deref(),
                            );
                            if let Err(e) = persisted.save(&dir) {
                                eprintln!("[openisi] failed to save session: {e}");
                            } else {
                                eprintln!("[openisi] session saved to {}", dir.display());
                            }
                        }
                        if let Some(ref tx) = s.threads.camera_tx {
                            let _ = tx.send(crate::messages::CameraCmd::Shutdown);
                        }
                        if let Some(ref tx) = s.threads.stimulus_tx {
                            let _ = tx.send(crate::messages::StimulusCmd::Shutdown);
                        }
                    }
                }
                // Give threads time to close hardware handles.
                // Camera needs to stop recording, disarm, and call PCO_CloseCamera.
                std::thread::sleep(std::time::Duration::from_millis(1000));
                eprintln!("[openisi] shutdown complete");
            }
        });
    Ok(())
}

// Path-resolution policy is unit-tested in `config_paths`; the exe-relative
// `find_config_dir` search (and its tests) was removed in favor of
// CARGO_MANIFEST_DIR (dev) + resource_dir (prod), which don't get shadowed
// by stray `target/` copies.
