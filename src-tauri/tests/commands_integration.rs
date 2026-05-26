//! Integration tests for Tauri commands that touch the canonical .oisi
//! record (set_active_oisi, get/set_analysis_params) and the schema-
//! migration logic in cmd_analyze.
//!
//! Tests exercise the `_impl` variants of each command (which take
//! `&SharedState` directly), so no Tauri runtime is required.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use openisi_lib::commands::SharedState;
use openisi_lib::commands::analysis::{
    get_analysis_params_impl, set_active_oisi_impl, set_analysis_params_impl,
};
use openisi_lib::state::AppState;
use openisi_lib::params::Registry;

/// Build a minimal `SharedState` for testing — Registry with defaults,
/// no real config dir loaded. The Registry's `new` constructor
/// accepts any path; we don't load TOML.
fn make_state() -> SharedState {
    let tmp_cfg = tempfile::tempdir().unwrap();
    let registry = Registry::new(tmp_cfg.path());
    Arc::new(Mutex::new(AppState::new(registry)))
}

/// Build a real (HDF5) .oisi tempfile so set_active_oisi's
/// existence check passes. Returns the path + the TempDir so the
/// caller keeps the dir alive (TempDir drops auto-delete).
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
fn get_analysis_params_returns_bootstrap_when_no_active_file() {
    let state = make_state();
    let result = get_analysis_params_impl(&state).unwrap();
    // Should be { "params": <bootstrap>, "source": { "source": "bootstrap_default" } }.
    let bootstrap_json = serde_json::to_value(isi_analysis::AnalysisParams::bootstrap()).unwrap();
    assert_eq!(result["params"], bootstrap_json);
    assert_eq!(result["source"]["source"], "bootstrap_default");
}

#[test]
fn get_analysis_params_returns_bootstrap_when_file_has_no_attr() {
    let state = make_state();
    let (path, _dir) = make_oisi_tempfile();
    set_active_oisi_impl(&state, path.to_string_lossy().into_owned()).unwrap();
    // File exists but has no /analysis_params attribute yet.
    let result = get_analysis_params_impl(&state).unwrap();
    let bootstrap_json = serde_json::to_value(isi_analysis::AnalysisParams::bootstrap()).unwrap();
    assert_eq!(result["params"], bootstrap_json);
    assert_eq!(result["source"]["source"], "bootstrap_default");
}

#[test]
fn set_analysis_params_rejects_when_no_active_file() {
    let state = make_state();
    let payload = serde_json::to_value(isi_analysis::AnalysisParams::bootstrap()).unwrap();
    let result = set_analysis_params_impl(&state, payload);
    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(msg.contains("no active") || msg.contains("NotAvailable") || msg.contains("not available"),
        "expected NotAvailable-like error, got: {msg}");
}

#[test]
fn set_analysis_params_rejects_malformed_json() {
    let state = make_state();
    let (path, _dir) = make_oisi_tempfile();
    set_active_oisi_impl(&state, path.to_string_lossy().into_owned()).unwrap();

    // JSON shape doesn't match AnalysisParams — has an unknown field.
    let bad = serde_json::json!({
        "cycle_combine": { "method": "marshel_garrett2011_delay_subtraction" },
        "this_is_not_a_valid_field": 42,
    });
    let result = set_analysis_params_impl(&state, bad);
    assert!(result.is_err(), "expected validation error");
    let msg = format!("{}", result.unwrap_err());
    assert!(msg.contains("invalid") || msg.contains("Validation") || msg.contains("unknown"),
        "expected validation message, got: {msg}");
}

#[test]
fn round_trip_set_get_analysis_params() {
    let state = make_state();
    let (path, _dir) = make_oisi_tempfile();
    set_active_oisi_impl(&state, path.to_string_lossy().into_owned()).unwrap();

    // Write a non-default AnalysisParams via set_analysis_params.
    let mut params = isi_analysis::AnalysisParams::bootstrap();
    params.cycle_combine = isi_analysis::methods::CycleCombineMethod::KalatskyStryker2003RawAverage;
    let payload = serde_json::to_value(&params).unwrap();
    set_analysis_params_impl(&state, payload).unwrap();

    // Read back via get_analysis_params; verify the params field matches.
    let got = get_analysis_params_impl(&state).unwrap();
    let expected = serde_json::to_value(&params).unwrap();
    assert_eq!(got["params"], expected);
    // After a write, source must be `stored_in_file`.
    assert_eq!(got["source"]["source"], "stored_in_file");
}

#[test]
fn round_trip_survives_overwrite() {
    let state = make_state();
    let (path, _dir) = make_oisi_tempfile();
    set_active_oisi_impl(&state, path.to_string_lossy().into_owned()).unwrap();

    // First write: default.
    let p1 = isi_analysis::AnalysisParams::bootstrap();
    set_analysis_params_impl(&state, serde_json::to_value(&p1).unwrap()).unwrap();

    // Second write: KalatskyStryker. Must overwrite.
    let mut p2 = isi_analysis::AnalysisParams::bootstrap();
    p2.cycle_combine = isi_analysis::methods::CycleCombineMethod::KalatskyStryker2003RawAverage;
    set_analysis_params_impl(&state, serde_json::to_value(&p2).unwrap()).unwrap();

    // Read: should be p2.
    let got = get_analysis_params_impl(&state).unwrap();
    let expected = serde_json::to_value(&p2).unwrap();
    assert_eq!(got["params"], expected);
}

