//! The analysis tensor ops, on the Burn substrate.
//!
//! All ops are written generically over `Tensor<Backend, D>` where
//! `Backend` is the single alias in `super::backend` — switching the
//! backend (ndarray → CUDA → WGPU) changes no code here.
//!
//! The retinotopy ops (`wrap_principal`, `compute_vfs`, `real_gradients`,
//! `gaussian_smooth`, `position_amplitude`, `compute_magnification_jacobian`,
//! `phase_gradients`, `amp_weighted_complex_smooth`,
//! `position_phasor_delay_subtracted`) are wired into production via
//! `math::compute_retinotopy`, gated by `tests/equivalence.rs` against the
//! committed baseline. `dft_projection_at_freq` feeds the cycle accumulator /
//! `compute::projection`; the per-pixel responsiveness metrics (reliability,
//! spectral SNR, Allen power-SNR) live in `super::responsiveness`.

use burn_tensor::{Tensor, TensorData};

use super::backend::Backend;
use super::complex::Complex2;

/// Reflection padding of a 2D tensor's **width** axis (dim 1) by `r`
/// columns on each side, matching torch `reflection_pad2d([r, r, 0, 0])`.
///
/// torch reflection does NOT repeat the border column: for a row
/// `[a, b, c, d]` padded by `r=2`, the result is `[c, b, a, b, c, d, c, b]`
/// — the left pad reflects columns `1..=r` (reversed), the right pad
/// reflects columns `w-1-r..=w-2` (reversed).
fn reflect_pad_w(t: Tensor<Backend, 2>, r: usize) -> Tensor<Backend, 2> {
    if r == 0 {
        return t;
    }
    let [h, w] = t.dims();
    // scipy 'reflect' (edge duplicated): left pad = columns 0..r, reversed.
    let left = t.clone().slice([0..h, 0..r]).flip([1]);
    // right pad = columns w-r..w, reversed.
    let right = t.clone().slice([0..h, w - r..w]).flip([1]);
    Tensor::cat(vec![left, t, right], 1)
}

/// Reflection padding of a 2D tensor's **height** axis (dim 0) by `r` rows on
/// each side, scipy `mode='reflect'` (edge pixel duplicated) to match the
/// canonical gaussian convention — NOT torch `reflection_pad2d`, which is the
/// edge-not-duplicated 'mirror' variant.
fn reflect_pad_h(t: Tensor<Backend, 2>, r: usize) -> Tensor<Backend, 2> {
    if r == 0 {
        return t;
    }
    let [h, w] = t.dims();
    let top = t.clone().slice([0..r, 0..w]).flip([0]);
    let bottom = t.clone().slice([h - r..h, 0..w]).flip([0]);
    Tensor::cat(vec![top, t, bottom], 0)
}

/// Normalized 1D Gaussian kernel, host-computed in `f32`: weights
/// `exp(−x²/2σ²)` for `x ∈ [−radius, radius]`, normalized to sum 1.
fn gaussian_kernel_1d(sigma: f64, radius: usize) -> Vec<f32> {
    let inv = -0.5 / (sigma * sigma);
    let unnorm: Vec<f64> = (0..=2 * radius)
        .map(|i| {
            let x = i as f64 - radius as f64;
            (x * x * inv).exp()
        })
        .collect();
    let sum: f64 = unnorm.iter().sum();
    unnorm.iter().map(|&v| (v / sum) as f32).collect()
}

