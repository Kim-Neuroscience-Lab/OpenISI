# OpenISI — Method Validation Scorecard

*Refreshed 2026-06-15. This file is the single source of truth for Invariant 3
of `PRINCIPLES.md` — "Validated against the field's oracles": every pipeline
stage/method is pinned to the **actual function the reference codebase executes**
(its real scipy / scikit-image / numpy call, or a verbatim transcription of the
Allen `corticalmapping` / SNLC source run in its native runtime), to synthetic
ground truth, or — where no external oracle exists — honestly marked as a
regression-lock on our own behaviour only. "Agreement" is the measured max
difference on stress inputs.*

*Stage order, method names, and per-stage contracts: see `PIPELINE_METHODS.md`.
Method enums live in `crates/openisi-params/src/config/analysis.rs`; their
`apply` impls and goldens in `crates/isi-analysis/src/methods/` and the in-crate
golden modules below. There is no longer a registry / `define_params` layer.*

## Standing CI gates (run every build)

| Gate | File | What it asserts |
|---|---|---|
| Per-component goldens | `src/compute/golden_vfs.rs`, `src/segmentation/golden_cortex_morph.rs`, in-module tests in `methods/`, `compute/responsiveness.rs` | each method vs its named oracle (Tier 1 below) on captured fixtures (`tests/golden/fixtures/`, produced by `tests/golden/*.py` / `*.m`) |
| Synthetic ground truth | `tests/synthetic_ground_truth.rs` | full retinotopy + segmentation composition recovers a KNOWN mirror-pair field (Tier 2 below); plus mid-stage cancellation |
| Cross-impl equivalence | `tests/equivalence.rs` | re-analyzing the committed `R43_smoke` fixture matches its captured `.baseline.oisi` within `tests/fixtures/tolerances.toml` — segmentation datasets **bit-exact**, float maps within committed cross-backend bounds |
| Incremental cache | `tests/incremental.rs` | per-stage Merkle fingerprint correctness + never-stale / never-needless-recompute (sentinel-tamper disk-restore) |

**Real-data substrate gate** (`tests/regression_oisi.rs`, `#[ignore]` — multi-GB
file, hardware-validated, not in CI): runs the **Burn DFT → retinotopy path from
raw frames** on a real acquisition and gates the forward-sweep **phase** against
the file's embedded `/complex_maps` (the param-free, convention-independent
signal); magnitude / reverse-phase are reported as diagnostics. This is the DFT's
validation of record on real data. It is a *device-stability* gate, not a
legacy-f64 gate (drift is 0 on the device that wrote the file; cross-device drift
reflects f32 ordering only).

## Tier 1 — per-component goldens (vs the reference's own oracle)

Status reflects the actual test that exists. Method names are the current enum
variants (`PIPELINE_METHODS.md` §2). Agreement is the asserted bound.