#[test]
fn get_method_tunables_returns_descriptors_for_default_params() {
    use openisi_lib::commands::analysis::get_method_tunables_impl;
    let state = make_state();
    let (path, _dir) = make_oisi_tempfile();
    set_active_oisi_impl(&state, path.to_string_lossy().into_owned()).unwrap();
    let result = get_method_tunables_impl(&state).unwrap();
    // Response shape: { "stages": [...], "source": {...} }
    let stages = result["stages"].as_array().expect("expected stages array");
    // Default params have tunables on:
    //   phase_smoothing, sign_map_smoothing, cortex_source,
    //   patch_threshold, patch_extraction (5 stages)
    // patch_refinement::None has no tunables; cycle_combine/vfs_computation/
    // quality_gate/eccentricity carry none.
    assert_eq!(stages.len(), 5, "expected 5 stages with tunables, got: {result:#}");
    let stage_names: Vec<&str> = stages.iter()
        .map(|s| s.get("stage").and_then(|v| v.as_str()).unwrap())
        .collect();
    for expected in ["phase_smoothing", "sign_map_smoothing", "cortex_source", "patch_threshold", "patch_extraction"] {
        assert!(stage_names.contains(&expected), "missing stage {expected} in {stage_names:?}");
    }
    // No-attr file → source must be bootstrap_default.
    assert_eq!(result["source"]["source"], "bootstrap_default");
}

#[test]
fn set_method_tunable_round_trips_via_get() {
    use openisi_lib::commands::analysis::{get_method_tunables_impl, set_method_tunable_impl};
    let state = make_state();
    let (path, _dir) = make_oisi_tempfile();
    set_active_oisi_impl(&state, path.to_string_lossy().into_owned()).unwrap();

    // Set sigma_um on sign_map_smoothing to 80 (default is 60).
    set_method_tunable_impl(
        &state,
        "sign_map_smoothing".into(),
        "sigma_um".into(),
        serde_json::json!(80.0),
    ).unwrap();

    // Get back and verify. Response is { "stages": [...], "source": ... }.
    let result = get_method_tunables_impl(&state).unwrap();
    let stages = result["stages"].as_array().unwrap();
    let stage = stages.iter()
        .find(|s| s.get("stage").and_then(|v| v.as_str()) == Some("sign_map_smoothing"))
        .expect("sign_map_smoothing stage present");
    let tunable = stage.get("tunables").and_then(|v| v.as_array()).unwrap()
        .iter().find(|t| t.get("name").and_then(|v| v.as_str()) == Some("sigma_um"))
        .expect("sigma_um descriptor present");
    let current = tunable.get("current").and_then(|v| v.as_f64()).unwrap();
    assert_eq!(current, 80.0);
}

#[test]
fn set_method_tunable_rejects_unknown_stage() {
    use openisi_lib::commands::analysis::set_method_tunable_impl;
    let state = make_state();
    let (path, _dir) = make_oisi_tempfile();
    set_active_oisi_impl(&state, path.to_string_lossy().into_owned()).unwrap();
    let result = set_method_tunable_impl(
        &state, "not_a_stage".into(), "anything".into(), serde_json::json!(1.0));
    assert!(result.is_err());
}

#[test]
fn set_method_tunable_rejects_unknown_tunable_name() {
    use openisi_lib::commands::analysis::set_method_tunable_impl;
    let state = make_state();
    let (path, _dir) = make_oisi_tempfile();
    set_active_oisi_impl(&state, path.to_string_lossy().into_owned()).unwrap();
    let result = set_method_tunable_impl(
        &state, "sign_map_smoothing".into(), "not_a_field".into(), serde_json::json!(1.0));
    assert!(result.is_err());
}

#[test]
fn set_method_tunable_rejects_when_no_active_file() {
    use openisi_lib::commands::analysis::set_method_tunable_impl;
    let state = make_state();
    let result = set_method_tunable_impl(
        &state, "sign_map_smoothing".into(), "sigma_um".into(), serde_json::json!(80.0));
    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(msg.contains("no active") || msg.contains("NotAvailable"));
}

#[test]
fn schema_migration_detection_via_io_helper() {
    // Construct a pre-2026 .oisi with /analysis_params containing a
    // moved field; verify is_pre_2026_analysis_params reports true.
    let (path, _dir) = make_oisi_tempfile();
    // Write a stale-schema /analysis_params JSON manually.
    let file = hdf5::File::open_rw(&path).unwrap();
    let attr = file.new_attr::<hdf5::types::VarLenUnicode>()
        .create("analysis_params").unwrap();
    let val: hdf5::types::VarLenUnicode =
        r#"{"azi_angular_range":120.0,"cycle_combine":{"method":"marshel_garrett2011_delay_subtraction"}}"#
            .parse().unwrap();
    attr.write_scalar(&val).unwrap();
    drop(file);

    assert!(isi_analysis::io::is_pre_2026_analysis_params(&path).unwrap(),
        "expected pre-2026 detection to fire");
}