/// Separable 2D Gaussian blur with reflection padding — scipy-faithful.
///
/// Kernel radius `int(4σ + 0.5)` and `mode='reflect'`, matching
/// `scipy.ndimage.gaussian_filter` (the filter Allen `RetinotopicMapping.py`
/// uses); validated by the `tensor_gaussian_smooth_matches_scipy` golden and
/// shared bit-for-bit (modulo f32/f64) with [`super::gaussian_smooth_f64`].
/// Normalized. Implemented as two 1D passes (horizontal then vertical), each a
/// reflection-pad followed by a weighted sum of shifted slices — algebraically
/// equivalent to a
/// `conv2d` with a `[1, size]` / `[size, 1]` kernel, but expressed with
/// `slice` + `mul_scalar` + `add` so it needs no `conv2d` reflection-pad
/// support (which Burn's `conv2d` lacks — see the architecture doc audit).
/// The Gaussian kernel is symmetric, so convolution and cross-correlation
/// coincide.
pub fn gaussian_smooth(input: Tensor<Backend, 2>, sigma: f64) -> Tensor<Backend, 2> {
    if sigma <= 0.0 {
        return input;
    }
    // Canonical gaussian convention shared with `gaussian_smooth_f64`: scipy
    // `gaussian_filter` truncation `int(4·sigma + 0.5)` and `mode='reflect'`
    // (edge duplicated, via `reflect_pad_*`). f32 here, f64 there; same
    // convention, both golden-tested against scipy.
    let radius = (sigma * 4.0 + 0.5).floor() as usize;
    let kernel = gaussian_kernel_1d(sigma, radius);
    let [h, w] = input.dims();
    let device = input.device();

    // Horizontal pass: reflect-pad width by `radius`, then accumulate
    // kernel[k] · padded[:, k..k+w] for k in 0..size.
    let padded = reflect_pad_w(input, radius);
    let mut acc = Tensor::<Backend, 2>::zeros([h, w], &device);
    for (k, &weight) in kernel.iter().enumerate() {
        acc = acc + padded.clone().slice([0..h, k..k + w]).mul_scalar(weight);
    }

    // Vertical pass on the horizontal result.
    let padded = reflect_pad_h(acc, radius);
    let mut out = Tensor::<Backend, 2>::zeros([h, w], &device);
    for (k, &weight) in kernel.iter().enumerate() {
        out = out + padded.clone().slice([k..k + h, 0..w]).mul_scalar(weight);
    }
    out
}

/// Wrap a real-valued tensor to its principal angle in `(−π, π]` via
/// `atan2(sin x, cos x)`.
///
/// `atan2(sin, cos)` is the canonical branch-free phase-wrap: it maps any
/// real angle onto the principal interval without the `rem_euclid`
/// discontinuity, and is exact at the ±π boundary.
pub fn wrap_principal(x: Tensor<Backend, 2>) -> Tensor<Backend, 2> {
    x.clone().sin().atan2(x.cos())
}

/// Visual field sign map: `sin(θ_alt − θ_azi)` where `θ = atan2(dy, dx)`.
///
/// The sign of `sin(Δθ)` distinguishes mirror-image from non-mirror
/// retinotopic representations (Sereno 1994; Garrett 2014): adjacent
/// visual areas have opposite field sign, which is what the segmentation
/// border-detection keys on.
pub fn compute_vfs(
    d_azi_dx: Tensor<Backend, 2>,
    d_azi_dy: Tensor<Backend, 2>,
    d_alt_dx: Tensor<Backend, 2>,
    d_alt_dy: Tensor<Backend, 2>,
) -> Tensor<Backend, 2> {
    let theta_azi = d_azi_dy.atan2(d_azi_dx);
    let theta_alt = d_alt_dy.atan2(d_alt_dx);
    (theta_alt - theta_azi).sin()
}

