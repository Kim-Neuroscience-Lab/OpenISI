"""Golden for magnification anisotropy, against SNLC getMagFactors.m
(the post-`gradient` block) verbatim:

    vecH = dhdx + 1i*dhdy;  vecV = dvdx + 1i*dvdy;
    Res  = abs(vecH).*exp(1i*(angle(vecH)+pi/2)*2)
         + abs(vecV).*exp(1i*(angle(vecV)+pi/2)*2);
    Res  = Res./(abs(vecH) + abs(vecV));
    Distrtion  = abs(Res);
    prefAxisMF = angle(Res)/2*180/pi;  prefAxisMF(prefAxisMF<0) += 180;

The four GRADIENT fields are the inputs (the op takes the same four gradients
`compute_magnification_jacobian`'s determinant does), so this golden isolates the
anisotropy FORMULA from the upstream smoothing/gradient stages. The fields are
deterministic and chosen non-degenerate: |vecH| >= 1 everywhere (dhdx = 2+sin),
so the |vecH|+|vecV| denominator never vanishes, and the angles vary enough to
exercise the [0,180) wrap.

Output: fixtures/maganiso_{dhdx,dhdy,dvdx,dvdy,axis,distortion}.bin
        (float64 row-major 48x48)
Run:  python gen_maganiso_golden.py   (via `cargo xtask goldens maganiso`)
"""

import os

import numpy as np

N = 48
t = np.linspace(-2.0, 2.0, N)
X, Y = np.meshgrid(t, t)

dhdx = 2.0 + np.sin(X)                 # |.| >= 1 → vecH never zero → denom > 0
dhdy = np.cos(Y) + 0.5 * np.sin(X * Y)
dvdx = np.cos(2.0 * X) + 0.3
dvdy = np.sin(X + Y) - 0.4 * np.cos(Y)

vecH = dhdx + 1j * dhdy
vecV = dvdx + 1j * dvdy
res = np.abs(vecH) * np.exp(1j * (np.angle(vecH) + np.pi / 2) * 2) + np.abs(
    vecV
) * np.exp(1j * (np.angle(vecV) + np.pi / 2) * 2)
res = res / (np.abs(vecH) + np.abs(vecV))

distortion = np.abs(res)
axis = np.angle(res) / 2 * 180 / np.pi
axis[axis < 0] += 180.0

FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
for name, arr in [
    ("maganiso_dhdx", dhdx),
    ("maganiso_dhdy", dhdy),
    ("maganiso_dvdx", dvdx),
    ("maganiso_dvdy", dvdy),
    ("maganiso_axis", axis),
    ("maganiso_distortion", distortion),
]:
    np.save(os.path.join(FIX, name + ".npy"), arr.astype("<f8"))

print(
    f"  maganiso: axis range [{axis.min():.2f}, {axis.max():.2f}] deg  "
    f"distortion [{distortion.min():.3f}, {distortion.max():.3f}]"
)
