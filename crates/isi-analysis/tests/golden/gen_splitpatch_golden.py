"""Golden for `split_patch` (methods/patch_refinement.rs:715) against a VERBATIM
transcription of Allen `Patch.split2` (RetinotopicMapping.py L2853-2909, the
`sm.watershed` branch — NOT the cv2.watershed `split`).

split2 below is verbatim from the Allen source except for Py2->Py3 / NumPy-2 fixes
(NO computational line altered):
  - `xrange` -> `range`
  - `np.int` removed in NumPy 2 -> `int`
  - `sm.watershed` -> `skimage.segmentation.watershed` (watershed moved package in
    newer skimage; same algorithm — priority-queue marker flooding, connectivity
    = full 3x3 / 8-conn, restricted to `mask`)
  - `sm.skeletonize` -> `skimage.morphology.skeletonize` (the LUT _fast_skeletonize)
  - `localMin` is also transcribed verbatim (the marker generator split2 calls).

Library-call fidelity:
  - ni.binary_dilation default structure = 4-conn cross (confirmed)
  - ni.label default = 4-conn (confirmed)
  - skeletonize = skimage LUT skeletonize (already validated bit-for-bit in-crate)

We bypass the eccentricity-map construction entirely: split2 takes a precomputed
`eccMap` (NaN outside the patch). We feed it a hand-built eccMap with multiple
local minima so the split actually fires, plus the patch mask `self.array`.

This isolates the split2 composition: localMin -> watershed -> border(union of
whole-patch border + per-region borders) -> skeletonize -> dilate -> cut from
dilated patch -> label -> AND patch. The Rust under test reimplements exactly
this composition, so the fixtures cross-check it end to end.

Inputs written:
  fixtures/splitpatch_mask.bin   (self.array, uint8, NxN)
  fixtures/splitpatch_ecc.bin    (eccMap, <f8, NxN, NaN outside patch)
Scalars baked into the Rust test: cutStep, borderWidth.
Expected outputs:
  fixtures/splitpatch_nlabels.bin  (1 x int32 LE: number of output patches)
  fixtures/splitpatch_labels.bin   (int32 LE NxN: labeledNewPatchMap*self.array,
                                    i.e. each output patch's label id, 0=background;
                                    this is the post-AND label field. Rust rebuilds
                                    its own per-patch masks and we compare the SET
                                    of masks, order-independent.)
  fixtures/splitpatch_union.bin    (uint8 NxN: union of all output patch masks =
                                    OR over the returned Patch arrays; order-free)

Run:  python gen_splitpatch_golden.py
"""
import os
import numpy as np
import scipy.ndimage as ni
from skimage.segmentation import watershed
from skimage.morphology import skeletonize

N = 48
CUT_STEP = 1.0
BORDER_WIDTH = 2
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


def localMin(eccMap, binSize):
    # --- verbatim RetinotopicMapping.py:382-413 (np.int->int) ---
    eccMap2 = np.array(eccMap)
    cutStep = np.arange(np.nanmin(eccMap2[:]) - binSize,
                        np.nanmax(eccMap2[:]) + binSize * 2,
                        binSize)
    NumOfMin = 0
    i = 0
    marker = np.zeros(eccMap.shape, dtype=int)
    while (NumOfMin <= 1) and (i < len(cutStep)):
        currThr = cutStep[i]
        marker = np.zeros(eccMap.shape, dtype=int)
        marker[eccMap2 <= (currThr)] = 1
        marker, NumOfMin = ni.label(marker)
        i = i + 1
    return marker
    # --- end verbatim ---


