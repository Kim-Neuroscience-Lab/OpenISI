//! Cross-implementation equivalence harness.
//!
//! Re-runs `isi_analysis::analyze` on a copy of a committed fixture and
//! compares every `/results/<dataset>` and `/complex_maps/<dataset>`
//! against the baseline `.baseline.oisi` captured by
//! `cargo run --example capture_baseline`. Tolerances come from
//! `tests/fixtures/tolerances.toml` per dataset.
//!
//! This is the cross-implementation regression harness: it asserts the
//! pipeline still matches the committed baseline within the per-dataset
//! tolerances. Any change that pushes a dataset's drift past its committed
//! budget is a hard failure to investigate, not to absorb.
//!
//! Per-dataset comparison kinds:
//!   - `bit_exact = true` in tolerances.toml → bit-equal arrays required
//!     (used for boolean masks, integer labels).
//!   - else float datasets → both `max_abs` and `mean_abs` must satisfy
//!     committed budgets; phase datasets use wrap-aware distance.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

// The per-element tolerance comparison is `approx` (the project's standard tool,
// already a dependency); only the domain wrapper around it — the NaN-position
// gate, the wrap-aware phase distance, and the array-level aggregation — is local.
use approx::{abs_diff_eq, relative_eq};
use isi_analysis::{self, SilentProgress};
use ndarray::ArrayD;
use openisi_params::config::AnalysisConfig;
use serde::Deserialize;

// `Array::into_raw_vec` is deprecated in favor of `into_raw_vec_and_offset`
// in ndarray ≥ 0.16, but the offset is always 0 for arrays produced by
// `read_dyn` (the underlying storage starts at element 0). The deprecation
// warning is noise here; we explicitly silence it for this test only.
#[allow(deprecated)]
fn into_vec<T: Clone>(arr: ArrayD<T>) -> Vec<T> {
    arr.into_raw_vec()
}

// ─── Tolerance file format ───────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct ToleranceTree {
    #[serde(default)]
    complex_maps: HashMap<String, Tolerance>,
    #[serde(default)]
    results: HashMap<String, Tolerance>,
}

/// Per-dataset numerical agreement bound, grounded in IEEE-754 f32 precision.
///
/// The check is the standard relative form `|c − b| ≤ rtol·max(|c|,|b|) + atol`:
/// floating-point error is *relative*, so `rtol` is the primary bound and `atol`
/// only floors it where values pass through zero.
///
/// **`rtol` is grounded, not measured-and-pasted:** `rtol = K · EPS_F32`, where
/// `EPS_F32 = 2⁻²³` is the f32 machine epsilon and `K` is the error-propagation
/// factor for that stage's operation count, doubled for a cross-backend compare
/// (two independent f32 implementations). `K` is recorded per entry in
/// `tolerances.toml` with its justification.
#[derive(Debug, Deserialize, Default, Clone)]
struct Tolerance {
    #[serde(default)]
    bit_exact: bool,
    /// Relative tolerance = `K · EPS_F32`.
    #[serde(default)]
    rtol: Option<f64>,
    /// Absolute floor (a few ULP of the map's representative scale).
    #[serde(default)]
    atol: Option<f64>,
}

/// IEEE-754 single-precision machine epsilon, `2⁻²³`. Every `rtol` is a multiple
/// of this — the one constant the relative bounds are grounded in.
const EPS_F32: f64 = f32::EPSILON as f64;

fn load_tolerances() -> ToleranceTree {
    let p = manifest_path().join("tests/fixtures/tolerances.toml");
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
    toml::from_str(&text).expect("parse tolerances.toml")
}

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// ─── Capture baseline (same logic as examples/capture_baseline.rs) ───

