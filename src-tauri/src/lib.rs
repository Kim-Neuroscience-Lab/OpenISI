pub mod camera_thread;
pub mod commands;
pub mod config;
pub mod error;
pub mod events;
pub mod export;
pub mod messages;
pub mod monitor;
pub mod session;
pub mod state;
pub mod stimulus_thread;
pub mod timing;

use std::sync::{Arc, Mutex};

use config::ConfigManager;
use state::AppState;

pub fn run() {
    // Load config from the config directory.
    // Try: 1) <exe_dir>/../config  2) <exe_dir>/config  3) ./config
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")));

    let config_dir = find_config_dir(&exe_dir);

    let config = match ConfigManager::load(&config_dir) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!(
                "[openisi] Failed to load config from {}: {e}",
                config_dir.display()
            );
            std::process::exit(1);
        }
    };

    start_tauri(config);
}

/// Build the list of candidate config directories relative to the given exe directory.
/// Extracted for testability.
fn config_candidates(exe_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    vec![
        exe_dir.join("config"),
        exe_dir.join("../config"),
        exe_dir.join("../../config"),
    ]
}

/// Find the config directory from the candidates list.
/// Returns the first candidate that contains `rig.toml`.
/// Panics if no config directory is found.
fn find_config_dir(exe_dir: &std::path::Path) -> std::path::PathBuf {
    config_candidates(exe_dir)
        .into_iter()
        .find(|p| p.join("rig.toml").exists())
        .unwrap_or_else(|| {
            let candidates: Vec<_> = config_candidates(exe_dir)
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            panic!(
                "[openisi] Cannot find config directory with rig.toml.\n\
                 Searched: {}",
                candidates.join(", ")
            );
        })
}