/// Central-difference gradients of a real-valued 2D map, with first-order
/// one-sided differences at the edges. Returns `(d/dx, d/dy)` — dx along
/// the width axis (dim 1), dy along the height axis (dim 0).
///
/// Interior: centered `(f[i+1] − f[i−1]) / 2`. Left/top edge: forward
/// difference `f[1] − f[0]`. Right/bottom edge: backward difference
/// `f[n−1] − f[n−2]`.
pub fn real_gradients(map: Tensor<Backend, 2>) -> (Tensor<Backend, 2>, Tensor<Backend, 2>) {
    let [h, w] = map.dims();
    let device = map.device();

    // ── ∂/∂x (along width, dim 1) ──────────────────────────────────────
    let mut dx = Tensor::<Backend, 2>::zeros([h, w], &device);
    // Interior central difference → columns 1..w-1.
    let cd =
        (map.clone().slice([0..h, 2..w]) - map.clone().slice([0..h, 0..w - 2])).div_scalar(2.0);
    dx = dx.slice_assign([0..h, 1..w - 1], cd);
    // Left edge forward difference → column 0.
    let left = map.clone().slice([0..h, 1..2]) - map.clone().slice([0..h, 0..1]);
    dx = dx.slice_assign([0..h, 0..1], left);
    // Right edge backward difference → column w-1.
    let right = map.clone().slice([0..h, w - 1..w]) - map.clone().slice([0..h, w - 2..w - 1]);
    dx = dx.slice_assign([0..h, w - 1..w], right);

    // ── ∂/∂y (along height, dim 0) ─────────────────────────────────────
    let mut dy = Tensor::<Backend, 2>::zeros([h, w], &device);
    let cd =
        (map.clone().slice([2..h, 0..w]) - map.clone().slice([0..h - 2, 0..w])).div_scalar(2.0);
    dy = dy.slice_assign([1..h - 1, 0..w], cd);
    let top = map.clone().slice([1..2, 0..w]) - map.clone().slice([0..1, 0..w]);
    dy = dy.slice_assign([0..1, 0..w], top);
    let bottom = map.clone().slice([h - 1..h, 0..w]) - map.clone().slice([h - 2..h - 1, 0..w]);
    dy = dy.slice_assign([h - 1..h, 0..w], bottom);

    (dx, dy)
}

/// Per-orientation amplitude from forward and reverse F1 magnitudes:
/// `0.5 · (|fwd| + |rev|)`.
pub fn position_amplitude(fwd: &Complex2, rev: &Complex2) -> Tensor<Backend, 2> {
    (fwd.abs() + rev.abs()).mul_scalar(0.5)
}

/// Absolute Jacobian determinant of the visual-field map, scaled to
/// degree units. Real-valued throughout (no complex), but lives here with
/// the rest of the retinotopy ops.
pub fn compute_magnification_jacobian(
    d_azi_dx: Tensor<Backend, 2>,
    d_azi_dy: Tensor<Backend, 2>,
    d_alt_dx: Tensor<Backend, 2>,
    d_alt_dy: Tensor<Backend, 2>,
    scale_azi: f64,
    scale_alt: f64,
) -> Tensor<Backend, 2> {
    let det = d_azi_dx * d_alt_dy.clone() - d_alt_dx * d_azi_dy.clone();
    det.abs().mul_scalar((scale_azi * scale_alt) as f32)
}

/// Wrap-free phase gradients via the chain rule on the complex phasor.
///
/// For `z = c + i·s`, `∂φ/∂x = (c·∂s/∂x − s·∂c/∂x) / (c² + s²)`, with the
/// magnitude-squared denominator clamped to `≥ 1e-12` so background
/// pixels (near-zero magnitude after amp-weighted smoothing) don't blow
/// up the division.
pub fn phase_gradients(z: &Complex2) -> (Tensor<Backend, 2>, Tensor<Backend, 2>) {
    let c = z.real();
    let s = z.imag();
    let (dc_dx, dc_dy) = real_gradients(c.clone());
    let (ds_dx, ds_dy) = real_gradients(s.clone());
    let mag_sq = c.clone() * c.clone() + s.clone() * s.clone();
    let mag_sq_safe = mag_sq.clamp_min(1e-12);
    let dphi_dx = (ds_dx * c.clone() - dc_dx * s.clone()) / mag_sq_safe.clone();
    let dphi_dy = (ds_dy * c - dc_dy * s) / mag_sq_safe;
    (dphi_dx, dphi_dy)
}

