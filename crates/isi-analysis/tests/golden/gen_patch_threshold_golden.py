"""Goldens for the two patch_threshold methods.

AllenZhuang2017FixedSignMapThr (RetinotopicMapping.py L1099-1103, default 0.35):
    patchmap = (signMapf >= thr) | (signMapf <= -thr)   ==   |signMapf| >= thr

Garrett2014SigmaScaled (SNLC getMouseAreasX.m, k=1.5):
    threshSeg = k * std(VFS);  imseg = |VFS| > threshSeg/2
MATLAB `std` defaults to N-1 (sample) — reference uses ddof=1 to match. (Our
`std_of_finite_within` currently divides by N; this golden will reveal whether
that deviates.) cortex_mask is all-true here, isolating the threshold itself.

Output: fixtures/pthr_vfs.bin (f64), pthr_allen.bin, pthr_garrett.bin (u8), 96x96
Run:  python gen_patch_threshold_golden.py
"""
import os
import numpy as np

N = 64
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")

xs = np.linspace(0.0, 1.0, N)
ys = np.linspace(0.0, 1.0, N)
X, Y = np.meshgrid(xs, ys)
# smooth VFS-like field, both signs, |vfs| up to ~0.9, structure across threshold
vfs = 0.8 * np.sin(2 * np.pi * X) * np.cos(2 * np.pi * Y) + 0.1 * (X - 0.5)

ALLEN_THR = 0.35
allen = (np.abs(vfs) >= ALLEN_THR)

K = 1.5
std_n1 = np.std(vfs, ddof=1)   # MATLAB/SNLC default (N-1)
std_n = np.std(vfs, ddof=0)    # population (N) — what our Rust uses now
thr = K * std_n1 * 0.5
garrett = np.abs(vfs) > thr

np.save(os.path.join(FIX, "pthr_vfs.npy"), np.ascontiguousarray(vfs, dtype="<f8"))
np.save(os.path.join(FIX, "pthr_allen.npy"), allen.astype(np.uint8))
np.save(os.path.join(FIX, "pthr_garrett.npy"), garrett.astype(np.uint8))

print(f"  vfs range [{vfs.min():.4f}, {vfs.max():.4f}]")
print(f"  allen(|vfs|>=0.35) sum={int(allen.sum())}")
print(f"  std N-1={std_n1:.6f}  N={std_n:.6f}  thr(N-1)={thr:.6f}  garrett sum={int(garrett.sum())}")
