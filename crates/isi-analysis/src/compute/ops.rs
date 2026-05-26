//! Tensor-based analysis operations.
//!
//! Runs on whichever device libtorch makes available (CUDA / MPS / CPU) —
//! see `device()`. All on-device computation is in `f32` (`Kind::Float`)
//! per `docs/ANALYSIS_COMPUTE.md` Principle 2. Kernel construction (DFT,
//! SNR noise bins, Gaussian) happens on-device using `Tensor::from_slice`
//! plus on-device `cos`/`sin`/`exp` — not as CPU `Vec<f64>` intermediates.
//!
//! Callers (`io.rs`, `math.rs`) work with `tch::Tensor` directly and use
//! `super::conversions` for the host↔device boundaries. There are no
//! ndarray-flavored wrappers in this file — they were removed once
//! the per-sweep streaming pipeline (io.rs) and tensor-driven retinotopy
//! (math.rs::compute_retinotopy) landed.

use std::f64::consts::PI;
use std::sync::{Mutex, OnceLock};
use tch::{Device, Kind, Tensor};

// =============================================================================
// Backend detection
// =============================================================================

static DEVICE: OnceLock<Device> = OnceLock::new();

/// Environment variable that forces a specific analysis device. Accepted values:
/// `cpu`, `cuda`, `mps`. Intended for cross-device regression validation, not
/// for end-user control. Setting it to a device that is not available on the
/// host is a hard error — no silent fallback.
const ENV_DEVICE_OVERRIDE: &str = "OPENISI_ANALYSIS_DEVICE";

/// Get the compute device for analysis. Resolved once on first call and cached
/// for the process. Priority when `OPENISI_ANALYSIS_DEVICE` is unset:
///
/// ```text
/// tch::Cuda::is_available()  → Device::Cuda(0)
/// tch::utils::has_mps()       → Device::Mps
/// else                        → Device::Cpu
/// ```
///
/// When `OPENISI_ANALYSIS_DEVICE` is set, that device is used if available.
/// User misconfiguration (e.g. forcing CUDA on a CPU-only host) prints a
/// clean error message to stderr and exits the process with code 1 —
/// matching the application-level `try_main` / `try_run` failure shape.
/// No panic, no stack trace.
///
/// For Result-based startup validation, call `try_init_device()` once
/// from your top-level error-propagation chain instead.
///
/// See `docs/ANALYSIS_COMPUTE.md`.
pub fn device() -> Device {
    *DEVICE.get_or_init(|| match resolve_device() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("openisi: {e}");
            std::process::exit(1);
        }
    })
}

/// Validate the device selection (env var override + tch availability)
/// and pre-initialize the cached `DEVICE` value. Call once at application
/// startup via `try_run` / `try_main` so user-misconfiguration errors
/// surface as a clean `AnalysisError::Compute` instead of a process exit
/// inside the first compute call.
pub fn try_init_device() -> crate::Result<Device> {
    if let Some(d) = DEVICE.get() {
        return Ok(*d);
    }
    let d = resolve_device()?;
    let _ = DEVICE.set(d);
    Ok(d)
}

fn resolve_device() -> crate::Result<Device> {
    if let Ok(raw) = std::env::var(ENV_DEVICE_OVERRIDE) {
        let name = raw.trim().to_lowercase();
        let dev = match name.as_str() {
            "cpu" => Device::Cpu,
            "cuda" => {
                if !tch::Cuda::is_available() {
                    return Err(crate::AnalysisError::Compute(format!(
                        "{ENV_DEVICE_OVERRIDE}=cuda but CUDA is not available on this host. \
                         Per the project's no-fallbacks principle, this is a hard error, not \
                         a silent demotion. Unset {ENV_DEVICE_OVERRIDE} to use auto-selection."
                    )));
                }
                Device::Cuda(0)
            }
            "mps" => {
                if !tch::utils::has_mps() {
                    return Err(crate::AnalysisError::Compute(format!(
                        "{ENV_DEVICE_OVERRIDE}=mps but MPS is not available on this host. \
                         MPS requires Apple Silicon and a libtorch build with MPS support. \
                         Unset {ENV_DEVICE_OVERRIDE} to use auto-selection."
                    )));
                }
                Device::Mps
            }
            other => return Err(crate::AnalysisError::Compute(format!(
                "{ENV_DEVICE_OVERRIDE}={other:?}: unrecognized device. \
                 Accepted values: cpu, cuda, mps."
            ))),
        };
        eprintln!("[compute] Using {} (forced via {ENV_DEVICE_OVERRIDE})", describe(dev));
        return Ok(dev);
    }

    let dev = if tch::Cuda::is_available() {
        Device::Cuda(0)
    } else if tch::utils::has_mps() {
        Device::Mps
    } else {
        Device::Cpu
    };
    eprintln!("[compute] Using {}", describe(dev));
    Ok(dev)
}

