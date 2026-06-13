"""Golden for `allen::sigma_area` (patch_refinement.rs:450) against a VERBATIM
transcription of Allen `Patch.getSigmaArea` (RetinotopicMapping.py L2798-2803):

    def getSigmaArea(self, detMap):
        sigmaArea = np.sum((self.array * detMap)[:])
        return sigmaArea

where `self.array` is the patch mask as int (0/1) -- see Patch.__init__
(L2670-2678): patchArray.astype(np.int8); arr[arr>0]=1 -> dense int 0/1 array
(via sparse.coo_matrix.toarray()). So getSigmaArea is literally:

    np.sum(mask_int01 * detMap)

The whole point of this fixture: NumPy elementwise `int * float` with NaN.
- 0 * NaN  == NaN   (so a NaN ANYWHERE in detMap, even OUTSIDE the patch where
  mask==0, makes np.sum -> NaN; integer 0 does NOT mask it out)
- 1 * NaN  == NaN   (NaN inside the patch -> np.sum -> NaN)
np.sum does NOT skip NaN. This is the divergence surface vs our Rust, which
iterates only masked pixels AND skips non-finite values.

Verbatim except: Py2->Py3 only affects the unused `raise ValueError,` syntax,
not the computation. No numpy alias issues (np.sum/np.array stable).

Cases (each writes a mask fixture + a detmap fixture + a scalar expected):
  A finite_all   : finite detMap, no NaN -> finite sum (Rust SHOULD match)
  B nan_in_patch : NaN at a masked pixel -> oracle NaN
  C nan_out_patch: NaN only at unmasked pixels -> oracle NaN (0*NaN=NaN!)
  D multi_comp   : several disjoint patch blobs, finite -> finite sum
  E neg_and_zero : detMap with negatives & zeros (det map is abs() in pipeline,
                   but pin the raw arithmetic: plain signed sum, no clamping)

Fixture byte format: little-endian C-order row-major. mask as uint8 (0/1),
detMap as '<f8'. Expected scalar written as a single '<f8' (NaN encodes as the
IEEE-754 NaN bit pattern, round-trips through f64::from_le_bytes).

Output: fixtures/sigarea_<case>_mask.bin (uint8 HxW),
        fixtures/sigarea_<case>_det.bin  (<f8 HxW),
        fixtures/sigarea_<case>_exp.bin  (<f8 scalar)
Run:  python gen_sigmaarea_golden.py
"""
import os
import numpy as np

H, W = 24, 32
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


def get_sigma_area(mask_int01, detMap):
    # --- verbatim Allen Patch.getSigmaArea (RetinotopicMapping.py L2798-2803) ---
    sigmaArea = np.sum((mask_int01 * detMap)[:])
    return sigmaArea
    # --- end verbatim ---


def write_case(name, mask_bool, detmap):
    mask_int = mask_bool.astype(np.int8)
    mask_int[mask_int > 0] = 1          # mirror Patch.__init__ binarization
    exp = get_sigma_area(mask_int.astype(np.int64), detmap.astype(np.float64))
    np.ascontiguousarray(mask_bool.astype(np.uint8)).tofile(
        os.path.join(FIX, f"sigarea_{name}_mask.bin"))
    np.ascontiguousarray(detmap.astype("<f8")).tofile(
        os.path.join(FIX, f"sigarea_{name}_det.bin"))
    np.ascontiguousarray(np.array([exp], dtype="<f8")).tofile(
        os.path.join(FIX, f"sigarea_{name}_exp.bin"))
    n_masked = int(mask_int.sum())
    n_nan = int(np.isnan(detmap).sum())
    print(f"  {name:14s} masked={n_masked:4d}  nan_in_det={n_nan:3d}  "
          f"expected_sigmaArea={exp!r}")
    return exp


def main():
    rng = np.random.default_rng(20260611)

    # --- A: finite_all -------------------------------------------------------
    mask = np.zeros((H, W), dtype=bool)
    mask[4:14, 6:20] = True
    det = rng.uniform(0.0, 3.0, size=(H, W))
    write_case("finiteall", mask, det)

    # --- B: nan_in_patch -----------------------------------------------------
    mask = np.zeros((H, W), dtype=bool)
    mask[4:14, 6:20] = True
    det = rng.uniform(0.0, 3.0, size=(H, W))
    det[8, 10] = np.nan          # NaN at a MASKED pixel
    write_case("nanin", mask, det)

    # --- C: nan_out_patch ----------------------------------------------------
    mask = np.zeros((H, W), dtype=bool)
    mask[4:14, 6:20] = True
    det = rng.uniform(0.0, 3.0, size=(H, W))
    det[0, 0] = np.nan           # NaN OUTSIDE the patch (mask==0 there)
    det[H - 1, W - 1] = np.nan
    write_case("nanout", mask, det)

    # --- D: multi_comp (disjoint blobs incl. border pixels) ------------------
    mask = np.zeros((H, W), dtype=bool)
    mask[0, 0] = True                    # top-left corner
    mask[2:6, 2:6] = True                # blob 1
    mask[16:22, 24:32] = True            # blob 2 touching bottom/right border
    mask[H - 1, 0] = True                # bottom-left corner
    det = rng.uniform(0.0, 5.0, size=(H, W))
    write_case("multicomp", mask, det)

    # --- E: neg_and_zero (signed arithmetic, no clamp) -----------------------
    mask = np.zeros((H, W), dtype=bool)
    mask[3:12, 5:18] = True
    det = rng.uniform(-2.0, 2.0, size=(H, W))
    det[5, 7] = 0.0
    det[6, 8] = -0.0
    write_case("negzero", mask, det)

    print(f"  grid H={H} W={W}; mask uint8, det <f8, expected <f8 scalar")
    print(f"  NOTE: nanin & nanout expected == NaN (oracle propagates; "
          f"Rust skips -> DIVERGES)")


if __name__ == "__main__":
    main()
