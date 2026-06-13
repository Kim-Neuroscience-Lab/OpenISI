"""Golden for `segment_threshold_only` (segmentation/mod.rs:158), whose docstring
claims to match Allen `signMapThr` direct-threshold segmentation.

The Allen oracle is `RetinotopicMapping._getRawPatchMap`
(`RetinotopicMapping.py` L1089-1124). The relevant lines (verbatim, only the
Py3/NumPy-2 alias fix `np.int`->`int` applied; no computational line altered):

    patchmap = np.zeros(signMapf.shape)
    patchmap[signMapf >= signMapThr] = 1
    patchmap[signMapf <= -1 * signMapThr] = 1
    patchmap[(signMapf < signMapThr) & (signMapf > -1 * signMapThr)] = 0
    patchmap = ni.binary_opening(np.abs(patchmap), iterations=openIter).astype(int)
    patches, patchNum = ni.label(patchmap)

KEY ORACLE FACTS captured here:
  * threshold predicate: |signMapf| >= signMapThr (boundary INCLUDED via >=).
  * opening: scipy.ndimage.binary_opening with DEFAULT structure
    = generate_binary_structure(2,1) = the 4-connected CROSS, iterated
    openIter (=3) times. This equals a Manhattan/diamond SE of radius 3
    (13 px), NOT a Euclidean disk of radius 3 (29 px).
  * border_value defaults to 0 in scipy binary_opening => border pixels that
    would need an out-of-image neighbour to survive erosion DO erode.
  * labeling: ni.label default structure = 4-conn cross.

Our Rust `segment_threshold_only` instead applies `binary_opening_disk(imseg, 3)`
(Euclidean disk radius 3, 29 px, and erosion pads the image border with 1s so
edge pixels never erode from the boundary alone). So we emit BOTH:

  thronly_open_disk3   = imopen with disk-3 (what OUR Rust would compute)  -- for diff
  thronly_open_allen   = ni.binary_opening(iterations=3) (the Allen oracle)
  thronly_label_allen  = ni.label of the Allen-opened mask (int32 labels)

The Rust test asserts our disk-3 opening EQUALS the Allen cross-iter-3 opening;
it is expected to FAIL (diverge), proving the citation mismatch. Once production
is switched to a cross-iter opening with border_value=0 it will pass.

Inputs are constructed to stress: the >= boundary (exact-threshold pixels),
thin 1px necks (open removes them under both SEs but at different widths),
disk-vs-cross corner behaviour, and image-border survival (border_value).

Output (all NxN, row-major, C-order, little-endian):
  fixtures/thronly_vfs.bin        signed VFS field            <f8
  fixtures/thronly_thr_mask.bin   |vfs|>=thr predicate mask   uint8
  fixtures/thronly_open_allen.bin Allen cross-iter-3 opening  uint8
  fixtures/thronly_open_disk3.bin Euclidean disk-3 opening    uint8 (reference only)
  fixtures/thronly_labels.bin     ni.label(Allen opening)     <i4
Run:  python gen_thronly_golden.py
"""
import os
import numpy as np
import scipy.ndimage as ni

N = 64
THR = 0.35           # Allen signMapThr default
OPEN_ITER = 3        # Allen openIter default
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


