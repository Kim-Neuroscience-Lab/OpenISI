"""Golden for `reflect` + `separable_filter` (math.rs:337,367) against scipy's
REAL `mode='reflect'` index mapping, specifically the large-radius periodic-wrap
branch that the gaussian_smooth_f64 goldens never exercise (they use radius < n).

`reflect` claims to match scipy.ndimage `mode='reflect'` (a.k.a. "grid-mirror":
the edge sample IS duplicated, so index -1 -> 0 and index n -> n-1), and to keep
reflecting periodically when |idx| exceeds the array size. scipy implements this
in C (ni_support.c `NI_ExtendLine`, case NI_EXTEND_REFLECT): it folds the index
back into [0, n) by repeated mirror-with-edge-duplication, which for radius > n
wraps around multiple periods. We pin BOTH:

  (1) the raw index mapping over a wide index range, recovered from scipy via
      single-tap correlate1d kernels (input = arange, so the output value at a
      given offset literally equals the mapped source index); and
  (2) the full separable 2-pass filter on a small grid (n=4) with a radius-7
      kernel (length 15 > n=4), i.e. the wrap branch is hit on every output
      pixel. scipy's `gaussian_filter`/`correlate1d` apply the SAME 1-D kernel
      per axis with `mode='reflect'`, identical to our `separable_filter`
      (horizontal pass then vertical pass).

No Allen/SNLC source involved — the oracle is scipy itself.

Outputs (all little-endian, C-order row-major):
  fixtures/reflect_wrap_idxmap.bin   int32  mapped source index, one per probe
  fixtures/reflect_wrap_input.bin    <f8    n x n input grid
  fixtures/reflect_wrap_kernel.bin   <f8    length-(2r+1) 1-D kernel
  fixtures/reflect_wrap_output.bin   <f8    scipy separable result, n x n

Run:  python gen_reflect_wrap_golden.py
"""
import os
import numpy as np
from scipy.ndimage import correlate1d

FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)

# ---- (1) index-mapping golden -------------------------------------------------
# Recover scipy's reflect mapping for a fixed n over a wide signed index range.
# A length-(2R+1) kernel with a single 1.0 at kernel-index k makes correlate1d
# pick input[i + (k-R)] at output i. With input = arange(n), out[i] == mapped
# source index for the input position (i + (k-R)). We sweep offsets to cover a
# wide index range, including several full reflection periods (period = 2n).
N_IDX = 4
R_IDX = 9                          # radius 9 > n 4  -> multi-period wrap
PROBE_LO, PROBE_HI = -R_IDX, N_IDX - 1 + R_IDX   # inclusive index range probed
probe_indices = list(range(PROBE_LO, PROBE_HI + 1))

mapping = {}
for off in range(-R_IDX, R_IDX + 1):
    k = off + R_IDX
    kern = np.zeros(2 * R_IDX + 1, dtype=np.float64)
    kern[k] = 1.0
    inp = np.arange(N_IDX, dtype=np.float64)
    out = correlate1d(inp, kern, mode="reflect")
    for i in range(N_IDX):
        mapping[i + off] = int(round(out[i]))

idxmap = np.array([mapping[i] for i in probe_indices], dtype=np.int32)
np.ascontiguousarray(idxmap).tofile(os.path.join(FIX, "reflect_wrap_idxmap.bin"))

# ---- (2) full separable-filter golden, radius > n ----------------------------
N = 4
R = 7                              # kernel length 15 > n 4 -> wrap on every pixel
# A non-symmetric, all-nonzero kernel so a sign/index bug cannot cancel out.
# (Does NOT need to sum to 1; we are pinning the reflect indexing + convolution,
#  not Gaussian normalization, which is covered elsewhere.)
kernel = (np.arange(2 * R + 1, dtype=np.float64) + 1.0) / 100.0  # 0.01 .. 0.15

rng = np.random.default_rng(20260611)
inp = rng.standard_normal((N, N)).astype(np.float64)

# scipy separable: horizontal pass (axis=1) then vertical pass (axis=0), reflect.
# This matches separable_filter exactly: temp = filter rows, output = filter cols.
temp = correlate1d(inp, kernel, axis=1, mode="reflect", origin=0)
out = correlate1d(temp, kernel, axis=0, mode="reflect", origin=0)

np.ascontiguousarray(inp).tofile(os.path.join(FIX, "reflect_wrap_input.bin"))
np.ascontiguousarray(kernel).tofile(os.path.join(FIX, "reflect_wrap_kernel.bin"))
np.ascontiguousarray(out).tofile(os.path.join(FIX, "reflect_wrap_output.bin"))

# ---- stats -------------------------------------------------------------------
print("INDEX MAP (n=%d, radius=%d):" % (N_IDX, R_IDX))
print("  probe range [%d .. %d], %d probes" % (PROBE_LO, PROBE_HI, len(probe_indices)))
print("  idxmap =", idxmap.tolist())
print("  all in [0,n): ", bool(((idxmap >= 0) & (idxmap < N_IDX)).all()))
print("SEPARABLE FILTER (n=%d, kernel_len=%d, radius=%d > n):" % (N, 2 * R + 1, R))
print("  kernel sum = %.6f  (intentionally != 1)" % kernel.sum())
print("  input  sum = %.6f  min=%.4f max=%.4f" % (inp.sum(), inp.min(), inp.max()))
print("  output sum = %.6f  min=%.6f max=%.6f" % (out.sum(), out.min(), out.max()))
print("  output[0,:] =", np.round(out[0], 6).tolist())
