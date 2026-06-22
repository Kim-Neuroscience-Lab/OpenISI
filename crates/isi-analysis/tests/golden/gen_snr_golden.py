"""Golden for `compute_snr` (compute/ops.rs:296) against a VERBATIM numpy
transcription of its DOCUMENTED bin-selection rule.

There is NO external library oracle for this SNR: it is an original multi-bin
spectral SNR heuristic (skip harmonics 2-4 i.e. start noise bins at k=5, cap the
noise-bin list at 20 by even subsampling, cap the highest bin at the Nyquist
multiple of the stimulus frequency, mean noise-bin power as the denominator with
a 1e-20 floor). Because nothing names an oracle and there is no golden, silent
drift in the bin set is invisible. This pins the rule.

The reference below (`snr_reference`) is a line-for-line numpy transcription of
the Rust selection + power math, computed with the SAME projection our Rust uses
(a direct DFT projection sum_t dff[t] * exp(-2*pi*i*f*t), NOT np.fft.fft — see
note), so any future change to the Rust bin set, the harmonic skip, the cap-20
subsample arithmetic, the Nyquist cap, or the mean-vs-sum denominator will be
caught.

IMPORTANT — why not np.fft.fft directly: production builds timestamps as
t_k = k*dt (uniform), and sets freq_stim = 1/period = 1/((n-1)*dt). The signal
kernel exp(-2*pi*i*freq_stim*t_k) = exp(-2*pi*i*k/(n-1)) therefore lands at
FRACTIONAL FFT bin n/(n-1), NOT integer bin 1. Likewise noise bin k lands at
fractional FFT bin k*n/(n-1). So this is a non-uniform-DFT-at-chosen-frequencies
SNR, and a plain np.fft.fft would silently use the wrong (integer) bins. The
reference projects at exactly the frequencies the Rust uses.

Inputs are exact-float-replayable: dff stored as '<f4' (f32) so the Rust f32
matmul path sees identical input; timestamps stored as '<f8'.

Two cases:
  small : n=30  -> max_bin = min(floor(0.5*(n-1)), n//2) = min(14,15)=14;
          all_noise=[5..14] (10 bins) -> use-all branch (<=20).
  large : n=120 -> max_bin = min(floor(0.5*119), 120//2)=min(59,60)=59;
          all_noise=[5..59] (55 bins) -> SUBSAMPLE branch (cap 20).

Output (per case <C>):
  fixtures/snr_<C>_dff.npy   (n*H*W f32, C-order [n,H,W])
  fixtures/snr_<C>_ts.npy    (n f64 timestamps)
  fixtures/snr_<C>_out.npy   (H*W f64 expected SNR map)
Run:  python gen_snr_golden.py
"""
import os
import numpy as np

H, W = 6, 8           # small map; SNR is per-pixel independent so size is free
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


