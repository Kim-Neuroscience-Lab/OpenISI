"""Golden for the V1-CENTER SELECTION of `math::compute_eccentricity`
(`EccentricityMethod::Garrett2014WholeCortexV1`, math.rs:223) against the
SNLC/Callaway MATLAB oracle it cites: `getAreaBorders.m` L211-224 plus the
helpers `getV1id.m` and `getPatchCoM.m` (reference/ISI/...).

WHY THIS GAP: the per-pixel great-circle formula is already golden
(gen_ecc_golden.py). UNTESTED is HOW the single V1 reference point (altC, aziC)
is chosen. Our Rust and the SNLC oracle choose it DIFFERENTLY, so this golden
pins the oracle's choice and lets the Rust test assert match-or-diverge.

SNLC oracle (transcribed VERBATIM from getAreaBorders.m / getV1id.m /
getPatchCoM.m, MATLAB 1-based -> Python 0-based noted inline):

  SE     = strel('disk',10);
  imdum  = imopen(imseg, SE);                 % morphological opening, disk r=10
  [CoMxy ...] = getPatchCoM(imdum);           % per-component centroid in PIXEL
                                              %   space: CoMxy(i,1)=x(col),
                                              %   CoMxy(i,2)=y(row); off-patch
                                              %   correction -> nearest in-patch
  V1id   = getV1id(imdum);                     % bwlabel(im,4); V1 = label with
                                              %   most pixels (MATLAB max -> FIRST
                                              %   on a tie)
  Vcent(1) = kmap_hor (round(CoMy), round(CoMx));  % AZIMUTH map sampled AT the
  Vcent(2) = kmap_vert(round(CoMy), round(CoMx));  % single pixel CoM (NOT a mean)
  az = (kmap_hor  - Vcent(1))*pi/180;
  alt= (kmap_vert - Vcent(2))*pi/180;
  kmap_rad = atan( sqrt( tan(az)^2 + tan(alt)^2 / cos(az)^2 ) )*180/pi;  % ecc

Two oracle subtleties this stresses (the "skeletonize-class" traps):
  (A) CENTER = single-pixel SAMPLE of the map at the PIXEL-space centroid of
      the (imopen'd) largest component. Our Rust uses the MEAN of azi/alt over
      ALL V1 pixels (a visual-field-space centroid). On any non-affine map these
      differ.
  (B) FORMULA cosine denominator is on AZIMUTH (cos(az)^2 with az dividing the
      ALT term), while Allen/our-Rust put it on ALTITUDE. (Already partly
      covered by gen_ecc_golden vs Allen, but reproduced here so the SNLC map
      is self-consistent.)
  (C) imopen(disk,10) runs BEFORE component selection; our Rust skips it. The
      layout includes a thin spur on V1 that opening removes, shifting the
      pixel centroid.

Layout: 64x64. alt/azi smooth but NON-affine (sinusoidal ripples) so the
single-pixel sample and the patch mean genuinely disagree. area_labels has:
  - a large convex V1 blob (label 1) with a THIN 1px spur (opening removes it),
  - a medium second area (label 2),
  - a small third area (label 3).

Outputs (all 64x64 unless noted, C-order row-major, little-endian):
  fixtures/v1ecc_alt.bin     altitude map, '<f8'
  fixtures/v1ecc_azi.bin     azimuth map, '<f8'
  fixtures/v1ecc_labels.bin  area_labels, '<i4' (0 background, 1..K areas)
  fixtures/v1ecc_snlc_center.bin   [altC, aziC] SNLC choice, '<f8' (len 2)
  fixtures/v1ecc_rust_center.bin   [altC, aziC] our-Rust mean choice, '<f8' (len 2)
  fixtures/v1ecc_snlc_map.bin      SNLC eccentricity map (masked to labels>0,
                                   0.0 outside), '<f8'
  fixtures/v1ecc_rust_map.bin      eccentricity using our-Rust center + our-Rust
                                   formula (masked, 0.0 outside), '<f8'

Run:  python gen_v1ecc_golden.py
"""
import os
import numpy as np
import scipy.ndimage as ni

N = 64
DISK_R = 10
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


def disk_se(r):
    """Flat Euclidean disk SE matching MATLAB strel('disk', r, 0)."""
    y, x = np.mgrid[-r:r + 1, -r:r + 1]
    return (x * x + y * y <= r * r)