| Stage / primitive | Method or fn | Oracle (what the reference runs) | Test | Agreement |
|---|---|---|---|---|
| 0 baseline | `temporal_mean_baseline` + ΔF/F | Allen `ImageAnalysis.normalizeMovie('mean')` | `dff_matches_allen_normalize_movie_mean` | F0 1e-9; dF/F 1e-5 |
| 0 baseline | `temporal_median_baseline` | numpy `np.median(axis=0)` (even-N convention) | `median_baseline_matches_numpy` | 1e-9 |
| 0 DFT | `dft_projection_at_freq` (F1 complex) | numpy `np.fft.fft(...)[1]` | `dft_projection_matches_numpy_fft_bin1` | 1e-3 (f32) |
| 0 cycle-avg | `SimpleComplexAverage` | DFT-of-averaged-frames identity | `simple_complex_average_equals_dft_of_averaged_frames` | property |
| 0 cycle-avg | `PhaseLockedAverage` | amplitude-preservation under global phase drift | `phase_locked_average_preserves_amplitude...` | property |
| 1 cycle-combine | `KalatskyStryker2003DelaySubtraction` | SNLC `Gprocesskret.m` 88-99 (Octave) | `kalatsky_combine_matches_snlc_gprocesskret` | 1e-5 |
| 2 phase-smooth | `SnlcAmpWeightedPhasor` | SNLC complex-F1 smoothing (phase identity) | `amp_weighted_phase_equals_snlc_complex_smoothing` | 1e-5 |
| 2 phase-smooth | `AllenZhuang2017PositionGaussian` | Allen `_getSignMap` `gaussian_filter(positionMap)` | `allen_position_gaussian_matches_scalar_gaussian_on_phase` (transitive via scipy gaussian golden) | 1e-5 |
| 3 VFS | `OpenIsiChainRulePhasorGradient` | Allen `visualSignMap` (RM.py 446-478) | `vfs_matches_allen_visual_sign_map_on_smooth_input` + `..._stable_across_phase_wraps...` | 1.3e-5 + wrap-stable |
| 4 sign-smooth | `Gaussian` (`gaussian_smooth_f64` / tensor) | scipy `gaussian_filter` (reflect, 4σ) | `gaussian_smooth_matches_scipy_gaussian_filter`, `tensor_gaussian_smooth_matches_scipy` | 2.2e-15 (f64) / 1e-4 (f32) |
| 5 cortex | `SnlcGarrett2014ImBound` | `getMouseAreasX.m` strel seq (Octave) | `cortex_morphology_matches_octave_strel_ops`, `snlc_cortex_endtoend_matches_octave` | 0 diff |
| 5 cortex | largest 4-conn CC (first-max tie) | SNLC `argmax` / `getMouseAreasX.m` | `keep_largest_component_tiebreak_matches_snlc_argmax` | 0 |
| 6 patch-thr | `AllenZhuang2017FixedSignMapThr` | Allen `_getRawPatchMap` (\|signMapf\|≥0.35) | `patch_threshold_matches_reference` | 0 |
| 6 patch-thr | `Garrett2014SigmaScaled` | Garrett `k·std·0.5`, MATLAB N−1 std | `patch_threshold_matches_reference` | 0 |
| 7 patch-extract | `raw_patch_map_allen` | Allen scipy.ndimage (verbatim) | `allen_raw_patch_map_matches_scipy` | 0 |
| 7 patch-extract | `dilation_patches2_allen` | Allen scipy + skimage (verbatim) | `dilation_patches2_matches_allen` | 0 |
| 7 patch-extract | `skeletonize` | skimage `_fast_skeletonize` LUT | `skeletonize_matches_skimage` | 0 |
| 7 patch-extract | `is_adjacent` | Allen scipy `binary_dilation` | `is_adjacent_matches_allen` | 40/40 |
| 7 patch-extract | `label_4conn` | scipy.ndimage `label` (4-conn) | `label_4conn_matches_scipy_ndimage_label` | 0 |
| 7 patch-extract | cross-iter open (thr-only path) | Allen scipy cross-iterated-3 | `segment_threshold_only_opening_matches_allen` | 0 |
| 7 patch-extract | majority patch sign | SNLC `getPatchSign` (except sign(0)) | `patch_sign_majority_matches_snlc_except_zero_mean` | 0 (zero-mean = documented +1) |
| 8 patch-refine | `watershed_from_markers` | skimage `segmentation.watershed` | `watershed_from_markers_matches_skimage` | 0 |
| 8 patch-refine | `split2` | Allen `Patch.split2` (verbatim) | `split2_matches_allen_watershed_branch` | 0 |
| 8 patch-refine | `local_min_markers` | Allen `localMin` | `local_min_matches_allen_localmin` | 0 |
| 8 patch-refine | `merge_two` | Allen `mergePatches` | `merge_two_matches_allen_mergepatches` | 0 |
| 8 patch-refine | `uniform_filter` | scipy `uniform_filter` (reflect) | `uniform_filter_matches_scipy_reflect` | 1e-14 |
| 8 patch-refine | `sigma_area` (NaN-propagating) | Allen `getSigmaArea` | `sigma_area_matches_allen_get_sigma_area` | NaN-exact |
| 8 patch-refine | `patch_visual_space` / center | Allen `getVisualSpace` (verbatim) | `patch_visual_space_matches_allen...`, `eccentricity_full_image_and_center_match_allen` | 1e-15 |
| 9 ecc | `OpenIsiWholeCortexV1` per-pixel formula | Allen `eccentricityMap` (RM.py 729-760) | `garrett_eccentricity_matches_allen_eccentricitymap` | 1e-9 |
| 9 ecc | `SnlcGetAreaBordersV1Center` | SNLC `getAreaBorders`/`getV1id`/`getPatchCoM` (verbatim) | `compute_eccentricity_snlc_matches_get_area_borders` | machine-precision (f64) |
| — magnification | `compute_magnification_jacobian` (\|det J\| = `magnification_raw`) | Allen `_getDeterminantMap` (\|det J\|) — Allen stops here, it NEVER inverts | `magnification_jacobian_matches_allen_determinant_map` (\|det J\| vs Allen) | 1e-3 |
| — magnification (display) | `magnification` leaf = `1/max(\|det J\|, eps)` | **OpenISI display transform, no oracle** — the physiological CMF direction; tail at near-singular px is an inversion artifact handled by the renderer (no cap — see `math.rs::cortical_magnification_factor`) | same test, CMF-vs-own-golden branch | 1e-2 (own golden) |
| — amplitude | `position_amplitude` | SNLC `Gprocesskret.m` `magS` | `position_amplitude_matches_snlc_mags` | 1e-5 |
| — reliability | `responsiveness::reliability` (coherence) | Engel 1994 / Zhuang 2017 `\|ΣZ\|/Σ\|Z\|` | `reliability_matches_coherence_formula` | 1e-5 |
| — responsiveness | `allen_spectral_power_snr_mask` | Allen `corticalmapping` `generatePhaseMap` (power) | `allen_power_snr_mask_matches_corticalmapping` (+`..._thresholded...`, `..._device...`) | 0 / f32 |
| — responsiveness | spectral SNR (multi-bin rule) | documented rule (numpy transcription) | `spectral_snr_matches_documented_bin_rule` | 1e-6 |
| — morphology | binary open/close/fill/dilate (disk/cross) | Octave `imerode/dilate/open/close/fill`, scipy | `allen_cross_morphology_matches_scipy`, cortex goldens | 0 |
| — separable | `reflect` separable, radius > n | scipy `mode='reflect'` | `reflect_and_separable_match_scipy_large_radius` | ~1e-16 |

