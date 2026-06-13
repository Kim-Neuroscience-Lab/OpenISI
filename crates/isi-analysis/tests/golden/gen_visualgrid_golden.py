"""Golden for `derive_visual_grid` + `patch_visual_space`
(`methods/patch_refinement.rs` L336 / L394) against a VERBATIM transcription of
Allen `RetinotopicMapping.getVisualSpace` grid/projection
(`RetinotopicMapping.py` L2745-2784).

THE ORACLE FACTS (verbatim from getVisualSpace):
  - altRange and aziRange are HARDCODED CONSTANTS, NOT derived from data:
        altRange = np.array([-40., 60.])
        aziRange = np.array([-20., 120.])
  - grid shape = (ceil((60-(-40))/ps), ceil((120-(-20))/ps))
                = (ceil(100/ps), ceil(140/ps))
  - origin   = (altRange[0]=-40, aziRange[0]=-20)
  - inclusion test is HALF-OPEN against the fixed range:
        corAlt >= -40 & corAlt < 60 & corAzi >= -20 & corAzi < 120
  - index = floor((cor - rangeMin) / ps)  (Py `//` floor-division)
  - closeIter>=1 -> ni.binary_closing(visualSpace, iterations=closeIter)
        (scipy default structure = 3x3 cross / rank-1 connectivity)

`getVisualSpace` below is verbatim except: Py2 `np.float`/`np.int` -> `float`/`int`
(NumPy 2 removed those aliases), and visualFieldOrigin=None branch (we pass
None, the default). No computational line altered.

Stress design: the cortical patch's alt/azi VALUES deliberately span only a
SUBSET of the fixed range (alt in ~[-5, 35], azi in ~[10, 80]). A
data-bounding-box grid (what our Rust derive_visual_grid does) would put the
origin near (-5-pad, 10-pad) and size the grid to ~the data span; Allen's grid
is the fixed [-40,60]x[-20,120] -> origin (-40,-20), size (ceil(100/ps),
ceil(140/ps)). Same pixels light up at DIFFERENT (i,j) and the grid SHAPE
differs. We also include a few pixels at/just-past the range borders (corAlt ==
-40 included; corAlt == 60 excluded; corAzi == 120 excluded) to pin the
half-open convention and floor indexing.

Outputs (all in fixtures/, row-major C-order):
  visgrid_patch.bin   patch mask (uint8) HxW
  visgrid_alt.bin      alt map (<f8)      HxW
  visgrid_azi.bin      azi map (<f8)      HxW
  visgrid_vs.bin       Allen visualSpace result (uint8) GH x GW (post-closing)
And prints the Allen grid dims (GH, GW), origin, uniqueArea so the Rust test can
hardcode the expected grid extent.

Run:  python gen_visualgrid_golden.py
"""
import os
import numpy as np
import scipy.ndimage as ni

H, W = 24, 24                 # cortical patch raster size
PIXEL_SIZE = 0.5              # matches PatchRefinementAllenVisualSpacePixelSize default
CLOSE_ITER = 3
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


def getVisualSpace(patchArray, altMap, aziMap, pixelSize=1., closeIter=None):
    # --- verbatim from RetinotopicMapping.py:2754-2784 (see module docstring) ---
    pixelSize = float(pixelSize)

    altRange = np.array([-40., 60.])
    aziRange = np.array([-20., 120.])

    # visualFieldOrigin=None -> no shift (default branch)

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
    return visualSpace, uniqueArea, altRange, aziRange
    # --- end verbatim ---