fn describe(dev: Device) -> String {
    // `resolve_device()` returns only Cuda / Mps / Cpu. Future variants
    // surface as `unknown device` rather than panicking — `describe`
    // produces display strings; an unrecognised variant is a missed
    // arm, not corrupt-data.
    match dev {
        Device::Cuda(i) => format!("CUDA device {i}"),
        Device::Mps => "Apple Metal (MPS)".into(),
        Device::Cpu => "CPU (libtorch)".into(),
        other => format!("unknown device {other:?}"),
    }
}

/// Human-readable backend identifier — suitable for display in the UI.
pub fn backend_info() -> String {
    describe(device())
}

/// Short, filename-safe device tag — `"cpu"`, `"cuda"`, or `"mps"`.
/// Used as a component in dev-figures run tags so different-device runs
/// don't overwrite each other. Falls back to `"unknown"` for future
/// tch::Device variants rather than panicking.
pub fn device_tag() -> &'static str {
    match device() {
        Device::Cuda(_) => "cuda",
        Device::Mps => "mps",
        Device::Cpu => "cpu",
        _ => "unknown",
    }
}

// =============================================================================
// Core operations — all f32, kernels constructed on-device
// =============================================================================

/// Uniform-sample DFT projection at an explicit frequency. Equivalent to
/// `np.fft.fft(data, axis=0)[bin]` when `freq · dt · N = bin` and the samples
/// are uniformly spaced by `dt` starting at `t = 0`.
///
/// This is the bin-exact variant used by the Allen-aligned pipeline: the
/// cycle-averaged movie has `N` uniformly-spaced frames over period `T = N·dt`,
/// and bin 1 sits at `freq = 1/T = 1/(N·dt)`. Pass `dt = T_cycle / N` and
/// `freq = 1 / T_cycle` to match `np.fft.fft(...)[1]` exactly.
///
/// Input:  `data` `[n, H, W]` f32, `dt` (seconds), `freq` (Hz).
/// Output: Tensor `[H, W]` `Kind::ComplexFloat` on device.
pub fn dft_projection_at_freq(data: &Tensor, dt: f64, freq: f64) -> Tensor {
    let n = data.size()[0];
    let two_pi_freq_dt = -2.0 * PI * freq * dt;
    let phase = Tensor::arange(n, (Kind::Float, device())) * two_pi_freq_dt;
    let kr_row = phase.cos().reshape([1, n]);
    let ki_row = phase.sin().reshape([1, n]);
    project_complex_matmul(data, &kr_row, &ki_row)
}

/// Compute `kernel · dff_flat` as two real matmuls and combine into a
/// `Kind::ComplexFloat` `[H, W]` result. Shared between DFT projection and the
/// SNR signal computation. Two real matmuls are strictly cheaper than one
/// complex matmul on a real `dff` (the imag-side multiplies would be against
/// zero), and both hit BLAS.
///
/// `kr_row`, `ki_row` are `[1, n]` f32 rows.
fn project_complex_matmul(dff: &Tensor, kr_row: &Tensor, ki_row: &Tensor) -> Tensor {
    let size = dff.size();
    let n_i64 = size[0];
    let h = size[1];
    let w = size[2];
    let dff_flat = dff.reshape([n_i64, h * w]);   // [n, H·W]
    let re = kr_row.matmul(&dff_flat).reshape([h, w]);
    let im = ki_row.matmul(&dff_flat).reshape([h, w]);
    Tensor::complex(&re, &im)
}