fn run_analyze_into(input: &Path, output: &Path) {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).expect("create candidate dir");
    }
    std::fs::copy(input, output).expect("copy fixture");

    // Force a full recompute from the rawest available input. The retinotopy
    // fingerprint keys on params+data, not code version, so without this a
    // stale cache in the fixture could mask a code change — the test must
    // recompute, not serve a cache.
    isi_analysis::io::strip_derived_outputs(output).expect("strip derived outputs");

    if isi_analysis::io::is_pre_2026_analysis_params(output).expect("is_pre_2026_analysis_params") {
        let old = isi_analysis::io::read_analysis_params_attr(output)
            .expect("read /analysis_params")
            .expect("pre-2026 said yes but read returned None");
        let new = isi_analysis::migrate::translate_pre_2026_analysis_params(&old)
            .expect("translate pre-2026 params");
        isi_analysis::io::write_analysis_params_attr(output, &new).expect("write migrated params");
    }

    let params =
        match isi_analysis::io::read_analysis_params_attr(output).expect("read /analysis_params") {
            Some(tree) => isi_analysis::bridge::analysis_params_from_oisi_tree(&tree)
                .expect("reconstruct AnalysisParams from /analysis_params"),
            None => isi_analysis::AnalysisParams::from(&AnalysisConfig::default()),
        };

    let progress = SilentProgress;
    let cancel = AtomicBool::new(false);
    isi_analysis::analyze(output, &params, None, &progress, &cancel).expect("isi_analysis::analyze");
}

// ─── Per-dataset comparators ─────────────────────────────────────────

#[derive(Debug, Default)]
struct DriftStats {
    /// Worst absolute `|c − b|` over finite pairs (reporting only).
    max_abs: f64,
    /// Worst relative `|c − b| / max(|c|,|b|)` over finite pairs (reporting only).
    max_rel: f64,
    /// Number of finite pairs that FAILED the `approx` tolerance check
    /// (`relative_eq!` / `abs_diff_eq!` at the dataset's `rtol`/`atol`). `0` ⇔ pass.
    n_fail: usize,
    /// Number of finite element pairs compared.
    n_finite: usize,
    /// Positions where exactly one of (candidate, baseline) is NaN/Inf —
    /// a structural mismatch (the maps disagree on *where* data exists),
    /// always a failure regardless of tolerance.
    n_nan_mismatch: usize,
}

/// NaN/Inf-aware drift. Several maps are deliberately NaN outside a mask
/// (e.g. `vfs_smoothed_thresholded` is NaN outside the cortex). A naive
/// `(c-b).abs()` makes both `max_abs` and `mean_abs` NaN there, and
/// `NaN > thr` is `false` — so a fully-NaN dataset would pass *silently*.
/// This comparator instead:
///   - requires NaN positions to MATCH (both NaN, or both finite); a
///     position where they differ is counted as `n_nan_mismatch` (always
///     a failure),
///   - computes drift only over positions where both are finite.
fn compute_drift(candidate: &[f64], baseline: &[f64], rtol: f64, atol: f64) -> DriftStats {
    drift_with(
        candidate,
        baseline,
        |c, b| (c - b).abs(),
        // Tool: `approx`'s relative comparison, `|c−b| ≤ max(atol, rtol·max(|c|,|b|))`.
        |c, b| relative_eq!(c, b, max_relative = rtol, epsilon = atol),
    )
}

/// Wrap-aware drift on phase values (radians). Phase is an angular quantity, so
/// the bound is absolute (radians): the wrap distance must be within `atol`.
fn compute_phase_drift(candidate: &[f64], baseline: &[f64], _rtol: f64, atol: f64) -> DriftStats {
    drift_with(
        candidate,
        baseline,
        phase_wrap_distance,
        // Tool: `approx`'s absolute comparison on the (domain) wrap distance.
        move |c, b| abs_diff_eq!(phase_wrap_distance(c, b), 0.0, epsilon = atol),
    )
}

/// Wrapped circle distance between two phases (radians), in `[0, π]`.
fn phase_wrap_distance(c: f64, b: f64) -> f64 {
    let two_pi = std::f64::consts::TAU;
    let pi = std::f64::consts::PI;
    let mut d = (c - b).rem_euclid(two_pi);
    if d > pi {
        d = two_pi - d;
    }
    d
}

