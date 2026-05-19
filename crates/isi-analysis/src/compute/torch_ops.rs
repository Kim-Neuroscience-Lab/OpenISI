//! Tensor-based compute operations using libtorch (tch-rs).
//!
//! Automatically uses CUDA when available, falls back to optimized CPU.
//! All operations are expressed as tensor operations — no manual loops.

use ndarray::{Array2, Array3};
use num_complex::Complex64;
use std::f64::consts::PI;
use std::sync::OnceLock;
use tch::{Device, Kind, Tensor};

// =============================================================================
// Backend detection
// =============================================================================

static DEVICE: OnceLock<Device> = OnceLock::new();

/// Get the compute device (CUDA GPU or CPU). Detected once, cached.
pub fn device() -> Device {
    *DEVICE.get_or_init(|| {
        if tch::Cuda::is_available() {
            let dev = Device::Cuda(0);
            eprintln!("[compute] Using CUDA GPU (device 0)");
            dev
        } else {
            eprintln!("[compute] Using CPU (libtorch)");
            Device::Cpu
        }
    })
}

/// Report backend info.
pub fn backend_info() -> String {
    let dev = device();
    match dev {
        Device::Cuda(i) => format!("CUDA device {i}"),
        Device::Cpu => "CPU (libtorch)".into(),
        _ => format!("Other ({dev:?})"),
    }
}

// =============================================================================
// ndarray ↔ Tensor conversions
// =============================================================================

/// Convert Array3<u16> to f32 Tensor on the compute device. Shape: [T, H, W].
pub fn frames_u16_to_tensor(frames: &Array3<u16>) -> Tensor {
    let (t, h, w) = frames.dim();
    let flat: Vec<i32> = frames.iter().map(|&v| v as i32).collect();
    Tensor::from_slice(&flat)
        .reshape([t as i64, h as i64, w as i64])
        .to_kind(Kind::Float)
        .to(device())
}

/// Convert Tensor [H, W] f64 → Array2<f64>.
pub fn tensor_to_f64_array2(t: &Tensor) -> Array2<f64> {
    let t_cpu = t.to_device(Device::Cpu).to_kind(Kind::Double);
    let size = t_cpu.size();
    let (h, w) = (size[0] as usize, size[1] as usize);
    let data: Vec<f64> = Vec::try_from(&t_cpu).expect("tensor to vec");
    Array2::from_shape_vec((h, w), data).expect("shape mismatch")
}

/// Convert Tensor pair [H, W] (real, imag) → Array2<Complex64>.
pub fn tensor_pair_to_complex(re: &Tensor, im: &Tensor) -> Array2<Complex64> {
    let re_arr = tensor_to_f64_array2(re);
    let im_arr = tensor_to_f64_array2(im);
    let (h, w) = re_arr.dim();
    Array2::from_shape_fn((h, w), |(r, c)| {
        Complex64::new(re_arr[[r, c]], im_arr[[r, c]])
    })
}

/// Convert Array2<Complex64> → (Tensor, Tensor) as (real, imag) on device.
pub fn complex_to_tensor_pair(arr: &Array2<Complex64>) -> (Tensor, Tensor) {
    let (h, w) = arr.dim();
    let re: Vec<f64> = arr.iter().map(|z| z.re).collect();
    let im: Vec<f64> = arr.iter().map(|z| z.im).collect();
    let shape = [h as i64, w as i64];
    (
        Tensor::from_slice(&re).reshape(&shape).to_kind(Kind::Double).to(device()),
        Tensor::from_slice(&im).reshape(&shape).to_kind(Kind::Double).to(device()),
    )
}

// =============================================================================
// Core operations
// =============================================================================

/// Compute baseline (temporal mean) from all frames.
/// Input: Tensor [T, H, W] f32 on device.
/// Output: Tensor [H, W] f64 on device.
pub fn baseline_mean(frames: &Tensor) -> Tensor {
    frames.mean_dim(0, false, Kind::Double)
}

