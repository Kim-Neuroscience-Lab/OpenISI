"""Golden for `gaussian_smooth_f64` (the keystone smoothing primitive behind
`sign_map_smoothing` and the amp-weighted phasor smooth) vs scipy
`ni.gaussian_filter` — the function Allen's `_getSignMap` / `phaseFilter` call.

scipy defaults: `truncate=4.0` (kernel radius = int(4*sigma+0.5)) and
`mode='reflect'`. Our `gaussian_smooth_f64` uses `ceil(3*sigma)` and a `reflect`
border, so a divergence here would be the truncation radius and/or border mode.

Deterministic input field with a ramp (nonzero at borders, so the border mode is
exercised) plus a few 2D gaussians. Output for sigma=4.0.

Output: fixtures/gauss_{input,sigma4}.bin (float64 row-major 96x96)
Run:  python gen_gaussian_golden.py
"""
import os
import numpy as np
import scipy.ndimage as ni

N = 96
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")

ys, xs = np.mgrid[0:N, 0:N].astype(float)
field = 0.01 * (xs + ys)                    # ramp → nonzero borders
for cy, cx, a, s in [(20, 15, 3.0, 5.0), (45, 60, -2.0, 8.0),
                     (12, 70, 2.5, 4.0), (70, 20, 1.5, 6.0)]:
    field += a * np.exp(-((ys - cy) ** 2 + (xs - cx) ** 2) / (2 * s * s))

SIGMA = 4.0
out = ni.gaussian_filter(field, sigma=SIGMA)   # truncate=4.0, mode='reflect'

np.ascontiguousarray(field, dtype="<f8").tofile(os.path.join(FIX, "gauss_input.bin"))
np.ascontiguousarray(out, dtype="<f8").tofile(os.path.join(FIX, "gauss_sigma4.bin"))

print(f"  input range [{field.min():.4f}, {field.max():.4f}]  "
      f"out range [{out.min():.4f}, {out.max():.4f}]")
print(f"  scipy kernel radius (truncate=4) = {int(4.0 * SIGMA + 0.5)}   "
      f"ours (ceil 3*sigma) = {int(np.ceil(3 * SIGMA))}")