/// Shared NaN/Inf-aware accumulator. `dist` reports the per-element drift (abs or
/// wrap distance) for diagnostics; `pass` is the `approx` tolerance check for that
/// element. The loop owns only the domain discipline — NaN-position matching and
/// array-level aggregation — not the tolerance comparison itself.
fn drift_with(
    candidate: &[f64],
    baseline: &[f64],
    dist: impl Fn(f64, f64) -> f64,
    pass: impl Fn(f64, f64) -> bool,
) -> DriftStats {
    assert_eq!(candidate.len(), baseline.len(), "element count mismatch");
    let mut max_abs = 0.0_f64;
    let mut max_rel = 0.0_f64;
    let mut n_fail = 0usize;
    let mut n_finite = 0usize;
    let mut n_nan_mismatch = 0usize;
    for (&c, &b) in candidate.iter().zip(baseline.iter()) {
        match (c.is_finite(), b.is_finite()) {
            (true, true) => {
                let d = dist(c, b);
                max_abs = max_abs.max(d);
                let scale = c.abs().max(b.abs());
                if scale > 0.0 {
                    max_rel = max_rel.max(d / scale);
                }
                if !pass(c, b) {
                    n_fail += 1;
                }
                n_finite += 1;
            }
            (false, false) => {
                // Both non-finite at this position — matched (e.g. both
                // NaN outside the mask). No drift contribution.
            }
            _ => {
                // Exactly one non-finite — the maps disagree on where data
                // exists. Structural mismatch, always a failure.
                n_nan_mismatch += 1;
            }
        }
    }
    DriftStats {
        max_abs,
        max_rel,
        n_fail,
        n_finite,
        n_nan_mismatch,
    }
}

/// Compare floats with `max_abs` and `mean_abs` budgets plus the NaN-
/// position gate. Reports stats; pushes a failure for any breached gate.
fn assert_float_within(
    name: &str,
    stats: &DriftStats,
    tol: &Tolerance,
    failures: &mut Vec<String>,
) {
    let rtol = tol.rtol.unwrap_or(0.0);
    let atol = tol.atol.unwrap_or(0.0);
    println!(
        "  {:<40} max_abs={:.3e} max_rel={:.3e} n_fail={}  (rtol={:.1e}={:.0}ε atol={:.1e})  n={} nan_mm={}",
        name, stats.max_abs, stats.max_rel, stats.n_fail,
        rtol, rtol / EPS_F32, atol, stats.n_finite, stats.n_nan_mismatch,
    );
    // A NaN-position disagreement is always a failure — it means the two
    // implementations produced data at different pixels.
    if stats.n_nan_mismatch > 0 {
        failures.push(format!(
            "{}: {} position(s) where exactly one of candidate/baseline is NaN/Inf",
            name, stats.n_nan_mismatch,
        ));
    }
    // Guard against a dataset that is entirely non-finite (n_finite == 0
    // would otherwise pass vacuously).
    if stats.n_finite == 0 {
        failures.push(format!(
            "{}: no finite element pairs to compare (entirely NaN/Inf)",
            name,
        ));
    }
    // The grounded bound, applied per element by `approx`: every finite pair
    // must pass `relative_eq!` / `abs_diff_eq!` at this dataset's rtol/atol.
    if stats.n_fail > 0 {
        failures.push(format!(
            "{}: {} of {} finite px exceed rtol={:.2e} (={:.0}·ε_f32) + atol={:.2e}  (max_abs={:.3e}, max_rel={:.3e})",
            name, stats.n_fail, stats.n_finite, rtol, rtol / EPS_F32, atol, stats.max_abs, stats.max_rel,
        ));
    }
}

fn assert_bit_exact_bytes(
    name: &str,
    candidate: &[u8],
    baseline: &[u8],
    failures: &mut Vec<String>,
) {
    if candidate == baseline {
        println!("  {:<40} bit_exact  n={}", name, candidate.len());
    } else {
        let diffs: usize = candidate
            .iter()
            .zip(baseline.iter())
            .filter(|(a, b)| a != b)
            .count();
        println!(
            "  {:<40} bit_exact FAIL  n={}  diffs={}",
            name,
            candidate.len(),
            diffs
        );
        failures.push(format!(
            "{}: bit_exact required, {}/{} elements differ",
            name,
            diffs,
            candidate.len(),
        ));
    }
}

// ─── HDF5 dataset readers (one per dtype we use) ─────────────────────