/// Compute dF/F for a subset of frames.
/// Input: frames Tensor [T, H, W] f32, baseline Tensor [H, W] f64, eps f64.
/// Output: Tensor [n, H, W] f64 on device.
pub fn compute_dff(frames: &Tensor, baseline: &Tensor, eps: f64) -> Tensor {
    let frames_f64 = frames.to_kind(Kind::Double);
    let baseline_expanded = baseline.unsqueeze(0); // [1, H, W]
    (&frames_f64 - &baseline_expanded) / (&baseline_expanded + eps)
}

/// Single-frequency DFT projection.
/// Input: dff Tensor [n, H, W] f64, timestamps &[f64], is_forward bool.
/// Output: (Tensor [H, W] f64, Tensor [H, W] f64) = (real, imag) on device.
pub fn dft_projection(dff: &Tensor, timestamps: &[f64], is_forward: bool) -> (Tensor, Tensor) {
    let n = timestamps.len();
    let t_first = timestamps[0];
    let t_last = timestamps[n - 1];
    let period = t_last - t_first;
    let freq = 1.0 / period;
    let sign = if is_forward { -1.0 } else { 1.0 };

    // Build kernel vectors.
    let kernel_re: Vec<f64> = timestamps.iter()
        .map(|&ts| (sign * 2.0 * PI * freq * (ts - t_first)).cos())
        .collect();
    let kernel_im: Vec<f64> = timestamps.iter()
        .map(|&ts| (sign * 2.0 * PI * freq * (ts - t_first)).sin())
        .collect();

    // Move kernels to device as [n, 1, 1] for broadcasting.
    let kr = Tensor::from_slice(&kernel_re)
        .to_kind(Kind::Double)
        .to(device())
        .reshape([n as i64, 1, 1]);
    let ki = Tensor::from_slice(&kernel_im)
        .to_kind(Kind::Double)
        .to(device())
        .reshape([n as i64, 1, 1]);

    // Broadcast multiply and sum over time dimension.
    let acc_re = (dff * &kr).sum_dim_intlist(0, false, Kind::Double);
    let acc_im = (dff * &ki).sum_dim_intlist(0, false, Kind::Double);

    (acc_re, acc_im)
}