def select_noise_bins(n_ts, dt_mean, freq_stim):
    """Verbatim transcription of ops.rs:308-321 bin selection."""
    freq_nyquist = 0.5 / dt_mean
    max_bin = max(min(int(np.floor(freq_nyquist / freq_stim)), n_ts // 2), 2)
    all_noise = list(range(5, max_bin + 1))           # 5..=max_bin inclusive
    if len(all_noise) <= 20:
        noise_bins = all_noise
    else:
        step = len(all_noise) / 20.0
        noise_bins = [all_noise[int(i * step)] for i in range(20)]  # int() == floor for >=0
    return noise_bins, max_bin


def snr_reference(dff, timestamps):
    """Verbatim transcription of compute_snr (ops.rs:296-361).

    dff: (n, H, W) float32 ; timestamps: (n,) float64. Returns (H, W) float64.
    """
    n, h, w = dff.shape
    n_ts = len(timestamps)
    if n_ts < 4:
        return np.zeros((h, w), dtype=np.float64)
    t_first = timestamps[0]
    period = timestamps[-1] - t_first
    freq_stim = 1.0 / period
    dt_mean = period / (n_ts - 1)

    noise_bins, _ = select_noise_bins(n_ts, dt_mean, freq_stim)
    n_noise = max(len(noise_bins), 1)

    ts = timestamps - t_first
    # Match Rust: f32 input flattened, but accumulate the projection. We mirror
    # the f32 matmul by casting dff to float32 and summing in float32 to stay
    # close to burn's f32 matmul (kernels are f32 too).
    dff_flat = dff.reshape(n, h * w).astype(np.float32)    # (n, HW)

    # --- signal term at freq_stim ---
    ang = (-2.0 * np.pi * freq_stim * ts).astype(np.float64)
    skr = np.cos(ang).astype(np.float32)                   # (n,)
    ski = np.sin(ang).astype(np.float32)
    sig_re = skr @ dff_flat                                 # (HW,) f32
    sig_im = ski @ dff_flat
    signal_power = (sig_re * sig_re + sig_im * sig_im).reshape(h, w)

    # --- noise term: mean power over selected bins ---
    noise_power_sum = np.zeros(h * w, dtype=np.float32)
    for k in noise_bins:
        f = freq_stim * k
        ph = (-2.0 * np.pi * f * ts).astype(np.float64)
        kr = np.cos(ph).astype(np.float32)
        ki = np.sin(ph).astype(np.float32)
        nre = kr @ dff_flat
        nim = ki @ dff_flat
        noise_power_sum += nre * nre + nim * nim
    noise_power = (noise_power_sum / np.float32(n_noise)).reshape(h, w)

    out = signal_power / np.maximum(noise_power, np.float32(1e-20))
    return out.astype(np.float64)


def make_dff(n, h, w, dt, seed):
    """Build a per-pixel signal with a known stimulus tone + harmonics + noise so
    the noise-bin set materially changes the denominator. Pixel (r,c) gets a
    different amplitude/phase so the map is non-degenerate."""
    rng = np.random.default_rng(seed)
    t = np.arange(n) * dt
    period = (n - 1) * dt
    f_stim = 1.0 / period
    dff = np.zeros((n, h, w), dtype=np.float64)
    for r in range(h):
        for c in range(w):
            idx = r * w + c
            amp = 1.0 + 0.1 * idx
            phi = 0.3 * idx
            sig = amp * np.cos(2 * np.pi * f_stim * t + phi)
            # harmonics 2,3,5,7 to feed both the skipped band and noise band
            sig += 0.4 * np.cos(2 * np.pi * (2 * f_stim) * t + 0.1 * idx)
            sig += 0.3 * np.cos(2 * np.pi * (3 * f_stim) * t)
            sig += 0.25 * np.cos(2 * np.pi * (5 * f_stim) * t + 0.2)
            sig += 0.2 * np.cos(2 * np.pi * (7 * f_stim) * t)
            sig += 0.15 * rng.standard_normal(n)          # broadband noise
            dff[:, r, c] = sig
    return dff.astype(np.float32)


def emit(case, n, dt, seed):
    dff = make_dff(n, H, W, dt, seed)
    ts = (np.arange(n) * dt).astype(np.float64)
    out = snr_reference(dff, ts)

    period = ts[-1] - ts[0]
    freq_stim = 1.0 / period
    dt_mean = period / (n - 1)
    noise_bins, max_bin = select_noise_bins(n, dt_mean, freq_stim)

    np.save(os.path.join(FIX, f"snr_{case}_dff.npy"), np.ascontiguousarray(dff.astype('<f4')))
    np.save(os.path.join(FIX, f"snr_{case}_ts.npy"), np.ascontiguousarray(ts.astype('<f8')))
    np.save(os.path.join(FIX, f"snr_{case}_out.npy"), np.ascontiguousarray(out.astype('<f8')))

    branch = "use-all" if len(list(range(5, max_bin + 1))) <= 20 else "subsample"
    print(f"[{case}] n={n} dt={dt} H={H} W={W}")
    print(f"  freq_stim={freq_stim:.6g} dt_mean={dt_mean:.6g} max_bin={max_bin} "
          f"n_noise={len(noise_bins)} branch={branch}")
    print(f"  noise_bins={noise_bins}")
    print(f"  SNR out: min={out.min():.6g} max={out.max():.6g} "
          f"mean={out.mean():.6g} sum={out.sum():.6g}")
    return out


def main():
    print("=== compute_snr golden (documented bin rule, numpy transcription) ===")
    emit("small", n=30, dt=0.1, seed=11)
    emit("large", n=120, dt=0.05, seed=23)
    print("Fixtures written to", FIX)


if __name__ == "__main__":
    main()