fn start_tauri(config: ConfigManager) {
    // Create channels for stimulus thread
    let (stim_cmd_tx, stim_cmd_rx) = crossbeam_channel::unbounded();
    let (stim_evt_tx, stim_evt_rx) = crossbeam_channel::unbounded();

    // Create channels for camera thread
    let (cam_cmd_tx, cam_cmd_rx) = crossbeam_channel::unbounded();
    let (cam_evt_tx, cam_evt_rx) = crossbeam_channel::unbounded();

    // Build app state
    let mut app_state = AppState::new(config);

    app_state.threads.stimulus_tx = Some(stim_cmd_tx);
    app_state.threads.stimulus_rx = Some(stim_evt_rx);
    app_state.threads.camera_tx = Some(cam_cmd_tx);
    app_state.threads.camera_rx = Some(cam_evt_rx);

    // Detect monitors at startup
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

    // Spawn camera thread (direct PCO SDK, no daemon needed).
    // The camera thread receives a snapshot of SystemTuning at startup.
    // Invariant: no command modifies rig.system at runtime, so this snapshot
    // stays in sync for the lifetime of the thread. If a command that writes
    // to rig.system is ever added, the camera thread must be notified via a
    // new CameraCmd::UpdateConfig message.
    let cam_cfg = match app_state.config.lock() {
        Ok(cfg) => cfg.rig.system.clone(),
        Err(_) => {
            eprintln!("[openisi] config lock poisoned during initialization");
            std::process::exit(1);
        }
    };
    if let Err(e) = std::thread::Builder::new()
        .name("camera".into())
        .spawn(move || {
            camera_thread::run(cam_cmd_rx, cam_evt_tx, cam_cfg);
        })
    {
        eprintln!("[openisi] Failed to spawn camera thread: {e}");
        std::process::exit(1);
    }

    // Stimulus thread is spawned on-demand when a display is selected.
    app_state.threads.stim_cmd_rx = Some(stim_cmd_rx);
    app_state.threads.stim_evt_tx = Some(stim_evt_tx);

    let shared_state = Arc::new(Mutex::new(app_state));
    let shared_state_for_shutdown = shared_state.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(shared_state.clone())
        .setup(move |app| {
            // Start the event forwarding loop — bridges crossbeam channels to Tauri events.
            let handle = app.handle().clone();
            let state_for_events = shared_state.clone();
            std::thread::Builder::new()
                .name("event_forwarder".into())
                .spawn(move || {
                    events::run_event_forwarder(handle, state_for_events);
                })
                .unwrap_or_else(|e| {
                    eprintln!("[openisi] Failed to spawn event forwarder: {e}");
                    std::process::exit(1);
                });

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
            commands::analysis::inspect_oisi,
            commands::analysis::run_analysis,
            commands::analysis::get_analysis_params,
            commands::analysis::set_analysis_params,
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
        ])
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| {
            eprintln!("[openisi] Failed to build Tauri application: {e}");
            std::process::exit(1);
        })
        .run(move |_app, event| {
            if let tauri::RunEvent::Exit = event {
                // Send shutdown commands to background threads so they clean up hardware.
                eprintln!("[openisi] shutting down...");
                if let Ok(state) = shared_state_for_shutdown.lock() {
                    if let Some(ref tx) = state.threads.camera_tx {
                        let _ = tx.send(crate::messages::CameraCmd::Shutdown);
                    }
                    if let Some(ref tx) = state.threads.stimulus_tx {
                        let _ = tx.send(crate::messages::StimulusCmd::Shutdown);
                    }
                }
                // Give threads time to close hardware handles.
                // Camera needs to stop recording, disarm, and call PCO_CloseCamera.
                std::thread::sleep(std::time::Duration::from_millis(1000));
                eprintln!("[openisi] shutdown complete");
            }
        });
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Helper: create a temp dir with config/rig.toml inside.
    fn make_config_tree(base: &std::path::Path, rel_config: &str) {
        let config_dir = base.join(rel_config);
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("rig.toml"), "# placeholder").unwrap();
    }

    #[test]
    fn candidates_has_correct_relative_paths() {
        let exe_dir = PathBuf::from("/app/bin");
        let candidates = config_candidates(&exe_dir);

        assert_eq!(candidates.len(), 3);
        assert_eq!(candidates[0], PathBuf::from("/app/bin/config"));
        assert_eq!(candidates[1], PathBuf::from("/app/bin/../config"));
        assert_eq!(candidates[2], PathBuf::from("/app/bin/../../config"));
    }

    #[test]
    fn find_config_dir_installed_layout() {
        let tmp = std::env::temp_dir().join("openisi_test_installed");
        let _ = fs::remove_dir_all(&tmp);
        let exe_dir = tmp.join("bin");
        fs::create_dir_all(&exe_dir).unwrap();
        make_config_tree(&exe_dir, "config");

        let found = find_config_dir(&exe_dir);
        assert!(found.join("rig.toml").exists(),
            "Should find config at <exe_dir>/config");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn find_config_dir_dev_build_layout() {
        let tmp = std::env::temp_dir().join("openisi_test_dev");
        let _ = fs::remove_dir_all(&tmp);
        let exe_dir = tmp.join("src-tauri").join("target").join("debug");
        fs::create_dir_all(&exe_dir).unwrap();
        make_config_tree(&tmp.join("src-tauri"), "config");

        let found = find_config_dir(&exe_dir);
        assert!(found.join("rig.toml").exists(),
            "Should find config via ../config for dev build layout");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn find_config_dir_workspace_layout() {
        let tmp = std::env::temp_dir().join("openisi_test_workspace");
        let _ = fs::remove_dir_all(&tmp);
        let exe_dir = tmp.join("target").join("debug");
        fs::create_dir_all(&exe_dir).unwrap();
        make_config_tree(&tmp, "config");

        let found = find_config_dir(&exe_dir);
        assert!(found.join("rig.toml").exists(),
            "Should find config via ../../config for workspace layout");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    #[should_panic(expected = "Cannot find config directory")]
    fn find_config_dir_panics_when_nothing_exists() {
        let tmp = std::env::temp_dir().join("openisi_test_fallback");
        let _ = fs::remove_dir_all(&tmp);
        let exe_dir = tmp.join("empty");
        fs::create_dir_all(&exe_dir).unwrap();

        find_config_dir(&exe_dir); // Should panic
    }

    #[test]
    fn find_config_dir_prefers_first_candidate() {
        let tmp = std::env::temp_dir().join("openisi_test_priority");
        let _ = fs::remove_dir_all(&tmp);
        let exe_dir = tmp.join("inner");
        fs::create_dir_all(&exe_dir).unwrap();
        make_config_tree(&exe_dir, "config");
        make_config_tree(&tmp, "config");

        let found = find_config_dir(&exe_dir);
        let canonical = fs::canonicalize(&found).unwrap();
        let expected = fs::canonicalize(exe_dir.join("config")).unwrap();
        assert_eq!(canonical, expected,
            "Should prefer exe_dir/config over ../config");

        let _ = fs::remove_dir_all(&tmp);
    }
}