/// Amplitude-weighted normalized convolution on a complex phasor:
/// `smooth(amp · z) / smooth(amp)`, component-wise.
pub fn amp_weighted_complex_smooth(z: &Complex2, amp: Tensor<Backend, 2>, sigma: f64) -> Complex2 {
    if sigma <= 0.0 {
        return z.clone();
    }
    let amp_re = amp.clone() * z.real();
    let amp_im = amp.clone() * z.imag();
    let num_re = gaussian_smooth(amp_re, sigma);
    let num_im = gaussian_smooth(amp_im, sigma);
    let den = gaussian_smooth(amp, sigma);
    let den_safe = den.clamp_min(1e-10);
    Complex2::new(num_re / den_safe.clone(), num_im / den_safe)
}

/// Allen `_getSignMap` position smoothing: an **unweighted scalar Gaussian** on
/// the phase, rebuilt as a unit phasor. Faithful to Allen
/// `gaussian_filter(positionMap, phaseMapFilterSigma)` (`RetinotopicMapping.py`
/// L1310) — the position map is a *linear* remap of the phase, and Gaussian
/// smoothing commutes with that linear remap, so smoothing the phase here yields
/// the same gradient directions (hence the same VFS) as Allen's. Unlike
/// [`amp_weighted_complex_smooth`], every pixel contributes equally regardless of
/// response amplitude.
///
/// The phase angle is taken in `(−π, π]`; Allen uses the `[0, 2π)` convention. The
/// two differ only in *where the 2π wrap sits*, and a retinotopic phase ramp does
/// not cross either wrap within responsive cortex — so the smoothed phase is
/// identical there; only the non-signal background differs.
pub fn position_gaussian_smooth(z: &Complex2, sigma: f64) -> Complex2 {
    if sigma <= 0.0 {
        return z.clone();
    }
    Complex2::from_phase(gaussian_smooth(z.angle(), sigma))
}

/// Marshel-Garrett delay subtraction, returning the position phase as a
/// unit complex phasor `exp(i·φ)`.
///
/// ```text
/// delay = angle(exp(i·ang_fwd) + exp(i·ang_rev))
/// delay = delay + (π/2)·(1 − sign(delay))          # force into (0, π]
/// φ     = 0.5·( wrap(ang_fwd − delay) − wrap(ang_rev − delay) )
/// ```
pub fn position_phasor_delay_subtracted(fwd: &Complex2, rev: &Complex2) -> Complex2 {
    let ang_fwd = fwd.angle();
    let ang_rev = rev.angle();
    // delay = angle of the sum of unit phasors at the two phases.
    let sin_sum = ang_fwd.clone().sin() + ang_rev.clone().sin();
    let cos_sum = ang_fwd.clone().cos() + ang_rev.clone().cos();
    let delay = sin_sum.atan2(cos_sum);
    // Force into (0, π]: add (π/2)·(1 − sign(delay)).
    let ones = delay.ones_like();
    let delay_corrected =
        delay.clone() + (ones - delay.sign()).mul_scalar((std::f64::consts::PI / 2.0) as f32);
    let corrected_fwd = wrap_principal(ang_fwd - delay_corrected.clone());
    let corrected_rev = wrap_principal(ang_rev - delay_corrected);
    let phi = (corrected_fwd - corrected_rev).mul_scalar(0.5);
    Complex2::from_phase(phi)
}

// =============================================================================
// DFT path ops (raw frames → complex maps). Validated end-to-end only on a
// raw-frames file (see module doc); unit-tested below.
// =============================================================================

/// Uniform-sample DFT projection at an explicit frequency.
///
/// `data` is `[n, H, W]` f32 (the cycle-averaged dF/F movie). Builds the
/// length-`n` complex DFT kernel at `freq` (host-computed
/// `arange`+`cos`/`sin`), then projects via two real matmuls
/// `[1, n] @ [n, H·W]` → `[H, W]` real and imaginary planes.
pub fn dft_projection_at_freq(data: Tensor<Backend, 3>, dt: f64, freq: f64) -> Complex2 {
    let [n, h, w] = data.dims();
    let two_pi_freq_dt = -2.0 * std::f64::consts::PI * freq * dt;
    let kr: Vec<f32> = (0..n)
        .map(|i| (i as f64 * two_pi_freq_dt).cos() as f32)
        .collect();
    let ki: Vec<f32> = (0..n)
        .map(|i| (i as f64 * two_pi_freq_dt).sin() as f32)
        .collect();
    project_complex_matmul(data, &kr, &ki, h, w)
}

