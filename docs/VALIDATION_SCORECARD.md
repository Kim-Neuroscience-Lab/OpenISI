# OpenISI — Method Validation Scorecard

*Generated 2026-06-11. Every published sub-component of the retinotopy pipeline
is pinned to the **actual function the reference codebase executes** (its real
scipy / scikit-image / numpy call, or a verbatim transcription of the Allen /
SNLC source run in its native runtime), not a textbook algorithm of the same
name. "Agreement" is the measured max difference on stress inputs.*

## Tier 1 — per-sub-component goldens (vs the reference's own oracle)

| Stage | Sub-component | Oracle (what the reference runs) | Agreement |
|---|---|---|---|
| 0 | F1 DFT (complex maps) | `numpy.fft` bin 1 | 8.4e-6 |
| 1 | Cycle combine (Kalatsky delay sub.) | SNLC `Gprocesskret.m` (Octave, verbatim) | 2.6e-7 |
| 2/4 | Gaussian smoothing (f64 / f32) | `scipy.ndimage.gaussian_filter` (reflect, 4σ) | 2.2e-15 / 5.9e-7 |
| 3 | Visual field sign (VFS) | Allen `visualSignMap` (numpy, verbatim) | 1.3e-5 + wrap-stable |
| 5 | SNLC cortex (`imbound`) | `getMouseAreasX.m` strel seq (Octave) | 0 |
| 5 | Largest CC (first-max tie-break) | SNLC `argmax` / `getMouseAreasX.m` | 0 |
| 6 | Patch threshold (Allen + Garrett σ) | scipy / MATLAB `std` (N−1, two-pass) | 0 |
| 7 | `_getRawPatchMap` | Allen (scipy.ndimage, verbatim) | 0 |
| 7 | `dilationPatches2` | Allen (scipy + skimage, verbatim) | 0 |
| 7 | **skeletonize** | `skimage.skeletonize` (`_fast_skeletonize` LUT) | 0 |
| 7 | `is_adjacent` | Allen (scipy `binary_dilation`) | 40/40 cases |
| 7 | `label_4conn` | `scipy.ndimage.label` (4-conn) | 0 |
| 8 | **watershed** | `skimage.segmentation.watershed` | 0 |
| 8 | `split2` (patch split) | Allen `Patch.split2` (verbatim) | 0 |
| 8 | `localMin` markers | Allen `localMin` (verbatim) | 0 |
| 8 | `mergePatches` | Allen `mergePatches` (verbatim) | 0 |
| 8 | `uniform_filter` | `scipy.ndimage.uniform_filter` (reflect) | 1e-14 |
| 8 | `getSigmaArea` (NaN-propagating) | Allen `getSigmaArea` | NaN-exact |
| 8 | `getPixelVisualCenter` + ecc map | Allen (verbatim) | 1e-15 |
| 9 | Eccentricity **formula** (per-pixel) | Allen `eccentricityMap` (verbatim) | 2.1e-14 |
| — | Binary morphology (disk/cross) | Octave `imerode/dilate/open/close`, scipy | 0 |
| — | `imfill` / fill-holes | Octave `imfill` | 0 |
| — | `reflect` / separable (radius>n) | `scipy` `mode='reflect'` | 1e-16 |
| — | Spectral SNR (multi-bin rule) | documented rule (numpy transcription) | 1e-6 |
| — | Spherical correction (Marshel 2012) | Allen `MonitorSetup.remap` (verbatim) | 1e-15 |

## Tier 2 — full-composition vs analytic ground truth (no fixture)

| Test | What it proves | Result |
|---|---|---|
| Phase + VFS recovery | known mirror-pair retinotopy → real `compute_retinotopy` | phase 1e-7; VFS 2688/2688 correct sign |
| Full-pipeline segmentation | same field → `pipeline::run` → segmentation | exactly 2 areas, opposite signs, correctly placed |

## Bugs the harness found and fixed (production code)

1. **skeletonize** was textbook Zhang-Suen; Allen calls `skimage.skeletonize` (a different LUT) — ported skimage's exact 256-entry LUT.
2. **watershed** left boundary pixels unlabelled (cv2/`watershed_line=True` behaviour) and mis-apportioned plateaus — rewrote as skimage's `(elevation, age)` flood-level priority queue.
3. `split2` omitted the whole-patch outer border.
4. `is_adjacent` inverted the scipy `iterations=0` (dilate-to-convergence) semantics.
5. `keep_largest_component` tie-break kept the *last* component; SNLC/`argmax` keep the *first*.
6. `segment_threshold_only` used a Euclidean disk-3 opening, not Allen's scipy cross-iterated-3.
7. `uniform_filter` was a truncated symmetric box, not scipy's reflect-mode separable filter.
8. `patch_visual_center` averaged alt/azi over the same finite subset; Allen filters each by `!= 0` independently.
9. `sigma_area` skipped NaN; Allen `np.sum(mask·detMap)` propagates it.
10. Earlier sweep: erosion border (MATLAB pads 1s), gaussian 3σ→4σ + reflect, std N→N−1.
11. **std** now uses ndarray's validated two-pass `.std(ddof=1)` — bit-matches MATLAB `std` (was one-pass).

## Open items — method *choices* for discussion (NOT bugs)

Places our output diverges from a reference by **deliberate design** or because
**two references genuinely disagree**. Each is pinned by a regression-lock test
recording current behaviour; the canonical choice is a judgment call deferred
pending review.

| Item | The choice | Why it's open |
|---|---|---|
| Visual-space grid | rig-adaptive data bbox **vs** Allen-fixed `[-40,60]×[-20,120]` | code comment marks the deviation intentional ("adapts to the rig") |
| Patch sign | collapsed `±1` **vs** three-valued `sign(mean)` incl. 0 | differs only at exact-zero-mean (measure-zero on real smoothed VFS) |

## NOT YET VALIDATED (no legitimate external oracle)

These methods are exposed/used but are **not** validated against any published
reference. Their tests are **regression-locks on our own current behaviour only**.

| Method | Status | Note |
|---|---|---|
| **Reliability cortex** (`cortex_from_reliability`) | **unvalidated** | The reliability *coherence* is published (Engel 1994 / Zhuang 2017), but the cortex-MASK derivation (min-threshold → largest-CC → fill) has no published code oracle. Allen `RetinotopicMapping.py` does NO cortex restriction. (Previously mis-attributed to KimLabISI — *our own past code*, not an oracle.) |
| **V1 eccentricity center** (`compute_eccentricity`) | **center unvalidated** | The per-pixel great-circle *formula* IS validated vs Allen `eccentricityMap` (2e-14). The whole-cortex V1 *center selection* (SNLC `getAreaBorders`/`getV1id`/`getPatchCoM`) is not — and Allen (per-patch, cos·altitude) vs SNLC (whole-cortex, cos·azimuth) genuinely conflict. |

## Precision

All host-path computation is f64; the device (Burn tensor) path is f32. Goldens
are run on both where applicable (e.g. gaussian f64 = 2.2e-15, f32 = 5.9e-7).
