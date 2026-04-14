//! Tauri IPC commands — frontend calls these via `invoke()`.
//!
//! Organized by domain. Each module's commands are independent.

pub mod acquire;
pub mod analysis;
pub mod experiment;
pub mod hardware;
pub mod library;

use std::sync::{Arc, Mutex};

use crate::state::AppState;

/// Shared application state, passed to every command via Tauri's `State` extractor.
pub type SharedState = Arc<Mutex<AppState>>;

// Re-export all commands so lib.rs can use `commands::function_name`.
pub use acquire::{
    discard_acquisition, get_session, get_workspace_status, save_acquisition,
    set_session_metadata, start_acquisition, start_preview, stop_acquisition, stop_preview,
    WorkspaceStatus,
};
pub use analysis::{
    export_map_png, get_analysis_params, inspect_oisi, read_anatomical, read_result,
    run_analysis, set_analysis_params,
};
pub use experiment::{
    get_duration_summary, get_experiment, list_experiments, load_experiment, save_experiment_as,
    update_experiment, DurationSummary, ExperimentSummary,
};
pub use hardware::{
    capture_anatomical, connect_camera, disconnect_camera, enumerate_cameras, get_monitors,
    get_rig_geometry, get_ring_overlay, select_display, set_display_dimensions, set_exposure,
    set_monitor_rotation, set_ring_overlay, set_viewing_distance, validate_display,
    validate_timing,
};
pub use library::{
    delete_oisi_files, get_data_directory, import_snlc, import_snlc_sample_data, list_oisi_files,
    set_data_directory, OisiFileInfo,
};
