"""Golden for `watershed_from_markers` (methods/patch_refinement.rs:642) against
the REAL `skimage.morphology.watershed` as Allen calls it in `Patch.split2`
(`RetinotopicMapping.py` L3540):

    newLabel = sm.watershed(eccMap, minMarker,
                            connectivity=np.array([[1,1,1],[1,1,1],[1,1,1]]),
                            mask=self.array)

i.e. 8-connectivity (full 3x3 footprint), watershed_line=False (default).

KEY POINT under test (skeletonize-class bug hunt): skimage's watershed is a
priority-queue immersion flood from the markers. With watershed_line=False it
LABELS EVERY masked pixel reachable from a marker -- it does NOT leave
"watershed boundary" pixels as 0. Our Rust hand-rolls an iterative immersion
that keeps a pixel at 0 when its already-labelled neighbours carry >=2 distinct
labels ("watershed line -> keep 0"). That is the cv2 / watershed_line=True
behaviour, not skimage default. So on any input with adjacent basins we expect
the Rust output to have spurious 0 pixels that skimage fills.

Tie-break note: skimage settles ties by (value, then entry-time/age into the
priority queue) -- the closer marker (by flood distance) wins on plateaus. Our
Rust resolves by ascending elevation then "first labelled neighbour" per sweep,
iterating to fixpoint. The fixtures below include plateaus and equidistant ties
so any tie-break divergence also shows up, not just the boundary-0 issue.

Fixtures (all N x N, C-order row-major, little-endian):
  ws_elev.bin    elevation map           '<f8'  (float64)
  ws_markers.bin marker labels           '<i4'  (int32, 0 = no marker)
  ws_mask.bin    mask                     uint8 (1 = inside)
  ws_out.bin     skimage watershed labels '<i4'  (int32)

Run: python gen_watershed_markers_golden.py
"""
import os
import numpy as np
# Allen calls `sm.watershed` where `sm = skimage.morphology`; in skimage 0.25
# the function lives at `skimage.segmentation.watershed` (same implementation,
# re-exported). Use the canonical location.
from skimage.segmentation import watershed as sm_watershed

FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)

CONN8 = np.array([[1, 1, 1], [1, 1, 1], [1, 1, 1]])


def build():
    """A single N x N scene with several stress features:

      - two basins whose flood fronts collide on a flat plateau (forces a
        watershed boundary -> the exact place Rust keeps 0 and skimage labels).
      - a marker touching the image border (border handling).
      - a thin one-pixel isthmus between two basins.
      - a third small basin in a corner.
      - mask holes (a couple of masked-out pixels) so masked region is non-convex.
    """
    N = 24
    elev = np.zeros((N, N), dtype=np.float64)
    # A smooth-ish double-well surface: high ridge down the middle column band.
    cols = np.arange(N)
    # ridge centred near col 12 -> two valleys left/right
    ridge = 5.0 - np.abs(cols - 12.0) * 0.0  # start flat
    # make a genuine ridge so there is an elevation max between two minima
    ridge = np.maximum(0.0, 5.0 - np.abs(cols - 12.0))
    elev = np.tile(ridge.astype(np.float64), (N, 1))
    # add a broad FLAT plateau region (rows 8..15, all same height) to test
    # plateau tie splitting between the two markers.
    elev[8:16, :] = 3.0
    # carve two clear minima where markers sit
    elev[2:6, 2:6] = -2.0
    elev[2:6, 18:22] = -2.0
    # a third minimum in the bottom-left corner (touches border)
    elev[20:24, 0:4] = -1.0

    mask = np.ones((N, N), dtype=bool)
    # punch a couple of holes in the mask (masked-out interior pixels)
    mask[12, 12] = False
    mask[13, 12] = False
    # exclude a border strip on the right edge to test mask+border interplay
    mask[:, 23] = False

    markers = np.zeros((N, N), dtype=np.int32)
    markers[3:5, 3:5] = 1          # left basin marker
    markers[3:5, 19:21] = 2        # right basin marker
    markers[21:23, 0:2] = 3        # corner marker, touches border (row/col 0 region)

    # marker '2' region sits partly under the masked-out right column; skimage
    # multiplies markers by mask, so any marker pixel outside mask is dropped.
    return elev, markers, mask


def main():
    elev, markers, mask = build()

    out = sm_watershed(elev, markers, connectivity=CONN8, mask=mask)

    N = elev.shape[0]
    np.ascontiguousarray(elev, dtype="<f8").tofile(os.path.join(FIX, "ws_elev.bin"))
    np.ascontiguousarray(markers, dtype="<i4").tofile(os.path.join(FIX, "ws_markers.bin"))
    np.ascontiguousarray(mask.astype(np.uint8)).tofile(os.path.join(FIX, "ws_mask.bin"))
    np.ascontiguousarray(out.astype("<i4")).tofile(os.path.join(FIX, "ws_out.bin"))

    masked = int(mask.sum())
    labelled = int(np.sum((out > 0) & mask))
    unlabelled_in_mask = masked - labelled
    print(f"  N={N}  masked_px={masked}")
    print(f"  marker counts: {[int((markers==k).sum()) for k in (1,2,3)]}")
    print(f"  out unique labels: {sorted(set(out.flatten().tolist()))}")
    print(f"  labelled-in-mask={labelled}  UNLABELLED-in-mask(0)={unlabelled_in_mask}")
    print(f"  per-label out counts: " +
          ", ".join(f"{k}:{int((out==k).sum())}" for k in (0,1,2,3)))
    print(f"  elev range [{elev.min()}, {elev.max()}]")


if __name__ == "__main__":
    main()
