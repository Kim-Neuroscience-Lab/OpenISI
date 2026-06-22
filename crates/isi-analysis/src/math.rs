//! Analysis math that runs on the host: retinotopy orchestration,
//! post-segmentation derived maps, and a few utility transforms.
//!
//! Heavy numerics (dF/F, DFT, SNR, smoothing, gradients, VFS) run on the
//! Burn tensor substrate (`crate::compute`). `compute_retinotopy` is the
//! production retinotopy entry point; its host-side scaffolding
//! (`rotated_complex_maps`, `degree_scales`) handles the optional rotation and
//! degree scaling, and it assembles the final maps inline.

use ndarray::Array2;
use num_complex::Complex64;
use std::f64::consts::PI;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::compute;
use crate::methods::{
    CycleCombineExt, DirectionSmoothingExt, PhaseSmoothingExt, VfsComputationExt,
};
use crate::{AcquisitionProperties, AnalysisError, AnalysisParams, ComplexMaps, RetinotopyMaps};

/// Bail out with [`AnalysisError::Cancelled`] when the run was cancelled. The
/// retinotopy stage checks this between its device sub-ops so a mid-stage param
/// change (which sets `cancel`) is honored without waiting for the stage to
/// finish — mirroring the per-cycle check in `compute::projection::run`.
fn check_cancel(cancel: &AtomicBool) -> crate::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(AnalysisError::Cancelled)
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Compute retinotopy maps from four complex maps and parameters.
///
/// Single canonical pipeline — every step computed exactly once. The
/// position phase is represented as a **complex phasor** `z = exp(i·φ)`
/// throughout: smoothing, gradients, and VFS all operate on `z` (real
/// and imaginary components are continuous through phase wraps). The
/// wrapped real `φ` is recovered via `arg(z)` only at the very end,
/// once, to populate the display fields `azi_phase` and `alt_phase`.
///
///   1. Optional 90° rotation on host (only when `rotation_k != 0`).
///   2. Upload the four complex maps to device as `Complex2` pairs.
///   3. Per-orientation position phasor via Marshel-Garrett delay
///      subtraction (`position_phasor_delay_subtracted`). Returns a
///      complex tensor `z = exp(i·φ)`; the wrapped real φ is never
///      exposed.
///   4. Per-orientation amplitude = mean of fwd/rev magnitudes.
///   5. Amplitude-weighted complex smoothing of `z` (normalized
///      convolution on real and imaginary parts).
///   6. Wrap-free phase gradients via the chain rule on `z`
///      (`phase_gradients`).
///   7. VFS = sin(θ_alt − θ_azi) where θ = atan2(∂φ/∂y, ∂φ/∂x).
///   8. Magnification = `|J_deg|` from the same gradients.
///   9. Single device→CPU download of all retinotopy results. Phase
///      recovered for display via `arg(z_smoothed)` at this boundary.
///  10. Phase → degrees on host (linear scale).
///
/// Apply the optional host-side 90°·k rotation to the four complex maps,
/// returning owned copies. The rotation convention lives in one place.
/// Small, sometimes-applied, four arrays only — host ndarray work.
fn rotated_complex_maps(
    maps: &ComplexMaps,
    rotation_k: i32,
) -> (
    Array2<Complex64>,
    Array2<Complex64>,
    Array2<Complex64>,
    Array2<Complex64>,
) {
    if rotation_k != 0 {
        (
            rot90(&maps.azi_fwd, rotation_k),
            rot90(&maps.azi_rev, rotation_k),
            rot90(&maps.alt_fwd, rotation_k),
            rot90(&maps.alt_rev, rotation_k),
        )
    } else {
        (
            maps.azi_fwd.clone(),
            maps.azi_rev.clone(),
            maps.alt_fwd.clone(),
            maps.alt_rev.clone(),
        )
    }
}

/// Per-orientation degree-scale factor `angular_range / 2π`.
fn degree_scales(acquisition: &AcquisitionProperties) -> (f64, f64) {
    (
        acquisition.azi_angular_range / (2.0 * PI),
        acquisition.alt_angular_range / (2.0 * PI),
    )
}

