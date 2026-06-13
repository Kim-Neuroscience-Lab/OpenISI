//! Synthetic ground-truth retinotopy test — recovery of a KNOWN map.
//!
//! Every per-stage golden pins one method to an external reference (scipy /
//! Octave / numpy). This test is the complementary tier: it validates the
//! *composition* of the retinotopy stages end-to-end against **analytic
//! truth computed in-process**, with no captured fixture. We forward-model a
//! retinotopy whose phase fields and visual-field-sign structure are known by
//! construction, encode it into the four direction complex maps exactly as the
//! periodic stimulus + bin-1 DFT would (phase = position ± a common
//! hemodynamic delay), then run the REAL pipeline (`compute_retinotopy`:
//! cycle-combine → phase-smooth → VFS) and assert it recovers the truth.
//!
//! Forward model (Kalatsky-Stryker continuous periodic encoding):
//!   - `pa(x,y)` — azimuth position phase, a *tent* in x: gradient `∂/∂x`
//!     flips sign at the midline (the canonical mirror border between two
//!     adjacent visual areas; cf. V1|V2).
//!   - `pl(x,y)` — altitude position phase, monotonic in y (`∂/∂y > 0`).
//!   - common delay `δ`, encoded into BOTH sweeps of each orientation:
//!     `*_fwd = exp(i·(+pos + δ))`, `*_rev = exp(i·(−pos + δ))`.
//!     Delay subtraction must remove `δ` and return `pos`.
//!
//! Known truth this pins:
//!   1. recovered `azi_phase ≈ pa`, `alt_phase ≈ pl` (delay removed,
//!      position preserved) — exercises cycle-combine + phase-smoothing.
//!   2. recovered VFS sign: `+1` left of the midline, `−1` right — exercises
//!      the chain-rule phase gradients + sign cross-product. Because `pa`
//!      depends only on x and `pl` only on y, `θ_azi ∈ {0, π}` and
//!      `θ_alt = +π/2`, so `VFS = sin(θ_alt − θ_azi) = ±1` analytically.
//!
//! This is also the substrate for old-vs-new method benchmarking: swap any
//! stage method and measure recovery error against the same known field.

use std::f64::consts::PI;

use ndarray::Array2;
use num_complex::Complex64;

use isi_analysis::methods::CortexSourceMethod;
use isi_analysis::{AcquisitionProperties, AnalysisParams, ComplexMaps, ProvenanceLevel};

const H: usize = 64;
const W: usize = 64;

/// Canonical default pipeline (Kalatsky combine, OpenISI amp-weighted phasor
/// smoothing, OpenISI chain-rule VFS) from a fresh registry snapshot — the
/// exact construction path production uses.
fn default_params() -> AnalysisParams {
    let here = std::path::Path::new(".");
    let snap = openisi_params::Registry::new(here, here).snapshot();
    isi_analysis::bridge::analysis_params_from_snapshot(&snap)
}

/// Identity geometry: `rotation_k = 0` (no pre-rotation of the complex maps),
/// fully-provided fields. Degree ranges only scale magnification (not tested
/// here); phase and VFS are independent of them.
fn identity_acq() -> AcquisitionProperties {
    AcquisitionProperties {
        azi_angular_range: 120.0,
        alt_angular_range: 110.0,
        offset_azi: 0.0,
        offset_alt: 0.0,
        rotation_k: 0,
        um_per_pixel: 10.0,
        provenance: ProvenanceLevel::Full,
    }
}

/// Azimuth position phase — a tent in x peaking at the midline. The gradient
/// `∂pa/∂x` is `+slope` left of `xmid` and `−slope` right of it; `∂pa/∂y = 0`.
/// Range kept well inside `(−π, π)` so there are no phase wraps to confound
/// the direct phase comparison.
fn pa(r: usize, c: usize) -> f64 {
    let _ = r;
    let xmid = (W as f64 - 1.0) / 2.0;
    let slope = 1.6 / xmid; // peak ≈ 1.6 rad at the midline
    slope * (xmid - (c as f64 - xmid).abs())
}

/// Altitude position phase — monotonic in y, centered on zero. `∂pl/∂y > 0`
/// everywhere, `∂pl/∂x = 0`.
fn pl(r: usize, c: usize) -> f64 {
    let _ = c;
    let ymid = (H as f64 - 1.0) / 2.0;
    let slope = 1.4 / ymid; // ±1.4 rad across the frame
    slope * (r as f64 - ymid)
}

