//! Part 3 — incremental cache tests.
//!
//! Two concerns:
//!  1. **Fingerprint correctness** — the retinotopy fingerprint is deterministic
//!     and sensitive to every input that affects retinotopy (its params, the
//!     acquisition geometry, and the recording identity). This is the logic
//!     that decides restore-vs-recompute, so it must never collide across
//!     genuinely-different inputs.
//!  2. **End-to-end disk restore** — a definitive *sentinel-tamper* proof that a
//!     cache hit restores retinotopy FROM DISK rather than recomputing it: we
//!     overwrite the cached map with a sentinel, re-run, and confirm the
//!     sentinel survives into the result (a recompute would have overwritten it).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use hdf5::File as H5File;
use ndarray::Array2;

use isi_analysis::methods::{
    BaselineMethod, CortexSourceMethod, CycleAverageMethod, CycleCombineMethod,
    DirectionSmoothingMethod, EccentricityMethod, RectificationMethod, ResponseNormalizationMethod,
};
use isi_analysis::pipeline::fingerprint::{self, StageFingerprints};
use isi_analysis::{AcquisitionProperties, AnalysisParams, SilentProgress};
use openisi_params::config::analysis::{
    CortexSource, Eccentricity, PhaseSmoothing, SignMapSmoothing,
};
use openisi_params::config::AnalysisConfig;

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A canonical default `AnalysisParams` from the typed `AnalysisConfig` defaults.
fn default_params() -> AnalysisParams {
    AnalysisParams::from(&AnalysisConfig::default())
}

/// Default params except the phase-smoothing σ_px tunable, set on the typed
/// config (the same path production uses).
fn params_with_sigma(sigma_px: f64) -> AnalysisParams {
    let cfg = AnalysisConfig {
        phase_smoothing: PhaseSmoothing::SnlcAmpWeightedPhasor { sigma_px },
        ..Default::default()
    };
    AnalysisParams::from(&cfg)
}

/// Fixed acquisition geometry so fingerprints are stable across a test.
fn fixed_acq() -> AcquisitionProperties {
    AcquisitionProperties {
        rotation_k: 1,
        azi_angular_range: 120.0,
        alt_angular_range: 110.0,
        offset_azi: 0.0,
        offset_alt: 0.0,
        um_per_pixel: 9.7,
        ..Default::default()
    }
}

// ─── Fingerprint correctness ─────────────────────────────────────────────

#[test]
fn fingerprint_is_deterministic() {
    let (p, a) = (default_params(), fixed_acq());
    assert_eq!(
        fingerprint::retinotopy(&p, &a, "rec1"),
        fingerprint::retinotopy(&p, &a, "rec1"),
    );
}

#[test]
fn fingerprint_sensitive_to_recording_identity() {
    let (p, a) = (default_params(), fixed_acq());
    assert_ne!(
        fingerprint::retinotopy(&p, &a, "recA"),
        fingerprint::retinotopy(&p, &a, "recB"),
        "different recordings must not share a retinotopy cache key"
    );
}

#[test]
fn fingerprint_sensitive_to_geometry() {
    let p = default_params();
    let base = fingerprint::retinotopy(&p, &fixed_acq(), "rec");
    let mut a2 = fixed_acq();
    a2.rotation_k = 2;
    assert_ne!(
        base,
        fingerprint::retinotopy(&p, &a2, "rec"),
        "a rotation change alters retinotopy output → must alter the fingerprint"
    );
}

#[test]
fn fingerprint_sensitive_to_baseline() {
    // The ΔF/F baseline feeds stage 0 (complex maps), which retinotopy derives
    // from — so a baseline change must invalidate BOTH the stage-0 and the
    // retinotopy caches, or a stale result computed under a different baseline
    // would be served silently.
    let a = fixed_acq();
    let mut p = default_params();
    p.baseline = BaselineMethod::AllenAllFrameMean;
    let retino_base = fingerprint::retinotopy(&p, &a, "rec");
    let projection_base = fingerprint::projection(&p, "rec");
    p.baseline = BaselineMethod::OpenIsiInterSweepMean;
    assert_ne!(
        retino_base,
        fingerprint::retinotopy(&p, &a, "rec"),
        "a baseline change must alter the retinotopy fingerprint"
    );
    assert_ne!(
        projection_base,
        fingerprint::projection(&p, "rec"),
        "a baseline change must alter the projection (complex maps) fingerprint"
    );
}