/// Compute retinotopy on the Burn substrate. Same stage order as the
/// docstring above: rotate → upload → cycle combine → phasor smoothing →
/// VFS + gradients → magnification → single device→host download. Gated
/// end-to-end by `tests/equivalence.rs` against the committed baseline.
pub fn compute_retinotopy(
    maps: &ComplexMaps,
    acquisition: &AcquisitionProperties,
    params: &AnalysisParams,
    cancel: &AtomicBool,
) -> crate::Result<RetinotopyMaps> {
    let (azi_fwd, azi_rev, alt_fwd, alt_rev) = rotated_complex_maps(maps, acquisition.rotation_k);

    // Stage 1a — optional per-direction smoothing of the four complex F1 maps,
    // applied BEFORE cycle-combine (where SNLC `Gprocesskret` applies its
    // adaptive smoother). `None` (default) is a clone → bit-identical pipeline.
    let ds = &params.direction_smoothing;
    let (azi_fwd, azi_rev, alt_fwd, alt_rev) = (
        ds.apply(&azi_fwd),
        ds.apply(&azi_rev),
        ds.apply(&alt_fwd),
        ds.apply(&alt_rev),
    );

    // Upload the four complex maps to device as Complex2 pairs.
    let a_fwd = compute::array2_complex_to_complex2(&azi_fwd);
    let a_rev = compute::array2_complex_to_complex2(&azi_rev);
    let l_fwd = compute::array2_complex_to_complex2(&alt_fwd);
    let l_rev = compute::array2_complex_to_complex2(&alt_rev);

    // Per-orientation amplitude = mean of forward and reverse magnitudes.
    let azi_amp_t = compute::position_amplitude(&a_fwd, &a_rev);
    let alt_amp_t = compute::position_amplitude(&l_fwd, &l_rev);

    check_cancel(cancel)?;
    // Stage 1 — cycle combine. The delay maps (SNLC `Gprocesskret` delay_hor/
    // _vert) are a byproduct of the same fwd+rev combine — `Some` only when the
    // method does delay subtraction. Computed lazily here; downloaded below.
    let (azi_z, alt_z) = params.cycle_combine.apply(&a_fwd, &a_rev, &l_fwd, &l_rev);
    let delays_t = params.cycle_combine.delays(&a_fwd, &a_rev, &l_fwd, &l_rev);

    check_cancel(cancel)?;
    // Stage 2 — position phasor smoothing.
    let (azi_z_s, alt_z_s) =
        params
            .phase_smoothing
            .apply(&azi_z, &alt_z, azi_amp_t.clone(), alt_amp_t.clone());

    check_cancel(cancel)?;
    // Stage 3 — VFS + the four phase gradients.
    let (vfs_t, d_azi_dx, d_azi_dy, d_alt_dx, d_alt_dy) =
        params.vfs_computation.apply(&azi_z_s, &alt_z_s);

    let (scale_azi, scale_alt) = degree_scales(acquisition);
    // The determinant (magnification) and the anisotropy (axis + distortion) are
    // invariants of the SAME Jacobian — both consume the four gradients, so they
    // are computed together here (clone the gradients into the anisotropy; the
    // determinant takes them by value).
    let (axis_t, distortion_t) = compute::magnification_anisotropy(
        d_azi_dx.clone(),
        d_azi_dy.clone(),
        d_alt_dx.clone(),
        d_alt_dy.clone(),
    );
    let mag_t = compute::compute_magnification_jacobian(
        d_azi_dx, d_azi_dy, d_alt_dx, d_alt_dy, scale_azi, scale_alt,
    );

    // Device→host downloads (the lazy op graph actually executes here). A last
    // cancel check lets a mid-stage cancellation skip the syncs entirely.
    check_cancel(cancel)?;
    // Phase recovered via arg(z_smoothed).
    let vfs = compute::tensor_to_array2_f64(vfs_t)?;
    let magnification_raw = compute::tensor_to_array2_f64(mag_t)?;
    let magnification_axis = compute::tensor_to_array2_f64(axis_t)?;
    let magnification_distortion = compute::tensor_to_array2_f64(distortion_t)?;
    let azi_phase = compute::tensor_to_array2_f64(azi_z_s.angle())?;
    let alt_phase = compute::tensor_to_array2_f64(alt_z_s.angle())?;
    let azi_amplitude = compute::tensor_to_array2_f64(azi_amp_t)?;
    let alt_amplitude = compute::tensor_to_array2_f64(alt_amp_t)?;
    // Delay maps: radians (0, π] → degrees (0, 180] (SNLC `delay*180/pi`),
    // unmasked. `None` passes straight through for non-delay-subtraction.
    let (azi_delay, alt_delay) = match delays_t {
        Some((az, al)) => {
            let to_deg = (180.0 / PI) as f32;
            (
                Some(compute::tensor_to_array2_f64(az.mul_scalar(to_deg))?),
                Some(compute::tensor_to_array2_f64(al.mul_scalar(to_deg))?),
            )
        }
        None => (None, None),
    };

    // Assemble the result: derive the degree-scaled phase maps, then pack the
    // struct by named field (so the six same-shaped `Array2<f64>` maps can't be
    // transposed). This is the one place the phase→degrees convention lives.
    let azi_phase_degrees = phase_to_degrees(
        &azi_phase,
        acquisition.azi_angular_range,
        acquisition.offset_azi,
    );
    let alt_phase_degrees = phase_to_degrees(
        &alt_phase,
        acquisition.alt_angular_range,
        acquisition.offset_alt,
    );
    Ok(RetinotopyMaps {
        azi_phase,
        alt_phase,
        azi_phase_degrees,
        alt_phase_degrees,
        azi_amplitude,
        alt_amplitude,
        vfs,
        magnification_raw,
        magnification_axis,
        magnification_distortion,
        azi_delay,
        alt_delay,
    })
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Derived maps (computed from retinotopy + segmentation)
// ---------------------------------------------------------------------------

/// Zero every pixel outside the segmented ROI (`area_labels == 0`); keep
/// the source value inside. Shared by every derived map that is meaningful
/// only within segmented patches: VFS thresholded, magnification, etc.
pub fn apply_label_roi(src: &Array2<f64>, area_labels: &Array2<i32>) -> Array2<f64> {
    let (h, w) = src.dim();
    Array2::from_shape_fn((h, w), |(r, c)| {
        if area_labels[[r, c]] > 0 {
            src[[r, c]]
        } else {
            0.0
        }
    })
}

/// Cortical magnification factor (**px²/deg²**) — the reciprocal of the
/// visual-field Jacobian determinant `|det J|` (deg²/px², our `magnification_raw`).
/// High where a small patch of visual space maps to a large patch of cortex (cortex
/// is *magnified*) — the physiologically meaningful direction.
///
/// **Oracle note (why there is no cap):** the oracle, Allen
/// `RetinotopicMapping._getDeterminantMap`, outputs `|det J|` — i.e.
/// `magnification_raw` — and stops there; it never inverts. This reciprocal leaf is
/// OpenISI's, for display only. So at near-singular pixels (`|det J| → 0`) this map
/// spikes — an artifact of *our* inversion, not a physical signal nor an oracle
/// quantity — and the renderer's 2–98 percentile scaling already absorbs it. We
/// therefore add NO cap: capping would diverge from the oracle to patch our own
/// transform, and a "physical" cap would need a max cortical magnification (V1
/// extent), itself a literature magic-number requiring V1 to be identified. `eps`
/// only prevents a literal divide-by-zero (it is never reached on real data: the
/// smallest observed `|det J|` is ~1e-6). Pixels outside any segmented patch are
/// zeroed (`area_labels == 0`), like the other ROI-gated derived maps.
pub fn cortical_magnification_factor(
    magnification_raw: &Array2<f64>,
    area_labels: &Array2<i32>,
) -> Array2<f64> {
    const EPS: f64 = 1e-12;
    let (h, w) = magnification_raw.dim();
    Array2::from_shape_fn((h, w), |(r, c)| {
        if area_labels[[r, c]] > 0 {
            1.0 / magnification_raw[[r, c]].max(EPS)
        } else {
            0.0
        }
    })
}

/// Angular eccentricity (degrees) of visual-field point `(alt, azi)` from
/// a reference point `(alt_c, azi_c)`. All angles in degrees.
///
/// Verbatim Allen `retinotopic_mapping/RetinotopicMapping.py::eccentricityMap`:
///
/// ```text
/// ecc = atan( sqrt( tan(alt - alt_c)² + tan(azi - azi_c)² / cos(alt - alt_c)² ) )
/// ```
///
/// Note the cosine denominator uses the *altitude* delta, not the
/// azimuth — `alt` is the latitude-like coordinate, `azi` is longitude-like,
/// and a degree of azimuth subtends `cos(alt)` of great-circle distance.
/// This is the single definition used by `compute_eccentricity` (V1-centric
/// display map) and `segmentation::pipeline::build_eccentricity_map` (Allen
/// per-patch eccentricity for the split step).
pub fn eccentricity_pixel_deg(alt_deg: f64, azi_deg: f64, alt_c_deg: f64, azi_c_deg: f64) -> f64 {
    let to_rad = PI / 180.0;
    let d_alt = (alt_deg - alt_c_deg) * to_rad;
    let d_azi = (azi_deg - azi_c_deg) * to_rad;
    let cos_d_alt = d_alt.cos();
    let term = d_alt.tan().powi(2) + d_azi.tan().powi(2) / (cos_d_alt * cos_d_alt).max(1e-12);
    term.sqrt().atan() * 180.0 / PI
}

/// The visual-field center of V1 (largest segmented area's center-of-mass), as
/// `(center_azi, center_alt)` in degrees — the shared reference point for the
/// V1-centric eccentricity and polar-angle maps. Returns `(0, 0)` when no area
/// is segmented.
pub fn v1_center(
    azi_deg: &Array2<f64>,
    alt_deg: &Array2<f64>,
    area_labels: &Array2<i32>,
) -> (f64, f64) {
    let (h, w) = azi_deg.dim();
    let max_label = *area_labels.iter().max().unwrap_or(&0);
    let mut counts = vec![0usize; max_label as usize + 1];
    for &l in area_labels.iter() {
        if l > 0 {
            counts[l as usize] += 1;
        }
    }
    let v1_label = (1..=max_label as usize)
        .max_by_key(|&i| counts[i])
        .unwrap_or(1) as i32;

    let mut sum_azi = 0.0f64;
    let mut sum_alt = 0.0f64;
    let mut n = 0usize;
    for r in 0..h {
        for c in 0..w {
            if area_labels[[r, c]] == v1_label {
                sum_azi += azi_deg[[r, c]];
                sum_alt += alt_deg[[r, c]];
                n += 1;
            }
        }
    }
    if n > 0 {
        (sum_azi / n as f64, sum_alt / n as f64)
    } else {
        (0.0, 0.0)
    }
}

/// V1-centric eccentricity map (degrees). V1 = largest segmented area; the
/// center is V1's center-of-mass in visual-field coordinates. Pixels
/// outside any segmented patch are zeroed via `apply_label_roi`.
pub fn compute_eccentricity(
    azi_deg: &Array2<f64>,
    alt_deg: &Array2<f64>,
    area_labels: &Array2<i32>,
) -> Array2<f64> {
    let (h, w) = azi_deg.dim();
    let (center_azi, center_alt) = v1_center(azi_deg, alt_deg, area_labels);

    Array2::from_shape_fn((h, w), |(r, c)| {
        if area_labels[[r, c]] == 0 {
            return 0.0;
        }
        eccentricity_pixel_deg(alt_deg[[r, c]], azi_deg[[r, c]], center_alt, center_azi)
    })
}

/// V1-centric **polar-angle** map (degrees, −180..180) — the angular companion to
/// [`compute_eccentricity`], sharing the same V1 center. Together they are the
/// two standard retinotopic-coordinate displays (eccentricity + polar angle).
///
/// Verbatim SNLC `getRadialEccMapX.m:56`:
/// `kmap_ang = atan2(alt, az)·180/π`, where `alt`/`az` are the V1-centered
/// position deltas. The `·π/180` scaling SNLC applies to both deltas cancels
/// inside `atan2`, so this is `atan2(altΔ, aziΔ)·180/π` on the degree maps.
/// Pixels outside any segmented patch are zeroed, like the other patch-scoped maps.
pub fn compute_polar_angle(
    azi_deg: &Array2<f64>,
    alt_deg: &Array2<f64>,
    area_labels: &Array2<i32>,
) -> Array2<f64> {
    let (h, w) = azi_deg.dim();
    let (center_azi, center_alt) = v1_center(azi_deg, alt_deg, area_labels);

    Array2::from_shape_fn((h, w), |(r, c)| {
        if area_labels[[r, c]] == 0 {
            return 0.0;
        }
        (alt_deg[[r, c]] - center_alt).atan2(azi_deg[[r, c]] - center_azi) * 180.0 / PI
    })
}

/// Angular eccentricity (degrees) with the **SNLC/Callaway** cosine
/// convention — the denominator cosine is on the **azimuth** delta, not the
/// altitude. Verbatim `getAreaBorders.m` L223-224 (`reference/ISI/...`):
///
/// ```text
/// az  = (kmap_hor  - aziC)·π/180
/// alt = (kmap_vert - altC)·π/180
/// ecc = atan( sqrt( tan(az)² + tan(alt)² / cos(az)² ) )·180/π
/// ```
///
/// This is the mirror of [`eccentricity_pixel_deg`] (Allen, cos-on-altitude);
/// the two genuinely disagree off the meridians. No cosine floor — the SNLC
/// oracle divides by `cos(az)²` directly (azimuth stays well inside ±90°).
pub fn eccentricity_pixel_deg_snlc(
    alt_deg: f64,
    azi_deg: f64,
    alt_c_deg: f64,
    azi_c_deg: f64,
) -> f64 {
    let to_rad = PI / 180.0;
    let az = (azi_deg - azi_c_deg) * to_rad;
    let alt = (alt_deg - alt_c_deg) * to_rad;
    let cos_az = az.cos();
    let term = az.tan().powi(2) + alt.tan().powi(2) / (cos_az * cos_az);
    term.sqrt().atan() * 180.0 / PI
}

/// V1-centric eccentricity map (degrees), **faithful to SNLC `getAreaBorders.m`**
/// (`getV1id.m` + `getPatchCoM.m`). The reference point differs from
/// [`compute_eccentricity`] (our OpenISI choice) in three transcribed ways:
///   1. `imopen(disk-10)` the segmented mask *before* component selection
///      (removes thin spurs that would drag the centroid);
///   2. V1 = the largest 4-connected component of the *opened* mask, first on a
///      tie (`bwlabel(im,4)` + MATLAB `max`);
///   3. the center is a **single-pixel sample** of azi/alt at the rounded
///      **pixel-space centroid** of that component (with off-patch snap to the
///      nearest in-patch pixel), NOT the visual-field mean over V1 pixels.
///
/// The per-pixel formula is the SNLC cos-on-azimuth one
/// ([`eccentricity_pixel_deg_snlc`]). Pixels outside any patch are zeroed.
pub fn compute_eccentricity_snlc(
    azi_deg: &Array2<f64>,
    alt_deg: &Array2<f64>,
    area_labels: &Array2<i32>,
) -> Array2<f64> {
    use crate::segmentation::connectivity::label_4conn;
    use crate::segmentation::morphology::binary_opening_disk;

    let (h, w) = azi_deg.dim();

    // (1) imopen(disk-10) on the binary segmentation mask.
    let mask = Array2::from_shape_fn((h, w), |(r, c)| area_labels[[r, c]] > 0);
    let opened = binary_opening_disk(&mask, 10);

    // (2) V1 = largest 4-connected component of the opened mask (first on tie).
    let (lbl, n) = label_4conn(&opened);
    let mut counts = vec![0usize; n + 1];
    for &l in lbl.iter() {
        if l > 0 {
            counts[l as usize] += 1;
        }
    }
    let mut v1: i32 = 0;
    let mut best = 0usize;
    for (l, &cnt) in counts.iter().enumerate().skip(1) {
        // strict `>` keeps the FIRST label on a tie (MATLAB `max` semantics).
        if cnt > best {
            best = cnt;
            v1 = l as i32;
        }
    }
    if v1 == 0 {
        // Opening erased everything — no reference point; zero map.
        return Array2::zeros((h, w));
    }

    // (3) Pixel-space centroid of V1: com_x = mean column, com_y = mean row.
    let (mut sx, mut sy, mut cnt) = (0.0f64, 0.0f64, 0usize);
    for r in 0..h {
        for c in 0..w {
            if lbl[[r, c]] == v1 {
                sx += c as f64;
                sy += r as f64;
                cnt += 1;
            }
        }
    }
    let mut com_x = sx / cnt as f64;
    let mut com_y = sy / cnt as f64;

    // Off-patch snap: if the rounded centroid is not on V1, replace it with the
    // in-patch pixel nearest the (float) centroid — first in row-major scan on a
    // distance tie (mirrors numpy `where(rdom==mind)`).
    let rr = com_y.round() as isize;
    let cc = com_x.round() as isize;
    let on_patch = rr >= 0
        && rr < h as isize
        && cc >= 0
        && cc < w as isize
        && lbl[[rr as usize, cc as usize]] == v1;
    if !on_patch {
        let mut min_d = f64::INFINITY;
        let (mut bx, mut by) = (com_x, com_y);
        for r in 0..h {
            for c in 0..w {
                if lbl[[r, c]] == v1 {
                    let dx = c as f64 - com_x;
                    let dy = r as f64 - com_y;
                    let d = (dx * dx + dy * dy).sqrt();
                    if d < min_d {
                        min_d = d;
                        bx = c as f64;
                        by = r as f64;
                    }
                }
            }
        }
        com_x = bx;
        com_y = by;
    }

    // Single-pixel sample of azi/alt at the rounded centroid.
    let pr = (com_y.round() as usize).min(h - 1);
    let pc = (com_x.round() as usize).min(w - 1);
    let center_azi = azi_deg[[pr, pc]];
    let center_alt = alt_deg[[pr, pc]];

    Array2::from_shape_fn((h, w), |(r, c)| {
        if area_labels[[r, c]] == 0 {
            return 0.0;
        }
        eccentricity_pixel_deg_snlc(alt_deg[[r, c]], azi_deg[[r, c]], center_alt, center_azi)
    })
}

/// Iso-contour mask at given interval (e.g., 4°). Pixels where the phase value
/// crosses an interval boundary between adjacent pixels → true.
/// Masked to areas (area_labels > 0 only).
pub fn compute_contours(
    phase_deg: &Array2<f64>,
    area_labels: &Array2<i32>,
    interval_deg: f64,
) -> Array2<bool> {
    let (h, w) = phase_deg.dim();
    let mut result = Array2::from_elem((h, w), false);

    for r in 0..h {
        for c in 0..w {
            if area_labels[[r, c]] == 0 {
                continue;
            }
            let val = phase_deg[[r, c]];
            let bin = (val / interval_deg).floor();

            // Check right neighbor.
            if c + 1 < w && area_labels[[r, c + 1]] > 0 {
                let nbin = (phase_deg[[r, c + 1]] / interval_deg).floor();
                if bin != nbin {
                    result[[r, c]] = true;
                }
            }
            // Check bottom neighbor.
            if r + 1 < h && area_labels[[r + 1, c]] > 0 {
                let nbin = (phase_deg[[r + 1, c]] / interval_deg).floor();
                if bin != nbin {
                    result[[r, c]] = true;
                }
            }
        }
    }
    result
}

/// Convert phase (radians) to visual field degrees.
fn phase_to_degrees(phase: &Array2<f64>, angular_range: f64, offset: f64) -> Array2<f64> {
    let scale = angular_range / (2.0 * PI);
    phase.mapv(|phi| phi * scale + offset)
}

// ---------------------------------------------------------------------------
// Helpers (host-side)
// ---------------------------------------------------------------------------

/// Normalized 1D Gaussian kernel — used by host-side segmentation smoothing
/// and by the figure exporter to produce a smoothed-VFS view.
/// (The on-device Gaussian for retinotopy is `compute::gaussian_smooth`, which
/// builds its kernel on-device via `Tensor::arange` + `exp`.)
pub fn gaussian_kernel_1d(sigma: f64, radius: usize) -> Vec<f64> {
    let size = 2 * radius + 1;
    let two_sigma_sq = 2.0 * sigma * sigma;
    let mut kernel = Vec::with_capacity(size);
    let mut sum = 0.0;
    for i in 0..size {
        let x = i as f64 - radius as f64;
        let val = (-x * x / two_sigma_sq).exp();
        kernel.push(val);
        sum += val;
    }
    for v in &mut kernel {
        *v /= sum;
    }
    kernel
}

/// Separable 2D convolution (horizontal then vertical) with reflected
/// boundaries. Used by host-side segmentation smoothing and by the figure
/// exporter to produce a smoothed-VFS view.
pub fn separable_filter(input: &Array2<f64>, kernel: &[f64]) -> Array2<f64> {
    let (h, w) = input.dim();
    let radius = kernel.len() / 2;

    let mut temp = Array2::zeros((h, w));
    for row in 0..h {
        for col in 0..w {
            let mut acc = 0.0;
            for (ki, &kv) in kernel.iter().enumerate() {
                let src = reflect(col as isize + ki as isize - radius as isize, w);
                acc += input[[row, src]] * kv;
            }
            temp[[row, col]] = acc;
        }
    }

    let mut output = Array2::zeros((h, w));
    for row in 0..h {
        for col in 0..w {
            let mut acc = 0.0;
            for (ki, &kv) in kernel.iter().enumerate() {
                let src = reflect(row as isize + ki as isize - radius as isize, h);
                acc += temp[[src, col]] * kv;
            }
            output[[row, col]] = acc;
        }
    }
    output
}

fn reflect(idx: isize, size: usize) -> usize {
    // scipy `mode='reflect'` (grid-mirror): the edge pixel IS duplicated, so
    // index -1 -> 0 and index s -> s-1 (NOT the 'mirror' variant -1 -> 1).
    // Loops so radii larger than `size` reflect periodically, as scipy does.
    let s = size as isize;
    if s == 1 {
        return 0;
    }
    let mut i = idx;
    loop {
        if i < 0 {
            i = -i - 1;
        } else if i >= s {
            i = 2 * s - 1 - i;
        } else {
            return i as usize;
        }
    }
}

/// Rotate a 2D complex array by k×90° counter-clockwise.
fn rot90(arr: &Array2<Complex64>, k: i32) -> Array2<Complex64> {
    let k = k.rem_euclid(4);
    match k {
        0 => arr.clone(),
        1 => {
            let (h, w) = arr.dim();
            Array2::from_shape_fn((w, h), |(r, c)| arr[[h - 1 - c, r]])
        }
        2 => {
            let (h, w) = arr.dim();
            Array2::from_shape_fn((h, w), |(r, c)| arr[[h - 1 - r, w - 1 - c]])
        }
        3 => {
            let (h, w) = arr.dim();
            Array2::from_shape_fn((w, h), |(r, c)| arr[[c, w - 1 - r]])
        }
        _ => unreachable!(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // (Cutover, objective 6) The frozen `reflect_and_separable_match_scipy_large_radius`
    // golden + its reflect_wrap_*.bin fixtures + gen_reflect_wrap_golden.py were
    // DELETED: the live `separable_filter_matches_genuine_scipy_live` (golden_vfs)
    // exercises `separable_filter` — and thus the `reflect` index fold in its
    // large-radius periodic-wrap branch — against the genuine scipy.ndimage.correlate1d
    // live, so the library-primitive is computed each run (no frozen fixture to drift).

    /// **Live library-primitive oracle**: our `separable_filter` (which exercises
    /// the `reflect` index fold, including the large-radius periodic-wrap branch)
    /// vs the GENUINE `scipy.ndimage.correlate1d(mode='reflect')` applied along
    /// cols then rows, executed live in the uv-locked env. scipy is the oracle;
    /// the bridge only calls it. Kernel length 15 > n=4 forces the wrap branch.
    /// Gated behind `oracle_live`.
    #[cfg(feature = "oracle_live")]
    #[test]
    fn separable_filter_matches_genuine_scipy_live() {
        use crate::test_support::oracle;
        use agreement::{Eps, Tol};
        const N: usize = 4;
        const K: usize = 15;
        let input = Array2::from_shape_fn((N, N), |(r, c)| (r * N + c) as f64 * 0.5 - 3.0);
        // An asymmetric (non-palindromic) kernel so a wrong axis/origin would show.
        let kernel: Vec<f64> = (0..K).map(|i| (i as f64 - 7.0) * 0.1 + 0.3).collect();
        let kernel_row = Array2::from_shape_fn((1, K), |(_, i)| kernel[i]);

        let genuine = oracle::nat("scipy_correlate1d_separable", &[input.clone(), kernel_row], &[])
            .remove(0);
        let ours = separable_filter(&input, &kernel);

        // Zero-crossing values (the kernel has negative taps and the input crosses
        // zero) → ABSOLUTE ε bound. MEASURED max_abs ≈ 5.33e-15 ≈ 24·ε_f64 ⇒ K=64
        // (smallest pow2 with ≳2× cross-platform margin) — was a magic `1e-12`.
        let (of, gf): (Vec<f64>, Vec<f64>) =
            (ours.iter().copied().collect(), genuine.iter().copied().collect());
        let tol = Tol::abs(64, Eps::F64);
        let d = tol.check(&of, &gf);
        eprintln!("separable_filter vs GENUINE scipy.correlate1d (live): max_abs={:.3e} ({} px)", d.max_abs, d.n_finite);
        tol.assert("separable_filter vs scipy.correlate1d", &of, &gf);
    }

    #[test]
    fn test_gaussian_kernel_normalizes() {
        let k = gaussian_kernel_1d(2.0, 6);
        let sum: f64 = k.iter().sum();
        assert!((sum - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_phase_to_degrees() {
        let phase = Array2::from_elem((2, 2), PI);
        let result = phase_to_degrees(&phase, 100.0, 10.0);
        for &v in result.iter() {
            assert!((v - 60.0).abs() < 1e-10); // PI * 100/(2PI) + 10 = 60
        }
    }

    #[test]
    fn test_rot90_roundtrip() {
        let arr = Array2::from_shape_fn((3, 4), |(r, c)| Complex64::new(r as f64, c as f64));
        let back = rot90(&rot90(&rot90(&rot90(&arr, 1), 1), 1), 1);
        for (a, b) in arr.iter().zip(back.iter()) {
            assert!((a - b).norm() < 1e-10);
        }
    }

    #[test]
    fn test_rot90_dimensions() {
        let arr = Array2::from_shape_fn((3, 5), |(r, c)| Complex64::new(r as f64, c as f64));
        let r1 = rot90(&arr, 1);
        assert_eq!(r1.dim(), (5, 3));
        let r2 = rot90(&arr, 2);
        assert_eq!(r2.dim(), (3, 5));
    }

    // Property: eccentricity at the reference centre is exactly 0. d_alt =
    // d_azi = 0 → tan² + tan² / cos²(0) = 0 → atan(0) = 0. Bit-exact at
    // any (alt_c, azi_c) within the valid range.
    #[test]
    fn property_eccentricity_at_center_is_zero() {
        for &(alt_c, azi_c) in &[
            (0.0, 0.0),
            (10.0, -25.0),
            (-45.0, 30.0),
            (60.0, 60.0),
            (-30.0, -60.0),
        ] {
            let v = eccentricity_pixel_deg(alt_c, azi_c, alt_c, azi_c);
            assert!(
                v.abs() < 1e-12,
                "ecc at center ({}, {}) should be 0, got {}",
                alt_c,
                azi_c,
                v,
            );
        }
    }

    // Property: eccentricity is invariant to the sign of either delta. The
    // formula depends only on tan²(Δ) and (tan/cos)² — both even functions.
    // ecc(alt_c + δ, azi_c) == ecc(alt_c − δ, azi_c).
    #[test]
    fn property_eccentricity_symmetric_under_delta_sign_reversal() {
        let alt_c = 5.0_f64;
        let azi_c = -10.0_f64;
        for &delta in &[1.0_f64, 5.0, 10.0, 25.0] {
            // Vary altitude
            let e_plus = eccentricity_pixel_deg(alt_c + delta, azi_c, alt_c, azi_c);
            let e_minus = eccentricity_pixel_deg(alt_c - delta, azi_c, alt_c, azi_c);
            assert!(
                (e_plus - e_minus).abs() < 1e-10,
                "altitude delta sign: ecc(+{}) = {} vs ecc(-{}) = {}",
                delta,
                e_plus,
                delta,
                e_minus,
            );

            // Vary azimuth
            let e_plus = eccentricity_pixel_deg(alt_c, azi_c + delta, alt_c, azi_c);
            let e_minus = eccentricity_pixel_deg(alt_c, azi_c - delta, alt_c, azi_c);
            assert!(
                (e_plus - e_minus).abs() < 1e-10,
                "azimuth delta sign: ecc(+{}) = {} vs ecc(-{}) = {}",
                delta,
                e_plus,
                delta,
                e_minus,
            );
        }
    }

    /// `compute_polar_angle` matches the SNLC `getRadialEccMapX.m:56` formula
    /// `atan2(altΔ, aziΔ)·180/π` about the V1 center (here the mean of a
    /// single all-in-ROI area), and zeroes pixels outside any patch.
    #[test]
    fn polar_angle_matches_snlc_atan2_about_v1_center() {
        // 3×3: azi = column, alt = row ⇒ V1 (one area) center is the middle (1,1).
        let azi = Array2::from_shape_fn((3, 3), |(_r, c)| c as f64);
        let alt = Array2::from_shape_fn((3, 3), |(r, _c)| r as f64);
        let labels = Array2::from_elem((3, 3), 1i32);

        let pa = compute_polar_angle(&azi, &alt, &labels);
        let expect = |dr: f64, dc: f64| dr.atan2(dc) * 180.0 / PI; // (altΔ, aziΔ)

        assert!((pa[[1, 1]] - 0.0).abs() < 1e-10); // at center → atan2(0,0)=0
        assert!((pa[[0, 2]] - expect(-1.0, 1.0)).abs() < 1e-10); // -45°
        assert!((pa[[2, 0]] - expect(1.0, -1.0)).abs() < 1e-10); // 135°
        assert!((pa[[2, 2]] - expect(1.0, 1.0)).abs() < 1e-10); // 45°

        // Outside any patch → zeroed, like the other patch-scoped maps.
        let mut labels2 = labels.clone();
        labels2[[0, 0]] = 0;
        let pa2 = compute_polar_angle(&azi, &alt, &labels2);
        assert_eq!(pa2[[0, 0]], 0.0);
    }
}