# --- VERBATIM getV1id.m (1-based -> 0-based labels) ---
def getV1id(imseg):
    # bwlabel(im,4): scipy.label with 4-conn cross structure
    cross = np.array([[0, 1, 0], [1, 1, 1], [0, 1, 0]], dtype=bool)
    lbl, n = ni.label(imseg, structure=cross)
    sizes = np.array([np.sum(lbl == q) for q in range(1, n + 1)])
    # MATLAB [dum V1id]=max(Sqmm): first maximum (1-based). argmax -> first max.
    v1 = int(np.argmax(sizes)) + 1
    return v1, lbl, n


# --- VERBATIM getPatchCoM.m centroid (pixel space), 1-based -> 0-based ---
def getPatchCoM(imseg):
    cross = np.array([[0, 1, 0], [1, 1, 1], [0, 1, 0]], dtype=bool)
    lbl, n = ni.label(imseg, structure=cross)
    xdom = np.arange(N)  # 0-based column indices (MATLAB 1..W)
    ydom = np.arange(N)
    com = np.zeros((n, 2))  # col(x), row(y)
    for i in range(1, n + 1):
        temp = (lbl == i).astype(float)
        tempx = temp.sum(axis=0)          # sum over rows -> per column
        com[i - 1, 0] = np.sum(tempx * xdom) / np.sum(tempx)   # x = col centroid
        tempy = temp.sum(axis=1)          # sum over cols -> per row
        com[i - 1, 1] = np.sum(tempy * ydom) / np.sum(tempy)   # y = row centroid
        # off-patch correction: if rounded CoM not on this patch, snap to the
        # in-patch pixel nearest the CoM.
        rr = int(round(com[i - 1, 1]))
        cc = int(round(com[i - 1, 0]))
        if lbl[rr, cc] != i:
            xg, yg = np.meshgrid(xdom - com[i - 1, 0], ydom - com[i - 1, 1])
            rdom = np.sqrt(xg ** 2 + yg ** 2)
            # min distance among patch pixels
            patch_idx = np.where(temp > 0)
            mind = np.min(rdom[patch_idx])
            ys, xs = np.where(rdom == mind)
            com[i - 1, 0] = xs[0]
            com[i - 1, 1] = ys[0]
    return com, lbl, n


def snlc_ecc(kmap_hor, kmap_vert, vcent_azi, vcent_alt):
    az = (kmap_hor - vcent_azi) * np.pi / 180.0    # azimuth delta
    alt = (kmap_vert - vcent_alt) * np.pi / 180.0  # altitude delta
    return np.arctan(np.sqrt(np.tan(az) ** 2 + np.tan(alt) ** 2 / np.cos(az) ** 2)) * 180.0 / np.pi


def rust_ecc(alt_deg, azi_deg, alt_c, azi_c):
    # our-Rust math::eccentricity_pixel_deg, vectorized
    to_rad = np.pi / 180.0
    d_alt = (alt_deg - alt_c) * to_rad
    d_azi = (azi_deg - azi_c) * to_rad
    cos_d_alt = np.cos(d_alt)
    term = np.tan(d_alt) ** 2 + np.tan(d_azi) ** 2 / np.maximum(cos_d_alt ** 2, 1e-12)
    return np.arctan(np.sqrt(term)) * 180.0 / np.pi


