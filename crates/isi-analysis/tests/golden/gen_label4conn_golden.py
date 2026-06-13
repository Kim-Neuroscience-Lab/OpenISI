"""Golden for `label_4conn` (segmentation/connectivity.rs:26) against the REAL
oracle: `scipy.ndimage.label(mask)` with its DEFAULT structure, which is the
4-connected cross `generate_binary_structure(2, 1)`:

    [[0 1 0]
     [1 1 1]
     [0 1 0]]

This is the same oracle our code cites (scipy.ndimage.label default 4-conn /
MATLAB bwlabel(mask,4)). The previously-validated path only checked the COUNT
(n) of components implicitly inside dilation_patches2. This golden pins the full
LABEL MAP including label VALUES, because downstream `patches_from_labels`
preserves IDs, so the ID-assignment ORDER is load-bearing.

KEY ORACLE FACT (verified empirically):
  scipy.ndimage.label uses union-find internally but RENUMBERS the final labels
  so that label k is assigned in raster (C-order, row-major) scan order of the
  FIRST-ENCOUNTERED pixel of each connected component. i.e. scanning row by row,
  left to right, the first new component you hit becomes 1, the next new one 2,
  etc. Background (0) stays 0. Diagonal neighbours are NOT connected.

Our Rust `label_4conn` scans row-major (`for r { for c {`), assigns the next
integer the moment it hits an unlabeled foreground pixel, then BFS-floods that
component. The flood does not change which component takes the next ID, so the
ID order is identical to scipy's raster-first-pixel order. This generator stress-
tests that equivalence on borders, ties, serpentine/U components whose first
pixel is far from their bulk, diagonal-only touches (must stay separate), and
many components.

Fixtures (all uint8 / int32, little-endian, C-order row-major, HxW):
  fixtures/label4conn_in_{case}.bin    input mask, uint8 (0/1)
  fixtures/label4conn_lab_{case}.bin   expected label map, int32 ('<i4')
plus a sidecar count is printed (and encoded as the array max).
Run:  python gen_label4conn_golden.py
"""
import os
import numpy as np
import scipy.ndimage as ni

FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


def emit(case, mask):
    mask = mask.astype(int)
    # REAL ORACLE: scipy default structure == 4-conn cross.
    lab, n = ni.label(mask)
    inp = np.ascontiguousarray(mask.astype(np.uint8))
    out = np.ascontiguousarray(lab.astype("<i4"))
    inp.tofile(os.path.join(FIX, f"label4conn_in_{case}.bin"))
    out.tofile(os.path.join(FIX, f"label4conn_lab_{case}.bin"))
    h, w = mask.shape
    print(f"  [{case:14}] shape={h}x{w} sum={int(mask.sum())} n={n} "
          f"label_max={int(lab.max())} label_sum={int(lab.sum())}")
    return n


def main():
    cases = {}

    # 1) BORDERS + ORDER: components touching every edge/corner; first-pixel
    #    raster order decides IDs. Top-left corner comp must be label 1.
    m = np.zeros((6, 8), dtype=int)
    m[0, 0] = 1                       # top-left corner -> label 1
    m[0, 7] = 1; m[1, 7] = 1         # top-right edge   -> label 2
    m[5, 0] = 1; m[5, 1] = 1         # bottom-left edge -> later
    m[5, 7] = 1                       # bottom-right corner
    m[2, 4] = 1                       # interior singleton
    cases["borders"] = m

    # 2) DIAGONAL ONLY: a checkerboard-ish pattern. Under 4-conn EVERY foreground
    #    pixel is its own component (diagonals don't connect). Stresses that we
    #    never accidentally 8-connect.
    m = np.zeros((5, 5), dtype=int)
    for r in range(5):
        for c in range(5):
            if (r + c) % 2 == 0:
                m[r, c] = 1
    cases["diag"] = m

    # 3) SERPENTINE U: a single component whose first raster pixel (0,5) is the
    #    tip of a long snake that wraps down/left/up. Tests that BFS-vs-union-find
    #    discovery order does not change the final single ID, and that the comp
    #    starting later in raster order gets a higher ID even though its bulk is
    #    above the snake's body.
    m = np.zeros((7, 7), dtype=int)
    m[0, 0] = 1                                  # comp 1 (first in raster)
    # comp 2: snake from (0,6) down the right wall, along bottom, up the left
    m[0, 6] = m[1, 6] = m[2, 6] = m[3, 6] = 1
    m[3, 5] = m[3, 4] = m[3, 3] = 1
    m[4, 3] = m[5, 3] = 1
    m[6, 6] = 1                                  # comp 3 starts at (6,6)
    cases["serpent"] = m

    # 4) THIN LINES + ALL-TRUE band: a full-true top row (one component spanning
    #    the whole width incl. both edges), a single-pixel-wide vertical line, and
    #    a 1-pixel gap that must split two components.
    m = np.zeros((6, 6), dtype=int)
    m[0, :] = 1                                  # full top row -> label 1
    m[2:6, 2] = 1                                # vertical thin line
    m[3, 4] = 1; m[3, 5] = 1                     # tiny isolated bit at right edge
    cases["thin"] = m

    # 5) TIES / MANY EQUAL-SIZE SINGLETONS: 9 singletons in a grid, each isolated
    #    by 4-conn (1-pixel spacing). Forces raster ID assignment 1..9 strictly by
    #    row then column. Direct ID-order check with no ambiguity.
    m = np.zeros((5, 5), dtype=int)
    for r in (0, 2, 4):
        for c in (0, 2, 4):
            m[r, c] = 1
    cases["singletons"] = m

    # 6) RANDOM DENSE: a larger pseudo-random mask to exercise many merges where
    #    union-find renumbering vs our BFS could diverge if anything is subtly off.
    rng = np.random.default_rng(20260611)
    m = (rng.random((24, 24)) < 0.45).astype(int)
    cases["rand"] = m

    # 7) EMPTY and FULL edge identities.
    cases["empty"] = np.zeros((4, 4), dtype=int)
    cases["full"] = np.ones((4, 4), dtype=int)

    print("label_4conn golden (oracle: scipy.ndimage.label, default 4-conn cross)")
    for name, mask in cases.items():
        emit(name, mask)
    print(f"  fixtures written to {FIX}")


if __name__ == "__main__":
    main()