/// `kernel · dff_flat` as two real matmuls → `Complex2 [H, W]`. Shared by
/// the DFT projection and the SNR signal term.
fn project_complex_matmul(
    dff: Tensor<Backend, 3>,
    kr: &[f32],
    ki: &[f32],
    h: usize,
    w: usize,
) -> Complex2 {
    let [n, _, _] = dff.dims();
    let device = dff.device();
    let dff_flat = dff.reshape([n, h * w]); // [n, H·W]
    let kr_row = Tensor::<Backend, 2>::from_data(TensorData::new(kr.to_vec(), [1, n]), &device);
    let ki_row = Tensor::<Backend, 2>::from_data(TensorData::new(ki.to_vec(), [1, n]), &device);
    let re = kr_row.matmul(dff_flat.clone()).reshape([h, w]);
    let im = ki_row.matmul(dff_flat).reshape([h, w]);
    Complex2::new(re, im)
}


#[cfg(test)]
mod tests {
    //! Synthetic phase-recovery validation of the beat-frequency core.
    //!
    //! These prove the DFT/coherence math itself — **independent of hardware,
    //! display vsync, or any biological signal** — by injecting a *known* signal
    //! and asserting it is recovered. This is the rigorous, CI-valid answer to
    //! "does the analysis correctly recover retinotopic phase and reliability?",
    //! which a real capture cannot answer without a live animal and a calibrated
    //! stimulus. The acquisition `--validate` path checks *plumbing* on real
    //! data; this checks *correctness* on known data.
    use super::*;
    use super::super::backend::device;
    use super::super::conversions::tensor_to_array2_f64;
    use std::f64::consts::PI;

    /// Wrap an angle difference into (−π, π].
    fn wrap(d: f64) -> f64 {
        d.sin().atan2(d.cos())
    }

    /// A per-pixel phase that ramps across the flattened image, deliberately
    /// covering the full [−π, π) circle — the "full phase coverage" a complete
    /// retinotopic sweep must produce.
    fn injected_phase(p: usize, n_px: usize) -> f64 {
        -PI + 2.0 * PI * (p as f64) / (n_px as f64)
    }

    #[test]
    fn dft_recovers_injected_phase_across_the_full_circle() {
        // 6 whole cycles of period 10 samples → the 2f cross term cancels
        // exactly, so recovery is limited only by f32 precision.
        let (n, h, w) = (60usize, 8usize, 8usize);
        let (dt, freq, amp) = (1.0f64, 0.1f64, 1.0f64);
        let n_px = h * w;

        let mut data = vec![0f32; n * n_px];
        for p in 0..n_px {
            let phi = injected_phase(p, n_px);
            for i in 0..n {
                let t = i as f64 * dt;
                data[i * n_px + p] = (amp * (2.0 * PI * freq * t + phi).cos()) as f32;
            }
        }
        let dev = device();
        let movie = Tensor::<Backend, 3>::from_data(TensorData::new(data, [n, h, w]), &dev);

        let z = dft_projection_at_freq(movie, dt, freq);
        let recovered = tensor_to_array2_f64(z.angle()).expect("read angle");

        let mut max_err = 0f64;
        let (mut min_phi, mut max_phi) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in 0..n_px {
            let (y, x) = (p / w, p % w);
            let inj = injected_phase(p, n_px);
            max_err = max_err.max(wrap(recovered[[y, x]] - inj).abs());
            min_phi = min_phi.min(inj);
            max_phi = max_phi.max(inj);
        }
        assert!(max_err < 1e-2, "max phase-recovery error {max_err} rad");
        assert!(
            max_phi - min_phi > 2.0 * PI - 0.3,
            "injected phases must cover the full circle; span was {} rad",
            max_phi - min_phi
        );
    }

}