#[test]
fn fingerprint_sensitive_to_retino_param() {
    let a = fixed_acq();
    let mut p = default_params();
    p.cycle_combine = CycleCombineMethod::KalatskyStryker2003DelaySubtraction;
    let base = fingerprint::retinotopy(&p, &a, "rec");
    p.cycle_combine = CycleCombineMethod::UnweightedCycleAverage;
    assert_ne!(
        base,
        fingerprint::retinotopy(&p, &a, "rec"),
        "a cycle-combine method change must alter the fingerprint"
    );
}

#[test]
fn fingerprint_sensitive_to_cycle_average() {
    // Cycle averaging changes the per-direction complex map, so it must
    // invalidate BOTH the projection (complex maps) and retinotopy caches.
    let a = fixed_acq();
    let mut p = default_params();
    p.cycle_average = CycleAverageMethod::SimpleComplexAverage;
    let retino_base = fingerprint::retinotopy(&p, &a, "rec");
    let projection_base = fingerprint::projection(&p, "rec");
    p.cycle_average = CycleAverageMethod::PhaseLockedAverage;
    assert_ne!(
        retino_base,
        fingerprint::retinotopy(&p, &a, "rec"),
        "a cycle-average change must alter the retinotopy fingerprint"
    );
    assert_ne!(
        projection_base,
        fingerprint::projection(&p, "rec"),
        "a cycle-average change must alter the projection fingerprint"
    );
}

#[test]
fn fingerprint_sensitive_to_response_normalization() {
    // Fractional ΔF/F vs absolute ΔF changes the per-direction complex map
    // amplitude, so it must invalidate BOTH the projection and retinotopy caches.
    let a = fixed_acq();
    let mut p = default_params();
    p.response_normalization = ResponseNormalizationMethod::OpenIsiFractionalDff;
    let retino_base = fingerprint::retinotopy(&p, &a, "rec");
    let projection_base = fingerprint::projection(&p, "rec");
    p.response_normalization = ResponseNormalizationMethod::OracleAbsoluteDeltaF;
    assert_ne!(
        retino_base,
        fingerprint::retinotopy(&p, &a, "rec"),
        "a response-normalization change must alter the retinotopy fingerprint"
    );
    assert_ne!(
        projection_base,
        fingerprint::projection(&p, "rec"),
        "a response-normalization change must alter the projection fingerprint"
    );
}

#[test]
fn fingerprint_sensitive_to_rectification() {
    // Pre-DFT rectification changes the per-direction complex map, so it must
    // invalidate BOTH the projection and retinotopy caches.
    let a = fixed_acq();
    let mut p = default_params();
    p.rectification = RectificationMethod::None;
    let retino_base = fingerprint::retinotopy(&p, &a, "rec");
    let projection_base = fingerprint::projection(&p, "rec");
    p.rectification = RectificationMethod::AllenZhuang2017ClipNegative;
    assert_ne!(
        retino_base,
        fingerprint::retinotopy(&p, &a, "rec"),
        "a rectification change must alter the retinotopy fingerprint"
    );
    assert_ne!(
        projection_base,
        fingerprint::projection(&p, "rec"),
        "a rectification change must alter the projection fingerprint"
    );
}

#[test]
fn fingerprint_sensitive_to_direction_smoothing() {
    // Pre-combine per-direction smoothing changes the complex maps fed to
    // cycle-combine, so it must alter the retinotopy fingerprint.
    let a = fixed_acq();
    let mut p = default_params();
    p.direction_smoothing = DirectionSmoothingMethod::None;
    let base = fingerprint::retinotopy(&p, &a, "rec");
    p.direction_smoothing = DirectionSmoothingMethod::SnlcAdaptiveSmoother { sigma_px: 2.0 };
    assert_ne!(
        base,
        fingerprint::retinotopy(&p, &a, "rec"),
        "a direction-smoothing change must alter the retinotopy fingerprint"
    );
}

/// The tunable-drift guard: changing ONLY a per-variant tunable (the
/// phase-smoothing σ_px, with the method unchanged) must change the retinotopy
/// fingerprint. This is precisely what the full-destructure match in
/// `fingerprint.rs` (`{ sigma_px }`, never `..`) guarantees — if a future edit
/// drops a tunable from the hash (e.g. by switching to `{ .. }`), this fails.
#[test]
fn fingerprint_sensitive_to_phase_smoothing_tunable() {
    let a = fixed_acq();
    assert_ne!(
        fingerprint::retinotopy(&params_with_sigma(1.0), &a, "rec"),
        fingerprint::retinotopy(&params_with_sigma(2.5), &a, "rec"),
        "changing only the phase-smoothing σ_px must alter the retinotopy fingerprint"
    );
}

// ─── Merkle per-stage invalidation (the never-stale property) ─────────────

