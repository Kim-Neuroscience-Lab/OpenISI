"""Golden for `local_min_markers` (methods/patch_refinement.rs, `mod allen`)
against a VERBATIM transcription of Allen `localMin`
(`RetinotopicMapping.py` L382-414).

`localMin` finds the seed markers for the eccentricity-watershed patch split:
starting from a threshold of `nanmin(ecc) - binSize`, it raises the cut in
`binSize` steps, labels `ecc <= cut` with `scipy.ndimage.label` (default 4-conn
cross) each step, and stops at the FIRST cut producing >= 2 connected
components, returning that label map.

Transcription notes (no computational line altered):
  * `np.int` -> `int` (NumPy 2 removed the `np.int` alias). This only affects
    `marker` dtype; values are 0/1 either way.
  * `ni.measurements.label` -> `ni.label` (scipy moved/aliased the symbol;
    identical function, identical default structure = 4-conn cross).
  * The threshold grid is EXACTLY Allen's:
        cutStep = np.arange(nanmin - binSize, nanmax + binSize*2, binSize)
    -> note the UPPER bound is `nanmax + 2*binSize` (arange is half-open), and
       the loop runs `while (NumOfMin <= 1) and (i < len(cutStep))`, i.e. it
       stops the instant NumOfMin >= 2.

scipy `ni.label` numbers components in C-order (row-major) first-touch raster
scan, which matches our Rust `label_4conn` BFS seeding order, so the *label
values* (not just the partition) are expected bit-identical.

Cases (each NxN f64 '<f8' ecc map -> i32 '<i4' marker map):
  A "two_basins"  : two clear low-ecc basins -> split at an early cut; exercises
                    the normal >=2-CC stop and exact label numbering (1,2).
  B "border_min"  : a low-ecc basin touching the image border + an interior one;
                    exercises edge components (no border padding in label) and a
                    NaN region that must never become a marker.
  C "tie_step"    : two basins at IDENTICAL ecc so they both appear at the SAME
                    cut step (tie); verifies the step at which the split fires
                    and that both are found together, not one-then-two.
  D "single_min"  : one monotone bowl (single global min). NumOfMin never
                    exceeds 1, so the loop exhausts `cutStep`; the returned
                    marker is the label map at the LAST cut (one big CC).
                    This is the branch most sensitive to the arange upper bound.

Output fixtures (in fixtures/): for each case
  lmin_<case>_ecc.bin   (f64 '<f8', NxN, row-major; NaN encoded as f64 NaN)
  lmin_<case>_marker.bin (i32 '<i4', NxN, row-major)
Run:  python gen_local_min_golden.py
"""
import os
import numpy as np
import scipy.ndimage as ni

N = 32
BIN_SIZE = 0.5
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


def localMin(eccMap, binSize):
    # --- verbatim from RetinotopicMapping.py:382-414 (see module docstring) ---
    eccMap2 = np.array(eccMap)
    cutStep = np.arange(np.nanmin(eccMap2[:]) - binSize,
                        np.nanmax(eccMap2[:]) + binSize * 2,
                        binSize)
    NumOfMin = 0
    i = 0
    while (NumOfMin <= 1) and (i < len(cutStep)):
        currThr = cutStep[i]
        marker = np.zeros(eccMap.shape, dtype=int)   # np.int -> int
        marker[eccMap2 <= (currThr)] = 1
        marker, NumOfMin = ni.label(marker)          # ni.measurements.label -> ni.label
        i = i + 1
    return marker, NumOfMin, i, len(cutStep)
    # --- end verbatim (return tuple extended for diagnostics only) ---


def make_two_basins():
    # Large flat high-ecc plateau with two distinct low-ecc wells, well separated.
    ecc = np.full((N, N), 10.0)
    # well 1 (upper-left interior)
    ecc[6:10, 6:10] = 1.0
    ecc[5:11, 5:11] = np.minimum(ecc[5:11, 5:11], 2.0)
    # well 2 (lower-right interior)
    ecc[22:26, 22:26] = 1.5
    ecc[21:27, 21:27] = np.minimum(ecc[21:27, 21:27], 3.0)
    return ecc


def make_border_min():
    # A low-ecc basin glued to the top-left CORNER (tests edge components, no
    # border padding) + an interior basin, + a NaN block that must never label.
    ecc = np.full((N, N), 8.0)
    ecc[0:5, 0:5] = 1.0          # corner basin (touches both borders)
    ecc[18:22, 18:22] = 1.0      # interior basin at SAME level (tie at same cut)
    ecc[0:6, 26:32] = np.nan     # NaN region (ignored by nanmin/nanmax and <=)
    return ecc


def make_tie_step():
    # Two basins at exactly the same minimum so they both cross threshold on the
    # identical arange step -> NumOfMin jumps 0 -> 2 at one step.
    ecc = np.full((N, N), 5.0)
    ecc[8:12, 4:8] = 0.25
    ecc[8:12, 24:28] = 0.25      # identical value, far apart
    return ecc


def make_single_min():
    # Monotone radial bowl: one global minimum, the sub-threshold region is
    # always a single connected blob -> NumOfMin stays 1, loop exhausts cutStep.
    yy, xx = np.mgrid[0:N, 0:N]
    cy = cx = (N - 1) / 2.0
    ecc = np.sqrt((yy - cy) ** 2 + (xx - cx) ** 2) * 0.3
    return ecc.astype(np.float64)


def dump(case, ecc):
    ecc = np.ascontiguousarray(ecc.astype(np.float64))
    marker, num, i, nsteps = localMin(ecc, BIN_SIZE)
    marker = np.ascontiguousarray(marker.astype(np.int32))
    ecc.tofile(os.path.join(FIX, f"lmin_{case}_ecc.bin"))
    marker.tofile(os.path.join(FIX, f"lmin_{case}_marker.bin"))
    vmin = np.nanmin(ecc)
    vmax = np.nanmax(ecc)
    nan_ct = int(np.isnan(ecc).sum())
    fire_thr = vmin - BIN_SIZE + (i - 1) * BIN_SIZE if num >= 2 else float("nan")
    print(f"  {case:11} N={N} bin={BIN_SIZE}  nanmin={vmin:.4f} nanmax={vmax:.4f} "
          f"nan={nan_ct} steps={nsteps} stopped_at_i={i} NumOfMin={num} "
          f"fire_thr={fire_thr:.4f} labelsum={int(marker.sum())} maxlabel={int(marker.max())}")


def main():
    print(f"localMin golden: grid {N}x{N}, bin_size={BIN_SIZE}, "
          f"ecc f64<f8, marker i32<i4, row-major")
    dump("two_basins", make_two_basins())
    dump("border_min", make_border_min())
    dump("tie_step", make_tie_step())
    dump("single_min", make_single_min())


if __name__ == "__main__":
    main()