/// Build the four direction complex maps from a position field and a common
/// delay: `fwd = exp(i(+pos + δ))`, `rev = exp(i(−pos + δ))`. Unit amplitude.
fn encode(pos: impl Fn(usize, usize) -> f64, delay: f64) -> (Array2<Complex64>, Array2<Complex64>) {
    let fwd = Array2::from_shape_fn((H, W), |(r, c)| Complex64::from_polar(1.0, pos(r, c) + delay));
    let rev = Array2::from_shape_fn((H, W), |(r, c)| Complex64::from_polar(1.0, -pos(r, c) + delay));
    (fwd, rev)
}

/// Smallest signed angular difference `a − b`, wrapped to `(−π, π]`.
fn ang_diff(a: f64, b: f64) -> f64 {
    let mut d = a - b;
    while d > PI {
        d -= 2.0 * PI;
    }
    while d <= -PI {
        d += 2.0 * PI;
    }
    d
}

/// A pixel is "interior" if it is at least `margin` from every edge and from
/// the azimuth tent's ridge (where the gradient is, by construction,
/// non-differentiable and central differences smear the sign reversal).
fn interior(r: usize, c: usize, margin: usize) -> bool {
    let xmid = (W as f64 - 1.0) / 2.0;
    r >= margin
        && r < H - margin
        && c >= margin
        && c < W - margin
        && (c as f64 - xmid).abs() >= margin as f64
}

#[test]
fn pipeline_recovers_known_phase_and_vfs_sign() {
    // δ in (0, π] so the delay-into-(0,π] correction is a no-op and the
    // recovered position is exactly pos (not pos shifted by the correction).
    let delay = PI / 4.0;
    let (azi_fwd, azi_rev) = encode(pa, delay);
    let (alt_fwd, alt_rev) = encode(pl, delay);
    let maps = ComplexMaps {
        azi_fwd,
        azi_rev,
        alt_fwd,
        alt_rev,
    };

    let never_cancel = std::sync::atomic::AtomicBool::new(false);
    let retino =
        isi_analysis::compute_retinotopy(&maps, &identity_acq(), &default_params(), &never_cancel)
            .expect("compute_retinotopy on synthetic maps");

    // ── 1. Phase recovery (delay removed, position preserved) ────────────
    // Away from the ridge and edges, amp-weighted smoothing of a locally
    // linear phasor is near-exact; the bound is f32-pipeline tight.
    let margin = 4;
    let mut max_azi_err = 0.0f64;
    let mut max_alt_err = 0.0f64;
    for r in 0..H {
        for c in 0..W {
            if !interior(r, c, margin) {
                continue;
            }
            max_azi_err = max_azi_err.max(ang_diff(retino.azi_phase[[r, c]], pa(r, c)).abs());
            max_alt_err = max_alt_err.max(ang_diff(retino.alt_phase[[r, c]], pl(r, c)).abs());
        }
    }
    eprintln!("phase recovery: max azi err = {max_azi_err:.3e}, max alt err = {max_alt_err:.3e}");
    assert!(
        max_azi_err < 2.0e-2,
        "azimuth phase not recovered (delay subtraction / smoothing): {max_azi_err:.3e}"
    );
    assert!(
        max_alt_err < 2.0e-2,
        "altitude phase not recovered: {max_alt_err:.3e}"
    );

    // ── 2. VFS sign recovery (+1 left of midline, −1 right) ──────────────
    let xmid = (W as f64 - 1.0) / 2.0;
    let mut checked = 0usize;
    let mut correct = 0usize;
    for r in 0..H {
        for c in 0..W {
            if !interior(r, c, margin) {
                continue;
            }
            let expected = if (c as f64) < xmid { 1.0 } else { -1.0 };
            let v = retino.vfs[[r, c]];
            checked += 1;
            // Strong, unambiguous sign: |vfs| should be near 1 in these
            // flat-gradient regions, not a marginal near-zero value.
            if v.signum() == expected && v.abs() > 0.5 {
                correct += 1;
            }
        }
    }
    let frac = correct as f64 / checked as f64;
    eprintln!("VFS sign recovery: {correct}/{checked} = {frac:.4} (|vfs|>0.5, correct sign)");
    assert!(
        frac > 0.99,
        "VFS sign structure not recovered: only {frac:.4} of interior pixels match the known \
         mirror-pair sign with |vfs|>0.5"
    );
}

