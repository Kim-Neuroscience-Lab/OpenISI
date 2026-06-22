//! Per-pixel responsiveness / signal-quality metrics — one home for the family.
//!
//! Each metric answers "is there real stimulus-driven signal at this pixel?" in
//! a different way. Provenance is explicit because it differs per metric:
//!
//! | Metric              | Provenance                          | Validation              |
//! |---------------------|-------------------------------------|-------------------------|
//! | [`reliability`]     | Allen/Engel cross-cycle coherence   | formula (numpy)         |
//! | [`allen_power_snr`] | Allen `corticalmapping` power z-score | bit-exact vs oracle     |
//! | [`spectral_snr`]    | **OpenISI** ratio heuristic         | regression-lock (no oracle) |
//!
//! `reliability` works in the cross-cycle (phasor coherence) domain;
//! `allen_power_snr` and `spectral_snr` work in the temporal-spectrum domain of
//! the cycle-averaged movie. The Allen *mask* ([`allen_spectral_power_snr_mask`])
//! is `allen_power_snr ≥ sigma`.

use burn_tensor::{Tensor, TensorData};
use ndarray::{Array2, Array3};

use super::backend::Backend;
use super::complex::Complex2;

/// Per-pixel cross-cycle reliability `|Σ Z_k| / Σ |Z_k|` — Allen Brain
/// Observatory (Zhuang 2017) / Engel 1994 coherence in the cycle domain. Takes
/// the per-cycle [`Complex2`] maps directly (the accumulator holds them as a
/// `Vec`). `1.0` = every cycle's phasor agrees (repeatable); `0.0` = cancel
/// (noise). Requires `K ≥ 2`. Validated by `reliability_*` golden.
pub fn reliability(cycles: &[Complex2]) -> Tensor<Backend, 2> {
    // `CycleAccumulator::finalize_direction` gates on K ≥ 2 before calling, so
    // this is a development-time invariant check, not a production panic path.
    debug_assert!(cycles.len() >= 2, "reliability requires K ≥ 2 cycles");
    // num = |Σ_k Z_k|.
    let mut sum_re = cycles[0].real();
    let mut sum_im = cycles[0].imag();
    for c in &cycles[1..] {
        sum_re = sum_re + c.real();
        sum_im = sum_im + c.imag();
    }
    let num = Complex2::new(sum_re, sum_im).abs();
    // denom = Σ_k |Z_k|.
    let mut denom = cycles[0].abs();
    for c in &cycles[1..] {
        denom = denom + c.abs();
    }
    num / denom.clamp_min(1e-20)
}

