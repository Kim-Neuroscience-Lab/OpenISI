"""Golden-vector generator for the VFS-computation stage.

Produces reference output from the Allen/Zhuang `visualSignMap` so the Rust
`VfsComputationMethod` can be cross-validated against the method it claims
equivalence to (`vfs_computation.rs`: "Mathematically equivalent to Allen
visualSignMap ... but more numerically stable near phase wraps").

`visualSignMap` below is a VERBATIM transcription of
  reference/corticalmapping/corticalmapping/RetinotopicMapping.py : 446-478
The ONLY change is the Python-2 `raise LookupError, "msg"` → Py3
`raise LookupError("msg")`; no computational line is altered. The module
cannot be imported directly because the surrounding file contains other
Python-2 syntax, so the self-contained function (numpy + math only) is copied.

Output fixtures (raw little-endian float64, C-order, shape in filename):
  fixtures/vfs_<case>_phi1.bin    input phase map 1 (azimuth-like)
  fixtures/vfs_<case>_phi2.bin    input phase map 2 (altitude-like)
  fixtures/vfs_<case>_allen.bin   Allen visualSignMap(phi1, phi2)

Run:  python gen_vfs_golden.py
"""
import os
import math
import numpy as np

N = 64  # grid size (both axes)
HERE = os.path.dirname(os.path.abspath(__file__))
FIX = os.path.join(HERE, "fixtures")


def visualSignMap(phasemap1, phasemap2):
    # --- verbatim from RetinotopicMapping.py:446-478 (see module docstring) ---
    if phasemap1.shape != phasemap2.shape:
        raise LookupError("'phasemap1' and 'phasemap2' should have same size.")

    gradmap1 = np.gradient(phasemap1)
    gradmap2 = np.gradient(phasemap2)

    graddir1 = np.zeros(np.shape(gradmap1[0]))
    graddir2 = np.zeros(np.shape(gradmap2[0]))

    for i in range(phasemap1.shape[0]):
        for j in range(phasemap2.shape[1]):
            graddir1[i, j] = math.atan2(gradmap1[1][i, j], gradmap1[0][i, j])
            graddir2[i, j] = math.atan2(gradmap2[1][i, j], gradmap2[0][i, j])

    vdiff = np.multiply(np.exp(1j * graddir1), np.exp(-1j * graddir2))
    areamap = np.sin(np.angle(vdiff))
    return areamap
    # --- end verbatim ---


def make_case_smooth():
    """Smooth phase maps that stay within (-pi, pi) — no wraps, so the Allen
    raw-gradient method and our chain-rule method should agree. This is the
    equivalence-claim test."""
    xs = np.linspace(0.0, 1.0, N)
    ys = np.linspace(0.0, 1.0, N)
    X, Y = np.meshgrid(xs, ys)  # X varies along axis 1 (cols), Y along axis 0
    # phi1 ramps in X (gradient ~ +col). phi2's gradient in Y flips sign at
    # Y=0.25,0.75 (cos zero-crossings) → graddir2 flips → VFS reverses sign in
    # horizontal bands. Cross terms keep both gradient components non-zero so
    # the atan2 is fully exercised. Amplitudes < pi → no wraps.
    phi1 = 0.8 * X + 0.15 * Y
    phi2 = 0.30 * np.sin(2.0 * np.pi * Y) + 0.15 * X
    return phi1, phi2


def make_case_wrap():
    """phi1 is a steep azimuth ramp that WRAPS (the stored phase is the wrapped
    angle, as a real F1 phase map is). Allen's `np.gradient` of the wrapped
    scalar spikes at each 2pi jump; our chain-rule path sees the continuous
    phasor and recovers the true gradient. Returns the unwrapped truth too, so
    the test can assert (a) ours == Allen-on-the-unwrapped-truth, and (b) ours
    diverges from Allen-on-the-wrapped-input at the wrap columns."""
    xs = np.linspace(0.0, 1.0, N)
    ys = np.linspace(0.0, 1.0, N)
    X, Y = np.meshgrid(xs, ys)
    true1 = 8.0 * (X - 0.5)                      # unwrapped: ramps -4..+4, wraps twice
    phi1 = np.angle(np.exp(1j * true1))          # wrapped to (-pi, pi]  (the real input)
    phi2 = 0.8 * (Y - 0.5)                       # gentle altitude, no wrap
    return true1, phi1, phi2


def dump(name, arr):
    arr = np.ascontiguousarray(arr, dtype="<f8")
    p = os.path.join(FIX, name)
    arr.tofile(p)
    return p, float(arr.min()), float(arr.max())


def main():
    os.makedirs(FIX, exist_ok=True)
    phi1, phi2 = make_case_smooth()
    assert np.abs(phi1).max() < math.pi and np.abs(phi2).max() < math.pi, "would wrap"
    vfs = visualSignMap(phi1, phi2)
    for nm, a in [("vfs_smooth_phi1.bin", phi1),
                  ("vfs_smooth_phi2.bin", phi2),
                  ("vfs_smooth_allen.bin", vfs)]:
        p, lo, hi = dump(nm, a)
        print(f"  wrote {nm:28s} shape={a.shape} range=[{lo:+.4f},{hi:+.4f}]")
    print(f"  Allen VFS: mean={vfs.mean():+.5f} absmax={np.abs(vfs).max():.5f} "
          f"frac|s|>0.5={np.mean(np.abs(vfs) > 0.5):.3f}")

    # --- wrap case: ours should recover Allen-on-truth, diverge from Allen-on-wrapped
    true1, wphi1, wphi2 = make_case_wrap()
    allen_wrapped = visualSignMap(wphi1, wphi2)    # has 2pi-jump artifacts
    allen_true = visualSignMap(true1, wphi2)       # the correct smooth VFS
    for nm, a in [("vfs_wrap_phi1.bin", wphi1),    # wrapped input (what ours consumes)
                  ("vfs_wrap_phi2.bin", wphi2),
                  ("vfs_wrap_allen_wrapped.bin", allen_wrapped),
                  ("vfs_wrap_allen_true.bin", allen_true)]:
        p, lo, hi = dump(nm, a)
        print(f"  wrote {nm:30s} range=[{lo:+.4f},{hi:+.4f}]")
    artifact = np.abs(allen_wrapped - allen_true)
    print(f"  wrap: Allen artifact max|wrapped-true|={artifact.max():.4f} "
          f"at {int((artifact > 0.5).sum())} px")
    print(f"  grid N={N}, layout=C-order row-major little-endian f64")


if __name__ == "__main__":
    main()
