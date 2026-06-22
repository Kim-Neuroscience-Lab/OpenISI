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

// The numerical-agreement comparison goes through the project's single ε-grounded
// comparator, the `agreement` crate (the same `Tol`/`Drift` the in-crate goldens
// use) — NOT a local re-implementation. This harness used to carry its own copy of
// the drift loop, the wrap distance, and the rtol/atol grounding; that duplication
// is gone. `agreement` owns the NaN-position gate, the wrap-aware distance, the
// aggregation, and the ε-grounding (a raw float bound is not expressible).
use agreement::{Drift, Eps, Tol};
use hdf5::types::{FloatSize, IntSize, TypeDescriptor};
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

/// Per-dataset agreement bound, parsed from `tolerances.toml` and mapped to a
/// grounded [`agreement::Tol`]. The file stores an integer factor `k` (+ a floor
/// `k_floor` for relative bounds) and a `kind`; the bound is `k · ε_f32`, built via
/// `Tol` — so the IEEE-754 grounding is **type-enforced**, not a comment that can
/// drift from a pasted float. (Cross-backend f32 implementations are compared, so
/// the ε is always `Eps::F32` here; the toml `k` carries the per-stage propagation
/// factor.)
#[derive(Debug, Deserialize, Default, Clone)]
struct Tolerance {
    /// `exact` | `abs` | `rel` | `wrap` — selects the `Tol` constructor.
    kind: String,
    /// Factor for the primary bound (`k·ε`): rtol for `rel`, atol for `abs`/`wrap`.
    #[serde(default)]
    k: u32,
    /// Floor factor for `rel` (`atol = k_floor·ε`); ignored by other kinds.
    #[serde(default)]
    k_floor: u32,
}

impl Tolerance {
    /// Map this spec to the grounded comparator. Phase wrap uses period `2π`
    /// (radians) with scale 1 (the toml `k` already in ε units).
    fn to_tol(&self) -> Tol {
        match self.kind.as_str() {
            "exact" => Tol::exact(),
            "abs" => Tol::abs(self.k, Eps::F32),
            "rel" => Tol::rel(self.k, Eps::F32, self.k_floor),
            "wrap" => Tol::wrap(std::f64::consts::TAU, self.k, Eps::F32, 1.0),
            other => panic!(
                "tolerances.toml: unknown kind {other:?} (expected exact|abs|rel|wrap)"
            ),
        }
    }
}

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

// ─── Per-dataset comparison (through `agreement::Tol`) ───────────────

/// Compare one dataset's flat values against the baseline through the grounded
/// comparator, print the per-dataset drift line, and push any failure. ALL kinds
/// (discrete `exact`, `abs`, `rel`, wrap-aware phase) go through the same path —
/// `agreement` owns the NaN-position gate, the wrap distance, and the bound.
fn compare_dataset(
    label: &str,
    candidate: &[f64],
    baseline: &[f64],
    tol: &Tolerance,
    failures: &mut Vec<String>,
) {
    let drift: Drift = tol.to_tol().check(candidate, baseline);
    println!(
        "  {:<40} {:<5} max_abs={:.3e} max_rel={:.3e} n_fail={}  (k={}{}) n={} nan_mm={}",
        label,
        tol.kind,
        drift.max_abs,
        drift.max_rel,
        drift.n_fail,
        tol.k,
        if tol.kind == "rel" {
            format!(" k_floor={}", tol.k_floor)
        } else {
            String::new()
        },
        drift.n_finite,
        drift.n_nan_mismatch,
    );
    // A NON-empty dataset with no finite pairs (all NaN/Inf on both sides) passes
    // is_agreement() vacuously — reject it. An empty dataset (e.g. area_signs when
    // no areas were segmented) is a valid agreement: equal shape, nothing to
    // compare. (The shape check above already verified both sides are empty.)
    if !candidate.is_empty() && drift.is_vacuous() {
        failures.push(format!(
            "{label}: vacuous — {} elements but none finite on both sides (all NaN/Inf)",
            candidate.len(),
        ));
        return;
    }
    if !drift.is_agreement() {
        failures.push(format!(
            "{label}: {} of {} finite px exceed the k={}·ε_f32 ({}) bound + {} NaN-position \
             mismatch(es)  (max_abs={:.3e}, max_rel={:.3e})",
            drift.n_fail,
            drift.n_finite,
            tol.k,
            tol.kind,
            drift.n_nan_mismatch,
            drift.max_abs,
            drift.max_rel,
        ));
    }
}