/// Multi-bin spectral SNR — **OpenISI heuristic, NO external oracle** (no
/// reference codebase computes a ratio SNR like this; the closest oracle
/// metrics are [`allen_power_snr`] and [`reliability`]). Signal power at the
/// stimulus frequency divided by mean power over non-harmonic noise bins.
///
/// Bin-selection rule (the OpenISI part): skip harmonics 2–4 (noise bins start
/// at k=5), cap the noise list at 20 by even subsample, Nyquist cap, mean-power
/// denominator with a `1e-20` floor. Validated only by `spectral_snr_*` golden
/// (a regression-lock against this documented rule).
pub fn spectral_snr(dff: Tensor<Backend, 3>, timestamps: &[f64]) -> Tensor<Backend, 2> {
    let [n, h, w] = dff.dims();
    let device = dff.device();
    if timestamps.len() < 4 {
        return Tensor::<Backend, 2>::zeros([h, w], &device);
    }
    let n_ts = timestamps.len();
    let hw = h * w;
    let t_first = timestamps[0];
    let period = timestamps[n_ts - 1] - t_first;
    let freq_stim = 1.0 / period;
    let dt_mean = period / (n_ts - 1) as f64;
    let freq_nyquist = 0.5 / dt_mean;
    let max_bin = ((freq_nyquist / freq_stim).floor() as usize)
        .min(n_ts / 2)
        .max(2);

    let all_noise: Vec<usize> = (5..=max_bin).collect();
    let noise_bins: Vec<usize> = if all_noise.len() <= 20 {
        all_noise
    } else {
        let step = all_noise.len() as f64 / 20.0;
        (0..20)
            .map(|i| all_noise[(i as f64 * step) as usize])
            .collect()
    };
    let n_noise = noise_bins.len().max(1);

    let ts: Vec<f64> = timestamps.iter().map(|&t| t - t_first).collect();
    let dff_flat = dff.reshape([n, hw]); // [n, H·W]

    // Signal term at the stimulus frequency.
    let sig_kr: Vec<f32> = ts
        .iter()
        .map(|&t| (-2.0 * std::f64::consts::PI * freq_stim * t).cos() as f32)
        .collect();
    let sig_ki: Vec<f32> = ts
        .iter()
        .map(|&t| (-2.0 * std::f64::consts::PI * freq_stim * t).sin() as f32)
        .collect();
    let skr = Tensor::<Backend, 2>::from_data(TensorData::new(sig_kr, [1, n]), &device);
    let ski = Tensor::<Backend, 2>::from_data(TensorData::new(sig_ki, [1, n]), &device);
    let sig_re = skr.matmul(dff_flat.clone()); // [1, H·W]
    let sig_im = ski.matmul(dff_flat.clone());
    let signal_power = (sig_re.clone() * sig_re + sig_im.clone() * sig_im).reshape([h, w]);

    // Noise term: [n_noise, n] kernel matrix, two batched matmuls.
    let mut kr_mat = vec![0.0f32; n_noise * n];
    let mut ki_mat = vec![0.0f32; n_noise * n];
    for (bi, &k) in noise_bins.iter().enumerate() {
        let f = freq_stim * k as f64;
        for (ti, &t) in ts.iter().enumerate() {
            let ph = -2.0 * std::f64::consts::PI * f * t;
            kr_mat[bi * n + ti] = ph.cos() as f32;
            ki_mat[bi * n + ti] = ph.sin() as f32;
        }
    }
    let kr = Tensor::<Backend, 2>::from_data(TensorData::new(kr_mat, [n_noise, n]), &device);
    let ki = Tensor::<Backend, 2>::from_data(TensorData::new(ki_mat, [n_noise, n]), &device);
    let noise_re = kr.matmul(dff_flat.clone()); // [n_noise, H·W]
    let noise_im = ki.matmul(dff_flat);
    let noise_power_per_bin = noise_re.clone() * noise_re + noise_im.clone() * noise_im;
    // Mean over the n_noise axis → [1, H·W] → [H, W].
    let noise_power = noise_power_per_bin.mean_dim(0).reshape([h, w]);

    signal_power / noise_power.clamp_min(1e-20)
}

/// Device (Burn) form of [`allen_power_snr`] for emission as a per-direction
/// result leaf — the continuous z-score `(power@cycles − mean)/std` over the
/// temporal power spectrum of the on-device averaged movie `[n, H, W]`, with no
/// host round-trip. `power[k] = |DFT_k|·2/n` over all `n` bins; `mean`/`std` are
/// over the bin axis (ddof=0). Matches the host [`allen_power_snr`] within f32
/// tolerance (`allen_power_snr_device_matches_host`).
pub fn allen_power_snr_device(movie: Tensor<Backend, 3>, cycles: usize) -> Tensor<Backend, 2> {
    let [n, h, w] = movie.dims();
    let device = movie.device();
    let hw = h * w;
    let two_pi_over_n = 2.0 * std::f64::consts::PI / n as f64;
    // Full DFT kernel [n bins, n samples] — same `ang0 = -2π/n·k`, `·t` order as
    // the host version, so the power spectrum is the same up to f32.
    let mut kr = vec![0.0f32; n * n];
    let mut ki = vec![0.0f32; n * n];
    for k in 0..n {
        let ang0 = -two_pi_over_n * k as f64;
        for t in 0..n {
            let a = ang0 * t as f64;
            kr[k * n + t] = a.cos() as f32;
            ki[k * n + t] = a.sin() as f32;
        }
    }
    let flat = movie.reshape([n, hw]);
    let kr_t = Tensor::<Backend, 2>::from_data(TensorData::new(kr, [n, n]), &device);
    let ki_t = Tensor::<Backend, 2>::from_data(TensorData::new(ki, [n, n]), &device);
    let re = kr_t.matmul(flat.clone()); // [n, hw]
    let im = ki_t.matmul(flat);
    let scale = 2.0f32 / n as f32;
    let power = (re.clone() * re + im.clone() * im).sqrt().mul_scalar(scale); // [n, hw]
    let mean = power.clone().mean_dim(0); // [1, hw], keepdim
    let diff = power.clone() - mean.clone(); // broadcast [n,hw] − [1,hw]
    let std = (diff.clone() * diff).mean_dim(0).sqrt(); // [1, hw]
    let signal = power.slice([cycles..cycles + 1, 0..hw]); // [1, hw] power@F1
    ((signal - mean) / std).reshape([h, w])
}