/// Multi-bin spectral SNR. Signal at the stimulus frequency vs. mean of noise
/// bins drawn from harmonics 5..max (capped at 20 bins, evenly subsampled if
/// more available). Identical bin-selection rule, harmonic skipping, Nyquist
/// cap, and division floor as the legacy code.
///
/// Both signal and noise computations are matmuls. The noise computation is a
/// single batched matmul `[n_noise, n] @ [n, H·W]` rather than a 4D broadcast.
///
/// Input:  `dff` `[n, H, W]` f32, timestamps.
/// Output: Tensor `[H, W]` f32 on device.
pub fn compute_snr(dff: &Tensor, timestamps: &[f64]) -> Tensor {
    let n = timestamps.len();
    if n < 4 {
        let size = dff.size();
        return Tensor::zeros([size[1], size[2]], (Kind::Float, device()));
    }

    let size = dff.size();
    let n_i64 = size[0];
    let h = size[1];
    let w = size[2];
    let hw = h * w;

    let t_first = timestamps[0];
    let period = timestamps[n - 1] - t_first;
    let freq_stim = 1.0 / period;
    let dt_mean = period / (n - 1) as f64;
    let freq_nyquist = 0.5 / dt_mean;
    let max_bin = ((freq_nyquist / freq_stim).floor() as usize).min(n / 2).max(2);

    // Noise-bin selection: skip harmonics 2-4, cap at 20 bins, even subsample.
    let all_noise: Vec<usize> = (5..=max_bin).collect();
    let noise_bins: Vec<usize> = if all_noise.len() <= 20 {
        all_noise
    } else {
        let step = all_noise.len() as f64 / 20.0;
        (0..20).map(|i| all_noise[(i as f64 * step) as usize]).collect()
    };
    let n_noise = noise_bins.len().max(1) as i64;

    // Shared timestamp tensor.
    let ts_f32: Vec<f32> = timestamps.iter().map(|&t| (t - t_first) as f32).collect();
    let ts = Tensor::from_slice(&ts_f32).to(device()); // [n]
    let dff_flat = dff.reshape([n_i64, hw]); // [n, H·W]

    // Signal: kernel rows × dff_flat via matmul.
    let sig_phase = &ts * (-2.0 * PI * freq_stim);
    let skr_row = sig_phase.cos().reshape([1, n_i64]);
    let ski_row = sig_phase.sin().reshape([1, n_i64]);
    let sig_re = skr_row.matmul(&dff_flat); // [1, H·W]
    let sig_im = ski_row.matmul(&dff_flat); // [1, H·W]
    let signal_power = (&sig_re * &sig_re + &sig_im * &sig_im).reshape([h, w]);

    // Noise: build a [n_noise, n] phase matrix on device, then one batched matmul.
    let freqs_f32: Vec<f32> = noise_bins.iter().map(|&k| (freq_stim * k as f64) as f32).collect();
    let freqs = Tensor::from_slice(&freqs_f32).to(device()).reshape([n_noise, 1]); // [n_noise, 1]
    let ts_row = ts.reshape([1, n_i64]); // [1, n]
    let noise_phase = &ts_row * &freqs * (-2.0 * PI); // [n_noise, n]
    let kr_mat = noise_phase.cos(); // [n_noise, n]
    let ki_mat = noise_phase.sin(); // [n_noise, n]

    // Two batched matmuls: [n_noise, n] @ [n, H·W] = [n_noise, H·W].
    let noise_re = kr_mat.matmul(&dff_flat);
    let noise_im = ki_mat.matmul(&dff_flat);
    let noise_power_per_bin = &noise_re * &noise_re + &noise_im * &noise_im; // [n_noise, H·W]
    let noise_power = noise_power_per_bin
        .mean_dim(0, false, Kind::Float)
        .reshape([h, w]);

    &signal_power / noise_power.clamp_min(1e-20_f64)
}

