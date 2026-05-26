//! Device-stability regression test against a real `.oisi` file with
//! embedded `/results/*` ground truth.
//!
//! **What this test actually validates** (revised 2026-05-23 after an
//! honest re-audit):
//!
//! The regression file's `/results/*` was most recently re-written by
//! the current f32 pipeline on whatever device the writer used —
//! there is NO legacy f64 ground truth preserved in the file. So:
//!
//! - When the test runs on the device that last wrote the file
//!   (MPS as of 2026-05-23), it compares MPS-output against MPS-truth.
//!   Drift is **0 by construction** — this run validates only that the
//!   pipeline is deterministic on a single device. The PASS is real
//!   but tautological.
//!
//! - When the test runs on a *different* device (e.g. CPU after MPS
//!   wrote the truth), it compares CPU-output against MPS-truth.
//!   Drift reflects **cross-device f32 ordering differences** — this
//!   is the real test signal. Tolerances are calibrated to this run.
//!
//! Therefore the test is properly understood as **Claim 2 only
//! (cross-device unification)**. The originally-aspirational "Claim 1
//! (vs legacy f64 ground truth)" requires a separate canonical
//! pre-refactor file that the test would gate against. That file does
//! not exist yet; adding it is a future improvement.
//!
//! To get real signal: run on both MPS and CPU and verify both pass.
//! The CPU run does the work; the MPS run confirms no nondeterminism
//! crept in. Cross-device tolerances are documented in the constants
//! below + measured 2026-05-23.
//!
//! `#[ignore]` by default — the regression dataset is 1.9 GB and not in CI.
//!
//! Run via:
//!
//! ```text
//! cargo test --test regression_oisi -- --ignored --nocapture
//! OPENISI_ANALYSIS_DEVICE=cpu cargo test --test regression_oisi -- --ignored --nocapture
//! OPENISI_ANALYSIS_DEVICE=mps cargo test --test regression_oisi -- --ignored --nocapture
//! ```
//!
//! The file path defaults to `/Users/Adam/openisi/data/5_14_2026_test5_1778801597.oisi`
//! and can be overridden via `OPENISI_REGRESSION_FILE`.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use isi_analysis::{self, SilentProgress};
use ndarray::Array2;

// =============================================================================
// Tolerances (Claim 1 + 2 — device vs. ground-truth, cross-device)
//
// Measured 2026-05-23 by running the test on CPU against `/results`
// last written by MPS on the canonical regression file. Drift split is:
//
// - SNR: uniformly tight (max_rel ≈ 7.6e-5). Single tolerance works.
// - Phase / amplitude / VFS: bulk drift is f32-quantization-tight
//   (mean_abs ≈ 1e-4), but a handful of degenerate low-amplitude pixels
//   produce catastrophic per-pixel errors (full phase wrap, sign flip).
//   Gating on `max_abs` would fail on those outliers while the bulk of
//   the map agrees. We therefore gate on **mean_abs / mean_rel** — the
//   metric that reflects scientific correctness — and report max for
//   diagnostics. Tolerances carry a ~4× safety margin over measured.
//
// All values: 4× the measured mean drift on a single CPU↔MPS run,
// rounded up. Tighten further once cross-device data accumulates and
// the noise envelope is better characterised.
// =============================================================================

/// Mean wrapped phase error (radians). Measured: ~3e-4. Tolerance 1e-3.
const PHASE_MEAN_ABS_TOL: f64 = 1e-3;
/// Mean absolute amplitude error normalized to truth scale.
/// Measured: ~3e-3 mean_abs on amplitudes ranging up to ~3e1 → ~1e-4 mean_rel.
/// Tolerance 1e-3 (1× safety margin on mean_rel; max_rel is dominated by
/// near-zero outliers and not gated here).
const AMP_MEAN_REL_TOL: f64 = 1e-3;
/// Mean absolute VFS error. VFS is bounded to [-1, 1]. Measured: ~5e-4.
const VFS_MEAN_ABS_TOL: f64 = 2e-3;
/// Max relative SNR error — SNR behaves much better than the others
/// because it's a magnitude ratio, no phase ambiguity. Measured: ~7.6e-5.
const SNR_MAX_REL_TOL: f64 = 5e-4;

// =============================================================================

#[derive(Debug, Default, Clone, Copy)]
struct Stats {
    max_abs_err: f64,
    mean_abs_err: f64,
    max_rel_err: f64,
}

fn regression_file() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("OPENISI_REGRESSION_FILE") {
        let path = PathBuf::from(p);
        return path.exists().then_some(path);
    }
    let default = PathBuf::from("/Users/Adam/openisi/data/5_14_2026_test5_1778801597.oisi");
    default.exists().then_some(default)
}

