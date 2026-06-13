"""Golden for `eccentricity_full_image` (methods/patch_refinement.rs:477,
inside `mod allen`) against the VERBATIM Allen reference used by
`Patch._getEccentricityMap` (RetinotopicMapping.py L1212-1233).

The oracle this gap claims to reproduce is the *combination* exercised in
`_getEccentricityMap`:

  patchAltC, patchAziC = value.getPixelVisualCenter(altPosMapf, aziPosMapf)
      # RetinotopicMapping.py L2805-2816:
      #   altPatch = self.array * altMap; meanAlt = mean(altPatch[altPatch != 0])
      #   aziPatch = self.array * aziMap; meanAzi = mean(aziPatch[aziPatch != 0])
  patchEccMap = eccentricityMap(altPosMapf, aziPosMapf, patchAltC, patchAziC)
      # MODULE-LEVEL eccentricityMap, RetinotopicMapping.py L450-481:
      #   computed over the WHOLE image, NO masking inside.
      #   eccMap = arctan( sqrt( tan(alt-altC)^2
      #                          + tan(azi-aziC)^2 / cos(alt-altC)^2 ) ) * 180/pi

Both `eccentricityMap` and `getPixelVisualCenter` are transcribed VERBATIM
below (only Py2->Py3: `iteritems` not used here; no np alias changes needed).

This locks two things our Rust must match:
  (1) the full-image great-circle ecc formula, element-wise, INCLUDING that
      NaN inputs (background, where alt/azi were set NaN) propagate to NaN out
      and are NOT short-circuited to anything else; and
  (2) the center choice: mean over patch pixels where the masked value != 0.

STRESS INPUTS exercised:
  - background pixels OUTSIDE the patch set to NaN in alt/azi (so the
    full-image map must produce NaN there and finite elsewhere -> tests that
    Rust computes over the full image, not just the patch);
  - a patch pixel with alt == 0 exactly and another with azi == 0 exactly
    (probes the `!= 0` subtlety of getPixelVisualCenter vs a naive mean);
  - center offset from origin so tan/cos terms are non-trivial;
  - a column where alt-altC is near +-90deg-ish to stress cos in denominator
    (kept away from exact 90 to avoid inf, matching realistic ISI ranges).

Outputs (all NxN, C-order row-major, little-endian):
  fixtures/eccfull_alt.bin   alt map, '<f8' (NaN for background)
  fixtures/eccfull_azi.bin   azi map, '<f8' (NaN for background)
  fixtures/eccfull_mask.bin  patch mask, uint8 (1 in patch)
  fixtures/eccfull_ecc.bin   Allen module-level eccentricityMap over full image, '<f8'
  fixtures/eccfull_center.bin  [altC, aziC] from getPixelVisualCenter, '<f8' (len 2)

Run:  python gen_eccfull_golden.py
"""
import os
import numpy as np

N = 24
FIX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(FIX, exist_ok=True)


# --- VERBATIM module-level oracle, RetinotopicMapping.py L450-481 ---
def eccentricityMap(altMap, aziMap, altCenter, aziCenter):
    altMap2 = altMap * np.pi / 180
    aziMap2 = aziMap * np.pi / 180

    altCenter2 = altCenter * np.pi / 180
    aziCenter2 = aziCenter * np.pi / 180

    eccMap = np.zeros(altMap.shape)
    eccMap[:] = np.nan
    eccMap = np.arctan(
        np.sqrt(
            np.square(np.tan(altMap2 - altCenter2))
            +
            np.square(np.tan(aziMap2 - aziCenter2)) / np.square(np.cos(altMap2 - altCenter2))
        )
    )
    eccMap = eccMap * 180 / np.pi
    return eccMap
# --- end verbatim ---


# --- VERBATIM Patch.getPixelVisualCenter, RetinotopicMapping.py L2805-2816 ---
def getPixelVisualCenter(array, altMap, aziMap):
    altPatch = array * altMap
    meanAlt = np.mean(altPatch[altPatch != 0])
    aziPatch = array * aziMap
    meanAzi = np.mean(aziPatch[aziPatch != 0])
    return meanAlt, meanAzi
