"""Golden for `patch_visual_space` (methods/patch_refinement.rs:394) against a
VERBATIM transcription of Allen `Patch.getVisualSpace`
(RetinotopicMapping.py L2745-2797).

getVisualSpace scatters every True pixel of the cortical patch into a visual-space
grid at index `(corAlt-altRange[0])//pixelSize, (corAzi-aziRange[0])//pixelSize`
(floor division), gating on `altRange[0] <= corAlt < altRange[1]` and likewise for
azi. It then applies `scipy.ndimage.binary_closing(visualSpace, iterations=closeIter)`
(default cross SE, border_value=0) and reports uniqueArea = sum(visualSpace)*pixelSize**2.

The Allen function below is verbatim except Py2->Py3/np fixes:
  - `np.float(x)`  -> `float(x)`        (np.float alias removed)
  - `np.int(x)`    -> `int(x)`          (np.int alias removed)
  - dropped the matplotlib `isplot` branch (returns same values).
No computational line altered. scipy `binary_closing` default structure is the
4-conn cross, matching our `binary_closing_cross` (dilate border0, erode border0).

IMPORTANT ON GRID: Allen HARDCODES altRange=[-40,60], aziRange=[-20,120] and
origin = (altRange[0], aziRange[0]) = (-40,-20). Our Rust `patch_visual_space`
takes a `VisualGrid {alt_min, azi_min, pixel_size, h, w}` parameter and uses
`floor((a-alt_min)/pixel_size)`. To isolate the scatter+closing+area logic, this
golden BUILDS the Rust grid to exactly match Allen's hardcoded ranges:
    alt_min=-40, azi_min=-20, pixel_size=ps,
    h = ceil((60-(-40))/ps), w = ceil((120-(-20))/ps)
so any divergence found is in the projection/closing math, NOT the (separately
known-divergent) `derive_visual_grid` bounding-box logic.

Fixtures (all row-major, C-order):
  patchvs_mask_{case}.bin   patch mask, uint8, MASK_H x MASK_W (cortex grid)
  patchvs_alt_{case}.bin    altitude map, '<f8', MASK_H x MASK_W
  patchvs_azi_{case}.bin    azimuth map,  '<f8', MASK_H x MASK_W
  patchvs_out_{case}.bin    expected visualSpace, uint8, VS_H x VS_W
  patchvs_meta_{case}.bin   '<f8'[6] = [alt_min, azi_min, pixel_size, vs_h, vs_w, unique_area]
Run:  python gen_patchvs_golden.py
"""
import os
import numpy as np
import scipy.ndimage as ni

FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)

ALT_RANGE = np.array([-40.0, 60.0])
AZI_RANGE = np.array([-20.0, 120.0])


def getVisualSpace(patchArray, altMap, aziMap, visualFieldOrigin=None,
                   pixelSize=1.0, closeIter=None):
    # --- verbatim from RetinotopicMapping.py:2745-2797 (np alias fixes only) ---
    pixelSize = float(pixelSize)

    altRange = np.array([-40., 60.])
    aziRange = np.array([-20., 120.])

    if visualFieldOrigin:
        altMap = altMap - visualFieldOrigin[0]
        aziMap = aziMap - visualFieldOrigin[1]

    gridAzi, gridAlt = np.meshgrid(np.arange(aziRange[0], aziRange[1], pixelSize),
                                   np.arange(altRange[0], altRange[1], pixelSize))

    visualSpace = np.zeros((int(np.ceil((altRange[1] - altRange[0]) / pixelSize)),
                            int(np.ceil((aziRange[1] - aziRange[0]) / pixelSize))))

    for i in range(patchArray.shape[0]):
        for j in range(patchArray.shape[1]):
            if patchArray[i, j]:
                corAlt = altMap[i, j]
                corAzi = aziMap[i, j]
                if (corAlt >= altRange[0]) & (corAlt < altRange[1]) & (corAzi >= aziRange[0]) & (
                    corAzi < aziRange[1]):
                    indAlt = (corAlt - altRange[0]) // pixelSize
                    indAzi = (corAzi - aziRange[0]) // pixelSize
                    visualSpace[int(indAlt), int(indAzi)] = 1

    if closeIter >= 1:
        visualSpace = ni.binary_closing(visualSpace, iterations=closeIter).astype(int)

    uniqueArea = np.sum(visualSpace[:]) * (pixelSize ** 2)

    return visualSpace, uniqueArea
    # --- end verbatim ---