def build_vfs():
    """A signed VFS-like field with structures that stress the operators."""
    rng = np.random.default_rng(20260611)
    vfs = np.zeros((N, N), dtype=np.float64)

    # Block A: solid positive square touching the LEFT image border (tests
    # border_value: scipy erodes the boundary column, disk-pad-1 keeps it).
    vfs[8:24, 0:14] = 0.8

    # Block B: solid negative square, interior.
    vfs[40:58, 40:58] = -0.7

    # A 1-px thin neck connecting two blobs (open removes thin necks; the
    # disk-3 erosion removes a wider margin than cross-iter-3).
    vfs[30, 8:40] = 0.9
    vfs[26:35, 6:12] = 0.9       # left lobe of the neck
    vfs[26:35, 36:42] = 0.9      # right lobe of the neck

    # Exact-threshold pixels: value == THR exactly -> included by >=, excluded
    # by >. A 3x3 block so it survives... under cross but maybe not disk.
    vfs[50:55, 4:9] = THR        # exactly +0.35

    # Just-below-threshold speckle (must be excluded): value = THR - 1e-9
    vfs[2:5, 28:32] = THR - 1e-6

    # Small isolated noise specks that opening should erase under both SEs.
    for (r, c) in [(60, 2), (2, 60), (61, 61), (33, 55)]:
        vfs[r, c] = 0.9

    # A diagonal-corner test: an L / staircase where 8-conn diagonal contact
    # matters for cross vs disk dilation re-growth.
    vfs[12:20, 50:58] = -0.6
    vfs[19, 58] = -0.6
    vfs[20, 59] = -0.6

    # light noise to make sign-means nontrivial (kept well below THR so it does
    # not flip the predicate)
    vfs += rng.normal(0.0, 0.02, size=(N, N))
    return vfs


def main():
    vfs = build_vfs()

    # --- Allen threshold predicate (verbatim) ---
    patchmap = np.zeros(vfs.shape)
    patchmap[vfs >= THR] = 1
    patchmap[vfs <= -1 * THR] = 1
    patchmap[(vfs < THR) & (vfs > -1 * THR)] = 0
    thr_mask = patchmap.astype(bool)

    # --- Allen opening: scipy default 4-conn cross, iterations=openIter ---
    open_allen = ni.binary_opening(np.abs(patchmap), iterations=OPEN_ITER).astype(bool)

    # --- ni.label default (4-conn cross) on the opened mask ---
    labels, num = ni.label(open_allen.astype(int))

    # --- Euclidean disk-3 opening, MATLAB strel('disk',3,0) semantics, with
    #     erosion border-padded by 1 (foreground) — this mirrors OUR Rust
    #     binary_opening_disk(.,3). Provided as a reference to show divergence. ---
    rr = 3
    yy, xx = np.mgrid[-rr:rr + 1, -rr:rr + 1]
    disk = (yy * yy + xx * xx) <= rr * rr + 1e-9
    # erosion with border padded TRUE (1s): pad, erode (no border_value flag in
    # grey_erosion; emulate via binary_erosion with border_value=1)
    eroded = ni.binary_erosion(thr_mask, structure=disk, border_value=1)
    open_disk3 = ni.binary_dilation(eroded, structure=disk, border_value=0)

    # write fixtures
    np.ascontiguousarray(vfs.astype("<f8")).tofile(os.path.join(FIX, "thronly_vfs.bin"))
    np.ascontiguousarray(thr_mask.astype(np.uint8)).tofile(os.path.join(FIX, "thronly_thr_mask.bin"))
    np.ascontiguousarray(open_allen.astype(np.uint8)).tofile(os.path.join(FIX, "thronly_open_allen.bin"))
    np.ascontiguousarray(open_disk3.astype(np.uint8)).tofile(os.path.join(FIX, "thronly_open_disk3.bin"))
    np.ascontiguousarray(labels.astype("<i4")).tofile(os.path.join(FIX, "thronly_labels.bin"))

    diff = int(np.sum(open_allen != open_disk3))
    print(f"  N={N}  THR={THR}  openIter={OPEN_ITER}")
    print(f"  vfs range=[{vfs.min():.4f},{vfs.max():.4f}] sum={vfs.sum():.4f}")
    print(f"  thr_mask sum (px>=thr) = {int(thr_mask.sum())}")
    print(f"  open_allen (cross-iter3) sum = {int(open_allen.sum())}")
    print(f"  open_disk3 (disk r3)     sum = {int(open_disk3.sum())}")
    print(f"  >>> opening DIVERGENCE (allen != disk3) = {diff} px")
    print(f"  ni.label components on Allen opening = {num}")
    print(f"  fixtures written to {FIX}")


if __name__ == "__main__":
    main()
