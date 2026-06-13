"""Golden for `position_amplitude` (compute/ops.rs:175) — the F1-amplitude
metric the SnlcF1Amplitude signal-quality mask thresholds.

Metric: per-orientation F1 magnitude = mean of the forward/reverse F1
magnitudes, exactly SNLC `Gprocesskret.m` `magS`:

    magS = (|fwd| + |rev|) / 2

(`Gprocesskret.m` L59-64: `mag0=abs(ang0); mag2=abs(ang2); magS.hor=(mag0+mag2)/2`.)
Our `position_amplitude(fwd, rev) = 0.5·(|fwd| + |rev|)` is the same. Verbatim
numpy transcription is the oracle.

Inputs stored f32 (device `Complex2` precision); expected magS f64.
Output: fixtures/amp_fwd_re.bin, amp_fwd_im.bin, amp_rev_re.bin, amp_rev_im.bin
        (f32 [H,W]) ; amp_expected.bin (f64 [H,W])
Run: python gen_amplitude_golden.py
"""
import os
import numpy as np

H, W = 16, 16
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


def main():
    rng = np.random.default_rng(20260611)
    # Mixed magnitudes incl. a near-zero pixel (floor behaviour) and large ones.
    fwd = (rng.standard_normal((H, W)) + 1j * rng.standard_normal((H, W))) * (0.1 + rng.random((H, W)))
    rev = (rng.standard_normal((H, W)) + 1j * rng.standard_normal((H, W))) * (0.1 + rng.random((H, W)))

    mags = (np.abs(fwd) + np.abs(rev)) / 2.0      # SNLC magS

    for name, arr in [("amp_fwd_re", fwd.real), ("amp_fwd_im", fwd.imag),
                      ("amp_rev_re", rev.real), ("amp_rev_im", rev.imag)]:
        np.ascontiguousarray(arr.astype(np.float32), dtype="<f4").tofile(os.path.join(FIX, name + ".bin"))
    np.ascontiguousarray(mags, dtype="<f8").tofile(os.path.join(FIX, "amp_expected.bin"))

    print(f"  H={H} W={W}  magS range [{mags.min():.4f}, {mags.max():.4f}]")


if __name__ == "__main__":
    main()
