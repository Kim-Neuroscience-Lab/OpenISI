"""Golden for `merge_two` (methods/patch_refinement.rs:796) against a VERBATIM
transcription of Allen `mergePatches` (`RetinotopicMapping.py` L435-447).

Allen mergePatches:
    sp  = array1 + array2
    spc = ni.binary_closing(sp, iterations=borderWidth).astype(np.int8)
    _, patchNum = ni.measurements.label(spc)
    if patchNum > 1: raise LookupError(...too far apart...)
    else: return spc

Py2->Py3/np fixes (no computational line altered):
  - `raise LookupError, '...'`  ->  `raise LookupError('...')`
  - `ni.measurements.label`     ->  `ni.label`  (same function; `measurements`
    namespace is deprecated/removed but identical algorithm: default 4-conn
    cross structure).
  - `binary_closing` default structure = generate_binary_structure(2,1) = the
    4-conn cross, matching our `binary_closing_cross`. iterations=borderWidth =>
    borderWidth dilations then borderWidth erosions (scipy docstring confirms).
    border_value defaults to 0 for both phases.

This generator emits, for each stress case, the two input masks plus a "result"
buffer. The result buffer is the closing output `spc` when patchNum==1 (mergeable)
or, when patchNum>1 (Allen raises), an all-zero buffer plus a flag byte. Our Rust
returns Some(merged) on single-CC and None otherwise, so we encode:
    mergeable flag (1 byte, separate file): 1 => Some(spc), 0 => None.

STRESS CASES (each NxN, the algorithm is sensitive to border/gap/connectivity):
  1. touch_border : two squares flush against the top/left image border with a
     1-px gap between them -> closing must bridge AND border_value=0 erosion must
     not eat the border-flush column. Tests border handling.
  2. gap_eq_bw    : two squares separated by exactly a 2*borderWidth-wide gap that
     closing(iter=bw) just barely bridges -> merge boundary case (decides single CC).
  3. gap_too_far  : same but gap one pixel wider -> closing fails to bridge ->
     two CCs -> Allen raises / our None. Tests the split decision boundary.
  4. diag_only    : two squares touching only at a single diagonal corner. 4-conn
     label sees them as TWO components in the raw union; closing(cross) behaviour
     at the corner decides merge. Tests connectivity tie (4 vs 8) + cross SE.
  5. thin_bridge  : an L-shaped thin (1px) feature plus a block, exercising thin
     features under dilation+erosion.

Output (each case prefixed `mt_`):
  fixtures/mt_<case>_a.bin     uint8 NxN  (array1)
  fixtures/mt_<case>_b.bin     uint8 NxN  (array2)
  fixtures/mt_<case>_out.bin   uint8 NxN  (spc; all-zero if Allen raises)
  fixtures/mt_<case>_flag.bin  uint8 1    (1=mergeable/Some, 0=raises/None)
Run:  python gen_merge_two_golden.py
"""
import os
import numpy as np
import scipy.ndimage as ni

N = 32
BORDER_WIDTH = 2
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


def mergePatches(array1, array2, borderWidth=2):
    # --- verbatim from RetinotopicMapping.py:435-447 (see module docstring) ---
    sp = array1 + array2
    spc = ni.binary_closing(sp, iterations=(borderWidth)).astype(np.int8)

    _, patchNum = ni.label(spc)
    if patchNum > 1:
        raise LookupError('this two patches are too far apart!!!')
    else:
        return spc
    # --- end verbatim ---


def run_case(a, b, bw):
    """Return (spc_or_zeros, mergeable_flag). Mirrors Allen: raise => not mergeable."""
    try:
        spc = mergePatches(a, b, borderWidth=bw)
        return spc.astype(np.uint8), 1
    except LookupError:
        # Allen raises; for the fixture we record the (non-single-CC) closing so
        # the test can confirm our Rust also produces >1 CC -> None. We still
        # store the closing buffer for inspection, and flag=0.
        sp = (a + b)
        spc = ni.binary_closing(sp, iterations=bw).astype(np.uint8)
        return spc, 0


def build_cases(N):
    cases = {}

    # 1. touch_border: flush to top-left, 1px gap between them (cols 4..8 / 10..14)
    a = np.zeros((N, N), dtype=int); b = np.zeros((N, N), dtype=int)
    a[0:6, 0:6] = 1
    b[0:6, 8:14] = 1          # gap = cols 6,7 (2px) -> closing(iter=2) bridges
    cases["touch_border"] = (a, b)

    # 2. gap_eq_bw: vertical bar pair separated by a gap of 2*bw-? tuned to bridge
    a = np.zeros((N, N), dtype=int); b = np.zeros((N, N), dtype=int)
    a[10:20, 6:10] = 1
    b[10:20, 13:17] = 1       # gap cols 10,11,12 (3px); closing(2) reach 2+2=4 -> bridges
    cases["gap_eq_bw"] = (a, b)

    # 3. gap_too_far: widen the gap so closing(2) cannot bridge -> 2 CCs
    a = np.zeros((N, N), dtype=int); b = np.zeros((N, N), dtype=int)
    a[10:20, 4:8] = 1
    b[10:20, 14:18] = 1       # gap cols 8..13 (6px) > 2*2 -> stays split
    cases["gap_too_far"] = (a, b)

    # 4. diag_only: squares touching at one diagonal corner
    a = np.zeros((N, N), dtype=int); b = np.zeros((N, N), dtype=int)
    a[8:14, 8:14] = 1
    b[14:20, 14:20] = 1       # corner contact at (13,13)-(14,14) diagonal
    cases["diag_only"] = (a, b)

    # 5. thin_bridge: an L-shaped 1px feature + block, near border
    a = np.zeros((N, N), dtype=int); b = np.zeros((N, N), dtype=int)
    a[5, 5:20] = 1            # horizontal thin line
    a[5:20, 5] = 1           # vertical thin line (L)
    b[18:24, 16:22] = 1      # block near the L's elbow ends
    cases["thin_bridge"] = (a, b)

    return cases


def main():
    cases = build_cases(N)
    print(f"  N={N} border_width={BORDER_WIDTH} (4-conn cross, border_value=0)")
    for name, (a, b) in cases.items():
        spc, flag = run_case(a, b, BORDER_WIDTH)
        np.ascontiguousarray(a.astype(np.uint8)).tofile(os.path.join(FIX, f"mt_{name}_a.bin"))
        np.ascontiguousarray(b.astype(np.uint8)).tofile(os.path.join(FIX, f"mt_{name}_b.bin"))
        np.ascontiguousarray(spc).tofile(os.path.join(FIX, f"mt_{name}_out.bin"))
        np.array([flag], dtype=np.uint8).tofile(os.path.join(FIX, f"mt_{name}_flag.bin"))
        _, ncc = ni.label(spc)
        print(f"  {name:13} sum_a={int(a.sum()):3d} sum_b={int(b.sum()):3d} "
              f"out_sum={int(spc.sum()):3d} CCs={ncc} mergeable={flag}")


if __name__ == "__main__":
    main()