/// All 11 stage fingerprints for an `AnalysisConfig` mutated by `mutate`. Fixed
/// geometry + recording identity so only the param change under test moves a key.
fn stage_fps(mutate: impl FnOnce(&mut AnalysisConfig)) -> StageFingerprints {
    let mut cfg = AnalysisConfig::default();
    mutate(&mut cfg);
    let params = AnalysisParams::from(&cfg);
    fingerprint::compute(&params, &fixed_acq(), "rec")
}

/// The load-bearing correctness property of the Merkle cache: a param change
/// invalidates **exactly** its sub-DAG — every stage downstream of the change
/// gets a new key (so it WILL recompute, never stale), and every stage upstream
/// keeps its key (so it stays cached, no needless recompute). Verified for an
/// upstream, a middle, and a terminal stage — including that `DerivedMaps` does
/// NOT depend on `Eccentricity` (it isn't in its `deps()`).
#[test]
fn merkle_invalidation_is_exact() {
    let base = stage_fps(|_| {});

    // (1) A SignSmoothing input (σ_um) → sign_smoothing and ALL downstream keys
    //     change; baseline/projection/retinotopy (upstream) are untouched.
    let s = stage_fps(|c| {
        // Default is 60.0; 123.0 is a different in-range (0..=500) value.
        c.sign_map_smoothing = SignMapSmoothing::Gaussian { sigma_um: 123.0 };
    });
    assert_eq!(s.baseline, base.baseline, "upstream baseline must stay cached");
    assert_eq!(s.projection, base.projection, "upstream projection cached");
    assert_eq!(s.retinotopy, base.retinotopy, "upstream retinotopy cached");
    assert_ne!(s.sign_smoothing, base.sign_smoothing);
    assert_ne!(s.cortex_source, base.cortex_source);
    assert_ne!(s.patch_threshold, base.patch_threshold);
    assert_ne!(s.patch_extraction, base.patch_extraction);
    assert_ne!(s.patch_refinement, base.patch_refinement);
    assert_ne!(s.labels, base.labels);
    assert_ne!(s.eccentricity, base.eccentricity);
    assert_ne!(s.derived_maps, base.derived_maps);

    // (2) A CortexSource method change → cortex_source + everything downstream of
    //     it changes; baseline..sign_smoothing (upstream) stay cached.
    let c = stage_fps(|c| {
        c.cortex_source = CortexSource::NoRestriction;
    });
    assert_eq!(c.retinotopy, base.retinotopy);
    assert_eq!(c.sign_smoothing, base.sign_smoothing);
    assert_ne!(c.cortex_source, base.cortex_source);
    assert_ne!(c.patch_threshold, base.patch_threshold);
    assert_ne!(c.derived_maps, base.derived_maps);

    // (3) An Eccentricity method change → ONLY eccentricity changes. It's a leaf
    //     consumer; DerivedMaps does not depend on it, so its key must NOT move
    //     (else we'd needlessly recompute derived maps on an ecc-only edit).
    let e = stage_fps(|c| {
        c.eccentricity = Eccentricity::SnlcGetAreaBordersV1Center;
    });
    assert_eq!(e.labels, base.labels);
    assert_eq!(e.patch_refinement, base.patch_refinement);
    assert_ne!(e.eccentricity, base.eccentricity);
    assert_eq!(
        e.derived_maps, base.derived_maps,
        "DerivedMaps must NOT depend on Eccentricity"
    );
}

// ─── End-to-end disk restore (sentinel tamper) ───────────────────────────