def write_case(name, mask, alt, azi, pixel_size, close_iter):
    vs, ua = getVisualSpace(mask, alt, azi, pixelSize=pixel_size, closeIter=close_iter)
    h, w = mask.shape
    vs_h, vs_w = vs.shape
    alt_min = float(ALT_RANGE[0])
    azi_min = float(AZI_RANGE[0])

    np.ascontiguousarray(mask.astype(np.uint8)).tofile(os.path.join(FIX, f"patchvs_mask_{name}.bin"))
    np.ascontiguousarray(alt.astype("<f8")).tofile(os.path.join(FIX, f"patchvs_alt_{name}.bin"))
    np.ascontiguousarray(azi.astype("<f8")).tofile(os.path.join(FIX, f"patchvs_azi_{name}.bin"))
    np.ascontiguousarray(vs.astype(np.uint8)).tofile(os.path.join(FIX, f"patchvs_out_{name}.bin"))
    meta = np.array([alt_min, azi_min, float(pixel_size), float(vs_h), float(vs_w), float(ua)], dtype="<f8")
    meta.tofile(os.path.join(FIX, f"patchvs_meta_{name}.bin"))

    print(f"[{name}] mask {h}x{w} sum={int(mask.sum())}  ps={pixel_size} close={close_iter}")
    print(f"        vs {vs_h}x{vs_w}  vs_sum={int(vs.sum())}  uniqueArea={ua}")
    print(f"        alt_min={alt_min} azi_min={azi_min}  alt[min,max]=[{np.nanmin(alt):.2f},{np.nanmax(alt):.2f}]"
          f" azi[min,max]=[{np.nanmin(azi):.2f},{np.nanmax(azi):.2f}]")
    return vs.sum(), ua


def main():
    rng = np.random.default_rng(20260611)
    H, W = 40, 40

    # ---- case "basic": ps=3, close=2, a diagonal sweep across visual space ----
    # alt/azi vary so projected pixels land in scattered cells (ties + gaps that
    # closing should bridge). Out-of-range pixels included to test the gate.
    mask = np.zeros((H, W), dtype=int)
    mask[8:32, 8:32] = 1
    # alt ramps -45..65 across rows (some below -40 and above/at 60 -> gated out),
    # azi ramps -25..125 across cols (some below -20 / >=120 -> gated out)
    rows = np.linspace(-45.0, 65.0, H)
    cols = np.linspace(-25.0, 125.0, W)
    alt = np.repeat(rows.reshape(H, 1), W, axis=1)
    azi = np.repeat(cols.reshape(1, W), H, axis=0)
    write_case("basic", mask, alt, azi, pixel_size=3.0, close_iter=2)

    # ---- case "exact": ps=2, close=1, values placed to hit floor-division
    # boundaries exactly (e.g. corAlt-(-40) == multiple of ps, and just below).
    mask2 = np.zeros((H, W), dtype=int)
    mask2[5:35, 5:35] = 1
    # craft alt so (corAlt+40) is exactly k*2 and also k*2 - tiny: tie sensitivity
    alt2 = np.zeros((H, W))
    azi2 = np.zeros((H, W))
    for i in range(H):
        for j in range(W):
            # exact boundary every other, fractional otherwise
            alt2[i, j] = -40.0 + (i * 2.0)            # exact multiples of ps=2
            azi2[i, j] = -20.0 + (j * 2.0) - 1e-9     # just below a boundary
    write_case("exact", mask2, alt2, azi2, pixel_size=2.0, close_iter=1)

    # ---- case "border": features projecting onto the visual-space border, ps=4
    # close=3, to stress binary_closing border erosion (border_value=0). Also
    # includes NaN pixels (must be skipped exactly like Allen's >= < comparisons,
    # which are False for NaN -> pixel skipped). ----
    mask3 = np.zeros((H, W), dtype=int)
    mask3[:, :] = 1
    alt3 = np.full((H, W), np.nan)
    azi3 = np.full((H, W), np.nan)
    # push a band of pixels to the extreme corners of visual space
    alt3[0:6, 0:6] = -39.5      # near alt_min -> row 0
    azi3[0:6, 0:6] = -19.5      # near azi_min -> col 0
    alt3[34:40, 34:40] = 59.5   # near alt_max
    azi3[34:40, 34:40] = 119.5  # near azi_max
    alt3[15:25, 15:25] = 10.0   # a central blob
    azi3[15:25, 15:25] = 50.0
    write_case("border", mask3, alt3, azi3, pixel_size=4.0, close_iter=3)

    # ---- case "random": ps=5, close=1, random within range; broad coverage ----
    mask4 = (rng.random((H, W)) < 0.5).astype(int)
    alt4 = rng.uniform(-40.0, 60.0, size=(H, W))
    azi4 = rng.uniform(-20.0, 120.0, size=(H, W))
    write_case("random", mask4, alt4, azi4, pixel_size=5.0, close_iter=1)


if __name__ == "__main__":
    main()
