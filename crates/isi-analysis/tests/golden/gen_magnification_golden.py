"""Golden for `compute::compute_magnification_jacobian` (the visual-field
Jacobian determinant |det J|, our `magnification_raw`) and the inverted
`magnification` leaf (Allen cortical magnification factor, px^2/deg^2), against
Allen `retinotopic_mapping/RetinotopicMapping.py::_getDeterminantMap` (L1184).

Allen oracle, transcribed VERBATIM (L1192-1199):

    gradAltMap = np.gradient(altPosMapf)   # [d/d_row(axis0), d/d_col(axis1)]
    gradAziMap = np.gradient(aziPosMapf)
    detMap = np.array([[gradAltMap[0], gradAltMap[1]],
                       [gradAziMap[0], gradAziMap[1]]])
    detMap = detMap.transpose(2, 3, 0, 1)
    detMap = np.abs(np.linalg.det(detMap))

`np.gradient` (central diff interior, one-sided edges) is exactly our
`compute::real_gradients`, and `np.linalg.det` of [[gAlt_y, gAlt_x],
[gAzi_y, gAzi_x]] is `gAlt_y*gAzi_x - gAlt_x*gAzi_y`, i.e. the SAME two product
terms as our `compute_magnification_jacobian` det (`d_azi_dx*d_alt_dy -
d_alt_dx*d_azi_dy`). The maps are already in degrees, so the oracle applies no
scale -> the Rust test uses scale_azi = scale_alt = 1.0.

The inversion check pins the new `magnification` leaf: cmf = 1/max(|detJ|, eps).

Layout: 48x48 NON-affine altitude/azimuth degree maps (sinusoidal ripples) so
the determinant varies across the frame.

Outputs (C-order row-major, little-endian, all 48x48):
  fixtures/mag_alt.bin   altitude map (deg), '<f8'
  fixtures/mag_azi.bin   azimuth  map (deg), '<f8'
  fixtures/mag_det.bin   |det J| (deg^2/px^2, Allen _getDeterminantMap), '<f8'
  fixtures/mag_cmf.bin   1/max(|det J|, 1e-12) (px^2/deg^2), '<f8'

Run:  python gen_magnification_golden.py
"""
import os
import numpy as np

N = 48
EPS = 1e-12
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


def main():
    rr, cc = np.mgrid[0:N, 0:N].astype(np.float64)
    # Non-affine smooth degree maps so |det J| genuinely varies.
    alt = (rr / (N - 1) * 50.0 - 25.0) + 5.0 * np.sin(cc / 6.0) + 3.0 * np.cos(rr / 8.0)
    azi = (cc / (N - 1) * 70.0 - 35.0) + 4.0 * np.cos(rr / 5.0) + 3.0 * np.sin(cc / 7.0)

    # --- VERBATIM Allen _getDeterminantMap ---
    g_alt = np.gradient(alt)   # [d/d_row, d/d_col]
    g_azi = np.gradient(azi)
    det = np.array([[g_alt[0], g_alt[1]],
                    [g_azi[0], g_azi[1]]])
    det = det.transpose(2, 3, 0, 1)
    det = np.abs(np.linalg.det(det))

    cmf = 1.0 / np.maximum(det, EPS)

    alt.astype('<f8').tofile(os.path.join(FIX, "mag_alt.bin"))
    azi.astype('<f8').tofile(os.path.join(FIX, "mag_azi.bin"))
    det.astype('<f8').tofile(os.path.join(FIX, "mag_det.bin"))
    cmf.astype('<f8').tofile(os.path.join(FIX, "mag_cmf.bin"))

    print(f"  N={N}")
    print(f"  |det J| range=[{det.min():.6f}, {det.max():.6f}]  mean={det.mean():.6f}")
    print(f"  CMF     range=[{cmf.min():.6f}, {cmf.max():.6f}]")


if __name__ == "__main__":
    main()