/// Per-pixel `(power@F1, mean(power), std(power))` over the temporal spectrum of
/// `movie [n, H, W]`. `power[k] = |FFT(trace)[k]|·2/n`; `mean`/`std` are over all
/// `n` bins (ddof=0). The shared core of the Allen power-SNR metric + mask. The
/// DFT cos/sin table is precomputed once (pixel-independent), so the angle is
/// formed in the exact `ang0 = -2π/n·k`, `·t` order — bit-identical to the naive
/// `np.fft.fft` magnitude the oracle uses.
fn allen_power_spectrum_stats(movie: &Array3<f64>, cycles: usize) -> Array2<(f64, f64, f64)> {
    let (n, h, w) = movie.dim();
    let two_pi_over_n = 2.0 * std::f64::consts::PI / n as f64;
    let mut cos_tab = vec![0.0f64; n * n];
    let mut sin_tab = vec![0.0f64; n * n];
    for k in 0..n {
        let ang0 = -two_pi_over_n * k as f64;
        for t in 0..n {
            let a = ang0 * t as f64;
            cos_tab[k * n + t] = a.cos();
            sin_tab[k * n + t] = a.sin();
        }
    }

    let mut out = Array2::from_elem((h, w), (0.0, 0.0, 0.0));
    let mut power = vec![0.0f64; n];
    for r in 0..h {
        for c in 0..w {
            for (k, pk) in power.iter_mut().enumerate() {
                let base = k * n;
                let (mut re, mut im) = (0.0f64, 0.0f64);
                for t in 0..n {
                    let v = movie[[t, r, c]];
                    re += v * cos_tab[base + t];
                    im += v * sin_tab[base + t];
                }
                *pk = (re * re + im * im).sqrt() * 2.0 / n as f64;
            }
            let mean = power.iter().sum::<f64>() / n as f64;
            let var = power.iter().map(|p| (p - mean) * (p - mean)).sum::<f64>() / n as f64;
            let std = var.max(0.0).sqrt();
            out[[r, c]] = (power[cycles], mean, std);
        }
    }
    out
}

/// Allen `corticalmapping` spectral power-SNR as a **continuous** per-pixel
/// z-score: `(power@F1 − mean) / std` over the movie's temporal power spectrum
/// (`generatePhaseMap`'s power-branch statistic, expressed continuously). `movie`
/// is the cycle-averaged movie `[n, H, W]`; `cycles` is the F1 FFT bin. Higher =
/// more responsive. `std → 0` pixels yield `±inf`/`NaN` (treated as non-finite by
/// the responsiveness masks). The validated [`allen_spectral_power_snr_mask`]
/// thresholds this at `sigma`.
pub fn allen_power_snr(movie: &Array3<f64>, cycles: usize) -> Array2<f64> {
    allen_power_spectrum_stats(movie, cycles).mapv(|(p, mean, std)| (p - mean) / std)
}

/// Allen `corticalmapping` power-SNR responsiveness **mask** — a faithful port of
/// `generatePhaseMap`'s power branch (`RetinotopicMapping.py` L169-185): keep a
/// pixel iff `power@F1 ≥ mean + sigma·std` (ddof=0). Bit-for-bit vs the verbatim
/// `corticalmapping` criterion (`allen_power_snr_mask_matches_corticalmapping`).
/// Kept in the exact `power ≥ mean + sigma·std` form (not derived from
/// [`allen_power_snr`]) so the `std = 0` boundary stays identical to the oracle.
pub fn allen_spectral_power_snr_mask(
    movie: &Array3<f64>,
    cycles: usize,
    sigma: f64,
) -> Array2<bool> {
    allen_power_spectrum_stats(movie, cycles).mapv(|(p, mean, std)| p >= mean + sigma * std)
}

#[cfg(test)]
mod golden {
    use super::*;

    /// Reference "keep finite `≥ threshold`" mask — the simple semantics a
    /// continuous metric (e.g. `allen_power_snr`) is expected to reduce to. Used
    /// here as a test oracle to cross-check the production
    /// `allen_spectral_power_snr_mask`. Not a production criterion: the real
    /// reliability / patch criteria (`segmentation::mod`) use their own
    /// strict-`>`, multi-map, cortex-gated logic.
    fn threshold_mask(metric: &Array2<f64>, threshold: f64) -> Array2<bool> {
        metric.mapv(|v| v.is_finite() && v >= threshold)
    }
    use agreement::{Eps, Tol};
    use crate::compute::backend::device;
    use crate::compute::conversions::tensor_to_array2_f64;
    use crate::test_support::{load_f32, load_f64, load_u8};
    use std::f64::consts::PI;