/// Multi-frequency SNR computation.
/// Input: dff Tensor [n, H, W] f64, timestamps &[f64].
/// Output: Tensor [H, W] f64 on device.
pub fn compute_snr(dff: &Tensor, timestamps: &[f64]) -> Tensor {
    let n = timestamps.len();
    if n < 4 {
        let size = dff.size();
        return Tensor::zeros([size[1], size[2]], (Kind::Double, device()));
    }

    let t_first = timestamps[0];
    let t_last = timestamps[n - 1];
    let period = t_last - t_first;
    let freq_stim = 1.0 / period;
    let dt_mean = period / (n - 1) as f64;
    let freq_nyquist = 0.5 / dt_mean;
    let max_bin = ((freq_nyquist / freq_stim).floor() as usize).min(n / 2).max(2);

    // Select noise bins (skip harmonics 2-4, cap at 20).
    let all_noise: Vec<usize> = (5..=max_bin).collect();
    let noise_bins: Vec<usize> = if all_noise.len() <= 20 {
        all_noise
    } else {
        let step = all_noise.len() as f64 / 20.0;
        (0..20).map(|i| all_noise[(i as f64 * step) as usize]).collect()
    };
    let n_noise = noise_bins.len().max(1);

    // Signal DFT at stimulus frequency.
    let sig_kernel_re: Vec<f64> = timestamps.iter()
        .map(|&ts| (-2.0 * PI * freq_stim * (ts - t_first)).cos())
        .collect();
    let sig_kernel_im: Vec<f64> = timestamps.iter()
        .map(|&ts| (-2.0 * PI * freq_stim * (ts - t_first)).sin())
        .collect();

    let skr = Tensor::from_slice(&sig_kernel_re).to_kind(Kind::Double).to(device()).reshape([n as i64, 1, 1]);
    let ski = Tensor::from_slice(&sig_kernel_im).to_kind(Kind::Double).to(device()).reshape([n as i64, 1, 1]);

    let sig_re = (dff * &skr).sum_dim_intlist(0, false, Kind::Double);
    let sig_im = (dff * &ski).sum_dim_intlist(0, false, Kind::Double);
    let signal_power = &sig_re * &sig_re + &sig_im * &sig_im;

    // Noise DFT — batch all noise bins.
    // Build kernel matrix [n_noise, n_frames] for real and imag.
    let mut all_kr = Vec::with_capacity(n_noise * n);
    let mut all_ki = Vec::with_capacity(n_noise * n);
    for &k in &noise_bins {
        let freq = freq_stim * k as f64;
        for &ts in timestamps {
            let angle = -2.0 * PI * freq * (ts - t_first);
            all_kr.push(angle.cos());
            all_ki.push(angle.sin());
        }
    }

    let kr_mat = Tensor::from_slice(&all_kr)
        .to_kind(Kind::Double)
        .to(device())
        .reshape([n_noise as i64, n as i64, 1, 1]);
    let ki_mat = Tensor::from_slice(&all_ki)
        .to_kind(Kind::Double)
        .to(device())
        .reshape([n_noise as i64, n as i64, 1, 1]);

    // dff is [n_frames, H, W], expand to [1, n_frames, H, W] for broadcast.
    let dff_expanded = dff.unsqueeze(0); // [1, n_frames, H, W]

    // [n_noise, n_frames, 1, 1] * [1, n_frames, H, W] → [n_noise, n_frames, H, W]
    // sum over dim=1 → [n_noise, H, W]
    let noise_re = (&dff_expanded * &kr_mat).sum_dim_intlist(1, false, Kind::Double);
    let noise_im = (&dff_expanded * &ki_mat).sum_dim_intlist(1, false, Kind::Double);
    let noise_power_per_bin = &noise_re * &noise_re + &noise_im * &noise_im;
    let noise_power = noise_power_per_bin.mean_dim(0, false, Kind::Double);

    // SNR = signal / noise, with floor to avoid division by zero.
    &signal_power / noise_power.clamp_min(1e-20)
}

/// Gaussian smoothing on a 2D f64 tensor. Separable convolution.
/// Input: Tensor [H, W] f64.
/// Output: Tensor [H, W] f64.
pub fn gaussian_smooth(input: &Tensor, sigma: f64) -> Tensor {
    if sigma <= 0.0 { return input.shallow_clone(); }
    let radius = (sigma * 3.0).ceil() as i64;
    let size = 2 * radius + 1;

    // Build 1D Gaussian kernel.
    let mut kernel_data = vec![0.0f64; size as usize];
    let mut sum = 0.0;
    for i in 0..size {
        let x = (i - radius) as f64;
        let v = (-0.5 * x * x / (sigma * sigma)).exp();
        kernel_data[i as usize] = v;
        sum += v;
    }
    for v in &mut kernel_data { *v /= sum; }

    let kernel = Tensor::from_slice(&kernel_data)
        .to_kind(Kind::Double)
        .to(device());

    // Separable: horizontal pass then vertical pass using conv1d.
    // Reshape input to [1, 1, H, W] for conv2d.
    let size_vec = input.size();
    let h = size_vec[0];
    let w = size_vec[1];
    let x = input.reshape([1, 1, h, w]);

    // Horizontal kernel: [1, 1, 1, kernel_size]
    let kh = kernel.reshape([1, 1, 1, size]);
    let padded = x.reflection_pad2d([radius, radius, 0, 0]);
    let after_h = padded.conv2d::<Tensor>(&kh, None, [1, 1], [0, 0], [1, 1], 1);

    // Vertical kernel: [1, 1, kernel_size, 1]
    let kv = kernel.reshape([1, 1, size, 1]);
    let padded = after_h.reflection_pad2d([0, 0, radius, radius]);
    let after_v = padded.conv2d::<Tensor>(&kv, None, [1, 1], [0, 0], [1, 1], 1);

    after_v.reshape([h, w])
}

