"""Golden for Allen `_getRawPatchMap` (RetinotopicMapping.py L1404-1439): the
binary patch-candidate map = binary_opening(open_iter) -> label -> close EACH
labeled component independently (binary_closing(close_iter)) -> recombine.

Validates the composition (especially the per-patch independent closing) of our
`raw_patch_map_allen` against scipy. Reuses the shared mask `cortex_morph_input.bin`
as the post-threshold `imseg`. open_iter=close_iter=3 (Allen defaults).

Output: fixtures/patchext_rawmap.bin (uint8 binary support, row-major 96x96)
Run:  python gen_patch_extraction_golden.py
"""
import os
import numpy as np
import scipy.ndimage as ni

N = 96
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")

imseg = (
    np.fromfile(os.path.join(FIX, "cortex_morph_input.bin"), dtype=np.uint8)
    .reshape(N, N)
    .astype(bool)
)

OPEN, CLOSE = 3, 3
patchmap = ni.binary_opening(imseg, iterations=OPEN)
patches, n = ni.label(patchmap)

patchmap2 = np.zeros((N, N), dtype=int)
for i in range(n):
    curr = patches == (i + 1)
    curr = ni.binary_closing(curr, iterations=CLOSE)
    patchmap2 += curr.astype(int)

support = (patchmap2 > 0).astype(np.uint8)
support.tofile(os.path.join(FIX, "patchext_rawmap.bin"))
print(f"  open+label components n={n}  raw-patch-map support sum={int(support.sum())}")