/// Per-pixel cross-cycle reliability — amplitude-weighted vector
/// coherence of per-cycle complex projections (Allen Brain Observatory,
/// Zhuang 2017; conceptually equivalent to Engel 1994 coherence in the
/// cycle domain rather than the frequency domain):
///
/// ```text
/// reliability(pixel) = | Σ_k Z_k(pixel) |  /  Σ_k |Z_k(pixel)|     ∈ [0, 1]
/// ```
///
/// `1.0` → every cycle's phasor at this pixel points the same direction
/// (perfectly repeatable retinotopic response). `0.0` → phasors cancel
/// (random phase across cycles, i.e. noise). Amplitude-weighted: a
/// low-amp cycle with noisy phase doesn't pollute a high-amp cycle.
///
/// Input:  `cycles` `[K, H, W]` `Kind::ComplexFloat` on device — the
///         stacked per-cycle complex projections at the stimulus
///         frequency. `K ≥ 2` required; with `K = 1` reliability is
///         trivially `1.0` (a single sample agrees with itself) and
///         carries no information, so the caller must error out.
/// Output: `[H, W]` f32 on device, values in `[0, 1]`.
///
/// Math note: `Tensor::abs` on a `Kind::ComplexFloat` tensor returns
/// the f32 magnitudes; `Tensor::sum_dim_intlist` on the K dim sums
/// the complex values component-wise.
pub fn compute_reliability(cycles: &Tensor) -> Tensor {
    let size = cycles.size();
    debug_assert_eq!(size.len(), 3, "expected [K, H, W] complex tensor");
    let k = size[0];
    let h = size[1];
    let w = size[2];
    debug_assert!(k >= 2, "reliability requires K ≥ 2 cycles");

    // |Σ Z_k| across the K axis.
    let sum_z = cycles.sum_dim_intlist(
        Some(vec![0_i64].as_slice()),
        /* keepdim */ false,
        Kind::ComplexFloat,
    );
    let num = sum_z.abs(); // f32 [H, W]

    // Σ |Z_k| across the K axis. abs() of a complex tensor yields f32.
    let abs_per_cycle = cycles.abs(); // f32 [K, H, W]
    let denom = abs_per_cycle.sum_dim_intlist(
        Some(vec![0_i64].as_slice()),
        /* keepdim */ false,
        Kind::Float,
    );

    // Bounded ratio. Floor on denominator: at a pixel with all-zero
    // amplitudes (impossible for f32 raw frames; possible in synthetic
    // tests), the reliability is undefined — emit 0 rather than NaN.
    let safe_denom = denom.clamp_min(1e-20_f64);
    (num / safe_denom).reshape([h, w])
}

/// Cached 1D Gaussian kernel, keyed by `(sigma_bits, radius)`. The same σ is
/// used to smooth azi and alt position maps back-to-back; without the cache
/// the kernel is built twice per analysis. Using the IEEE-754 bit pattern of
/// σ as the cache key avoids float equality concerns.
fn gaussian_kernel_1d_cached(sigma: f64, radius: i64) -> Tensor {
    static CACHE: OnceLock<Mutex<Vec<(u64, i64, Tensor)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(Vec::new()));
    let key = sigma.to_bits();
    let mut guard = cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((_, _, k)) = guard.iter().find(|(s, r, _)| *s == key && *r == radius) {
        return k.shallow_clone();
    }
    let x = Tensor::arange_start(-radius, radius + 1, (Kind::Float, device()));
    let unnorm = (&x * &x * (-0.5 / (sigma * sigma))).exp();
    let kernel = &unnorm / unnorm.sum(Kind::Float);
    guard.push((key, radius, kernel.shallow_clone()));
    kernel
}