/// Compute phase gradients using central differences on complex map.
/// Input: (real, imag) tensors [H, W] f64.
/// Output: (dphi_dx, dphi_dy) tensors [H, W] f64.
pub fn phase_gradients(re: &Tensor, im: &Tensor) -> (Tensor, Tensor) {
    let size = re.size();
    let h = size[0];
    let w = size[1];

    // dZ/dx via central differences (columns).
    let dre_dx = Tensor::zeros_like(re);
    let dim_dx = Tensor::zeros_like(im);
    // Interior: central difference.
    let cd = &(re.narrow(1, 2, w - 2) - re.narrow(1, 0, w - 2)) / 2.0;
    let _ = dre_dx.narrow(1, 1, w - 2).copy_(&cd);
    let cd = &(im.narrow(1, 2, w - 2) - im.narrow(1, 0, w - 2)) / 2.0;
    let _ = dim_dx.narrow(1, 1, w - 2).copy_(&cd);
    // Boundaries: forward/backward.
    let bd = re.narrow(1, 1, 1) - re.narrow(1, 0, 1);
    let _ = dre_dx.narrow(1, 0, 1).copy_(&bd);
    let bd = im.narrow(1, 1, 1) - im.narrow(1, 0, 1);
    let _ = dim_dx.narrow(1, 0, 1).copy_(&bd);
    let bd = re.narrow(1, w - 1, 1) - re.narrow(1, w - 2, 1);
    let _ = dre_dx.narrow(1, w - 1, 1).copy_(&bd);
    let bd = im.narrow(1, w - 1, 1) - im.narrow(1, w - 2, 1);
    let _ = dim_dx.narrow(1, w - 1, 1).copy_(&bd);

    // dZ/dy via central differences (rows).
    let dre_dy = Tensor::zeros_like(re);
    let dim_dy = Tensor::zeros_like(im);
    let cd = &(re.narrow(0, 2, h - 2) - re.narrow(0, 0, h - 2)) / 2.0;
    let _ = dre_dy.narrow(0, 1, h - 2).copy_(&cd);
    let cd = &(im.narrow(0, 2, h - 2) - im.narrow(0, 0, h - 2)) / 2.0;
    let _ = dim_dy.narrow(0, 1, h - 2).copy_(&cd);
    let bd = re.narrow(0, 1, 1) - re.narrow(0, 0, 1);
    let _ = dre_dy.narrow(0, 0, 1).copy_(&bd);
    let bd = im.narrow(0, 1, 1) - im.narrow(0, 0, 1);
    let _ = dim_dy.narrow(0, 0, 1).copy_(&bd);
    let bd = re.narrow(0, h - 1, 1) - re.narrow(0, h - 2, 1);
    let _ = dre_dy.narrow(0, h - 1, 1).copy_(&bd);
    let bd = im.narrow(0, h - 1, 1) - im.narrow(0, h - 2, 1);
    let _ = dim_dy.narrow(0, h - 1, 1).copy_(&bd);

    // dphi/dx = Im{conj(Z) * dZ/dx} = re * dim_dx - im * dre_dx
    let dphi_dx = re * &dim_dx - im * &dre_dx;
    // dphi/dy = Im{conj(Z) * dZ/dy} = re * dim_dy - im * dre_dy
    let dphi_dy = re * &dim_dy - im * &dre_dy;

    (dphi_dx, dphi_dy)
}

