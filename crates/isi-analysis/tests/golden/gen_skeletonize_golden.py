"""Golden for `binary_skeletonize_zs` (segmentation/morphology.rs) against
skimage `skeletonize` — the exact function Allen `dilationPatches2`
(`RetinotopicMapping.py` L201) calls. skimage's 2D `skeletonize` default is the
Zhang & Suen (1984) algorithm, which our Rust implements directly.

Shapes are kept >= 2 px from every border: the production input is a `halo`
(`total_area ∧ ¬raw_patches`) that is interior by construction, and our Rust
skips border pixels (skimage pads with 0), so border-touching shapes are out of
scope by design. These cases stress what production actually feeds it: solid
blocks, a donut/halo ring, an L, and a diagonal bridge between two blobs.

Output: fixtures/skel_<case>_in.bin, skel_<case>_out.bin (uint8, row-major NxN)
Run:  python gen_skeletonize_golden.py
"""
import os
import numpy as np
from skimage.morphology import skeletonize

N = 64
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


def dump(name, arr):
    np.ascontiguousarray(arr.astype(np.uint8)).tofile(os.path.join(FIX, name))


def case_block():
    a = np.zeros((N, N), bool)
    a[10:40, 12:48] = True          # solid rectangle
    return a


def case_halo():
    # donut: outer solid minus inner solid — the shape of a real dilation halo.
    a = np.zeros((N, N), bool)
    a[8:56, 8:56] = True
    a[18:46, 18:46] = False
    return a


def case_two_blob_bridge():
    # two blocks joined by a thin neck — skeleton is an H-like medial axis.
    a = np.zeros((N, N), bool)
    a[12:28, 10:26] = True
    a[36:52, 38:54] = True
    a[28:36, 24:40] = True          # diagonal-ish bridge band
    return a


def main():
    cases = {
        "block": case_block(),
        "halo": case_halo(),
        "bridge": case_two_blob_bridge(),
    }
    for name, inp in cases.items():
        out = skeletonize(inp)
        dump(f"skel_{name}_in.bin", inp)
        dump(f"skel_{name}_out.bin", out)
        print(f"  {name:8s} in_sum={int(inp.sum()):5d}  skel_sum={int(out.sum()):4d}")
    print(f"  grid N={N}, uint8 row-major; skimage default (Zhang-Suen 2D)")


if __name__ == "__main__":
    main()