// ─── HDF5 dataset reader (dtype read from the file, widened to f64) ──
//
// One reader for every dataset: the `.oisi` is self-describing (HDF5 carries each
// dataset's dtype), so the storage type is read FROM THE FILE and widened to f64
// rather than tracked in a hand-maintained name→dtype table. Discrete datasets
// (u8 masks, i32 labels, i8 signs) widen exactly (every value is representable in
// f64), and the `exact` tolerance kind compares them bit-for-bit after widening —
// so masks/labels go through the same `agreement::Tol` path as the float maps.

/// Read a dataset's values as a flat `Vec<f64>` plus its shape, decoding whatever
/// numeric dtype it is stored as (the `.oisi` schema's choice, read from the file).
fn read_as_f64(path: &Path, ds_path: &str) -> (Vec<f64>, Vec<usize>) {
    let file = hdf5::File::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let ds = file
        .dataset(ds_path)
        .unwrap_or_else(|e| panic!("dataset {ds_path}: {e}"));
    let shape = ds.shape();
    let descr = ds
        .dtype()
        .and_then(|d| d.to_descriptor())
        .unwrap_or_else(|e| panic!("dtype of {ds_path}: {e}"));
    let flat: Vec<f64> = match descr {
        TypeDescriptor::Float(FloatSize::U8) => into_vec(ds.read_dyn::<f64>().expect("read f64")),
        TypeDescriptor::Float(FloatSize::U4) => into_vec(ds.read_dyn::<f32>().expect("read f32"))
            .into_iter()
            .map(f64::from)
            .collect(),
        TypeDescriptor::Integer(IntSize::U4) => into_vec(ds.read_dyn::<i32>().expect("read i32"))
            .into_iter()
            .map(f64::from)
            .collect(),
        TypeDescriptor::Integer(IntSize::U1) => into_vec(ds.read_dyn::<i8>().expect("read i8"))
            .into_iter()
            .map(f64::from)
            .collect(),
        TypeDescriptor::Unsigned(IntSize::U1) => into_vec(ds.read_dyn::<u8>().expect("read u8"))
            .into_iter()
            .map(f64::from)
            .collect(),
        other => panic!("{ds_path}: unsupported stored dtype {other:?} for equivalence compare"),
    };
    (flat, shape)
}

fn dataset_exists(path: &Path, ds_path: &str) -> bool {
    let Ok(file) = hdf5::File::open(path) else {
        return false;
    };
    file.dataset(ds_path).is_ok()
}

// ─── Main equivalence test ───────────────────────────────────────────

/// **Always-on, CI-runnable equivalence gate** on a committed SYNTHETIC fixture.
/// The synthetic `.oisi` is tiny, deterministic, and verified to recover its known
/// ground truth (see `examples/gen_synthetic_smoke.rs`), so this regression pin runs
/// everywhere — including a clean CI checkout — unlike the gitignored real R43 data.
#[test]
fn equivalence_synthetic_smoke() {
    let manifest = manifest_path();
    let fixture = manifest.join("tests/fixtures/synthetic/smoke.oisi");
    let baseline = manifest.join("tests/fixtures/synthetic/smoke.baseline.oisi");
    assert!(
        fixture.exists() && baseline.exists(),
        "committed synthetic smoke fixtures missing — regenerate with \
         `cargo run -p isi-analysis --example gen_synthetic_smoke` + `capture_baseline`:\n  {}\n  {}",
        fixture.display(),
        baseline.display()
    );
    run_equivalence("synthetic_smoke", &fixture, &baseline);
}