/// Compute VFS from gradient components.
/// Input: 4 tensors [H, W] f64 (d_azi_dx, d_azi_dy, d_alt_dx, d_alt_dy).
/// Output: Tensor [H, W] f64.
pub fn compute_vfs(d_azi_dx: &Tensor, d_azi_dy: &Tensor, d_alt_dx: &Tensor, d_alt_dy: &Tensor) -> Tensor {
    let theta_azi = d_azi_dy.atan2(d_azi_dx);
    let theta_alt = d_alt_dy.atan2(d_alt_dx);
    (&theta_alt - &theta_azi).sin()
}

// =============================================================================
// High-level ndarray ↔ ndarray wrappers (used by io.rs and math.rs)
// =============================================================================

/// Process a sweep on GPU: extract frames by index, compute dF/F, DFT projection.
/// All data stays on GPU until the final conversion back to Complex64.
pub fn gpu_sweep_dft(
    all_frames: &Tensor,
    baseline: &Tensor,
    frame_indices: &[usize],
    timestamps: &[f64],
    is_forward: bool,
    eps: f64,
) -> Array2<Complex64> {
    let idx: Vec<i64> = frame_indices.iter().map(|&i| i as i64).collect();
    let idx_tensor = Tensor::from_slice(&idx).to(device());
    let sweep_frames = all_frames.index_select(0, &idx_tensor);
    let dff = compute_dff(&sweep_frames, baseline, eps);
    let (re, im) = dft_projection(&dff, timestamps, is_forward);
    tensor_pair_to_complex(&re, &im)
}

/// Compute SNR for a sweep on GPU.
pub fn gpu_sweep_snr(
    all_frames: &Tensor,
    baseline: &Tensor,
    frame_indices: &[usize],
    timestamps: &[f64],
    eps: f64,
) -> Array2<f64> {
    let idx: Vec<i64> = frame_indices.iter().map(|&i| i as i64).collect();
    let idx_tensor = Tensor::from_slice(&idx).to(device());
    let sweep_frames = all_frames.index_select(0, &idx_tensor);
    let dff = compute_dff(&sweep_frames, baseline, eps);
    let snr = compute_snr(&dff, timestamps);
    tensor_to_f64_array2(&snr)
}

/// Gaussian smooth a complex map on GPU (smooth real and imag parts separately).
pub fn gpu_smooth_complex(map: &Array2<Complex64>, sigma: f64) -> Array2<Complex64> {
    if sigma <= 0.0 { return map.clone(); }
    let (re_t, im_t) = complex_to_tensor_pair(map);
    let re_smooth = gaussian_smooth(&re_t, sigma);
    let im_smooth = gaussian_smooth(&im_t, sigma);
    tensor_pair_to_complex(&re_smooth, &im_smooth)
}

/// Amplitude-weighted phase gradients on GPU.
/// Returns (dphi_dx, dphi_dy) as Array2<f64>.
pub fn gpu_phase_gradients(map: &Array2<Complex64>) -> (Array2<f64>, Array2<f64>) {
    let (re_t, im_t) = complex_to_tensor_pair(map);
    let (dx, dy) = phase_gradients(&re_t, &im_t);
    (tensor_to_f64_array2(&dx), tensor_to_f64_array2(&dy))
}

/// VFS computation on GPU.
pub fn gpu_compute_vfs(
    d_azi_dx: &Array2<f64>,
    d_azi_dy: &Array2<f64>,
    d_alt_dx: &Array2<f64>,
    d_alt_dy: &Array2<f64>,
) -> Array2<f64> {
    let to_tensor = |arr: &Array2<f64>| {
        let (h, w) = arr.dim();
        Tensor::from_slice(arr.as_slice().unwrap())
            .reshape([h as i64, w as i64])
            .to_kind(Kind::Double)
            .to(device())
    };
    let result = compute_vfs(
        &to_tensor(d_azi_dx), &to_tensor(d_azi_dy),
        &to_tensor(d_alt_dx), &to_tensor(d_alt_dy),
    );
    tensor_to_f64_array2(&result)
}
