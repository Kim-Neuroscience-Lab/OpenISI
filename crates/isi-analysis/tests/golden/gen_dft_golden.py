"""Golden for the single-bin F1 DFT (`compute::dft_projection_at_freq`, stage 0)
against numpy's FFT. Our kernel is `exp(-2πi·freq·dt·t)`; for `freq·dt = 1/n`
that is exactly `np.fft.fft(...)[1]` (bin 1).

Synthetic movie: a per-pixel sinusoid `DC + A·cos(2π·t/n + φ)` with a constant
DC offset (so the test also confirms bin-1 rejects DC), A and φ varying across
the field to cover amplitude and the full phase circle.

Output: fixtures/dft_movie.bin (float32 [n,H,W] row-major),
        fixtures/dft_f1_re.bin, dft_f1_im.bin (float64 [H,W])
Run:  python gen_dft_golden.py
"""
import os
import numpy as np

N, H, W = 24, 16, 16
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")

y, x = np.mgrid[0:H, 0:W]
A = 1.0 + 0.5 * (x / W)                     # amplitude varies along x
phi = 2.0 * np.pi * (y / H) - np.pi         # phase covers (-pi, pi] along y
DC = 5.0
t = np.arange(N)
movie = DC + A[None, :, :] * np.cos(2 * np.pi * t[:, None, None] / N + phi[None, :, :])

F1 = np.fft.fft(movie, axis=0)[1]           # bin 1 == our freq·dt = 1/N

np.ascontiguousarray(movie, dtype="<f4").tofile(os.path.join(FIX, "dft_movie.bin"))
np.ascontiguousarray(F1.real, dtype="<f8").tofile(os.path.join(FIX, "dft_f1_re.bin"))
np.ascontiguousarray(F1.imag, dtype="<f8").tofile(os.path.join(FIX, "dft_f1_im.bin"))

print(f"  movie [{N},{H},{W}]  |F1| range [{np.abs(F1).min():.3f}, {np.abs(F1).max():.3f}]")
print(f"  (DC={DC} → bin-1 should reject it)")