/// Separable 2D Gaussian blur with reflection padding. Kernel radius
/// `ceil(3σ)`, normalized. The 1D kernel is built on-device via `Tensor::arange`
/// + `exp` and cached per σ (azi and alt share a kernel). Same kernel formula
/// and padding mode as the legacy code.
///
/// Input:  Tensor `[H, W]` f32 on device.
/// Output: Tensor `[H, W]` f32 on device.
pub fn gaussian_smooth(input: &Tensor, sigma: f64) -> Tensor {
    if sigma <= 0.0 { return input.shallow_clone(); }
    let radius = (sigma * 3.0).ceil() as i64;
    let size = 2 * radius + 1;

    let kernel = gaussian_kernel_1d_cached(sigma, radius);

    let size_vec = input.size();
    let h = size_vec[0];
    let w = size_vec[1];
    let x4 = input.reshape([1, 1, h, w]);

    // Horizontal pass.
    let kh = kernel.reshape([1, 1, 1, size]);
    let padded = x4.reflection_pad2d([radius, radius, 0, 0]);
    let after_h = padded.conv2d::<Tensor>(&kh, None, [1, 1], [0, 0], [1, 1], 1);

    // Vertical pass.
    let kv = kernel.reshape([1, 1, size, 1]);
    let padded = after_h.reflection_pad2d([0, 0, radius, radius]);
    let after_v = padded.conv2d::<Tensor>(&kv, None, [1, 1], [0, 0], [1, 1], 1);

    after_v.reshape([h, w])
}

/// Amplitude-weighted normalized convolution on a **complex phasor**.
///
/// Input `z` is a `Kind::ComplexFloat` `[H, W]` tensor representing the
/// position phasor `exp(i·φ)`. The smoothing operates on the real and
/// imaginary parts separately (each is a continuous, non-wrapping
/// real-valued field) so phase wraps in the underlying φ do not produce
/// the gradient artifacts that smoothing the wrapped φ directly would.
///
/// `z_smoothed = smooth(amp · z) / smooth(amp)` — the same normalized-
/// convolution form used for real maps, just applied component-wise to
/// the complex phasor. Background pixels with `amp ≈ 0` contribute zero
/// to both numerator and denominator and become near-zero complex values
/// (small magnitude, undefined phase) which downstream code handles via
/// the `|z|² ≥ 1e-12` clamp in `phase_gradients`.
///
/// Output is the canonical smoothed representation of the position
/// phase — used throughout the rest of `compute_retinotopy`. The
/// wrapped real phase is recovered (via `.angle()`) only once, at the
/// very end, when populating the display field of `RetinotopyMaps`.
pub fn amp_weighted_complex_smooth(z: &Tensor, amp: &Tensor, sigma: f64) -> Tensor {
    if sigma <= 0.0 { return z.shallow_clone(); }
    let amp_re = amp * z.real();
    let amp_im = amp * z.imag();
    let num_re = gaussian_smooth(&amp_re, sigma);
    let num_im = gaussian_smooth(&amp_im, sigma);
    let den = gaussian_smooth(amp, sigma);
    let den_safe = den.clamp(1e-10, f64::INFINITY);
    let re_out = num_re / &den_safe;
    let im_out = num_im / &den_safe;
    Tensor::complex(&re_out, &im_out)
}

/// Wrap a real-valued angle tensor into the principal interval `(-π, π]`.
/// Implemented as `atan2(sin(x), cos(x))` — handles any input range.
pub fn wrap_principal(x: &Tensor) -> Tensor {
    x.sin().atan2(&x.cos())
}

