"""Golden for `is_adjacent` (segmentation/connectivity.rs:153) against a VERBATIM
transcription of Allen `tools.ImageAnalysis.is_adjacent` (`ImageAnalysis.py` L918).

The oracle is a one-liner pair of `scipy.ndimage.binary_dilation(array,
iterations=borderWidth-1)` followed by the predicate `np.amax(p1d + p2d) > 1`.
scipy's default structure is the 4-conn cross (diamond), border_value=0 — the
same SE our `binary_dilation_cross` uses.

CRITICAL semantic captured here: scipy treats `iterations <= 0` NOT as "leave the
array unchanged" but as "iterate the dilation to convergence" — which on a finite
image fills the entire frame for any non-empty input. So at `borderWidth = 1`
(iterations = 0) the oracle declares EVERY pair of non-empty patches adjacent.
That is the divergent case vs our Rust, which short-circuits `bw == 0` to the
un-dilated patch (overlap-only).

Transcription is verbatim except the Py3/NumPy fixes: none needed for the body
(`np.int8`, `np.amax` unchanged). We mirror Allen's exact call signature.

For each case we emit:
  fixtures/isadj_<id>_a.bin     patch A (uint8, NxN row-major)
  fixtures/isadj_<id>_b.bin     patch B (uint8, NxN row-major)
and a single combined expectations file
  fixtures/isadj_expected.bin   one uint8 per (case, border_width) row, the
                                oracle's True(1)/False(0), in the order printed.

All grids are N=32. Border-width values exercised per pair: 1 (iter0/converge),
2 (iter1, the Allen default merge path), 3 (iter2), 4 (iter3).

Run:  python gen_is_adjacent_golden.py
"""
import os
import numpy as np
import scipy.ndimage as ni

N = 32
BORDER_WIDTHS = [1, 2, 3, 4]
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


def is_adjacent(array1, array2, borderWidth=2):
    # --- verbatim from ImageAnalysis.py:918-929 (Py3, np aliases unchanged) ---
    p1d = ni.binary_dilation(array1, iterations=borderWidth - 1).astype(np.int8)
    p2d = ni.binary_dilation(array2, iterations=borderWidth - 1).astype(np.int8)

    if np.amax(p1d + p2d) > 1:
        return True
    else:
        return False
    # --- end verbatim ---


def blank():
    return np.zeros((N, N), dtype=bool)


def make_cases():
    cases = []

    # 0: overlapping patches (share a pixel) — adjacent at every bw incl iter0
    a = blank(); a[5:9, 5:9] = True
    b = blank(); b[7:11, 7:11] = True
    cases.append(("overlap", a, b))

    # 1: touching edge-to-edge (column 9 vs column 10, 0-px background gap)
    a = blank(); a[5:10, 5:10] = True
    b = blank(); b[5:10, 10:15] = True
    cases.append(("touch", a, b))

    # 2: exactly 1 background column between them (gap of 1 px)
    #    cross-dilation by 1 each closes a 2-px gap -> adjacent at bw=2
    a = blank(); a[5:10, 5:10] = True
    b = blank(); b[5:10, 11:16] = True
    cases.append(("gap1", a, b))

    # 3: exactly 2 background columns between them (gap of 2 px)
    #    bw=2 (iter1 each) just touches; bw=2 -> True, bw=1 special
    a = blank(); a[5:10, 5:10] = True
    b = blank(); b[5:10, 12:17] = True
    cases.append(("gap2", a, b))

    # 4: 3 background columns between (gap 3) — needs more dilation
    a = blank(); a[5:10, 5:10] = True
    b = blank(); b[5:10, 13:18] = True
    cases.append(("gap3", a, b))

    # 5: diagonal corner-touch (8-conn touching but 4-conn separated by 1)
    a = blank(); a[5:9, 5:9] = True
    b = blank(); b[9:13, 9:13] = True
    cases.append(("diag", a, b))

    # 6: far apart in opposite corners — only adjacent under iter0 convergence
    a = blank(); a[0:3, 0:3] = True
    b = blank(); b[N-3:N, N-3:N] = True
    cases.append(("far", a, b))

    # 7: thin single-pixel-wide patches with 1-px gap (border/thin-feature stress)
    a = blank(); a[15, 5:12] = True
    b = blank(); b[15, 13:20] = True
    cases.append(("thin_gap1", a, b))

    # 8: patch B empty — oracle: dilation of empty stays empty, sum max <=1 ->
    #    False at every bw (even iter0, since empty cannot fill)
    a = blank(); a[5:10, 5:10] = True
    b = blank()
    cases.append(("b_empty", a, b))

    # 9: patches touching the image border (edge handling, border_value=0)
    a = blank(); a[0:5, 0:5] = True
    b = blank(); b[0:5, 7:12] = True   # 2-px gap, both pinned to top edge
    cases.append(("edge_gap2", a, b))

    return cases


def main():
    cases = make_cases()
    expected = []
    print(f"  grid N={N}, border_widths={BORDER_WIDTHS}")
    print(f"  {'case':<12} {'sumA':>5} {'sumB':>5}  " +
          "  ".join(f"bw{bw}" for bw in BORDER_WIDTHS))
    for cid, a, b in cases:
        np.ascontiguousarray(a.astype(np.uint8)).tofile(
            os.path.join(FIX, f"isadj_{cid}_a.bin"))
        np.ascontiguousarray(b.astype(np.uint8)).tofile(
            os.path.join(FIX, f"isadj_{cid}_b.bin"))
        row = []
        for bw in BORDER_WIDTHS:
            res = bool(is_adjacent(a, b, borderWidth=bw))
            expected.append(1 if res else 0)
            row.append(res)
        print(f"  {cid:<12} {int(a.sum()):>5} {int(b.sum()):>5}  " +
              "    ".join(f"{int(r)}" for r in row))

    exp = np.array(expected, dtype=np.uint8)
    exp.tofile(os.path.join(FIX, "isadj_expected.bin"))
    print(f"  wrote {len(cases)} pairs x {len(BORDER_WIDTHS)} bw = "
          f"{len(expected)} expectations; True count={int(exp.sum())}")
    # case id order for the Rust test (must match exactly):
    print("  case order: " + ", ".join(c[0] for c in cases))


if __name__ == "__main__":
    main()