/// Compare two 2D arrays pointwise. `mode_abs` returns absolute-error stats;
/// otherwise returns relative-error stats (normalized by max |truth|).
fn compare(actual: &Array2<f64>, truth: &Array2<f64>) -> Stats {
    assert_eq!(actual.dim(), truth.dim(), "shape mismatch");
    let truth_scale = truth
        .iter()
        .map(|&v| v.abs())
        .fold(0.0_f64, f64::max)
        .max(1e-12);
    let mut max_abs = 0.0_f64;
    let mut sum_abs = 0.0_f64;
    let mut max_rel = 0.0_f64;
    let mut count = 0usize;
    for (a, t) in actual.iter().zip(truth.iter()) {
        let abs_err = (a - t).abs();
        let denom = t.abs().max(1e-12);
        let rel_err = abs_err / denom;
        max_abs = max_abs.max(abs_err);
        sum_abs += abs_err;
        max_rel = max_rel.max(rel_err);
        count += 1;
    }
    let _ = truth_scale;
    Stats {
        max_abs_err: max_abs,
        mean_abs_err: sum_abs / count.max(1) as f64,
        max_rel_err: max_rel,
    }
}

/// Compare phase maps with 2π wraparound — actual phase wrap-aware distance.
fn compare_phase(actual: &Array2<f64>, truth: &Array2<f64>) -> Stats {
    assert_eq!(actual.dim(), truth.dim(), "phase shape mismatch");
    let two_pi = std::f64::consts::TAU;
    let mut max_abs = 0.0_f64;
    let mut sum_abs = 0.0_f64;
    let mut count = 0usize;
    for (a, t) in actual.iter().zip(truth.iter()) {
        let mut diff = (a - t).rem_euclid(two_pi);
        if diff > std::f64::consts::PI {
            diff = two_pi - diff;
        }
        max_abs = max_abs.max(diff);
        sum_abs += diff;
        count += 1;
    }
    Stats {
        max_abs_err: max_abs,
        mean_abs_err: sum_abs / count.max(1) as f64,
        max_rel_err: f64::NAN,
    }
}

/// Mean of `|actual - truth| / mean(|truth|)` — a relative error
/// metric robust to near-zero pixels (which would blow up plain
/// per-pixel `|a-t|/|t|`). Used for amplitude gates where the
/// per-pixel max_rel is dominated by physically-meaningless pixels.
fn mean_relative_error(actual: &Array2<f64>, truth: &Array2<f64>) -> f64 {
    let mean_truth_abs = truth.iter().map(|v| v.abs()).sum::<f64>()
        / (truth.len().max(1) as f64);
    let denom = mean_truth_abs.max(1e-12);
    let mean_abs = actual.iter().zip(truth.iter())
        .map(|(a, t)| (a - t).abs())
        .sum::<f64>() / (truth.len().max(1) as f64);
    mean_abs / denom
}

fn report(label: &str, stats: &Stats) {
    println!(
        "  {:<22} max_abs={:.6e}  mean_abs={:.6e}  max_rel={:.6e}",
        label, stats.max_abs_err, stats.mean_abs_err, stats.max_rel_err,
    );
}

fn read_map(path: &Path, name: &str) -> Option<Array2<f64>> {
    isi_analysis::io::read_result_map(path, name).ok()
}