    fn injected_phase(p: usize, n_px: usize) -> f64 {
        -PI + 2.0 * PI * (p as f64) / (n_px as f64)
    }

    fn cycle_map(h: usize, w: usize, amp: f64, phase: impl Fn(usize) -> f64) -> Complex2 {
        let n_px = h * w;
        let mut re = vec![0f32; n_px];
        let mut im = vec![0f32; n_px];
        for (p, (r, i)) in re.iter_mut().zip(im.iter_mut()).enumerate() {
            let ph = phase(p);
            *r = (amp * ph.cos()) as f32;
            *i = (amp * ph.sin()) as f32;
        }
        let dev = device();
        Complex2::new(
            Tensor::<Backend, 2>::from_data(TensorData::new(re, [h, w]), &dev),
            Tensor::<Backend, 2>::from_data(TensorData::new(im, [h, w]), &dev),
        )
    }

    /// `reliability` is ~1 for repeatable cycles and ~0 for incoherent ones.
    #[test]
    fn reliability_is_one_for_repeatable_cycles_and_low_for_incoherent() {
        let (h, w) = (8usize, 8usize);
        let n_px = h * w;
        let coherent: Vec<Complex2> = (0..4)
            .map(|_| cycle_map(h, w, 1.0, |p| injected_phase(p, n_px)))
            .collect();
        let rel = tensor_to_array2_f64(reliability(&coherent)).expect("read rel");
        let mean_coh = rel.iter().sum::<f64>() / rel.len() as f64;
        assert!(mean_coh > 0.99, "coherent reliability should be ~1, got {mean_coh}");

        let incoherent: Vec<Complex2> = (0..4)
            .map(|k| {
                let rot = k as f64 * std::f64::consts::FRAC_PI_2;
                cycle_map(h, w, 1.0, move |p| injected_phase(p, n_px) + rot)
            })
            .collect();
        let rel2 = tensor_to_array2_f64(reliability(&incoherent)).expect("read rel");
        let mean_inc = rel2.iter().sum::<f64>() / rel2.len() as f64;
        assert!(mean_inc < 0.2, "incoherent reliability should be ~0, got {mean_inc}");
    }

    /// `spectral_snr` vs a verbatim numpy transcription of its documented multi-bin
    /// rule (no external oracle): noise bins start at k=5, cap 20 by even
    /// subsample, Nyquist cap, mean-power 1e-20 floor. `small` (n=30) = use-all
    /// branch, `large` (n=120) = subsample branch. Fixtures from `gen_snr_golden.py`.
    #[test]
    fn spectral_snr_matches_documented_bin_rule() {
        const H: usize = 6;
        const W: usize = 8;
        fn run(name: &str, n: usize, dff_b: &[u8], ts_b: &[u8], out_b: &[u8]) {
            let dff_f32 = load_f32(dff_b);
            let ts = load_f64(ts_b);
            let exp = load_f64(out_b);
            let dev = device();
            let dff = Tensor::<Backend, 3>::from_data(TensorData::new(dff_f32, [n, H, W]), &dev);
            let got = tensor_to_array2_f64(spectral_snr(dff, &ts)).expect("snr to array");
            // f32 SNR ratio vs numpy f64; observed ≤ 1.0e-6 ≈ 8.5·ε_f32 → K=16
            // relative (was a magic 1e-3).
            Tol::rel(16, Eps::F32, 16).assert(
                &format!("spectral_snr {name}"),
                got.as_slice().expect("contiguous"),
                &exp,
            );
        }
        run(
            "small",
            30,
            include_bytes!("../../tests/golden/fixtures/snr_small_dff.npy"),
            include_bytes!("../../tests/golden/fixtures/snr_small_ts.npy"),
            include_bytes!("../../tests/golden/fixtures/snr_small_out.npy"),
        );
        run(
            "large",
            120,
            include_bytes!("../../tests/golden/fixtures/snr_large_dff.npy"),
            include_bytes!("../../tests/golden/fixtures/snr_large_ts.npy"),
            include_bytes!("../../tests/golden/fixtures/snr_large_out.npy"),
        );
    }