def split2(self_array, sign, eccMap, patchName='patch00', cutStep=1, borderWidth=2):
    # --- verbatim RetinotopicMapping.py:2853-2909 (sm.watershed branch),
    #     xrange->range, np.int8 kept, sm.->skimage., np alias fixes ---
    minMarker = localMin(eccMap, cutStep)

    connectivity = np.array([[1, 1, 1], [1, 1, 1], [1, 1, 1]])

    newLabel = watershed(eccMap, minMarker, connectivity=connectivity, mask=self_array)

    border = ni.binary_dilation(self_array).astype(np.int8) - self_array

    for i in range(1, np.amax(newLabel) + 1):
        currArray = np.zeros(self_array.shape, dtype=np.int8)
        currArray[newLabel == i] = 1
        currBorder = ni.binary_dilation(currArray).astype(np.int8) - currArray
        border = border + currBorder

    border[border > 1] = 1
    border = skeletonize(border)

    if borderWidth > 1:
        border = ni.binary_dilation(border, iterations=borderWidth - 1).astype(np.int8)

    newPatchMap = ni.binary_dilation(self_array).astype(np.int8) * (-1 * (border - 1))

    labeledNewPatchMap, patchNum = ni.label(newPatchMap)

    newPatchDict = {}
    for j in range(1, patchNum + 1):
        currPatchName = patchName + '.' + str(j)
        currArray = np.zeros(self_array.shape, dtype=np.int8)
        currArray[labeledNewPatchMap == j] = 1
        currArray = currArray * self_array
        if np.sum(currArray[:]) > 0:
            newPatchDict.update({currPatchName: currArray})

    return newPatchDict, labeledNewPatchMap, newLabel, minMarker
    # --- end verbatim composition ---


def build_case():
    """Plateau-heavy eccentricity field: a flat valley (lots of TIES) with two
    point minima. This deliberately stresses the watershed tie-break / flooding
    order — the regime where skimage's priority-queue flood (no watershed line,
    FIFO age tie-break) diverges most from a naive synchronous-BFS watershed.
    Patch is a rectangle; the flat plateau between the two wells is where the
    region boundary placement matters for the subsequent border cut."""
    mask = np.zeros((N, N), dtype=np.int8)
    mask[5:35, 5:35] = 1  # rectangular patch (N=48, so well inside)

    ecc = np.full((N, N), np.nan, dtype=np.float64)
    ecc[mask == 1] = 10.0          # flat plateau everywhere in patch
    ecc[10, 12] = 0.0              # well 1
    ecc[28, 26] = 0.0              # well 2
    return mask, ecc


def main():
    mask, ecc = build_case()
    sign = 1

    patches, labeledNewPatchMap, newLabel, minMarker = split2(
        mask, sign, ecc, cutStep=CUT_STEP, borderWidth=BORDER_WIDTH
    )

    # union of output patch masks (order-independent ground truth)
    union = np.zeros((N, N), dtype=np.uint8)
    for arr in patches.values():
        union[arr != 0] = 1

    # post-AND label field: relabel each output patch with a fresh id 1..K,
    # but to keep it order-free for comparison we just emit labeledNewPatchMap
    # masked by self_array (the raw label ids that survived).
    post_and_labels = (labeledNewPatchMap * mask).astype(np.int32)

    n_labels = np.array([len(patches)], dtype=np.int32)

    np.ascontiguousarray(mask.astype(np.uint8)).tofile(os.path.join(FIX, "splitpatch_mask.bin"))
    np.ascontiguousarray(ecc.astype("<f8")).tofile(os.path.join(FIX, "splitpatch_ecc.bin"))
    np.ascontiguousarray(n_labels.astype("<i4")).tofile(os.path.join(FIX, "splitpatch_nlabels.bin"))
    np.ascontiguousarray(post_and_labels.astype("<i4")).tofile(os.path.join(FIX, "splitpatch_labels.bin"))
    np.ascontiguousarray(union).tofile(os.path.join(FIX, "splitpatch_union.bin"))

    print(f"  N={N}  cutStep={CUT_STEP}  borderWidth={BORDER_WIDTH}")
    print(f"  mask sum={int(mask.sum())}  ecc finite={int(np.isfinite(ecc).sum())}")
    print(f"  ecc min={np.nanmin(ecc):.4f} max={np.nanmax(ecc):.4f}")
    print(f"  minMarker n_local_min={int(minMarker.max())}")
    print(f"  watershed newLabel max={int(newLabel.max())} (regions)")
    print(f"  N output patches={len(patches)}  union sum={int(union.sum())}")
    sizes = sorted(int((a != 0).sum()) for a in patches.values())
    print(f"  output patch sizes (sorted)={sizes}")


if __name__ == "__main__":
    main()
