//! All analysis math — dF/F, DFT, smoothing, gradients, VFS, phase extraction.
//!
//! Pure functions operating on ndarray types. No I/O, no side effects.

use ndarray::{Array2, Array3, Axis, Zip};
use num_complex::Complex64;
use std::f64::consts::PI;

use crate::{AnalysisParams, ComplexMaps, RetinotopyMaps};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Compute retinotopy maps from four complex maps and parameters.
pub fn compute_retinotopy(maps: &ComplexMaps, params: &AnalysisParams) -> RetinotopyMaps {
    // Rotate if needed
    let (azi_fwd, azi_rev, alt_fwd, alt_rev) = if params.rotation_k != 0 {
        (
            rot90(&maps.azi_fwd, params.rotation_k),
            rot90(&maps.azi_rev, params.rotation_k),
            rot90(&maps.alt_fwd, params.rotation_k),
            rot90(&maps.alt_rev, params.rotation_k),
        )
    } else {
        (
            maps.azi_fwd.clone(),
            maps.azi_rev.clone(),
            maps.alt_fwd.clone(),
            maps.alt_rev.clone(),
        )
    };

    // Combine opposite directions: Z = fwd * conj(rev) → encodes 2φ
    let azi_combined = combine_directions(&azi_fwd, &azi_rev);
    let alt_combined = combine_directions(&alt_fwd, &alt_rev);

    // Gaussian smooth in complex plane
    let azi_smooth = gaussian_smooth_complex(&azi_combined, params.smoothing_sigma);
    let alt_smooth = gaussian_smooth_complex(&alt_combined, params.smoothing_sigma);

    // Phase gradients (amplitude-weighted)
    let (d_azi_dx, d_azi_dy) = phase_gradients(&azi_smooth);
    let (d_alt_dx, d_alt_dy) = phase_gradients(&alt_smooth);

    // VFS
    let vfs = compute_vfs(&d_azi_dx, &d_azi_dy, &d_alt_dx, &d_alt_dy);

    // Phase and amplitude
    let azi_phase = azi_smooth.mapv(|z| z.arg());
    let alt_phase = alt_smooth.mapv(|z| z.arg());
    let azi_amplitude = azi_smooth.mapv(|z| z.norm());
    let alt_amplitude = alt_smooth.mapv(|z| z.norm());

    // Convert to visual field degrees
    let azi_phase_degrees = phase_to_degrees(&azi_phase, params.azi_angular_range, params.offset_azi);
    let alt_phase_degrees = phase_to_degrees(&alt_phase, params.alt_angular_range, params.offset_alt);

    RetinotopyMaps {
        azi_phase,
        alt_phase,
        azi_phase_degrees,
        alt_phase_degrees,
        azi_amplitude,
        alt_amplitude,
        vfs,
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Derived maps (computed from retinotopy + segmentation)
// ---------------------------------------------------------------------------

/// VFS masked to segmentation regions. Pixels outside patches → 0.
pub fn compute_vfs_thresholded(vfs: &Array2<f64>, area_labels: &Array2<i32>) -> Array2<f64> {
    let (h, w) = vfs.dim();
    Array2::from_shape_fn((h, w), |(r, c)| {
        if area_labels[[r, c]] > 0 { vfs[[r, c]] } else { 0.0 }
    })
}

/// Eccentricity map: angular distance from V1 center (degrees).
/// V1 = largest area. Pixels outside patches → 0.
/// Formula from Juavinett: atan(sqrt(tan(az)² + tan(alt)²/cos(az)²)) × 180/π
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

    // Center-of-mass of V1 in visual field coordinates.
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

    // Compute eccentricity per pixel.
    Array2::from_shape_fn((h, w), |(r, c)| {
        if area_labels[[r, c]] == 0 { return 0.0; }
        let az = (azi_deg[[r, c]] - center_azi) * PI / 180.0;
        let alt = (alt_deg[[r, c]] - center_alt) * PI / 180.0;
        let cos_az = az.cos();
        let ecc = (az.tan().powi(2) + alt.tan().powi(2) / (cos_az * cos_az).max(1e-10))
            .sqrt()
            .atan();
        ecc * 180.0 / PI
    })
}

/// Magnification factor: |Jacobian determinant| of the retinotopic mapping.
/// det = |∂azi/∂x · ∂alt/∂y - ∂alt/∂x · ∂azi/∂y|
pub fn compute_magnification(
    azi_deg: &Array2<f64>,
    alt_deg: &Array2<f64>,
) -> Array2<f64> {
    let (h, w) = azi_deg.dim();
    let mut result = Array2::zeros((h, w));

    for r in 1..h.saturating_sub(1) {
        for c in 1..w.saturating_sub(1) {
            let dazi_dx = (azi_deg[[r, c + 1]] - azi_deg[[r, c.saturating_sub(1)]]) / 2.0;
            let dazi_dy = (azi_deg[[r + 1, c]] - azi_deg[[r.saturating_sub(1), c]]) / 2.0;
            let dalt_dx = (alt_deg[[r, c + 1]] - alt_deg[[r, c.saturating_sub(1)]]) / 2.0;
            let dalt_dy = (alt_deg[[r + 1, c]] - alt_deg[[r.saturating_sub(1), c]]) / 2.0;
            result[[r, c]] = (dazi_dx * dalt_dy - dalt_dx * dazi_dy).abs();
        }
    }
    result
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

// ---------------------------------------------------------------------------
// Raw frame processing (Phase One in Aaron's code)
// ---------------------------------------------------------------------------

/// Compute dF/F in-place: (frame - mean) / (mean + eps) for each pixel over time.
pub fn delta_f_over_f(frames: &mut Array3<f32>, eps: f32) {
    let (t, h, w) = frames.dim();
    let t_f32 = t as f32;

    // Temporal mean per pixel
    let mut mean = Array2::<f32>::zeros((h, w));
    for frame in frames.axis_iter(Axis(0)) {
        mean += &frame;
    }
    mean.mapv_inplace(|v| v / t_f32);

    // Normalize in-place
    for mut frame in frames.axis_iter_mut(Axis(0)) {
        for (px, &m) in frame.iter_mut().zip(mean.iter()) {
            *px = (*px - m) / (m + eps);
        }
    }
}

/// Single-frequency DFT projection: dot product of dF/F time series with complex kernel.
///
/// kernel[t] = exp(±2πi·f·(t - t₀)) where f = 1/period, sign depends on direction.
/// Returns complex map (H, W).
///
/// Optimized: iterates time as outer loop for sequential memory access,
/// accumulates into flat real/imag buffers to avoid Complex64 overhead per pixel.
pub fn dft_projection(
    frames: &Array3<f32>,
    timestamps: &[f64],
    is_forward: bool,
) -> Array2<Complex64> {
    let (t, h, w) = frames.dim();
    assert_eq!(t, timestamps.len());

    let t_first = timestamps[0];
    let t_last = timestamps[t - 1];
    let period = t_last - t_first;
    let freq = 1.0 / period;
    let sign = if is_forward { -1.0 } else { 1.0 };

    // Precompute kernel as separate real/imag arrays.
    let kernel_re: Vec<f64> = timestamps.iter()
        .map(|&ts| (sign * 2.0 * PI * freq * (ts - t_first)).cos())
        .collect();
    let kernel_im: Vec<f64> = timestamps.iter()
        .map(|&ts| (sign * 2.0 * PI * freq * (ts - t_first)).sin())
        .collect();

    let n_pixels = h * w;
    let mut acc_re = vec![0.0f64; n_pixels];
    let mut acc_im = vec![0.0f64; n_pixels];

    // Time as outer loop — each frame's pixels are contiguous in memory.
    for ti in 0..t {
        let kr = kernel_re[ti];
        let ki = kernel_im[ti];
        let frame_slice = frames.slice(ndarray::s![ti, .., ..]);
        let frame_data = frame_slice.as_slice().unwrap_or_else(|| {
            // Fallback for non-contiguous — shouldn't happen with standard layout
            &[]
        });
        if frame_data.len() == n_pixels {
            for px in 0..n_pixels {
                let v = frame_data[px] as f64;
                acc_re[px] += kr * v;
                acc_im[px] += ki * v;
            }
        } else {
            // Non-contiguous fallback
            for r in 0..h {
                for c in 0..w {
                    let v = frames[[ti, r, c]] as f64;
                    let px = r * w + c;
                    acc_re[px] += kr * v;
                    acc_im[px] += ki * v;
                }
            }
        }
    }

    // Convert to Array2<Complex64>.
    Array2::from_shape_fn((h, w), |(r, c)| {
        let px = r * w + c;
        Complex64::new(acc_re[px], acc_im[px])
    })
}

/// Compute signal-to-noise ratio per pixel.
///
/// SNR = |DFT at stimulus frequency|² / mean(|DFT at other frequencies|²).
/// Evaluates DFT at the stimulus frequency and a set of non-harmonic noise
/// frequencies. Uses cache-friendly time-outer iteration.
pub fn compute_snr_map(
    frames: &Array3<f32>,
    timestamps: &[f64],
) -> Array2<f64> {
    let (t, h, w) = frames.dim();
    assert_eq!(t, timestamps.len());
    if t < 4 {
        return Array2::zeros((h, w));
    }

    let n_pixels = h * w;
    let t_first = timestamps[0];
    let t_last = timestamps[t - 1];
    let period = t_last - t_first;
    let freq_stim = 1.0 / period;
    let dt_mean = period / (t - 1) as f64;
    let freq_nyquist = 0.5 / dt_mean;
    let max_bin = (freq_nyquist / freq_stim).floor() as usize;
    let max_bin = max_bin.min(t / 2).max(2);

    // Select noise frequency bins — skip harmonics (2, 3, 4).
    // Cap at 20 evenly-spaced bins to keep computation tractable for large datasets.
    let all_noise_bins: Vec<usize> = (5..=max_bin).collect();
    let noise_bins: Vec<usize> = if all_noise_bins.len() <= 20 {
        all_noise_bins
    } else {
        let step = all_noise_bins.len() as f64 / 20.0;
        (0..20).map(|i| all_noise_bins[(i as f64 * step) as usize]).collect()
    };
    let n_noise = noise_bins.len().max(1);

    // Compute signal DFT using cache-friendly accumulation.
    let mut sig_re = vec![0.0f64; n_pixels];
    let mut sig_im = vec![0.0f64; n_pixels];
    for ti in 0..t {
        let angle = -2.0 * PI * freq_stim * (timestamps[ti] - t_first);
        let kr = angle.cos();
        let ki = angle.sin();
        let frame_slice = frames.slice(ndarray::s![ti, .., ..]);
        if let Some(data) = frame_slice.as_slice() {
            for px in 0..n_pixels {
                let v = data[px] as f64;
                sig_re[px] += kr * v;
                sig_im[px] += ki * v;
            }
        }
    }

    // Compute noise power: accumulate DFT power across noise bins.
    // Instead of storing per-bin complex values, accumulate power directly.
    let mut noise_power = vec![0.0f64; n_pixels];

    for &k in &noise_bins {
        let freq = freq_stim * k as f64;
        let mut nr = vec![0.0f64; n_pixels];
        let mut ni = vec![0.0f64; n_pixels];

        for ti in 0..t {
            let angle = -2.0 * PI * freq * (timestamps[ti] - t_first);
            let kr = angle.cos();
            let ki = angle.sin();
            let frame_slice = frames.slice(ndarray::s![ti, .., ..]);
            if let Some(data) = frame_slice.as_slice() {
                for px in 0..n_pixels {
                    let v = data[px] as f64;
                    nr[px] += kr * v;
                    ni[px] += ki * v;
                }
            }
        }

        // Accumulate power for this bin.
        for px in 0..n_pixels {
            noise_power[px] += nr[px] * nr[px] + ni[px] * ni[px];
        }
    }

    // Compute SNR per pixel.
    Array2::from_shape_fn((h, w), |(r, c)| {
        let px = r * w + c;
        let sp = sig_re[px] * sig_re[px] + sig_im[px] * sig_im[px];
        let np = noise_power[px] / n_noise as f64;
        if np > 1e-20 { sp / np } else { 0.0 }
    })
}

// ---------------------------------------------------------------------------
// Retinotopy computation
// ---------------------------------------------------------------------------

/// Combine forward and reverse maps: Z = fwd * conj(rev).
/// Encodes 2φ where φ is the true retinotopic phase.
fn combine_directions(fwd: &Array2<Complex64>, rev: &Array2<Complex64>) -> Array2<Complex64> {
    let mut result = Array2::zeros(fwd.raw_dim());
    Zip::from(&mut result)
        .and(fwd)
        .and(rev)
        .for_each(|r, &f, &rv| *r = f * rv.conj());
    result
}

/// Gaussian smooth a complex map (smooth real and imaginary parts separately).
fn gaussian_smooth_complex(map: &Array2<Complex64>, sigma: f64) -> Array2<Complex64> {
    if sigma <= 0.0 {
        return map.clone();
    }

    let (h, w) = map.dim();
    let real = map.mapv(|z| z.re);
    let imag = map.mapv(|z| z.im);

    let radius = (sigma * 3.0).ceil() as usize;
    let kernel = gaussian_kernel_1d(sigma, radius);

    let real_smooth = separable_filter(&real, &kernel);
    let imag_smooth = separable_filter(&imag, &kernel);

    let mut result = Array2::zeros((h, w));
    Zip::from(&mut result)
        .and(&real_smooth)
        .and(&imag_smooth)
        .for_each(|r, &re, &im| *r = Complex64::new(re, im));
    result
}

/// Amplitude-weighted phase gradients via complex differentiation.
///
/// dφ/dx = Im{ conj(Z) · ∂Z/∂x }
/// dφ/dy = Im{ conj(Z) · ∂Z/∂y }
fn phase_gradients(map: &Array2<Complex64>) -> (Array2<f64>, Array2<f64>) {
    let (h, w) = map.dim();

    // ∂Z/∂x — central differences along columns
    let mut dz_dx = Array2::<Complex64>::zeros((h, w));
    for row in 0..h {
        dz_dx[[row, 0]] = map[[row, 1]] - map[[row, 0]];
        for col in 1..w - 1 {
            dz_dx[[row, col]] = (map[[row, col + 1]] - map[[row, col - 1]]) * 0.5;
        }
        dz_dx[[row, w - 1]] = map[[row, w - 1]] - map[[row, w - 2]];
    }

    // ∂Z/∂y — central differences along rows
    let mut dz_dy = Array2::<Complex64>::zeros((h, w));
    for col in 0..w {
        dz_dy[[0, col]] = map[[1, col]] - map[[0, col]];
        for row in 1..h - 1 {
            dz_dy[[row, col]] = (map[[row + 1, col]] - map[[row - 1, col]]) * 0.5;
        }
        dz_dy[[h - 1, col]] = map[[h - 1, col]] - map[[h - 2, col]];
    }

    // Extract amplitude-weighted phase gradients
    let mut dphi_dx = Array2::zeros((h, w));
    let mut dphi_dy = Array2::zeros((h, w));
    Zip::from(&mut dphi_dx)
        .and(&mut dphi_dy)
        .and(map)
        .and(&dz_dx)
        .and(&dz_dy)
        .for_each(|dx, dy, &z, &dzx, &dzy| {
            let cj = z.conj();
            *dx = (cj * dzx).im;
            *dy = (cj * dzy).im;
        });

    (dphi_dx, dphi_dy)
}

/// VFS = sin(θ_alt - θ_azi) where θ = atan2(dy, dx) of the phase gradient.
fn compute_vfs(
    d_azi_dx: &Array2<f64>,
    d_azi_dy: &Array2<f64>,
    d_alt_dx: &Array2<f64>,
    d_alt_dy: &Array2<f64>,
) -> Array2<f64> {
    let mut vfs = Array2::zeros(d_azi_dx.raw_dim());
    Zip::from(&mut vfs)
        .and(d_azi_dx)
        .and(d_azi_dy)
        .and(d_alt_dx)
        .and(d_alt_dy)
        .for_each(|v, &hdx, &hdy, &vdx, &vdy| {
            let theta_h = hdy.atan2(hdx);
            let theta_v = vdy.atan2(vdx);
            *v = (theta_v - theta_h).sin();
        });
    vfs
}

/// Convert phase (radians) to visual field degrees.
fn phase_to_degrees(phase: &Array2<f64>, angular_range: f64, offset: f64) -> Array2<f64> {
    let scale = angular_range / (2.0 * PI);
    phase.mapv(|phi| phi * scale + offset)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Normalized 1D Gaussian kernel.
pub(crate) fn gaussian_kernel_1d(sigma: f64, radius: usize) -> Vec<f64> {
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

/// Separable 2D convolution (horizontal then vertical) with reflected boundaries.
pub(crate) fn separable_filter(input: &Array2<f64>, kernel: &[f64]) -> Array2<f64> {
    let (h, w) = input.dim();
    let radius = kernel.len() / 2;

    // Horizontal pass
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

    // Vertical pass
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

/// Reflect index at boundaries (mirror padding).
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
    fn test_combine_directions() {
        let fwd = Array2::from_elem((2, 2), Complex64::new(1.0, 1.0));
        let rev = Array2::from_elem((2, 2), Complex64::new(1.0, -1.0));
        let result = combine_directions(&fwd, &rev);
        // (1+i) * conj(1-i) = (1+i)(1+i) = 2i
        for &v in result.iter() {
            assert!((v.re).abs() < 1e-10);
            assert!((v.im - 2.0).abs() < 1e-10);
        }
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
    fn test_delta_f_over_f_uniform() {
        let mut frames = Array3::from_elem((10, 4, 4), 100.0f32);
        delta_f_over_f(&mut frames, 1e-6);
        for &v in frames.iter() {
            assert!(v.abs() < 1e-4);
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

    #[test]
    fn test_smoothing_preserves_constant() {
        let map = Array2::from_elem((10, 10), Complex64::new(3.0, -2.0));
        let smoothed = gaussian_smooth_complex(&map, 2.0);
        for &v in smoothed.iter() {
            assert!((v.re - 3.0).abs() < 1e-6);
            assert!((v.im - (-2.0)).abs() < 1e-6);
        }
    }

    #[test]
    fn test_vfs_orthogonal_gradients() {
        // If azi gradient points purely in x and alt gradient purely in y,
        // the angle difference is π/2, so VFS = sin(π/2) = 1.0
        let ones = Array2::from_elem((3, 3), 1.0);
        let zeros = Array2::from_elem((3, 3), 0.0);
        let vfs = compute_vfs(&ones, &zeros, &zeros, &ones);
        for &v in vfs.iter() {
            assert!((v - 1.0).abs() < 1e-10);
        }
    }
}