/// Marshel-Garrett delay subtraction, returning the position phase as a
/// **complex phasor** `exp(i·φ)` (`Kind::ComplexFloat` `[H, W]`).
///
/// Reference: SNLC `ISI/ISI_Processing/Gprocesskret.m`; Marshel et al. 2011.
///
/// ```matlab
/// delay = angle(exp(i·ang_fwd) + exp(i·ang_rev));
/// delay = delay + π/2 · (1 − sign(delay));   % force into (0, π]
/// kmap  = 0.5 · ( wrap(ang_fwd − delay) − wrap(ang_rev − delay) );
/// ```
///
/// Internally computes the wrapped real φ via the recipe above, then
/// immediately converts to the complex phasor representation. The
/// wrapped real φ never escapes this function — it's a transient
/// intermediate, not a value carried into downstream computation. This
/// avoids the wrap-induced gradient artifacts that would result from
/// passing the wrapped scalar phase through smoothing and gradient
/// steps; everything downstream operates on the continuous-valued real
/// and imaginary components of the complex phasor.
///
/// The `Z_fwd · conj(Z_rev)` shortcut algebraically gives `2·position`
/// but folds at `±π`; the phase-domain form recovers true position over
/// the full range without folding.
pub fn position_phasor_delay_subtracted(fwd: &Tensor, rev: &Tensor) -> Tensor {
    let ang_fwd = fwd.angle();
    let ang_rev = rev.angle();
    // delay = angle of the sum of unit phasors at the two phases.
    let sin_sum = ang_fwd.sin() + ang_rev.sin();
    let cos_sum = ang_fwd.cos() + ang_rev.cos();
    let delay = sin_sum.atan2(&cos_sum);
    // Force delay into (0, π]: shift by π/2 if delay == 0, by π if delay < 0.
    let delay_corrected = &delay + (Tensor::ones_like(&delay) - delay.sign()) * (PI / 2.0);
    let corrected_fwd = wrap_principal(&(&ang_fwd - &delay_corrected));
    let corrected_rev = wrap_principal(&(&ang_rev - &delay_corrected));
    let phi = (&corrected_fwd - &corrected_rev) * 0.5;
    // Convert immediately to the complex phasor representation. φ itself
    // is not returned — downstream uses `(cos, sin)` via the complex
    // tensor's real/imag parts.
    Tensor::complex(&phi.cos(), &phi.sin())
}

/// Wrap-free phase gradients via the chain rule on the complex phasor.
///
/// If `z = c + i·s` where `c = cos(φ)` and `s = sin(φ)`, then
/// `∂φ/∂x = (c · ∂s/∂x − s · ∂c/∂x) / (c² + s²)`. Because `c` and `s`
/// are continuous through phase wraps, their central-difference
/// gradients are well-behaved, and the recovered phase gradient is
/// wrap-free by construction. Pixels with `|z|² < 1e-12` (background
/// where amp-weighted smoothing left near-zero magnitude) are clamped
/// to avoid division blow-up.
///
/// Replaces the previous "take gradient of wrapped phase" approach,
/// which produced spurious sign flips along phase-wrap lines and
/// visible artifacts in the VFS map.
pub fn phase_gradients(z: &Tensor) -> (Tensor, Tensor) {
    let c = z.real();
    let s = z.imag();
    let (dc_dx, dc_dy) = real_gradients(&c);
    let (ds_dx, ds_dy) = real_gradients(&s);
    let mag_sq = &c.square() + &s.square();
    let mag_sq_safe = mag_sq.clamp(1e-12, f64::INFINITY);
    let dphi_dx = (&ds_dx * &c - &dc_dx * &s) / &mag_sq_safe;
    let dphi_dy = (&ds_dy * &c - &dc_dy * &s) / &mag_sq_safe;
    (dphi_dx, dphi_dy)
}

/// Per-orientation amplitude from forward and reverse F1 magnitudes.
/// Reference: SNLC `Gprocesskret_batch.m`:
/// `mag_az = 0.5 * (mag_hor_fwd + mag_hor_rev);`
pub fn position_amplitude(fwd: &Tensor, rev: &Tensor) -> Tensor {
    (fwd.abs() + rev.abs()) * 0.5
}