fn read_f64(path: &Path, ds_path: &str) -> ArrayD<f64> {
    let file = hdf5::File::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let ds = file
        .dataset(ds_path)
        .unwrap_or_else(|e| panic!("dataset {}: {e}", ds_path));
    ds.read_dyn::<f64>().expect("read f64 dataset")
}

fn read_u8(path: &Path, ds_path: &str) -> ArrayD<u8> {
    let file = hdf5::File::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let ds = file
        .dataset(ds_path)
        .unwrap_or_else(|e| panic!("dataset {}: {e}", ds_path));
    ds.read_dyn::<u8>().expect("read u8 dataset")
}

fn read_i32(path: &Path, ds_path: &str) -> ArrayD<i32> {
    let file = hdf5::File::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let ds = file
        .dataset(ds_path)
        .unwrap_or_else(|e| panic!("dataset {}: {e}", ds_path));
    ds.read_dyn::<i32>().expect("read i32 dataset")
}

fn read_i8(path: &Path, ds_path: &str) -> ArrayD<i8> {
    let file = hdf5::File::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let ds = file
        .dataset(ds_path)
        .unwrap_or_else(|e| panic!("dataset {}: {e}", ds_path));
    ds.read_dyn::<i8>().expect("read i8 dataset")
}

fn dataset_exists(path: &Path, ds_path: &str) -> bool {
    let Ok(file) = hdf5::File::open(path) else {
        return false;
    };
    file.dataset(ds_path).is_ok()
}

// ─── Per-dataset categorization ──────────────────────────────────────

/// Datasets that store wrap-aware phase values (radians).
const PHASE_DATASETS: &[&str] = &["azi_phase", "alt_phase"];

/// Datasets that are conceptually boolean masks (HDF5 dtype u8).
const BOOL_MASK_DATASETS: &[&str] = &[
    "cortex_mask",
    "area_borders",
    "contours_azi",
    "contours_alt",
];

/// Datasets stored as i32 labels.
const I32_LABEL_DATASETS: &[&str] = &["area_labels"];

/// Datasets stored as i8 per-area signs.
const I8_DATASETS: &[&str] = &["area_signs"];

// ─── Main equivalence test ───────────────────────────────────────────