#[test]
fn cache_restores_retinotopy_from_disk() {
    let input = manifest().join("tests/fixtures/oisi/R43_smoke.oisi");
    if !input.exists() {
        eprintln!(
            "[incremental] fixture missing, skipping: {}",
            input.display()
        );
        return;
    }
    let out = std::env::temp_dir().join(format!("oisi_incr_{}.oisi", std::process::id()));
    std::fs::copy(&input, &out).expect("copy fixture");

    // Migrate pre-2026 params, then build the param set (same as the harness).
    if isi_analysis::io::is_pre_2026_analysis_params(&out).unwrap() {
        let old = isi_analysis::io::read_analysis_params_attr(&out)
            .unwrap()
            .unwrap();
        let new = isi_analysis::migrate::translate_pre_2026_analysis_params(&old).unwrap();
        isi_analysis::io::write_analysis_params_attr(&out, &new).unwrap();
    }
    let params = match isi_analysis::io::read_analysis_params_attr(&out).unwrap() {
        Some(tree) => isi_analysis::bridge::analysis_params_from_oisi_tree(&tree).unwrap(),
        None => AnalysisParams::from(&AnalysisConfig::default()),
    };

    let progress = SilentProgress;
    let cancel = AtomicBool::new(false);

    // Run A — computes retinotopy, writes /results + the fingerprint.
    isi_analysis::analyze(&out, &params, None, &progress, &cancel).expect("run A");
    assert!(
        isi_analysis::io::read_stage_fingerprint(&out, "retinotopy")
            .unwrap()
            .is_some(),
        "run A must record the retinotopy fingerprint"
    );

    // Tamper the cached retinotopy map with a sentinel (fingerprint untouched).
    const SENTINEL: f64 = 7.0;
    let (h, w) = tamper_with_sentinel(&out, "results/azi_phase_degrees", SENTINEL);

    // Run B — identical params → retinotopy fingerprint matches → restore from
    // disk. The pipeline reads the tampered map, so the sentinel flows into the
    // result and back to /results. A recompute would have produced real values.
    isi_analysis::analyze(&out, &params, None, &progress, &cancel).expect("run B");

    let after = read_f64_2d(&out, "results/azi_phase_degrees");
    assert_eq!(after.dim(), (h, w));
    assert!(
        after.iter().all(|&v| (v - SENTINEL).abs() < 1e-9),
        "retinotopy was recomputed, not restored from cache — the sentinel was \
         overwritten with real values"
    );

    let _ = std::fs::remove_file(&out);
}

/// Overwrite a `/results` f64 dataset in place with a constant sentinel,
/// returning its `(h, w)` dims. Leaves the stored fingerprint untouched.
fn tamper_with_sentinel(path: &Path, dataset: &str, value: f64) -> (usize, usize) {
    let f = H5File::open_rw(path).expect("open rw");
    let ds = f.dataset(dataset).expect("dataset");
    let shape = ds.shape();
    let (h, w) = (shape[0], shape[1]);
    let sentinel = Array2::<f64>::from_elem((h, w), value);
    ds.write(&sentinel).expect("overwrite with sentinel");
    (h, w)
}

fn read_f64_2d(path: &Path, dataset: &str) -> Array2<f64> {
    let f = H5File::open(path).expect("open");
    f.dataset(dataset)
        .expect("dataset")
        .read_2d::<f64>()
        .expect("read 2d")
}

// ─── Per-stage never-stale cut (sentinel tamper across the cut boundary) ──────
//
// These prove the demand-driven cut both ways on a real recording: a
// downstream-only param change RESTORES every upstream stage from disk
// (including the expensive patch chain) and recomputes only the affected leaf;
// a mid-pipeline change RECOMPUTES the whole tail (never stale) while still
// restoring the stages above the change. The sentinel-tamper technique is the
// same as `cache_restores_retinotopy_from_disk`: a restored stage's `/results`
// value, overwritten with a constant after run A, survives run B iff that stage
// was read from disk rather than recomputed.

/// Copy the smoke fixture to a fresh temp path and load its (migrated) params.
/// Returns `None` when the fixture is absent (CI without the data) so the test
/// can skip cleanly.
fn setup_fixture(tag: &str) -> Option<(PathBuf, AnalysisParams)> {
    let input = manifest().join("tests/fixtures/oisi/R43_smoke.oisi");
    if !input.exists() {
        eprintln!("[incremental] fixture missing, skipping: {}", input.display());
        return None;
    }
    let out = std::env::temp_dir().join(format!("oisi_cut_{}_{}.oisi", tag, std::process::id()));
    std::fs::copy(&input, &out).expect("copy fixture");

    if isi_analysis::io::is_pre_2026_analysis_params(&out).unwrap() {
        let old = isi_analysis::io::read_analysis_params_attr(&out).unwrap().unwrap();
        let new = isi_analysis::migrate::translate_pre_2026_analysis_params(&old).unwrap();
        isi_analysis::io::write_analysis_params_attr(&out, &new).unwrap();
    }
    let params = match isi_analysis::io::read_analysis_params_attr(&out).unwrap() {
        Some(tree) => isi_analysis::bridge::analysis_params_from_oisi_tree(&tree).unwrap(),
        None => AnalysisParams::from(&AnalysisConfig::default()),
    };
    Some((out, params))
}

fn run_analyze(path: &Path, params: &AnalysisParams) {
    let cancel = AtomicBool::new(false);
    isi_analysis::analyze(path, params, None, &SilentProgress, &cancel).expect("analyze");
}