/// Device-stability regression test. See module-level docstring for
/// what this actually validates (Claim 2 / cross-device unification;
/// Claim 1 vs legacy f64 truth is aspirational pending a canonical
/// pre-refactor ground-truth file).
///
/// On the device that last wrote the file's `/results/*` (MPS as of
/// 2026-05-23), drift is 0 by construction — the run only validates
/// determinism. On a different device, drift reflects real
/// cross-device f32 ordering differences; tolerances are calibrated
/// to a 2026-05-23 CPU↔MPS measurement.
///
/// This test exercises the `compute_complex_maps_from_raw +
/// compute_retinotopy` path (read-only — does *not* call `analyze()`,
/// which would overwrite the file's `/results/*` reference).
#[test]
#[ignore]
fn device_stability_vs_embedded_results() {
    let path = match regression_file() {
        Some(p) => p,
        None => {
            eprintln!("regression file not found (set OPENISI_REGRESSION_FILE or place at default path); skipping.");
            return;
        }
    };

    println!();
    println!("=== Device-stability test (current pipeline vs file's embedded /results) ===");
    println!("  device = {}", isi_analysis::compute::backend_info());
    println!("  file   = {}", path.display());
    println!("  note   = on the device that last wrote the file, drift is 0 by");
    println!("           construction; on a different device, drift reflects real");
    println!("           cross-device f32 ordering differences. See module docstring.");

    // Build a RegistrySnapshot from the file's `/analysis_params`
    // registry tree if present; otherwise from `PARAM_DEFS` defaults.
    // Then bridge to `AnalysisParams`. Single SSoT path — no separate
    // serde-derived `AnalysisParams` route, no method-enum `Default`.
    let snapshot = match isi_analysis::io::read_analysis_params_attr(&path)
        .expect("read_analysis_params_attr failed")
    {
        Some(tree) => openisi_params::RegistrySnapshot::from_json_tree(
            openisi_params::PersistTarget::Analysis,
            &tree,
        ).expect("from_json_tree failed"),
        None => openisi_params::Registry::new(std::path::Path::new("/tmp/regression"))
            .snapshot(),
    };
    let params = isi_analysis::bridge::analysis_params_from_snapshot(&snapshot);
    let rig = isi_analysis::io::read_rig_params(&path).ok().flatten();
    let exp = isi_analysis::io::read_experiment_params(&path).ok().flatten();
    let acquisition = isi_analysis::AcquisitionProperties::from_oisi_attrs(
        rig.as_ref(),
        exp.as_ref(),
    );

    let progress = SilentProgress;
    let cancel = AtomicBool::new(false);

    // Run the raw → complex → retinotopy path. We compare against the
    // file's embedded `/results/*` reference. As of 2026-05-23 those
    // results were written by the current MPS pipeline, so this is a
    // device-stability test: same device → 0 drift; different device
    // → measurable cross-device f32 ordering drift. Scope is intentionally
    // narrow — no cortex/segmentation
    // comparison, since those stages are method-dispatched and may have
    // intentionally evolved.
    let raw = isi_analysis::io::compute_complex_maps_from_raw(&path, &params, &progress, &cancel)
        .expect("compute_complex_maps_from_raw failed");
    let retino = isi_analysis::compute_retinotopy(&raw.complex_maps, &acquisition, &params)
        .expect("compute_retinotopy failed");

    let mut failed = Vec::<String>::new();

    // Phase maps — wrap-aware comparison. Gated on mean_abs (bulk
    // correctness); max_abs reported as a diagnostic — degenerate
    // near-zero-amplitude pixels can wrap by ~π without indicating real
    // pipeline drift.
    if let Some(truth) = read_map(&path, "azi_phase") {
        let s = compare_phase(&retino.azi_phase, &truth);
        report("azi_phase (wrap)", &s);
        if s.mean_abs_err > PHASE_MEAN_ABS_TOL {
            failed.push(format!("azi_phase: mean_abs={:.6e} > {:.6e}", s.mean_abs_err, PHASE_MEAN_ABS_TOL));
        }
    }
    if let Some(truth) = read_map(&path, "alt_phase") {
        let s = compare_phase(&retino.alt_phase, &truth);
        report("alt_phase (wrap)", &s);
        if s.mean_abs_err > PHASE_MEAN_ABS_TOL {
            failed.push(format!("alt_phase: mean_abs={:.6e} > {:.6e}", s.mean_abs_err, PHASE_MEAN_ABS_TOL));
        }
    }

    // Amplitudes — gated on mean_rel (mean_abs / mean truth scale).
    // Pure max_rel is dominated by near-zero pixels and not informative.
    if let Some(truth) = read_map(&path, "azi_amplitude") {
        let s = compare(&retino.azi_amplitude, &truth);
        report("azi_amplitude", &s);
        let mean_rel = mean_relative_error(&retino.azi_amplitude, &truth);
        if mean_rel > AMP_MEAN_REL_TOL {
            failed.push(format!("azi_amplitude: mean_rel={:.6e} > {:.6e}", mean_rel, AMP_MEAN_REL_TOL));
        }
    }
    if let Some(truth) = read_map(&path, "alt_amplitude") {
        let s = compare(&retino.alt_amplitude, &truth);
        report("alt_amplitude", &s);
        let mean_rel = mean_relative_error(&retino.alt_amplitude, &truth);
        if mean_rel > AMP_MEAN_REL_TOL {
            failed.push(format!("alt_amplitude: mean_rel={:.6e} > {:.6e}", mean_rel, AMP_MEAN_REL_TOL));
        }
    }

    // VFS — gated on mean_abs (VFS is bounded to [-1, 1]; per-pixel
    // sign flips at gradient zeros are the same outlier pattern as
    // phase wraps).
    if let Some(truth) = read_map(&path, "vfs") {
        let s = compare(&retino.vfs, &truth);
        report("vfs", &s);
        if s.mean_abs_err > VFS_MEAN_ABS_TOL {
            failed.push(format!("vfs: mean_abs={:.6e} > {:.6e}", s.mean_abs_err, VFS_MEAN_ABS_TOL));
        }
    }

    // SNR — uniformly tight, gate on max_rel.
    if let Some(ref snr) = raw.snr {
        if let Some(truth) = read_map(&path, "snr_azi") {
            let s = compare(&snr.snr_azi, &truth);
            report("snr_azi", &s);
            if s.max_rel_err > SNR_MAX_REL_TOL {
                failed.push(format!("snr_azi: max_rel={:.6e} > {:.6e}", s.max_rel_err, SNR_MAX_REL_TOL));
            }
        }
        if let Some(truth) = read_map(&path, "snr_alt") {
            let s = compare(&snr.snr_alt, &truth);
            report("snr_alt", &s);
            if s.max_rel_err > SNR_MAX_REL_TOL {
                failed.push(format!("snr_alt: max_rel={:.6e} > {:.6e}", s.max_rel_err, SNR_MAX_REL_TOL));
            }
        }
    }

    if !failed.is_empty() {
        panic!(
            "Device-stability test failed for {} map(s) on device {}:\n  {}",
            failed.len(),
            isi_analysis::compute::backend_info(),
            failed.join("\n  "),
        );
    }
    println!("  PASS — all maps within tolerance on {}", isi_analysis::compute::backend_info());
}