/// Central-difference gradients of a real-valued 2D tensor with edge
/// handling: forward difference at left/top, backward at right/bottom.
/// Output: `(dx, dy)` f32 tensors `[H, W]` on device.
pub fn real_gradients(map: &Tensor) -> (Tensor, Tensor) {
    let size = map.size();
    let h = size[0];
    let w = size[1];

    // ∂/∂x — central in interior, edges first-order.
    let dx = Tensor::zeros_like(map);
    let cd = &(map.narrow(1, 2, w - 2) - map.narrow(1, 0, w - 2)) / 2.0;
    let _ = dx.narrow(1, 1, w - 2).copy_(&cd);
    let bd = map.narrow(1, 1, 1) - map.narrow(1, 0, 1);
    let _ = dx.narrow(1, 0, 1).copy_(&bd);
    let bd = map.narrow(1, w - 1, 1) - map.narrow(1, w - 2, 1);
    let _ = dx.narrow(1, w - 1, 1).copy_(&bd);

    // ∂/∂y — central in interior, edges first-order.
    let dy = Tensor::zeros_like(map);
    let cd = &(map.narrow(0, 2, h - 2) - map.narrow(0, 0, h - 2)) / 2.0;
    let _ = dy.narrow(0, 1, h - 2).copy_(&cd);
    let bd = map.narrow(0, 1, 1) - map.narrow(0, 0, 1);
    let _ = dy.narrow(0, 0, 1).copy_(&bd);
    let bd = map.narrow(0, h - 1, 1) - map.narrow(0, h - 2, 1);
    let _ = dy.narrow(0, h - 1, 1).copy_(&bd);

    (dx, dy)
}

/// VFS = sin(θ_alt − θ_azi) where θ = atan2(dy, dx).
/// Input/Output: f32 tensors `[H, W]` on device.
pub fn compute_vfs(d_azi_dx: &Tensor, d_azi_dy: &Tensor, d_alt_dx: &Tensor, d_alt_dy: &Tensor) -> Tensor {
    let theta_azi = d_azi_dy.atan2(d_azi_dx);
    let theta_alt = d_alt_dy.atan2(d_alt_dx);
    (&theta_alt - &theta_azi).sin()
}

/// Sum a `Kind::ComplexFloat` `[H, W]` tensor's real and imaginary parts to
/// scalar f64s. Used by phase-locked cycle averaging to compute the global
/// per-cycle phase `arg(Σ_pixels cm)`. Sum kept in `Kind::Float` because MPS
/// does not implement f64 reduction.
pub fn complex_tensor_real_imag_sum(cm: &Tensor) -> (f64, f64) {
    let re_sum: f64 = cm.real().sum(Kind::Float).double_value(&[]);
    let im_sum: f64 = cm.imag().sum(Kind::Float).double_value(&[]);
    (re_sum, im_sum)
}

/// Multiply each element of a `Kind::ComplexFloat` `[H, W]` tensor by
/// `exp(i·phase_offset)` (rotate all complex values by the same phase).
/// Used by phase-locked cycle averaging.
pub fn complex_phase_shift(cm: &Tensor, phase_offset: f64) -> Tensor {
    let (c, s) = (phase_offset.cos() as f32, phase_offset.sin() as f32);
    let re = cm.real();
    let im = cm.imag();
    let new_re = &re * (c as f64) - &im * (s as f64);
    let new_im = &re * (s as f64) + &im * (c as f64);
    Tensor::complex(&new_re, &new_im)
}

/// Absolute Jacobian determinant `|∂azi/∂x · ∂alt/∂y - ∂alt/∂x · ∂azi/∂y|` of
/// the visual-field-coordinate map, computed from radian gradients and scaled
/// by `scale_azi · scale_alt` (= `(angular_range / 2π)` per orientation) to
/// produce the determinant in degree units.
///
/// Reuses the same gradient tensors that `compute_vfs` consumed; no second
/// gradient pass on host or device.
///
/// Input/Output: f32 tensors `[H, W]` on device.
pub fn compute_magnification_jacobian(
    d_azi_dx: &Tensor,
    d_azi_dy: &Tensor,
    d_alt_dx: &Tensor,
    d_alt_dy: &Tensor,
    scale_azi: f64,
    scale_alt: f64,
) -> Tensor {
    let det = d_azi_dx * d_alt_dy - d_alt_dx * d_azi_dy;
    det.abs() * (scale_azi * scale_alt)
}