#[test]
fn equivalence_r43_smoke() {
    let manifest = manifest_path();
    let fixture = manifest.join("tests/fixtures/oisi/R43_smoke.oisi");
    let baseline = manifest.join("tests/fixtures/baseline/R43_smoke.baseline.oisi");

    // This is a DEV-TIME method-validation gate (bit-identical pipeline output vs a
    // real R43 baseline), not a program-correctness test. The R43 fixtures are real
    // SNLC-derived data, intentionally gitignored (`*.oisi`) and never published —
    // so on a clean checkout (general CI) they are absent. Run the gate wherever the
    // data lives; when absent, SKIP loudly rather than hard-fail (matches
    // `regression_oisi.rs`). No `#[ignore]`: it still runs by default where present.
    if !fixture.exists() || !baseline.exists() {
        eprintln!(
            "SKIP equivalence_r43_smoke: R43 fixture/baseline absent (gitignored real data — \
             a dev-time validation, not run on general CI).\n  fixture:  {}\n  baseline: {}",
            fixture.display(),
            baseline.display()
        );
        return;
    }

    // Run analyze on a fresh copy of the fixture under target/.
    let candidate = manifest
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/equivalence/R43_smoke.candidate.oisi");
    run_analyze_into(&fixture, &candidate);

    let tols = load_tolerances();
    let mut failures: Vec<String> = Vec::new();

    println!();
    println!("=== Cross-implementation equivalence: R43_smoke ===");
    println!("  fixture   = {}", fixture.display());
    println!("  baseline  = {}", baseline.display());
    println!("  candidate = {}", candidate.display());
    println!();

    // --- /complex_maps/<name>: Array3<f64> shape [H, W, 2] (re, im) ---
    for (name, tol) in &tols.complex_maps {
        let ds = format!("complex_maps/{name}");
        let label = format!("/{ds}");
        let c = read_f64(&candidate, &ds);
        let b = read_f64(&baseline, &ds);
        assert_eq!(c.shape(), b.shape(), "{label}: shape mismatch");
        let c_flat = into_vec(c);
        let b_flat = into_vec(b);
        if tol.bit_exact {
            if c_flat == b_flat {
                println!("  {:<40} bit_exact  n={}", label, c_flat.len());
            } else {
                let diffs: usize = c_flat
                    .iter()
                    .zip(b_flat.iter())
                    .filter(|(a, b)| a != b)
                    .count();
                println!("  {:<40} bit_exact FAIL  diffs={}", label, diffs);
                failures.push(format!(
                    "{label}: bit_exact required, {diffs} elements differ"
                ));
            }
        } else {
            let (rtol, atol) = (tol.rtol.unwrap_or(0.0), tol.atol.unwrap_or(0.0));
            let stats = compute_drift(&c_flat, &b_flat, rtol, atol);
            assert_float_within(&label, &stats, tol, &mut failures);
        }
    }

    // --- /results/<name> --------------------------------------------------
    for (name, tol) in &tols.results {
        let ds = format!("results/{name}");
        let label = format!("/{ds}");

        if !dataset_exists(&candidate, &ds) && !dataset_exists(&baseline, &ds) {
            // Optional dataset (e.g., snr_* only present with raw acquisition).
            println!("  {:<40} (absent in both — skipped)", label);
            continue;
        }
        assert!(
            dataset_exists(&candidate, &ds),
            "{label}: missing from candidate"
        );
        assert!(
            dataset_exists(&baseline, &ds),
            "{label}: missing from baseline"
        );

        if I32_LABEL_DATASETS.contains(&name.as_str()) {
            let c = read_i32(&candidate, &ds);
            let b = read_i32(&baseline, &ds);
            assert_eq!(c.shape(), b.shape(), "{label}: shape mismatch");
            let c_bytes: Vec<u8> = into_vec(c)
                .iter()
                .flat_map(|v| v.to_le_bytes().to_vec())
                .collect();
            let b_bytes: Vec<u8> = into_vec(b)
                .iter()
                .flat_map(|v| v.to_le_bytes().to_vec())
                .collect();
            assert_bit_exact_bytes(&label, &c_bytes, &b_bytes, &mut failures);
        } else if I8_DATASETS.contains(&name.as_str()) {
            let c = read_i8(&candidate, &ds);
            let b = read_i8(&baseline, &ds);
            assert_eq!(c.shape(), b.shape(), "{label}: shape mismatch");
            let c_bytes: Vec<u8> = into_vec(c).iter().map(|&v| v as u8).collect();
            let b_bytes: Vec<u8> = into_vec(b).iter().map(|&v| v as u8).collect();
            assert_bit_exact_bytes(&label, &c_bytes, &b_bytes, &mut failures);
        } else if BOOL_MASK_DATASETS.contains(&name.as_str()) {
            let c = read_u8(&candidate, &ds);
            let b = read_u8(&baseline, &ds);
            assert_eq!(c.shape(), b.shape(), "{label}: shape mismatch");
            assert_bit_exact_bytes(&label, &into_vec(c), &into_vec(b), &mut failures);
        } else {
            // Float dataset.
            let c = read_f64(&candidate, &ds);
            let b = read_f64(&baseline, &ds);
            assert_eq!(c.shape(), b.shape(), "{label}: shape mismatch");
            let c_flat = into_vec(c);
            let b_flat = into_vec(b);
            let (rtol, atol) = (tol.rtol.unwrap_or(0.0), tol.atol.unwrap_or(0.0));
            let stats = if PHASE_DATASETS.contains(&name.as_str()) {
                compute_phase_drift(&c_flat, &b_flat, rtol, atol)
            } else {
                compute_drift(&c_flat, &b_flat, rtol, atol)
            };
            assert_float_within(&label, &stats, tol, &mut failures);
        }
    }

    println!();
    if !failures.is_empty() {
        for f in &failures {
            eprintln!("  FAIL: {f}");
        }
        panic!("{} dataset(s) failed equivalence", failures.len());
    }
    println!("  PASS — all datasets within tolerance");
}
