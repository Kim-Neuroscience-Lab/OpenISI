"""Golden for the ΔF/F stage (`temporal_mean_baseline` + the dF/F formula in
`frames_u16_subset_to_dff_tensor`, compute/conversions.rs) against a verbatim
transcription of Allen `corticalmapping/core/ImageAnalysis.py::normalizeMovie`
(L561-573, `baselineType='mean'`):

    averageImage   = np.mean(movie, axis=0)        # per-pixel temporal mean F0
    normalizedMovie = movie - averageImage         # (F - F0)
    dFoverFMovie    = normalizedMovie / averageImage   # (F - F0) / F0

So Allen's F0 is the per-pixel temporal MEAN and dF/F divides by it with NO
floor. Our `temporal_mean_baseline` is the same mean; our dF/F is
`(F - F0)/max(F0, denom_floor)`. With `denom_floor = 0` (and F0 > 0, which holds
for illuminated pixels) the two are identical — that is the faithful Allen path
this golden pins. The `0.5·median` floor we use in production is a documented
robustness deviation (bounds dark/vignette pixels), NOT part of Allen.

Synthetic movie: every pixel has a nonzero DC baseline (so F0 > 0) plus a
per-pixel F1 tone, stored as uint16 frames (the raw camera dtype our Rust reads).

Output: fixtures/dff_frames.bin (u16 [n,H,W] row-major)
        fixtures/dff_f0.bin     (f64 [H,W]  Allen averageImage)
        fixtures/dff_dff.bin    (f32 [n,H,W] Allen dFoverF)
Run: python gen_dff_golden.py
"""
import os
import numpy as np

N, H, W = 20, 16, 16
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


def main():
    rng = np.random.default_rng(20260611)
    t = np.arange(N)
    base_level = 800.0 + 600.0 * rng.random((H, W))     # nonzero per-pixel F0
    phi = 2.0 * np.pi * rng.random((H, W))
    amp = 20.0 + 80.0 * rng.random((H, W))

    movie = np.zeros((N, H, W), dtype=np.float64)
    for r in range(H):
        for c in range(W):
            movie[:, r, c] = base_level[r, c] + amp[r, c] * np.cos(2 * np.pi * t / N + phi[r, c])
    frames = np.round(movie).astype(np.uint16)           # raw camera frames

    # --- verbatim Allen normalizeMovie (mean) on the u16 frames as f64 ---
    fr = frames.astype(np.float64)
    f0 = np.mean(fr, axis=0)
    dff = (fr - f0) / f0
    # --- end ---

    # Allen baselineType='median' (np.median, axis=0; N even → avg of two middle).
    f0_median = np.median(fr, axis=0)

    np.save(os.path.join(FIX, "dff_frames.npy"), np.ascontiguousarray(frames.astype(np.uint16)))
    np.save(os.path.join(FIX, "dff_f0.npy"), np.ascontiguousarray(f0, dtype="<f8"))
    np.save(os.path.join(FIX, "dff_f0_median.npy"), np.ascontiguousarray(f0_median, dtype="<f8"))
    np.save(os.path.join(FIX, "dff_dff.npy"), np.ascontiguousarray(dff.astype(np.float32), dtype="<f4"))

    print(f"  N={N} H={H} W={W}  (N {'even' if N % 2 == 0 else 'odd'})")
    print(f"  F0 mean   range [{f0.min():.2f}, {f0.max():.2f}]")
    print(f"  F0 median range [{f0_median.min():.2f}, {f0_median.max():.2f}]")
    print(f"  dF/F range [{dff.min():.4f}, {dff.max():.4f}]")


if __name__ == "__main__":
    main()
