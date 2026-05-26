//! Analysis math that runs on the host: retinotopy orchestration,
//! post-segmentation derived maps, and a few utility transforms. Heavy
//! numerics (dF/F, DFT, SNR, smoothing, gradients, VFS) live in
//! `crate::compute::ops` and operate on `tch::Tensor`.

use ndarray::Array2;
use num_complex::Complex64;
use std::f64::consts::PI;

use crate::{AcquisitionProperties, AnalysisParams, ComplexMaps, RetinotopyMaps};
use crate::compute;

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
///   2. Upload the four complex maps to device as `Kind::ComplexFloat`.
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
pub fn compute_retinotopy(
    maps: &ComplexMaps,
    acquisition: &AcquisitionProperties,
    params: &AnalysisParams,
) -> crate::Result<RetinotopyMaps> {
    // Optional rotation on host — small, sometimes-applied, four arrays only.
    let (azi_fwd, azi_rev, alt_fwd, alt_rev) = if acquisition.rotation_k != 0 {
        (
            rot90(&maps.azi_fwd, acquisition.rotation_k),
            rot90(&maps.azi_rev, acquisition.rotation_k),
            rot90(&maps.alt_fwd, acquisition.rotation_k),
            rot90(&maps.alt_rev, acquisition.rotation_k),
        )
    } else {
        (
            maps.azi_fwd.clone(),
            maps.azi_rev.clone(),
            maps.alt_fwd.clone(),
            maps.alt_rev.clone(),
        )
    };

    // Upload the four complex maps to device as native Kind::ComplexFloat.
    let a_fwd = compute::array2_complex_to_complex_tensor(&azi_fwd);
    let a_rev = compute::array2_complex_to_complex_tensor(&azi_rev);
    let l_fwd = compute::array2_complex_to_complex_tensor(&alt_fwd);
    let l_rev = compute::array2_complex_to_complex_tensor(&alt_rev);

    // Per-orientation amplitude = mean of forward and reverse magnitudes
    // (SNLC `Gprocesskret_batch.m`: `mag_az = 0.5*(mag_fwd + mag_rev)`).
    let azi_amp_t = compute::position_amplitude(&a_fwd, &a_rev);
    let alt_amp_t = compute::position_amplitude(&l_fwd, &l_rev);

    // Stage 1 — cycle combine (fwd+rev → position phasor).
    let (azi_z, alt_z) = params.cycle_combine.apply(&a_fwd, &a_rev, &l_fwd, &l_rev);

    // Stage 2 — position phasor smoothing.
    let (azi_z_s, alt_z_s) = params.phase_smoothing.apply(
        &azi_z, &alt_z, &azi_amp_t, &alt_amp_t,
    );

    // Stage 3 — VFS computation (also returns the four phase gradients,
    // which magnification consumes).
    let (vfs_t, d_azi_dx, d_azi_dy, d_alt_dx, d_alt_dy) =
        params.vfs_computation.apply(&azi_z_s, &alt_z_s);

    let scale_azi = acquisition.azi_angular_range / (2.0 * PI);
    let scale_alt = acquisition.alt_angular_range / (2.0 * PI);
    let mag_t = compute::compute_magnification_jacobian(
        &d_azi_dx, &d_azi_dy, &d_alt_dx, &d_alt_dy, scale_azi, scale_alt,
    );

    // Single device→CPU download. Phase recovered for display via
    // `arg(z)` at this boundary — the only place atan2 of the smoothed
    // phasor is taken, and only for the display field.
    let vfs = compute::tensor_to_array2_f64(&vfs_t)?;
    let magnification_raw = compute::tensor_to_array2_f64(&mag_t)?;
    let azi_phase = compute::tensor_to_array2_f64(&azi_z_s.angle())?;
    let alt_phase = compute::tensor_to_array2_f64(&alt_z_s.angle())?;
    let azi_amplitude = compute::tensor_to_array2_f64(&azi_amp_t)?;
    let alt_amplitude = compute::tensor_to_array2_f64(&alt_amp_t)?;

    let azi_phase_degrees = phase_to_degrees(&azi_phase, acquisition.azi_angular_range, acquisition.offset_azi);
    let alt_phase_degrees = phase_to_degrees(&alt_phase, acquisition.alt_angular_range, acquisition.offset_alt);

    Ok(RetinotopyMaps {
        azi_phase,
        alt_phase,
        azi_phase_degrees,
        alt_phase_degrees,
        azi_amplitude,
        alt_amplitude,
        vfs,
        magnification_raw,
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
        if area_labels[[r, c]] > 0 { src[[r, c]] } else { 0.0 }
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
    let term = d_alt.tan().powi(2)
        + d_azi.tan().powi(2) / (cos_d_alt * cos_d_alt).max(1e-12);
    term.sqrt().atan() * 180.0 / PI
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

    // Find V1 center: center-of-mass of the largest area.
    let max_label = *area_labels.iter().max().unwrap_or(&0);
    let mut counts = vec![0usize; max_label as usize + 1];
    for &l in area_labels.iter() {
        if l > 0 { counts[l as usize] += 1; }
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
    let center_azi = if n > 0 { sum_azi / n as f64 } else { 0.0 };
    let center_alt = if n > 0 { sum_alt / n as f64 } else { 0.0 };

    Array2::from_shape_fn((h, w), |(r, c)| {
        if area_labels[[r, c]] == 0 { return 0.0; }
        eccentricity_pixel_deg(alt_deg[[r, c]], azi_deg[[r, c]], center_alt, center_azi)
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
            if area_labels[[r, c]] == 0 { continue; }
            let val = phase_deg[[r, c]];
            let bin = (val / interval_deg).floor();

            // Check right neighbor.
            if c + 1 < w && area_labels[[r, c + 1]] > 0 {
                let nbin = (phase_deg[[r, c + 1]] / interval_deg).floor();
                if bin != nbin { result[[r, c]] = true; }
            }
            // Check bottom neighbor.
            if r + 1 < h && area_labels[[r + 1, c]] > 0 {
                let nbin = (phase_deg[[r + 1, c]] / interval_deg).floor();
                if bin != nbin { result[[r, c]] = true; }
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
    let s = size as isize;
    if idx < 0 {
        (-idx).min(s - 1) as usize
    } else if idx >= s {
        (2 * s - 2 - idx).max(0) as usize
    } else {
        idx as usize
    }
}

/// Rotate a 2D complex array by k×90° counter-clockwise.
fn rot90(arr: &Array2<Complex64>, k: i32) -> Array2<Complex64> {
    let k = ((k % 4) + 4) % 4;
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

}
