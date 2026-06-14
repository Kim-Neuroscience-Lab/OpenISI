//! End-to-end NWB-export test against a **real** Rust-written `.oisi`.
//!
//! The Python conformance gate (`tools/export_nwb/validate_export.py`) exercises
//! the export on a fixture, but a synthetic Python-authored `.oisi` can drift from
//! what `write_oisi` actually emits (dtypes, empty arrays, path names). This test
//! closes that gap: it writes a genuine raw acquisition `.oisi` through the real
//! `openisi_lib::export::write_oisi`, then runs the export bridge + the round-trip
//! fidelity check on it and asserts both succeed.
//!
//! Skips (does not fail) when Python or `pynwb` is unavailable — the export is an
//! optional, export-time-only path, so the Rust suite must not hard-depend on it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use openisi_lib::export::{write_oisi, AccumulatedData, OisiBundle, SweepSchedule};
use openisi_stimulus::dataset::{DatasetConfig, EnvelopeType, StimulusDataset};
use openisi_stimulus::geometry::{DisplayGeometry, ProjectionType};
use openisi_stimulus::sequencer::Order;

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = <repo>/src-tauri
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn python() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

/// True iff `python -c "import pynwb"` succeeds.
fn pynwb_available() -> bool {
    Command::new(python())
        .args(["-c", "import pynwb, nwbinspector, h5py"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn build_real_oisi(path: &Path) {
    let config = DatasetConfig {
        envelope: EnvelopeType::Bar,
        stimulus_params: HashMap::new(),
        conditions: vec!["LR".into(), "RL".into()],
        repetitions: 1,
        order: Order::Sequential,
        baseline_start_sec: 1.0,
        baseline_end_sec: 1.0,
        inter_stimulus_sec: 0.5,
        inter_direction_sec: 0.5,
        sweep_duration_sec: 3.0,
        geometry: DisplayGeometry::new(
            ProjectionType::Cartesian, 25.0, 0.0, 0.0, 0.0, 0.0, 53.0, 30.0, 1920, 1080,
        ),
        display_physical_source: "test".into(),
        reported_refresh_hz: 60.0,
        measured_refresh_hz: 59.94,
        target_stimulus_fps: 0,
        drop_detection_warmup_frames: 10,
        drop_detection_threshold: 1.5,
        fps_window_frames: 10,
    };
    let mut ds = StimulusDataset::new(config);
    ds.start_recording();

    let (w, h, t) = (8u32, 8u32, 6usize);
    let camera_data = AccumulatedData {
        frames: (0..t).map(|i| vec![(i as u16) * 10 + 1; (w * h) as usize]).collect(),
        hardware_timestamps_us: (0..t as i64).map(|i| i * 33_400).collect(),
        system_timestamps_us: (0..t as i64).map(|i| i * 33_400 + 50).collect(),
        sequence_numbers: (0..t as u64).collect(),
        width: w,
        height: h,
    };
    // A populated schedule so the sweeps/TimeIntervals branch is exercised too.
    let schedule = SweepSchedule {
        sweep_sequence: vec!["LR".into(), "RL".into()],
        sweep_start_us: vec![0, 100_000],
        sweep_end_us: vec![60_000, 160_000],
    };
    let snapshot = openisi_params::config::ConfigStore::new(Path::new("."), Path::new("."))
        .snapshot();

    write_oisi(
        path,
        OisiBundle {
            stimulus_dataset: &ds,
            camera_data,
            snapshot: &snapshot,
            hardware: None,
            schedule: &schedule,
            timing: None,
            session_meta: None,
            anatomical: None,
            acquisition_complete: true,
            stimulus_timing_validatable: true,
        },
    )
    .expect("write_oisi must succeed");
}

#[test]
fn real_oisi_exports_to_valid_nwb() {
    if !pynwb_available() {
        eprintln!("[nwb_export_e2e] python/pynwb unavailable — skipping (export is optional)");
        return;
    }
    let root = repo_root();
    let dir = std::env::temp_dir().join(format!("oisi_nwb_e2e_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let oisi = dir.join("real.oisi");
    let nwb = dir.join("real.nwb");

    build_real_oisi(&oisi);

    // Export via the Python bridge.
    let status = Command::new(python())
        .arg(root.join("tools/export_nwb/export_oisi_to_nwb.py"))
        .arg(&oisi)
        .arg(&nwb)
        .arg("--metadata")
        .arg(root.join("tools/export_nwb/metadata.example.json"))
        .status()
        .expect("launch export bridge");
    assert!(status.success(), "export bridge failed on real .oisi");

    // Round-trip fidelity: every present datum byte-identical.
    let status = Command::new(python())
        .arg(root.join("tools/export_nwb/roundtrip_check.py"))
        .arg(&oisi)
        .arg(&nwb)
        .status()
        .expect("launch roundtrip check");
    assert!(status.success(), "round-trip fidelity check failed on real .oisi");

    let _ = std::fs::remove_dir_all(&dir);
}
