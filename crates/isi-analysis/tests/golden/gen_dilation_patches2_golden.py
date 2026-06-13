"""Golden for `dilation_patches2_allen` (segmentation/connectivity.rs) against a
VERBATIM transcription of Allen `dilationPatches2`
(`RetinotopicMapping.py` L190-225). This is the patch-separation sub-component:
bulk-dilate the seed patches, skeletonize the halo where dilations collide, cut
that skeleton out of the dilated union, and keep only the components that still
overlap a seed.

`dilationPatches2` below is verbatim except: Py2 `xrange`->`range`,
`np.int`->`int` (NumPy 2 removed the alias), and the import of the two library
calls it makes (`scipy.ndimage` `binary_dilation`/`label`, `skimage`
`skeletonize`) — no computational line altered. scipy `binary_dilation` default
structure is the 4-conn cross, matching our `binary_dilation_cross`.

Input seeds: two square patches placed so a `dilation_iter` dilation makes their
halos collide, forcing a separating skeleton (the interesting case). border_width
left at the Allen default of 1.

Output: fixtures/dilpatch_raw.bin (seed patches, uint8),
        fixtures/dilpatch_out.bin (Allen dilationPatches2 result, uint8); NxN.
Run:  python gen_dilation_patches2_golden.py
"""
import os
import numpy as np
import scipy.ndimage as ni
from skimage.morphology import skeletonize

N = 64
DILATION_ITER = 8
BORDER_WIDTH = 1
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


def dilationPatches2(rawPatches, dilationIter=20, borderWidth=1):
    # --- verbatim from RetinotopicMapping.py:190-225 (see module docstring) ---
    total_area = ni.binary_dilation(rawPatches, iterations=dilationIter).astype(int)
    patchBorder = total_area - rawPatches

    patchBorder = skeletonize(patchBorder)

    if borderWidth > 1:
        patchBorder = ni.binary_dilation(patchBorder, iterations=borderWidth - 1).astype(int)

    newPatches = np.multiply(-1 * (patchBorder - 1), total_area)

    labeledPatches, patchNum = ni.label(newPatches)

    newPatches2 = np.zeros(newPatches.shape, dtype=int)

    for i in range(1, patchNum + 1):
        currPatch = np.zeros(labeledPatches.shape, dtype=int)
        currPatch[labeledPatches == i] = 1
        currPatch[labeledPatches != i] = 0

        if (np.sum(np.multiply(currPatch, rawPatches)[:]) > 0):
            newPatches2[currPatch == 1] = 1

    return newPatches2
    # --- end verbatim ---


def main():
    raw = np.zeros((N, N), dtype=int)
    raw[16:30, 14:28] = 1          # patch A
    raw[34:50, 36:52] = 1          # patch B (close enough to collide when dilated)

    out = dilationPatches2(raw, dilationIter=DILATION_ITER, borderWidth=BORDER_WIDTH)

    np.ascontiguousarray(raw.astype(np.uint8)).tofile(os.path.join(FIX, "dilpatch_raw.bin"))
    np.ascontiguousarray(out.astype(np.uint8)).tofile(os.path.join(FIX, "dilpatch_out.bin"))
    _, n_raw = ni.label(raw)
    _, n_out = ni.label(out)
    print(f"  dilation_iter={DILATION_ITER} border_width={BORDER_WIDTH}")
    print(f"  raw patches={n_raw} sum={int(raw.sum())}  ->  out patches={n_out} sum={int(out.sum())}")
    print(f"  grid N={N}, uint8 row-major")


if __name__ == "__main__":
    main()