## Tier 2 — full-composition vs analytic ground truth (`synthetic_ground_truth.rs`)

| Test | What it proves | Result |
|---|---|---|
| `pipeline_recovers_known_phase_and_vfs_sign` | known mirror-pair retinotopy → real `compute_retinotopy` (combine → smooth → VFS) | phase err < 2e-2; VFS sign > 99% correct, \|vfs\|>0.5 |
| `full_pipeline_segments_two_areas_of_opposite_sign` | same field → full `pipeline::run` → segmentation | exactly 2 areas, opposite signs, left/right placed |
| `compute_retinotopy_honors_cancellation`, `patch_refinement_honors_cancellation` | the two long stages abort promptly on the shared cancel flag | `Cancelled` returned |

## Bugs the harness found and fixed (production code)

1. **skeletonize** was textbook Zhang-Suen; Allen calls `skimage.skeletonize` (a different LUT) — ported skimage's exact 256-entry LUT.
2. **watershed** left boundary pixels unlabelled and mis-apportioned plateaus — rewrote as skimage's `(elevation, age)` flood-level priority queue.
3. `split2` omitted the whole-patch outer border.
4. `is_adjacent` inverted the scipy `iterations=0` (dilate-to-convergence) semantics.
5. `keep_largest_component` tie-break kept the *last* component; SNLC/`argmax` keep the *first*.
6. threshold-only path used a Euclidean disk-3 opening, not Allen's scipy cross-iterated-3.
7. `uniform_filter` was a truncated symmetric box, not scipy's reflect-mode separable filter.
8. `patch_visual_center` averaged alt/azi over the same finite subset; Allen filters each by `!= 0` independently.
9. `sigma_area` skipped NaN; Allen `np.sum(mask·detMap)` propagates it.
10. Earlier sweep: erosion border (MATLAB pads 1s), gaussian 3σ→4σ + reflect, std N→N−1.
11. **std** now uses ndarray's two-pass `.std(ddof=1)` — bit-matches MATLAB `std` (was one-pass).

## Open items — method *choices* for discussion (NOT bugs)

Places our output diverges from a reference by **deliberate design**, or where
**two references genuinely disagree**. Each is pinned by a regression-lock test
recording current behaviour; the canonical choice is a judgment call deferred
pending review.

| Item | The choice | Why it's open | Lock |
|---|---|---|---|
| Visual-space grid | rig-adaptive data bbox **vs** Allen-fixed `[-40,60]×[-20,120]` | code comment marks the deviation intentional ("adapts to the rig") | `derive_visual_grid_is_openisi_data_bbox_not_allen_fixed_range` |
| Patch sign | collapsed `±1` **vs** three-valued `sign(mean)` incl. 0 | differs only at exact-zero-mean (measure-zero on smoothed VFS) | `patch_sign_majority_matches_snlc_except_zero_mean` |
| V1 ecc center | Allen-convention CoM (`OpenIsiWholeCortexV1`) **vs** SNLC `getAreaBorders` (`SnlcGetAreaBordersV1Center`) | the two references genuinely conflict (cos·altitude vs cos·azimuth; imopen pre-step); both variants now exist and each is golden-pinned to its own oracle, so this is a default-selection choice, not a gap | `compute_eccentricity_v1_center_pins_current_allen_convention` |

