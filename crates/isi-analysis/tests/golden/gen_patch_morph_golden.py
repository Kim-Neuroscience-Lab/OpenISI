"""Golden-vector generator for the Allen patch-extraction morphology
primitives, against scipy.ndimage (the library `RetinotopicMapping.py` uses).

`AllenZhuang2017LabelOpenCloseDilate` builds on `binary_opening_cross`,
`binary_closing_cross`, and `label_4conn`, which mirror scipy's
`ni.binary_opening(iterations=N)`, `ni.binary_closing(iterations=N)`, and
`ni.label` with the DEFAULT 4-connected cross structure. scipy's default
`border_value=0` means the image edge DOES erode — the opposite of the MATLAB
disk family, so this is a distinct convention worth pinning.

Reuses the shared mask fixture `cortex_morph_input.bin` (uint8, 96x96).
Outputs: fixtures/patch_morph_{open,close}.bin (uint8, row-major).

Run:  python gen_patch_morph_golden.py
"""
import os
import numpy as np
import scipy.ndimage as ni

N = 96
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")

mask = (
    np.fromfile(os.path.join(FIX, "cortex_morph_input.bin"), dtype=np.uint8)
    .reshape(N, N)
    .astype(bool)
)

op = ni.binary_opening(mask, iterations=3)   # default structure = 4-conn cross
cl = ni.binary_closing(mask, iterations=3)
op.astype(np.uint8).tofile(os.path.join(FIX, "patch_morph_open.bin"))
cl.astype(np.uint8).tofile(os.path.join(FIX, "patch_morph_close.bin"))

# Label: confirm scipy's default structure is the 4-connected cross (what
# `ni.label(patchmap)` uses in Allen, and what our `label_4conn` implements).
cross = ni.generate_binary_structure(2, 1)
_, n_default = ni.label(mask)
_, n_cross = ni.label(mask, structure=cross)
print(f"  scipy open sum={int(op.sum())}  close sum={int(cl.sum())}")
print(f"  scipy label n_default={n_default}  n_cross={n_cross}  "
      f"default==cross: {n_default == n_cross}")
print(f"  cross structure = {cross.astype(int).tolist()}")