/// Same gate on the REAL R43 data — a dev-time validation, not a program test. The
/// R43 fixtures are real SNLC-derived data, intentionally gitignored (`*.oisi`) and
/// never published, so on a clean checkout / general CI they are absent → SKIP
/// loudly (matches `regression_oisi.rs`). No `#[ignore]`: runs by default where the
/// data lives. Real data adds real-world value ranges the synthetic gate can't.
#[test]
fn equivalence_r43_smoke() {
    let manifest = manifest_path();
    let fixture = manifest.join("tests/fixtures/oisi/R43_smoke.oisi");
    let baseline = manifest.join("tests/fixtures/baseline/R43_smoke.baseline.oisi");
    if !fixture.exists() || !baseline.exists() {
        eprintln!(
            "SKIP equivalence_r43_smoke: R43 fixture/baseline absent (gitignored real data — \
             a dev-time validation; the synthetic gate covers CI).\n  fixture:  {}\n  baseline: {}",
            fixture.display(),
            baseline.display()
        );
        return;
    }
    run_equivalence("R43_smoke", &fixture, &baseline);
}

/// Re-run `analyze` on a fresh copy of `fixture` and compare every
/// `/complex_maps/*` and `/results/*` against `baseline` within the committed
/// per-dataset tolerances. Shared by the synthetic (always-on) and R43 (local) gates.
fn run_equivalence(tag: &str, fixture: &Path, baseline: &Path) {
    let manifest = manifest_path();
    // Run analyze on a fresh copy of the fixture under target/.
    let candidate = manifest
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(format!("target/equivalence/{tag}.candidate.oisi"));
    run_analyze_into(fixture, &candidate);

    let tols = load_tolerances();
    let mut failures: Vec<String> = Vec::new();

    println!();
    println!("=== Cross-implementation equivalence: {tag} ===");
    println!("  fixture   = {}", fixture.display());
    println!("  baseline  = {}", baseline.display());
    println!("  candidate = {}", candidate.display());
    println!();

    // --- /complex_maps/<name>: Array3<f64> shape [H, W, 2] (re, im) ---
    for (name, tol) in &tols.complex_maps {
        let ds = format!("complex_maps/{name}");
        let label = format!("/{ds}");
        let (c_flat, c_shape) = read_as_f64(&candidate, &ds);
        let (b_flat, b_shape) = read_as_f64(baseline, &ds);
        assert_eq!(c_shape, b_shape, "{label}: shape mismatch");
        compare_dataset(&label, &c_flat, &b_flat, tol, &mut failures);
    }

    // --- /results/<name> --------------------------------------------------
    // Every dataset — discrete masks/labels/signs (kind = "exact") and float maps
    // alike — reads its stored dtype from the file, widens to f64, and compares
    // through the one grounded comparator. The shape is read from the file and
    // checked too, so a transpose that preserves element count cannot pass.
    for (name, tol) in &tols.results {
        let ds = format!("results/{name}");
        let label = format!("/{ds}");

        if !dataset_exists(&candidate, &ds) && !dataset_exists(baseline, &ds) {
            // Optional dataset (e.g., snr_* only present with raw acquisition).
            println!("  {:<40} (absent in both — skipped)", label);
            continue;
        }
        assert!(
            dataset_exists(&candidate, &ds),
            "{label}: missing from candidate"
        );
        assert!(
            dataset_exists(baseline, &ds),
            "{label}: missing from baseline"
        );

        let (c_flat, c_shape) = read_as_f64(&candidate, &ds);
        let (b_flat, b_shape) = read_as_f64(baseline, &ds);
        assert_eq!(c_shape, b_shape, "{label}: shape mismatch");
        compare_dataset(&label, &c_flat, &b_flat, tol, &mut failures);
    }

    println!();
    if !failures.is_empty() {
        for f in &failures {
            eprintln!("  FAIL: {f}");
            // Also emit as a GitHub Actions annotation (stdout `::error::` is parsed
            // by any shell) so the exact per-dataset cross-device drift is visible
            // on CI without log access — the data needed to set principled,
            // device-independent tolerances.
            println!("::error title=equivalence {tag}::{f}");
        }
        panic!("{} dataset(s) failed equivalence ({tag})", failures.len());
    }
    println!("  PASS — all datasets within tolerance");
}