#[test]
fn full_pipeline_segments_two_areas_of_opposite_sign() {
    // Same synthetic mirror-pair field, but now driven through the WHOLE
    // pipeline (`pipeline::run`): retinotopy → sign-map smoothing → threshold
    // → patch extraction → split/merge → labeling → sign assignment. The tent
    // azimuth + monotonic altitude is the textbook two-area mirror pair, so a
    // faithful segmentation must recover exactly two areas of OPPOSITE sign.
    //
    // Cortex source is `NoRestriction`: the entire synthetic frame is valid
    // cortex by construction, so segmentation operates on the full VFS and we
    // test the patch stages, not the cortex mask (which has its own goldens).
    let delay = PI / 4.0;
    let (azi_fwd, azi_rev) = encode(pa, delay);
    let (alt_fwd, alt_rev) = encode(pl, delay);
    let maps = ComplexMaps {
        azi_fwd,
        azi_rev,
        alt_fwd,
        alt_rev,
    };

    let mut params = default_params();
    params.cortex_source = CortexSourceMethod::no_restriction();

    // The pure seeded-maps path: complex maps are the input (stage 0 skipped),
    // no reliability/polygon, full tail recompute. `compute_analysis` is the
    // public wrapper over `pipeline::run` for exactly this case.
    let result = isi_analysis::compute_analysis(&maps, None, None, &identity_acq(), &params)
        .expect("full pipeline on synthetic maps");

    // Area signs: one ±1 per segmented area (background label 0 excluded).
    let signs = &result.area_signs;
    let n_areas = signs.len();
    let n_pos = signs.iter().filter(|&&s| s > 0).count();
    let n_neg = signs.iter().filter(|&&s| s < 0).count();
    eprintln!("segmented areas = {n_areas}  (+{n_pos} / -{n_neg})  signs = {signs:?}");

    assert_eq!(
        n_areas, 2,
        "expected exactly two areas (mirror pair), got {n_areas}: {signs:?}"
    );
    assert!(
        n_pos == 1 && n_neg == 1,
        "expected one +1 and one -1 area (opposite signs), got +{n_pos}/-{n_neg}: {signs:?}"
    );

    // The two area labels must occupy the left and right halves: the +1 area's
    // centroid left of the midline, the -1 area's centroid right of it.
    let labels = &result.area_labels;
    let xmid = (W as f64 - 1.0) / 2.0;
    let mut sum_c = vec![0.0f64; n_areas + 1];
    let mut cnt = vec![0.0f64; n_areas + 1];
    for r in 0..H {
        for c in 0..W {
            let l = labels[[r, c]];
            if l > 0 && (l as usize) <= n_areas {
                sum_c[l as usize] += c as f64;
                cnt[l as usize] += 1.0;
            }
        }
    }
    for (label, &sign) in (1..=n_areas).zip(signs.iter()) {
        let cx = sum_c[label] / cnt[label].max(1.0);
        let side = if cx < xmid { "left" } else { "right" };
        eprintln!("  area {label}: sign {sign}, centroid_x = {cx:.1} ({side})");
        if sign > 0 {
            assert!(cx < xmid, "+1 area should be left of midline, centroid_x={cx:.1}");
        } else {
            assert!(cx > xmid, "-1 area should be right of midline, centroid_x={cx:.1}");
        }
    }
}

// ─── Mid-stage cancellation (responsiveness) ─────────────────────────────────
//
// The two stages long enough to need interruption *within* a single execute —
// retinotopy (device sub-ops) and patch refinement (the split/merge hotspot) —
// honor the shared `cancel` flag, so a param change mid-run aborts promptly
// instead of waiting the stage out. A pre-set flag fires at the first internal
// checkpoint, which is deterministic to assert.

#[test]
fn compute_retinotopy_honors_cancellation() {
    let z = || Array2::<Complex64>::zeros((H, W));
    let maps = ComplexMaps {
        azi_fwd: z(),
        azi_rev: z(),
        alt_fwd: z(),
        alt_rev: z(),
    };
    let cancelled = std::sync::atomic::AtomicBool::new(true);
    let result =
        isi_analysis::compute_retinotopy(&maps, &identity_acq(), &default_params(), &cancelled);
    assert!(
        matches!(result, Err(isi_analysis::AnalysisError::Cancelled)),
        "a cancelled run must abort with Cancelled, not return retinotopy maps"
    );
}

#[test]
fn patch_refinement_honors_cancellation() {
    // The default refinement method is Allen split/merge, which checks `cancel`
    // before its split pass and at the top of every merge round.
    let refine = default_params().patch_refinement;
    let v = Array2::<f64>::zeros((H, W));
    let cancelled = std::sync::atomic::AtomicBool::new(true);
    let result = refine.apply(vec![], &v, &v, &v, &cancelled);
    assert!(
        matches!(result, Err(isi_analysis::AnalysisError::Cancelled)),
        "a cancelled split/merge must abort with Cancelled"
    );
}