/// Overwrite an i32 `/results` dataset in place with a constant sentinel.
fn tamper_i32_with_sentinel(path: &Path, dataset: &str, value: i32) {
    let f = H5File::open_rw(path).expect("open rw");
    let ds = f.dataset(dataset).expect("dataset");
    let shape = ds.shape();
    let sentinel = Array2::<i32>::from_elem((shape[0], shape[1]), value);
    ds.write(&sentinel).expect("overwrite i32 sentinel");
}

fn read_i32_2d(path: &Path, dataset: &str) -> Array2<i32> {
    let f = H5File::open(path).expect("open");
    f.dataset(dataset)
        .expect("dataset")
        .read_2d::<i32>()
        .expect("read 2d i32")
}

fn all_eq_f64(a: &Array2<f64>, v: f64) -> bool {
    a.iter().all(|&x| (x - v).abs() < 1e-9)
}
fn all_eq_i32(a: &Array2<i32>, v: i32) -> bool {
    a.iter().all(|&x| x == v)
}

/// Changing ONLY a leaf param (the eccentricity method) must restore every
/// stage above it from disk — crucially the patch chain, whose refinement stage
/// is the pipeline's hotspot — and recompute just the eccentricity leaf.
/// `DerivedMaps` does not depend on eccentricity, so it too restores.
#[test]
fn cut_restores_patch_chain_when_only_eccentricity_changes() {
    let Some((out, base)) = setup_fixture("ecc") else {
        return;
    };

    // Run A — full compute, writes /results + /cache + tail fingerprints.
    run_analyze(&out, &base);
    assert!(
        isi_analysis::io::read_stage_fingerprint(&out, "labels")
            .unwrap()
            .is_some(),
        "run A must record per-stage tail fingerprints"
    );

    // Tamper a Labels output and a DerivedMaps output (both upstream-independent
    // of eccentricity → must be restored on run B) plus the eccentricity leaf
    // itself (must be overwritten by recompute).
    const S: f64 = 7.0;
    tamper_i32_with_sentinel(&out, "results/area_labels", 7);
    tamper_with_sentinel(&out, "results/magnification", S);
    tamper_with_sentinel(&out, "results/eccentricity", S);

    // Run B — identical params except the eccentricity method.
    let mut changed = base.clone();
    changed.eccentricity = EccentricityMethod::SnlcGetAreaBordersV1Center;
    run_analyze(&out, &changed);

    assert!(
        all_eq_i32(&read_i32_2d(&out, "results/area_labels"), 7),
        "Labels was recomputed, not restored — the patch chain (incl. refinement) \
         needlessly reran on an eccentricity-only change"
    );
    assert!(
        all_eq_f64(&read_f64_2d(&out, "results/magnification"), S),
        "DerivedMaps was recomputed, not restored, on an eccentricity-only change"
    );
    assert!(
        !all_eq_f64(&read_f64_2d(&out, "results/eccentricity"), S),
        "the eccentricity leaf was served stale — its method changed but the \
         cached value survived (cache must never serve stale)"
    );

    let _ = std::fs::remove_file(&out);
}

/// Changing a mid-pipeline param (the cortex source) must recompute the whole
/// downstream tail — never serving a stale cached value — while still restoring
/// the stages above the change (retinotopy, sign-map smoothing) from disk.
#[test]
fn cut_recomputes_tail_but_restores_upstream_on_cortex_change() {
    let Some((out, base)) = setup_fixture("cortex") else {
        return;
    };

    run_analyze(&out, &base);

    // Upstream of cortex source (must be restored): retinotopy + sign smoothing.
    // Downstream (must be recomputed, never stale): the area labels.
    const S: f64 = 7.0;
    tamper_with_sentinel(&out, "results/azi_phase_degrees", S);
    tamper_with_sentinel(&out, "results/vfs_smoothed", S);
    tamper_i32_with_sentinel(&out, "results/area_labels", 7);

    let mut changed = base.clone();
    changed.cortex_source = CortexSourceMethod::NoRestriction;
    run_analyze(&out, &changed);

    assert!(
        all_eq_f64(&read_f64_2d(&out, "results/azi_phase_degrees"), S),
        "Retinotopy (upstream of the cortex-source change) was recomputed, not restored"
    );
    assert!(
        all_eq_f64(&read_f64_2d(&out, "results/vfs_smoothed"), S),
        "SignSmoothing (upstream of the cortex-source change) was recomputed, not restored"
    );
    assert!(
        !all_eq_i32(&read_i32_2d(&out, "results/area_labels"), 7),
        "the segmentation tail was served stale — the cortex source changed but \
         the cached area labels survived (cache must never serve stale)"
    );

    let _ = std::fs::remove_file(&out);
}
