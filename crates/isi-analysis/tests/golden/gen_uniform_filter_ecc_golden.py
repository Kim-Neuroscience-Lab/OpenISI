"""Golden for `uniform_filter_finite` (patch_refinement.rs:553) against the REAL
oracle: `scipy.ndimage.uniform_filter(arr, size=eccMapFilterSigma)` with scipy's
default `mode='reflect'`, exactly as Allen calls it at
NeuroAnalysisTools/RetinotopicMapping.py:1230

    patchEccMapf = ni.filters.uniform_filter(patchEccMap, eccMapFilterSigma)

CRITICAL CONTEXT (why this is faithful to Allen, not a textbook box-blur):

* `patchEccMap` is produced by `eccentricityMap(...)` over the FULL alt/azi
  position maps. It is a plain arctan-of-distances array with NO NaNs at the
  point of filtering. Allen masks to the patch (`value.array == 1`) only AFTER
  filtering. So the oracle the filter must match is plain scipy uniform_filter
  on a finite, full-array input -- NaN-handling never enters Allen's path here.

* scipy `uniform_filter` is a SEPARABLE sequence of 1-D `uniform_filter1d`
  passes, default `mode='reflect'`, `origin=0`. Per axis the window length is
  EXACTLY `int(size)` (size is cast to int; `eccMapFilterSigma` defaults are
  floats 10.0 / 15.0). For ODD size s the window is symmetric of radius s//2.
  For EVEN size s the window is ASYMMETRIC: indices [i - s//2, ..., i - s//2 +
  s - 1] (more samples on the left). `mode='reflect'` reflects about the edge
  DUPLICATING the edge sample: (... c b a | a b c d | d c b ...).

eccMapFilterSigma defaults in the Allen source: 10.0 (RetinotopicMapping.py:959)
and 15.0 (test params). We golden BOTH an odd size (15) and an even size (10) to
exercise the size->window mapping and the asymmetric even-window, plus borders.

A separate small fixture (size 5) on a tiny grid makes the border-reflect and
impulse-spread differences hand-checkable.

Outputs (all '<f8', C-order row-major, NxN unless noted):
  fixtures/uf_in_a.bin      input A  (smooth ecc-like map)   NxN
  fixtures/uf_out_a15.bin   uniform_filter(A, 15)            NxN
  fixtures/uf_out_a10.bin   uniform_filter(A, 10)            NxN
  fixtures/uf_in_b.bin      input B  (impulse + ramp, small) MxM
  fixtures/uf_out_b5.bin    uniform_filter(B, 5)             MxM
Run:  python gen_uniform_filter_ecc_golden.py
"""
import os
import numpy as np
import scipy.ndimage as ni

FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)

N = 48   # main grid
M = 11   # small grid for hand-checkable border/impulse behavior


def ecc_like(n):
    """An eccentricity-map-like array: radial distance from an off-center point,
    smooth and finite everywhere -- mirrors what `eccentricityMap` feeds the
    filter (arctan of position distances). Values are non-trivial so border
    reflect vs truncate, and even-window asymmetry, both bite."""
    yy, xx = np.mgrid[0:n, 0:n].astype(np.float64)
    cy, cx = 0.30 * n, 0.65 * n
    r = np.sqrt((yy - cy) ** 2 + (xx - cx) ** 2)
    # add a mild anisotropic tilt so the filter result is not symmetric and
    # the even-size left-bias is observable
    return r + 0.15 * xx - 0.10 * yy


def main():
    rng = np.random.default_rng(0)  # only used if needed; kept deterministic

    A = ecc_like(N)
    A_out15 = ni.uniform_filter(A, 15)            # odd  -> symmetric radius 7
    A_out10 = ni.uniform_filter(A, 10)            # even -> asymmetric window 10

    # Small grid: impulse at an off-center spot + a ramp, to expose reflect at
    # borders and the size-5 (radius-2, window-5) spread.
    B = np.zeros((M, M), dtype=np.float64)
    B += np.mgrid[0:M, 0:M][1].astype(np.float64)  # column ramp 0..M-1
    B[2, 8] += 100.0                                # near-border impulse
    B[8, 1] += 50.0                                 # other near-border impulse
    B_out5 = ni.uniform_filter(B, 5)

    def dump(arr, name):
        np.ascontiguousarray(arr.astype("<f8")).tofile(os.path.join(FIX, name))

    dump(A, "uf_in_a.bin")
    dump(A_out15, "uf_out_a15.bin")
    dump(A_out10, "uf_out_a10.bin")
    dump(B, "uf_in_b.bin")
    dump(B_out5, "uf_out_b5.bin")

    print("scipy", ni.__name__, "uniform_filter mode=reflect (default)")
    print(f"A: N={N}  in[min,max]=[{A.min():.4f},{A.max():.4f}] sum={A.sum():.4f}")
    print(f"  out15 [min,max]=[{A_out15.min():.4f},{A_out15.max():.4f}] sum={A_out15.sum():.4f}")
    print(f"  out10 [min,max]=[{A_out10.min():.4f},{A_out10.max():.4f}] sum={A_out10.sum():.4f}")
    print(f"  corner out15[0,0]={A_out15[0,0]:.6f}  out10[0,0]={A_out10[0,0]:.6f}")
    print(f"B: M={M}  in sum={B.sum():.4f}")
    print(f"  out5 [min,max]=[{B_out5.min():.4f},{B_out5.max():.4f}] sum={B_out5.sum():.4f}")
    print(f"  out5[2,8]={B_out5[2,8]:.6f}  out5[0,0]={B_out5[0,0]:.6f} out5[8,1]={B_out5[8,1]:.6f}")


if __name__ == "__main__":
    main()
