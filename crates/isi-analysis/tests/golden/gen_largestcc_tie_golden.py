"""Golden for `keep_largest_component` (segmentation/connectivity.rs:64)
against the SNLC `getMouseAreasX.m` largest-CC-at-FOV selection step
(L122-134), whose computational core is

    bwlab = bwlabel(imbound,4);          % 4-conn labeling
    labid = unique(bwlab);               % ascending: 0,1,2,...,N
    for i ...   S(i) = nnz(bwlab==labid(i)); end
    S(1) = 0;                            % drop background (label 0)
    [dum id] = max(S);                   % <-- MATLAB max: FIRST max index
    id = find(bwlab == labid(id));       % keep that component

The oracle's tie-break is the load-bearing detail: MATLAB `max` and
NumPy `np.argmax` both return the FIRST index attaining the maximum, so
when two components share the max size the LOWEST label (= first in
raster/scan order) wins.

We reproduce the oracle two equivalent ways and assert they agree:
  (a) scipy.ndimage.label (default 4-conn cross) + np.argmax over sizes
      (the scipy path named in the gap rationale);
  (b) a verbatim transcription of the MATLAB selection above using the
      same scipy label map (MATLAB bwlabel column-major scan-order labels
      differ from scipy's row-major, but `unique` re-sorts and the
      tie-break is "lowest label", so on a transcription over a single
      consistent label map the two are identical).

scipy.ndimage.label assigns labels 1..N in C-order (row-major) raster
scan — IDENTICAL ordering to our Rust `label_4conn` (r outer, c inner,
incrementing next_label). So the label numbering lines up exactly and the
only thing under test is first-vs-last tie-break.

Cases (each NxN, N=16, uint8 row-major):
  case_tie   : two equal 9-px squares, left one earlier in raster scan
               -> oracle keeps the LEFT (lowest-label) square.
  case_clear : a clear 12-px winner among three components (no tie) ->
               sanity that the non-tie path also matches.

Output fixtures:
  fixtures/largestcc_tie_input.npy    (uint8 mask)
  fixtures/largestcc_tie_out.npy      (uint8, oracle-kept component)
  fixtures/largestcc_clear_input.npy  (uint8 mask)
  fixtures/largestcc_clear_out.npy    (uint8, oracle-kept component)
Run:  python gen_largestcc_tie_golden.py
"""
import os
import numpy as np
import scipy.ndimage as ni

N = 16
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


def keep_largest_scipy(mask):
    """scipy.ndimage.label (4-conn) + np.argmax tie-break (FIRST max)."""
    lab, n = ni.label(mask)  # default structure = 4-conn cross
    if n == 0:
        return np.zeros_like(mask), lab, n, None
    sizes = np.array([int((lab == i).sum()) for i in range(1, n + 1)])
    winner = int(np.argmax(sizes)) + 1  # argmax -> FIRST max => lowest label
    return (lab == winner).astype(np.uint8), lab, n, winner


def keep_largest_matlab_transcription(mask):
    """Verbatim MATLAB getMouseAreasX.m L122-134 selection, transcribed
    over the same scipy label map (Py2->Py3, 1-based->0-based)."""
    bwlab, n = ni.label(mask)             # bwlab = bwlabel(imbound,4)
    labid = np.unique(bwlab)              # ascending incl. 0 (background)
    S = np.array([int((bwlab == L).sum()) for L in labid], dtype=float)
    S[0] = 0.0                            # S(1)=0; drop background
    idx = int(np.argmax(S))               # [dum id] = max(S)  (FIRST max)
    winner_label = int(labid[idx])
    out = (bwlab == winner_label).astype(np.uint8)
    return out, winner_label


def emit(name, mask):
    mask = mask.astype(np.uint8)
    out_scipy, lab, n, winner = keep_largest_scipy(mask)
    out_mat, winner_mat = keep_largest_matlab_transcription(mask)
    assert np.array_equal(out_scipy, out_mat), (
        f"{name}: scipy-argmax and MATLAB-transcription disagree"
    )
    sizes = [int((lab == i).sum()) for i in range(1, n + 1)]
    np.save(os.path.join(FIX, f"largestcc_{name}_input.npy"), np.ascontiguousarray(mask))
    np.save(os.path.join(FIX, f"largestcc_{name}_out.npy"), np.ascontiguousarray(out_scipy))
    print(f"  [{name}] N={N} ncomp={n} sizes={sizes} "
          f"winner_label={winner} (scipy) / {winner_mat} (matlab) "
          f"in_sum={int(mask.sum())} out_sum={int(out_scipy.sum())}")


def main():
    # --- case_tie: two equal 3x3 (=9px) squares. Left appears earlier in
    #     raster scan -> gets lower label -> oracle keeps the LEFT one. ---
    tie = np.zeros((N, N), dtype=np.uint8)
    tie[2:5, 2:5] = 1     # left square  (rows 2-4, cols 2-4)  9 px, label 1
    tie[2:5, 10:13] = 1   # right square (rows 2-4, cols10-12) 9 px, label 2
    emit("tie", tie)

    # --- case_clear: three components, a clear 12-px winner, no ties. ---
    clear = np.zeros((N, N), dtype=np.uint8)
    clear[1:3, 1:3] = 1            # 4 px  (label 1, earliest)
    clear[5:9, 11:14] = 1          # 12 px (label, the clear winner)
    clear[12:14, 2:4] = 1          # 4 px
    emit("clear", clear)


if __name__ == "__main__":
    main()
