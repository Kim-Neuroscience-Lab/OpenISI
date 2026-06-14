//! Integration tests for orchestrator-level Tauri command behavior that
//! does not need a Tauri runtime — exercises the `_impl` variants
//! directly with a hand-built `SharedState`.
//!
//! Scope is intentionally narrow: active-oisi-path tracking and schema-
//! migration detection. Analysis-param read/write tests previously lived
//! here against an API shape (`AnalysisParams::bootstrap`,
//! `set_analysis_params_impl`, `get/set_method_tunable_impl`) that was
//! retired in favor of the Registry SSoT path — params now persist as
//! the Registry-tree JSON, not as serde of `AnalysisParams`. Those tests
//! should be rewritten against `params::commands::set_params` /
//! `get_param_descriptors` if/when that coverage is wanted; the discarded
//! versions were not preserved.

use std::path::PathBuf;
use std::sync::Arc;

use openisi_lib::commands::SharedState;
use openisi_lib::commands::analysis::{migrate_oisi, set_active_oisi_impl};
use openisi_lib::state::{AppState, StimulusSpawn, ThreadHandles};

/// Build a minimal `SharedState` for testing — `ConfigStore` with defaults,
/// no real config dir loaded. `AppState` is decomposed into per-group
/// `parking_lot` mutexes, so the shared handle is a plain `Arc`.
fn make_state() -> SharedState {
    let tmp_cfg = tempfile::tempdir().unwrap();
    let config = openisi_params::config::ConfigStore::new(tmp_cfg.path(), tmp_cfg.path());

    let (stim_cmd_tx, stim_cmd_rx) = crossbeam_channel::unbounded();
    let (stim_evt_tx, stim_evt_rx) = crossbeam_channel::unbounded();
    let (cam_cmd_tx, _cam_cmd_rx) = crossbeam_channel::unbounded();
    let (_cam_evt_tx, cam_evt_rx) = crossbeam_channel::unbounded();
    let (analysis_cmd_tx, _analysis_cmd_rx) = crossbeam_channel::unbounded();
    let (_analysis_evt_tx, analysis_evt_rx) = crossbeam_channel::unbounded();

    let threads = ThreadHandles {
        stimulus_tx: stim_cmd_tx,
        stimulus_rx: stim_evt_rx,
        camera_tx: cam_cmd_tx,
        camera_rx: cam_evt_rx,
        analysis_tx: analysis_cmd_tx,
        analysis_rx: analysis_evt_rx,
        stimulus_spawn: parking_lot::Mutex::new(StimulusSpawn {
            cmd_rx: Some(stim_cmd_rx),
            evt_tx: Some(stim_evt_tx),
            spawned: false,
        }),
    };

    Arc::new(AppState::new(config, threads, Vec::new()))
}

/// Build a real (HDF5) .oisi tempfile so set_active_oisi's existence
/// check passes. Returns the path + the TempDir so the caller keeps the
/// dir alive (TempDir drops auto-delete).
fn make_oisi_tempfile() -> (PathBuf, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.oisi");
    isi_analysis::io::create(&path, "test").expect("create .oisi");
    (path, dir)
}

#[test]
fn set_active_oisi_rejects_nonexistent_path() {
    let state = make_state();
    let result = set_active_oisi_impl(&state, "/nonexistent/path/to/file.oisi".into());
    assert!(result.is_err(), "expected error for nonexistent path");
    let err = result.unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("does not exist"), "got: {msg}");
}

#[test]
fn set_active_oisi_accepts_existing_path() {
    let state = make_state();
    let (path, _dir) = make_oisi_tempfile();
    set_active_oisi_impl(&state, path.to_string_lossy().into_owned()).unwrap();
    assert_eq!(state.active_oisi.lock().as_ref().unwrap(), &path);
}

#[test]
fn set_active_oisi_empty_string_clears_active_path() {
    let state = make_state();
    let (path, _dir) = make_oisi_tempfile();
    set_active_oisi_impl(&state, path.to_string_lossy().into_owned()).unwrap();
    assert!(state.active_oisi.lock().is_some());

    set_active_oisi_impl(&state, String::new()).unwrap();
    assert!(state.active_oisi.lock().is_none());
}

#[test]
fn schema_migration_detection_via_io_helper() {
    // Construct a pre-2026 .oisi with /analysis_params containing a
    // moved field; verify is_pre_2026_analysis_params reports true.
    let (path, _dir) = make_oisi_tempfile();
    let file = hdf5::File::open_rw(&path).unwrap();
    let attr = file
        .new_attr::<hdf5::types::VarLenUnicode>()
        .create("analysis_params")
        .unwrap();
    let val: hdf5::types::VarLenUnicode =
        r#"{"azi_angular_range":120.0,"cycle_combine":{"method":"marshel_garrett2011_delay_subtraction"}}"#
            .parse()
            .unwrap();
    attr.write_scalar(&val).unwrap();
    drop(file);

    assert!(
        isi_analysis::io::is_pre_2026_analysis_params(&path).unwrap(),
        "expected pre-2026 detection to fire"
    );
}

#[test]
fn migrate_oisi_command_brings_old_file_forward() {
    // Write a pre-2026 /analysis_params: a flat tunable sibling of `method`
    // (the genuine legacy marker). Raw attr write to avoid a serde_json dep
    // in the test crate, mirroring the detection test above.
    let (path, _dir) = make_oisi_tempfile();
    {
        let file = hdf5::File::open_rw(&path).unwrap();
        let attr = file
            .new_attr::<hdf5::types::VarLenUnicode>()
            .create("analysis_params")
            .unwrap();
        let val: hdf5::types::VarLenUnicode =
            r#"{"phase_smoothing":{"method":"open_isi_amp_weighted_phasor","sigma_px":2.5}}"#
                .parse()
                .unwrap();
        attr.write_scalar(&val).unwrap();
    }
    assert!(isi_analysis::io::is_pre_2026_analysis_params(&path).unwrap());

    // Migrate via the Tauri command.
    let msg = migrate_oisi(path.to_string_lossy().into_owned()).unwrap();
    assert!(msg.contains("Migrated"), "got: {msg}");
    assert!(
        !isi_analysis::io::is_pre_2026_analysis_params(&path).unwrap(),
        "file should be current-schema after migrate"
    );

    // Idempotent: a second call on the now-current file is a no-op.
    let msg2 = migrate_oisi(path.to_string_lossy().into_owned()).unwrap();
    assert!(msg2.contains("no migration needed"), "got: {msg2}");
}