def main():
    rr, cc = np.mgrid[0:N, 0:N].astype(np.float64)
    # NON-affine smooth maps so single-pixel sample != patch mean.
    # altitude ~ -25..+25 with a ripple; azimuth ~ -35..+35 with a ripple.
    alt = (rr / (N - 1) * 50.0 - 25.0) + 4.0 * np.sin(cc / 5.0) + 3.0 * np.cos(rr / 7.0)
    azi = (cc / (N - 1) * 70.0 - 35.0) + 4.0 * np.cos(rr / 6.0) + 3.0 * np.sin(cc / 8.0)

    # area_labels
    labels = np.zeros((N, N), dtype=np.int32)
    # V1 = large convex blob, label 1
    yy, xx = np.mgrid[0:N, 0:N]
    v1blob = ((xx - 22) ** 2 + (yy - 30) ** 2) <= 14 ** 2
    labels[v1blob] = 1
    # thin 1px spur off V1 (opening with disk-10 removes it; shifts pixel CoM)
    labels[30, 36:50] = 1
    # medium second area, label 2
    a2 = ((xx - 48) ** 2 + (yy - 18) ** 2) <= 8 ** 2
    labels[a2 & (labels == 0)] = 2
    # small third area, label 3
    a3 = ((xx - 50) ** 2 + (yy - 50) ** 2) <= 5 ** 2
    labels[a3 & (labels == 0)] = 3

    # ---- SNLC oracle center selection ----
    imseg = labels > 0
    se = disk_se(DISK_R)
    imdum = ni.binary_opening(imseg, structure=se)

    v1id, _, _ = getV1id(imdum)
    com, comlbl, _ = getPatchCoM(imdum)
    # CoMxy for V1 component (note: getV1id & getPatchCoM both relabel imdum the
    # same way since same input/connectivity, so v1id indexes com consistently).
    com_x = com[v1id - 1, 0]
    com_y = com[v1id - 1, 1]
    pr = int(round(com_y))
    pc = int(round(com_x))
    vcent_azi = azi[pr, pc]   # kmap_hor sampled at CoM
    vcent_alt = alt[pr, pc]   # kmap_vert sampled at CoM

    # ---- our-Rust center selection (mean over V1 pixels, no imopen) ----
    # V1 = largest label in the ORIGINAL labels (Rust uses area_labels directly).
    rust_counts = {l: int(np.sum(labels == l)) for l in range(1, labels.max() + 1)}
    rust_v1 = max(rust_counts, key=lambda k: rust_counts[k])
    rmask = labels == rust_v1
    rust_alt_c = float(alt[rmask].mean())
    rust_azi_c = float(azi[rmask].mean())

    # ---- maps ----
    snlc_full = snlc_ecc(azi, alt, vcent_azi, vcent_alt)
    rust_full = rust_ecc(alt, azi, rust_alt_c, rust_azi_c)
    mask = labels > 0
    snlc_map = np.where(mask, snlc_full, 0.0)
    rust_map = np.where(mask, rust_full, 0.0)

    # ---- write fixtures ----
    alt.astype('<f8').tofile(os.path.join(FIX, "v1ecc_alt.bin"))
    azi.astype('<f8').tofile(os.path.join(FIX, "v1ecc_azi.bin"))
    labels.astype('<i4').tofile(os.path.join(FIX, "v1ecc_labels.bin"))
    np.array([rust_alt_c, rust_azi_c], dtype='<f8').tofile(
        os.path.join(FIX, "v1ecc_rust_center.bin"))
    np.array([vcent_alt, vcent_azi], dtype='<f8').tofile(
        os.path.join(FIX, "v1ecc_snlc_center.bin"))
    snlc_map.astype('<f8').tofile(os.path.join(FIX, "v1ecc_snlc_map.bin"))
    rust_map.astype('<f8').tofile(os.path.join(FIX, "v1ecc_rust_map.bin"))

    # ---- stats ----
    print(f"  N={N}  labels: 1={rust_counts.get(1)} 2={rust_counts.get(2)} 3={rust_counts.get(3)}")
    print(f"  imseg sum={int(imseg.sum())}  imdum(opened) sum={int(imdum.sum())}  "
          f"(opening removed {int(imseg.sum()-imdum.sum())} px)")
    print(f"  SNLC V1 label(after open)={v1id}  pixel CoM=(row {com_y:.3f}, col {com_x:.3f}) "
          f"-> round=({pr},{pc})")
    print(f"  Rust  V1 label(orig)={rust_v1}")
    print(f"  SNLC center  altC={vcent_alt:.6f}  aziC={vcent_azi:.6f}  (single-pixel sample)")
    print(f"  Rust  center altC={rust_alt_c:.6f}  aziC={rust_azi_c:.6f}  (patch mean)")
    print(f"  center delta: dAlt={abs(vcent_alt-rust_alt_c):.6f}  dAzi={abs(vcent_azi-rust_azi_c):.6f} deg")
    insum_s = float(snlc_map[mask].sum())
    insum_r = float(rust_map[mask].sum())
    print(f"  SNLC ecc map: in-mask sum={insum_s:.4f}  range=[{snlc_map[mask].min():.4f},{snlc_map[mask].max():.4f}]")
    print(f"  Rust ecc map: in-mask sum={insum_r:.4f}  range=[{rust_map[mask].min():.4f},{rust_map[mask].max():.4f}]")
    print(f"  max|SNLC-Rust| over mask = {np.abs(snlc_map[mask]-rust_map[mask]).max():.4f} deg")


if __name__ == "__main__":
    main()