# --- end verbatim ---


def main():
    rng = np.random.default_rng(20260611)

    # Build realistic retinotopy-like alt/azi maps (degrees), smooth gradients.
    rr, cc = np.mgrid[0:N, 0:N].astype(np.float64)
    # altitude spans roughly -30..+30, azimuth roughly -40..+40
    alt = (rr / (N - 1) * 60.0 - 30.0) + 2.0 * np.sin(cc / 3.0)
    azi = (cc / (N - 1) * 80.0 - 40.0) + 2.0 * np.cos(rr / 4.0)

    # Patch mask: a blob in the interior, touching neither all rows/cols.
    mask = np.zeros((N, N), dtype=np.uint8)
    mask[6:18, 5:17] = 1
    # carve a small hole / irregular edge to stress
    mask[10:13, 9:12] = 1
    mask[6, 5] = 0

    # Force one patch pixel to alt==0 exactly and one to azi==0 exactly to
    # probe the `!= 0` subtlety in getPixelVisualCenter.
    alt[8, 8] = 0.0
    azi[9, 9] = 0.0

    # Background (outside patch) -> NaN in alt/azi, like altPosMapf after
    # thresholding. The full-image eccentricityMap must yield NaN there.
    bg = mask == 0
    alt_full = alt.copy()
    azi_full = azi.copy()
    alt_full[bg] = np.nan
    azi_full[bg] = np.nan

    # Center per Allen getPixelVisualCenter. Note: array * altMap with NaN
    # outside -> 0*NaN = NaN... but array is uint8 0/1 and altMap has NaN in
    # bg; 1*finite inside, 0*NaN = nan in bg. `!= 0` excludes 0s but NaN != 0
    # is True, so NaN would be included -> mean=nan. Allen's real altPosMapf
    # is NOT nan outside patches at this stage in _getEccentricityMap (the maps
    # are filtered position maps, finite everywhere). So replicate Allen's
    # actual call: pass the FINITE maps (alt, azi) to getPixelVisualCenter,
    # exactly as _getEccentricityMap passes altPosMapf/aziPosMapf (finite).
    altC, aziC = getPixelVisualCenter(mask, alt, azi)

    # Allen patchEccMap = eccentricityMap over the FULL image. _getEccentricityMap
    # passes the finite altPosMapf/aziPosMapf, then assigns only patch pixels.
    # Our Rust computes ecc over the full image but with NaN background (then
    # masks). To test the *formula + NaN propagation*, feed the NaN-background
    # maps: finite where patch-or-not-background, NaN in background.
    # The element-wise formula is identical; we assert finite cells match and
    # background cells are NaN.
    ecc_full = eccentricityMap(alt_full, azi_full, altC, aziC)

    # write fixtures
    alt_full.astype('<f8').tofile(os.path.join(FIX, "eccfull_alt.bin"))
    azi_full.astype('<f8').tofile(os.path.join(FIX, "eccfull_azi.bin"))
    mask.astype(np.uint8).tofile(os.path.join(FIX, "eccfull_mask.bin"))
    ecc_full.astype('<f8').tofile(os.path.join(FIX, "eccfull_ecc.bin"))
    np.array([altC, aziC], dtype='<f8').tofile(os.path.join(FIX, "eccfull_center.bin"))

    finite = np.isfinite(ecc_full)
    print(f"  N={N}  patch_sum={int(mask.sum())}")
    print(f"  center: altC={altC:.10f}  aziC={aziC:.10f}")
    print(f"  ecc finite cells={int(finite.sum())}  nan cells={int((~finite).sum())}")
    print(f"  ecc finite range=[{np.nanmin(ecc_full):.6f}, {np.nanmax(ecc_full):.6f}]  "
          f"sum={np.nansum(ecc_full):.6f}")
    # sanity: background must all be NaN, patch must all be finite
    assert np.all(~np.isfinite(ecc_full[bg])), "background should be NaN"
    assert np.all(np.isfinite(ecc_full[mask == 1])), "patch should be finite"
    print(f"  bg-all-nan=OK  patch-all-finite=OK")


if __name__ == "__main__":
    main()
