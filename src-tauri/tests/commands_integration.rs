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
use std::sync::{Arc, Mutex};

use openisi_lib::commands::SharedState;
use openisi_lib::commands::analysis::set_active_oisi_impl;
use openisi_lib::state::AppState;
use openisi_lib::params::Registry;

/// Build a minimal `SharedState` for testing — Registry with defaults,
/// no real config dir loaded.
fn make_state() -> SharedState {
    let tmp_cfg = tempfile::tempdir().unwrap();
    let registry = Registry::new(tmp_cfg.path(), tmp_cfg.path());
    Arc::new(Mutex::new(AppState::new(registry)))
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
    let app = state.lock().unwrap();
    assert_eq!(app.active_oisi_path.as_ref().unwrap(), &path);
}

#[test]
fn set_active_oisi_empty_string_clears_active_path() {
    let state = make_state();
    let (path, _dir) = make_oisi_tempfile();
    set_active_oisi_impl(&state, path.to_string_lossy().into_owned()).unwrap();
    assert!(state.lock().unwrap().active_oisi_path.is_some());

    set_active_oisi_impl(&state, String::new()).unwrap();
    assert!(state.lock().unwrap().active_oisi_path.is_none());
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