def main():
    rng = np.random.default_rng(20260611)

    # Patch mask: a connected-ish blob plus a couple of border-probe pixels.
    patch = np.zeros((H, W), dtype=int)
    patch[6:18, 6:18] = 1
    patch[3, 3] = 1     # border-probe A (exact range minimum)
    patch[4, 4] = 1     # border-probe B (just past azi max -> excluded)
    patch[5, 5] = 1     # border-probe C (exact alt max == 60 -> excluded, half-open)

    # alt/azi maps. Blob spans alt~[-5,35], azi~[10,80] (a SUBSET of the fixed
    # range, so a data-bbox grid would diverge from Allen's fixed grid).
    alt = np.full((H, W), np.nan, dtype=np.float64)
    azi = np.full((H, W), np.nan, dtype=np.float64)
    # smooth ramps over the blob region
    rr = np.linspace(-5.0, 35.0, 12)
    cc = np.linspace(10.0, 80.0, 12)
    for ii, r in enumerate(range(6, 18)):
        for jj, c in enumerate(range(6, 18)):
            alt[r, c] = rr[ii] + 0.3 * rng.standard_normal()
            azi[r, c] = cc[jj] + 0.3 * rng.standard_normal()

    # border-probe values (pin half-open + floor):
    alt[3, 3] = -40.0; azi[3, 3] = -20.0      # exact minima -> included at (0,0)
    alt[4, 4] = 5.0;   azi[4, 4] = 120.0      # azi == 120 -> EXCLUDED (half-open)
    alt[5, 5] = 60.0;  azi[5, 5] = 30.0       # alt == 60  -> EXCLUDED (half-open)

    vs, ua, altRange, aziRange = getVisualSpace(
        patch, alt, azi, pixelSize=PIXEL_SIZE, closeIter=CLOSE_ITER)

    GH, GW = vs.shape

    # Replace NaN with a sentinel for the f8 fixtures? No: keep NaN. The Rust
    # side treats non-finite as "skip", same as Allen (NaN fails the >= / <
    # comparisons -> excluded). Write raw little-endian f8 incl. NaN bit pattern.
    np.ascontiguousarray(patch.astype(np.uint8)).tofile(os.path.join(FIX, "visgrid_patch.bin"))
    np.ascontiguousarray(alt.astype("<f8")).tofile(os.path.join(FIX, "visgrid_alt.bin"))
    np.ascontiguousarray(azi.astype("<f8")).tofile(os.path.join(FIX, "visgrid_azi.bin"))
    np.ascontiguousarray(vs.astype(np.uint8)).tofile(os.path.join(FIX, "visgrid_vs.bin"))

    on = int(vs.sum())
    print(f"  patch H={H} W={W} pixel_size={PIXEL_SIZE} close_iter={CLOSE_ITER}")
    print(f"  patch sum (on pixels)         = {int(patch.sum())}")
    print(f"  ALLEN grid shape (GH, GW)     = ({GH}, {GW})  [= ceil(100/ps), ceil(140/ps)]")
    print(f"  ALLEN origin (alt0, azi0)     = ({altRange[0]}, {aziRange[0]})")
    print(f"  visualSpace on-pixels (post-close) = {on}")
    print(f"  uniqueArea                    = {ua}")
    print(f"  expected GH=ceil(100/{PIXEL_SIZE})={int(np.ceil(100/PIXEL_SIZE))} "
          f"GW=ceil(140/{PIXEL_SIZE})={int(np.ceil(140/PIXEL_SIZE))}")
    # what a DATA-BBOX grid (our Rust) would produce, for contrast:
    fa = alt[np.isfinite(alt)]; fz = azi[np.isfinite(azi)]
    bb_h = int(np.ceil(((fa.max()+PIXEL_SIZE) - (fa.min()-PIXEL_SIZE)) / PIXEL_SIZE))
    bb_w = int(np.ceil(((fz.max()+PIXEL_SIZE) - (fz.min()-PIXEL_SIZE)) / PIXEL_SIZE))
    print(f"  [contrast] data-bbox grid would be (~{bb_h}, ~{bb_w}) origin "
          f"(~{fa.min()-PIXEL_SIZE:.2f}, ~{fz.min()-PIXEL_SIZE:.2f}) -- DIVERGES")


if __name__ == "__main__":
    main()