    /// **FORMULA-PIN** (honest label, NOT a live code oracle). The Allen
    /// power-responsiveness rule — `mask = |fft(movie)|@F1 ≥ mean+σ·std` (over all
    /// freqs) — from `corticalmapping/RetinotopicMapping.py::generatePhaseMap`
    /// (power branch). That `generatePhaseMap` exists **only in the deprecated py2
    /// `corticalmapping`** (absent from NeuroAnalysisTools 3.1.0), so it cannot run
    /// in the locked py3 env without a forbidden 2to3 shim — **irreducible gap.**
    /// So this pins the published formula computed via numpy primitives (`np.fft`
    /// is itself a live library oracle; the mean/σ threshold is Allen's rule),
    /// labelled as a formula-pin, not dressed as a live reference oracle. Fixtures
    /// from `gen_power_snr_golden.py` (n=24, cycles=4, sigma=1).
    #[test]
    fn allen_power_snr_mask_matches_corticalmapping() {
        const N: usize = 24;
        const H: usize = 16;
        const W: usize = 16;
        let movie_f32 = load_f32(include_bytes!("../../tests/golden/fixtures/powersnr_movie.npy"));
        let exp = load_u8(include_bytes!("../../tests/golden/fixtures/powersnr_mask.npy"));
        let movie =
            Array3::from_shape_fn((N, H, W), |(t, r, c)| movie_f32[t * H * W + r * W + c] as f64);
        let mask = allen_spectral_power_snr_mask(&movie, 4, 1.0);

        let mut diff = 0usize;
        for r in 0..H {
            for c in 0..W {
                if (mask[[r, c]] as u8) != exp[r * W + c] {
                    diff += 1;
                }
            }
        }
        eprintln!("Allen power-SNR mask vs corticalmapping: differing px = {diff}");
        assert_eq!(diff, 0, "allen_spectral_power_snr_mask diverges from corticalmapping");
    }

    /// The continuous `allen_power_snr` z-score reproduces the validated mask:
    /// thresholding it at `sigma` equals `allen_spectral_power_snr_mask`. This
    /// pins the continuous metric to the bit-exact oracle mask (away from the
    /// `std=0` boundary, which the fixture's noise rows avoid).
    #[test]
    fn allen_power_snr_thresholded_matches_mask() {
        const N: usize = 24;
        const H: usize = 16;
        const W: usize = 16;
        let movie_f32 = load_f32(include_bytes!("../../tests/golden/fixtures/powersnr_movie.npy"));
        let movie =
            Array3::from_shape_fn((N, H, W), |(t, r, c)| movie_f32[t * H * W + r * W + c] as f64);
        let z = allen_power_snr(&movie, 4);
        let from_z = threshold_mask(&z, 1.0);
        let mask = allen_spectral_power_snr_mask(&movie, 4, 1.0);
        assert_eq!(from_z, mask, "z ≥ sigma must equal the Allen power-SNR mask");
    }

    /// The on-device `allen_power_snr_device` reproduces the host
    /// `allen_power_snr` z-score within f32 tolerance (finite pixels). Pins the
    /// device emission path to the validated host metric.
    #[test]
    fn allen_power_snr_device_matches_host() {
        const N: usize = 24;
        const H: usize = 16;
        const W: usize = 16;
        let movie_f32 = load_f32(include_bytes!("../../tests/golden/fixtures/powersnr_movie.npy"));
        let movie =
            Array3::from_shape_fn((N, H, W), |(t, r, c)| movie_f32[t * H * W + r * W + c] as f64);
        let host = allen_power_snr(&movie, 4);

        let dev = device();
        let movie_t = Tensor::<Backend, 3>::from_data(
            TensorData::new(movie_f32.clone(), [N, H, W]),
            &dev,
        );
        let device_z = tensor_to_array2_f64(allen_power_snr_device(movie_t, 4)).expect("z");

        // f32 device path vs f64 host (a cross-backend compare); observed ≈
        // 1.78e-5 ≈ 149·ε_f32 → K=256 relative. NaN positions handled by the
        // comparator. (Was a magic 1e-3.)
        Tol::rel(256, Eps::F32, 256).assert(
            "allen_power_snr device vs host",
            device_z.as_slice().expect("contiguous"),
            host.as_slice().expect("contiguous"),
        );
    }

    /// The shared threshold semantics: keep finite `≥ threshold`, drop sub-threshold
    /// and non-finite.
    #[test]
    fn threshold_mask_keeps_finite_at_or_above_threshold() {
        let m = ndarray::array![[0.2, 0.5], [0.8, f64::NAN]];
        let mask = threshold_mask(&m, 0.5);
        assert_eq!(mask, ndarray::array![[false, true], [true, false]]);
    }
}