## NOT YET VALIDATED (no legitimate external oracle)

Exposed/used but **not** validated against any published reference. Their tests
are **regression-locks on our own current behaviour only**.

| Method | Status | Note |
|---|---|---|
| **Reliability cortex** (`CortexSource::Reliability` → `cortex_from_reliability`) | **OpenISI's own — no oracle for the mask** | The cross-cycle *coherence* metric IS golden (`reliability_matches_coherence_formula`, Engel 1994 / Zhuang 2017). The cortex-MASK derivation on top (min-over-directions threshold → largest-CC → fill) has NO oracle: Zhuang `RetinotopicMapping.py` uses no power/coherence ROI mask and segments full-frame (verified from source 2026-06-16). Pinned only by `cortex_from_reliability_pins_current_threshold_rule`. Threshold is `>=` (inclusive), following the reference's own threshold convention (`signMapf >= signMapThr`); KimLabISI (our predecessor, not an oracle) also used `>=`. |
| `CortexSource::NoRestriction` | n/a | trivial pass-through — no oracle applies. |
| `PatchRefinement::None` | n/a | identity pass-through (`none_passes_through_unchanged`). |

## Oracle COVERAGE gaps — outputs/methods the oracles produce that we do NOT

From a function-level enumeration of the vendored oracles (`reference/corticalmapping/`
Allen-Zhuang Python, `reference/ISI/` SNLC MATLAB; ori/dir/SF/color/ocdom modalities
and sparse-noise STRF are out of scope — different experiment types). **NOT proven
fully exhaustive.** ✅ = confirmed by reading the oracle source; 🟡 = from the
function inventory, source not yet line-confirmed.

| Missing output / method | Oracle source | Conf. | Note |
|---|---|---|---|
| **Polar-angle map** (companion to eccentricity) | SNLC `getRadialEccMapX.m:56` `kmap_ang = atan2(alt,az)` | ✅ | We emit `eccentricity` only. LOW-effort, HIGH-value: trivially derivable from the same (alt,azi,center) we already use for eccentricity. |
| **Magnification anisotropy** — preferred axis + distortion | SNLC `getMagFactors.m` `prefAxisMF`,`Distrtion` | ✅ | We emit only scalar `|det J|`. |
| **Per-direction hemodynamic phase-delay maps** | SNLC `Gprocesskret.m:88-105` `delay_hor`,`delay_vert` | ✅ | We *apply* Kalatsky delay-subtraction but discard the delays. |
| **Visual-field coverage / "shadow" map** | SNLC `Gprocesskret.m:111` `sh=shadow(...)`; Allen `getVisualSpace` | ✅ | Cortex→visual-space coverage projection. |
| **Per-patch principal axis** | SNLC `getPatchCoM.m:30` `Axisxy` | ✅ | We compute patch CoM, not its orientation axis. |
| **SNLC `splitPatchesX`/`fusePatchesX`** refinement | `reference/ISI/*.m` | ✅ | Over-representation split + same-sign fuse; distinct from Allen `SplitMerge` (ported). Large port. |
| **Per-area summary scalars**: cortical area (mm²), magnification (mm²/deg²), mean response power, baseline fluorescence | Allen `getCorticalArea`/`getMagnification`/`getMeanPowerAmplitude`/`getBaselineFluorscence` | 🟡 | Per-area aggregates (some V1-normalized → naming-gated). We emit per-pixel maps + labels, no per-area scalars. Depend on `um_per_pixel` (see ring calibration). |
| **Map normalization** (rotate/center to V1) | Allen `normalize`/`generateNormalizedMaps` | 🟡 | Canonical-orientation registration of all maps. |
| Area NAMING (V1/LM/AL…) | both | ✅ | NOT a gap — manual in both oracles (verified); our numeric `area_labels` matches their automated output. |

None are correctness gaps for the default pipeline (defaults use the ported Allen
methods, golden-pinned above); they are missing *additional* oracle outputs/methods.

## Notes

- **Attribution policy** (`PIPELINE_METHODS.md` §6): every author/year-named
  variant must cite its source and reach a golden. This is enforced by code review
  plus the per-method goldens above — a source-named variant without a golden against
  its reference is debt, tracked in this scorecard.
- **Precision.** Host-path computation is f64; the device (Burn tensor) path is
  f32. Goldens run on whichever the primitive uses (e.g. gaussian f64 = 2.2e-15,
  f32 = 1e-4; DFT / reliability / amplitude on the f32 device path → ~1e-5–1e-3).
  Cross-backend drift on real data is bounded by `tolerances.toml`
  (`equivalence.rs`).
