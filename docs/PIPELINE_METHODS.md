# Pipeline Method Architecture

How analysis methods are selected, grouped, parameterized, and validated. It builds
on the stage/method enums (`crates/isi-analysis/src/methods/`), the typed tagged-enum
param system (`crates/openisi-params/`), and the per-stage fingerprint cache
(`crates/isi-analysis/src/pipeline/fingerprint.rs`).

## 1. Principle: the *method* is the unit, not the pipeline

A user mixes and matches **at the individual method level**. Every stage's
method is selected independently; no choice in one stage constrains another
except through declared data prerequisites (§3). "Reproduce paper X" is a
convenience **overlay** (§5), never a mode you are locked into. The instant you
change one stage, you are still in a fully valid, fully described configuration.

This is already half-true: stages are independent enums dispatched by `params`.
What this doc adds is (a) a uniform method *contract* so new methods drop in
without touching the orchestrator or the threading, (b) verified faithfulness for
methods that *claim* a source, and (c) a parameter grouping that scales to many
methods per stage.

## 1b. Scope: mix-and-match is a dev capability, not a storage model

Method/parameter mix-and-match exists **first as a development experiment** — to
let *us* compare methods on real data. It may later graduate to a user-facing
feature, but in either case the user selects **one** configuration and gets
**one** result. The `.oisi` persistence model therefore stays **overwrite-one**:
the raw dataset plus a single current analysis (`complex_maps` + `results` +
per-stage fingerprints + the `analysis_params` that produced them), replaced on
re-analysis. We do **not** enumerate or persist result sets across the
(unbounded) space of method/parameter combinations.

Comparison is **ephemeral**: re-run — made cheap by the per-stage fingerprint
cache, since changing one stage only recomputes that stage and downstream — and
diff the exported figures (`--compare-methods` → `dev_figures/…/compare/`). The
fingerprint cache is precisely what makes persisting combinations unnecessary.

## 2. The stage map

Ordered slots. Each is one selectable method today; `certainty` is new (§7).

| # | stage slot            | consumes                         | produces                  |
|---|-----------------------|----------------------------------|---------------------------|
| 0a| `response_normalization` | raw frames, F0                | ΔF/F vs absolute-ΔF movie |
| 0 | `complex_maps` (DFT)  | (normalized, rectified) movie    | per-direction F1 complex  |
| 0c| `rectification`       | per-cycle movie (pre-DFT)        | half-wave-rectified movie |
| 0b| **`certainty`** (new) | raw frames / per-cycle phasors   | per-pixel validity field  |
| 1a| `direction_smoothing` | per-direction F1 (pre-combine)   | smoothed per-direction F1 |
| 1 | `cycle_combine`       | F1 fwd+rev (+certainty)          | position phasor           |
| 2 | `phase_smoothing`     | position phasor (+certainty)     | smoothed phasor           |
| 3 | `vfs_computation`     | smoothed azi+alt (+certainty)    | visual field sign         |
| 4 | `sign_map_smoothing`  | VFS                              | smoothed VFS              |
| 5 | `cortex_source`       | certainty / VFS / user ROI       | cortex mask               |
| 6 | `patch_threshold`     | smoothed VFS, cortex mask        | patch candidates          |
| 7 | `patch_extraction`    | candidates                       | labeled patches           |
| 8 | `patch_refinement`    | patches, visual-space            | split/merged patches      |
| 9 | `eccentricity`        | position maps, patches           | eccentricity map          |

## 3. The method contract (what makes a method pluggable, threadable, cacheable)

A method is **any type that satisfies one stage trait**. The orchestrator,
threading, and cache are method-agnostic; they only see the trait. To be a legal
method a type MUST:

1. **Be a pure function of its declared inputs + its own tunables.** No hidden
   state, no reading the file directly. This is what makes the fingerprint cache
   correct: `fingerprint(stage) = hash(method_id, tunables, input_fingerprints)`.
   Swapping a method changes `method_id` → that stage and everything downstream
   recompute; upstream is restored from disk (`lib.rs:361`).
2. **Declare its inputs explicitly** (`requires: &[ProductId]`). The orchestrator
   uses this both to schedule (a method that needs `certainty` forces stage 0b
   first) and to *gate the menu*: a method whose prerequisite is absent (e.g.
   `Reliability` cortex with <2 cycles) is reported unavailable, not run-then-fail.
3. **Run inside the existing threaded compute context** — Burn tensors / `rayon`,
   the same primitives the current `apply` fns use. Parallelism lives *inside* a
   method (per-direction, per-pixel) and *across independent stages*; the method
   author does not manage threads, only writes data-parallel kernels.
4. **Carry provenance metadata** (§6), not just a docstring.

Dispatch stays as today's enum-per-stage (closed set, no `dyn`), which keeps it
allocation-free and exhaustively matched. "Plugin" here means *add a variant +
its impl*; the orchestrator never changes.

## 4. Parameter grouping (4 layers)

The current flat `[stage] method=…` + `[stage.method] tunables` is the right
core but conflates three different *kinds* of value. Group by kind:

```
1. acquisition/        # data facts — NOT method choices. Read from the .oisi.
   ├─ stimulus_freq     #   stimulus geometry, frame timing, calibration.
   └─ …                 #   Live here so they never sit beside tunables.

2. preset = "…"        # OPTIONAL named overlay (§5). Default: "custom".

3. stages/             # the menu — one method id per slot, independently set.
   ├─ certainty       = "multitaper_f"
   ├─ cycle_combine   = "kalatsky_stryker_2003"
   ├─ cortex_source   = "snlc_garrett2014_imbound"
   └─ …

4. tunables/           # per-method, nested under (stage, method). Only the
   └─ cortex_source/   #   selected method's block is live; others inert.
      └─ snlc_garrett2014_imbound/
         ├─ k        = 1.5     # @source Garrett2014 getMouseAreasX.m:76
         ├─ close    = 10      # @source Garrett2014 getMouseAreasX.m:107
         └─ dilate   = 3       # @source Garrett2014 getMouseAreasX.m:115
```

Each tunable (a `Tagged<>` newtype) carries metadata: **default, units,
constraint, and `@source`** (the exact reference+line a faithful default comes
from, or `ours` for novel methods). The UI groups the parameter panel by the
selected method and shows `@source` inline, so "is this Garrett's 1.5 or ours?"
is answerable by looking.

## 5. Presets are overlays, not pipelines

A preset is a named, partial map `{stage → (method_id, tunable_overrides)}`
applied as **defaults over the menu**. Selecting `Garrett2014` sets the stages
and the paper's exact tunables; overriding *any* stage drops the `preset` field
to `custom` while keeping every other selection. Presets never gate or hide
other methods — they are a starting point, nothing more. Shipped presets:
`Garrett2014`, `AllenZhuang2017`, `SNLC`, `KalatskyStryker2003`,
`OpenISI_Rigorous` (ours), `Custom`.

This also resolves the *which-defaults* ambiguity we found: `corticalmapping`
and `retinotopic_mapping` disagree (`signMapThr` 0.3 vs 0.35, `smallPatchThr`
200 vs 100, `signMapFilterSigma` 10 vs 9). The preset pins one specific
published configuration; the method itself just exposes the knobs.

## 6. Faithfulness: citation → verified contract

A method named after a source (`SnlcGarrett2014ImBound`) currently only *claims*
fidelity in a docstring. Promote that to a verified contract:

1. **Pin the exact reference**: `repo@commit / file:lines / version-defaults` in
   structured metadata, not prose. Not "Allen" — `retinotopic_mapping@<sha>
   RetinotopicMapping.py:1089` with that file's defaults.
2. **Golden cross-tests**: a harness runs the cloned reference (MATLAB/Python in
   `reference/`) and our Rust on identical synthetic + real input, asserting
   agreement within a stated tolerance. Lives in `crates/isi-analysis/tests/`.
   This is what turns the citation into something we can *defend*, and it is the
   regression net every new method — ours included — is held to.
3. **Document deviations**: where an exact match is impossible (MATLAB 1-indexing,
   a library-specific filter edge mode), state it in the method metadata as a
   declared, tested-around difference — never a silent one.

A method missing a golden test against its cited source is, by policy, *not* a
faithful-reproduction method; it must be renamed to drop the attribution or get
its test.

### 6a. Naming convention (locked 2026-06-18)

There is no formal standard for *naming* method variants; the de-facto
convention in mature scientific software is the one we adopt — descriptive
identifier, eponym only where it is the field-standard term, attribution in
structured metadata (scikit-learn names estimators by what they do;
scipy.optimize uses eponymous `method=` strings like `'Nelder-Mead'`/`'BFGS'`
*because the eponym is the recognized term*; FORCE11 / `CITATION.cff` / CodeMeta
standardize attribution as *metadata*, never as identifiers).

**Rule — `<Lineage><Descriptor>`:**
- **Descriptor** says what the method does.
- **Lineage** ∈ {`Allen`, `Snlc`, `KalatskyStryker`, `OpenIsi`} is included **only
  when the lineage is the semantic choice** — i.e. the variant exists to offer
  "do it lab X's way" (the direct analogue of scipy's `method=` selector). It is
  **omitted** for convergent / field-standard / multi-origin methods, which are
  named descriptively (`AbsoluteDeltaF`, `SimpleComplexAverage`, `Gaussian`,
  `Reliability`).
- **No year in the name.** Author / year / DOI / `repo@commit / file:lines` live
  in the structured per-method citation (§6, point 1), not the type name. The one
  retained eponym-with-no-year is `KalatskyStryker…` (genuinely eponymous in the
  field).

Target renames (executed in the 5b pass, with `.oisi`/config migration):
`AllenZhuang2017PositionGaussian → AllenPositionGaussian`,
`SnlcGarrett2014ImBound → SnlcImBound`, `Garrett2014SigmaScaled → SnlcSigmaScaled`,
`Garrett2014SplitFuse → SnlcSplitFuse`, `OracleAbsoluteDeltaF → AbsoluteDeltaF`,
`KalatskyStryker2003DelaySubtraction → KalatskyStrykerDelaySubtraction`. Already
conform: `AllenAllFrameMean`, `SnlcAdaptiveSmoother`, `SnlcMagThreshold`,
`OpenIsi*`.

### 6b. Version-adoption policy (locked 2026-06-18)

When our port differs from a *newer* version of the same oracle method, decide
**per oracle, per method**, by classifying the diff:

1. **Bug-fix / refactor / version-drift** (newer is strictly more-correct or
   behavior-identical) → **adopt the newer, ditch the older.** A superseded
   behavior has no value as a selectable variant. *(Default — the common case.)*
2. **Deliberate methodological change, both versions in active field use** → the
   *only* case that may justify keeping both as selectable variants, and only on
   real demand; otherwise track the latest and document the old.
3. **Reproducing a specific landmark result** → pin to *that* version, labelled.

Whichever applies, **pin the exact source version** (`repo@commit / file:lines`)
in the per-method citation, so "faithful to what?" is a checkable fact and future
drift is *detectable* (re-diff against the pin) rather than stumbled upon. The
pinned oracle commits live in [`docs/ORACLE_VERSIONS.md`]; the project- and
method-level citations follow `CITATION.cff` (see repo-root `CITATION.cff`).

*Worked example:* the only Allen 2020↔2024 method diff (`getVisualSpace`
inclusive `<=` → exclusive `<` boundary) is case (1); we are already on the 2024
canonical version, so nothing was kept stale.

## 7. Certainty as a cross-cutting stage

Our rigorous methods (multitaper-F detection, normalized-convolution VFS,
weighted phase unwrapping, TFCE) all share one per-pixel **certainty field**.
Rather than smear it across stages, make certainty estimation its own slot (0b)
whose method produces a shared field:

- `certainty` methods: `single_bin_amplitude` (today's implicit behavior),
  `multitaper_f` (Thomson 1982 harmonic F-test → per-pixel p-value),
  `cross_cycle_reliability` (resultant length; Mardia & Jupp), `snr`.
- Downstream stages gain **certainty-aware variants** that take the field as a
  declared input (§3.2): e.g. `vfs_computation = "normalized_convolution"`
  (Knutsson & Westin 1993) weights the gradient by certainty so noise can't
  contaminate across the cortex boundary.

This is the one place that touches the `lib.rs` "data is data — masks are views"
contract: certainty must flow *into* the gradient, not be applied after. See
Open Decisions.

## 8. Invariants

- **Purity** (§3.1) is mandatory; it is the precondition for the fingerprint
  cache being correct. A method that reads wall-clock, RNG, or the filesystem
  breaks incremental re-runs.
- **Determinism across thread counts**: parallel reductions must give identical
  results regardless of `rayon` pool size (fixed reduction order or
  associativity-safe ops), or the fingerprint cache serves results that depend
  on the machine.
- **Every default has an `@source`.** No anonymous magic numbers.

## 9. Open decisions

1. **Data-is-data vs. upstream certainty.** Keep strict stage isolation (rigorous
   methods can only mask as a view → contamination rim remains), or promote
   certainty to a first-class shared product consumed *before* the gradient
   (correct, but amends the `lib.rs` contract). Recommendation: the latter — it
   is the only form in which our own methods are correct.
2. **Enum dispatch vs. trait objects.** Recommendation: stay with enums
   (exhaustive, allocation-free); "extensibility" is adding a variant, which is
   cheap and keeps the menu a closed, audited set.
3. **Deferred `wfield` preprocessing stages (surveyed 2026-06-18, not adopted).**
   The `wfield` package (Couto / Churchland) offers two modality-agnostic
   preprocessing stages OpenISI lacks — **motion correction** (`registration.py`,
   cv2 phase-correlation / ECC) and **SVD / low-rank denoising**
   (`decomposition.py`, the compress-then-analyze paradigm). Both are *optional
   capability adds, not faithfulness gaps* (no oracle compels them), and their
   value for **anesthetized periodic ISI** retinotopy is unproven (little motion
   under anesthesia; the bin-1 DFT already isolates one frequency). **Decision:
   deferred** — revisit motion correction only if real recordings show motion, and
   SVD denoising only if the compress-then-analyze workflow is specifically wanted.
   `wfield`'s hemodynamic correction is fluorescence-only and **does not apply** to
   reflectance ISI. (Its `visual_sign_map` is verbatim Allen — no new retinotopy
   method; see `docs/ORACLE_VERSIONS.md`.)

## 10. The golden-test net

Every author/year-named method is validated by a golden test against its reference
implementation, across **both** reference ecosystems — Python/scipy
(`tests/golden/*.py`) and genuine MATLAB (`tests/golden/*.m`) — with in-crate
golden tests in `compute/golden_vfs.rs` and `segmentation/golden_cortex_morph.rs` and
binary fixtures in `tests/golden/fixtures/`. The net both proves a claimed
reproduction is faithful and pins the exact defaults the presets need; the live
coverage matrix is §11.

## 11. Faithful-reproduction coverage matrix

Audited from the stage method enums (`methods/*.rs`). Policy (§6): a method named
after a source must reach **✅ golden** (Tier A) or a documented Tier-B
validation, or drop the attribution.

Legend: ✅ golden · 🟡 partial · ⬜ gap · — n/a (trivial / our own).
**Tier A** = runnable reference (Python/scipy, or genuine MATLAB) → bit/precision golden.
**Tier B** = paper-only → transcription + property/analytic tests + shipped reference outputs (e.g. SNLC `Example Maps` R43).

| stage | method | source | ref/tier | status |
|---|---|---|---|---|
Status names the actual validating test (run `cargo test -p isi-analysis`; SNLC
live oracle tests need genuine MATLAB via `OPENISI_MATLAB`). Reconciled against the
test suite 2026-06-18 — every prior `⬜`/`NOT PORTED` mark was **stale**.

| stage | method | source | ref/tier | status (test) |
|---|---|---|---|---|
| complex_maps (F1 DFT) | single-bin DFT | Kalatsky-Stryker 2003 | numpy / A | ✅ `dft_projection_matches_genuine_numpy_fft_live` |
| response_normalization | `OpenIsiFractionalDff` | OpenISI default `(F−F0)/F0` | — | — (default; ≈ Allen `normalize_movie` dFoverF) |
| response_normalization | `OracleAbsoluteDeltaF` | SNLC `Gf1image.m` 72 / Allen `generatePhaseMap2` (`F−F0`, no divide) | internal / A | ✅ `response_normalization_absolute_vs_fractional_phase_equivalence` (`F1_frac·F0 == F1_abs`, 64·ε_f32); `1/F0` amplitude divergence documented |
| rectification | `None` | Allen `isRectify=False` (default) | — | — |
| rectification | `AllenZhuang2017ClipNegative` | Allen `HighLevel.getMappingMovies` 607-612 (`isRectify=True`) | internal / A | ✅ `clip_negative_matches_allen_half_wave_rectify` (0 diff vs `max(x,0)`) |
| direction_smoothing | `None` | OpenISI default (smooth post-combine) | — | — |
| direction_smoothing | `SnlcAdaptiveSmoother` | SNLC `adaptiveSmoother.m` + `Gprocesskret.m` 36-41 (pre-combine, per-direction) | MATLAB / A | ✅ `adaptive_smoother_matches_genuine_snlc_live` (64·ε_f64) |
| cycle_combine | `KalatskyStryker2003DelaySubtraction` | `Gprocesskret.m` 88-99 | MATLAB / A | ✅ `combine_and_delay_match_genuine_snlc_gprocesskret_live` (2.6e-7) |
| cycle_combine | `UnweightedCycleAverage` | not published (debug fallback) | — | — |
| cycle_average | `SimpleComplexAverage` | Allen `get_average_movie` / SNLC `Gf1image` (DFT-linearity) | algebraic / A | ✅ `simple_complex_average_equals_dft_of_averaged_frames` |
| cycle_average | `PhaseLockedAverage` | OpenISI (no oracle) | — | — (property: `phase_locked_average_preserves_amplitude_under_global_phase_drift`) |
| phase_smoothing | `SnlcAmpWeightedPhasor` | phase ≡ SNLC complex smooth; amplitude (normalized convolution) is an OpenISI refinement with NO oracle | MATLAB / A (phase) | 🟡 phase-pinned `amp_weighted_phase_equals_snlc_complex_smoothing`; amplitude is ours by design |
| phase_smoothing | `AllenZhuang2017PositionGaussian` | Allen `_getSignMap` `gaussian_filter(positionMap)` (RM.py) | scipy / A | ✅ `allen_position_gaussian_matches_scalar_gaussian_on_phase` |
| vfs_computation | `OpenIsiChainRulePhasorGradient` | ours; ≡ Allen `visualSignMap` on smooth input, stabler at wraps (RM.py 446-478) | Python / A | ✅ `vfs_matches_genuine_nat_visual_sign_map_live` (+ `vfs_stable_across_phase_wraps_where_allen_gradient_spikes`) |
| sign_map_smoothing | `Gaussian` | Allen `_getSignMap` `gaussian_filter` (RM.py) | scipy / A | ✅ `gaussian_smooth_matches_genuine_scipy_live` (<1e-6) |
| cortex_source | `Reliability` | OpenISI's own (NO oracle for the mask; Zhuang does no cortex restriction, verified) | regression-lock | — (OpenISI method; property-pinned `cortex_from_reliability_pins_current_threshold_rule`) |
| cortex_source | `SnlcGarrett2014ImBound` | `getMouseAreasX.m` 76-95 | MATLAB / A | ✅ `snlc_cortex_endtoend_matches_reference` (+ live morph-op goldens) |
| cortex_source | `SnlcMagThreshold` | SNLC `overlaymaps.m` 205-215 (`norm(mag^1.1) ≥ .12`) | MATLAB / A | ✅ `snlc_mag_threshold_roi_matches_overlaymaps` (0 diff) |
| cortex_source | `NoRestriction` | Allen default (trivial) | — | — |
| patch_threshold | `AllenZhuang2017FixedSignMapThr` | Allen `_getRawPatchMap` (RM.py, 0.35) | numpy / A | ✅ `patch_threshold_matches_reference` |
| patch_threshold | `Garrett2014SigmaScaled` | `getMouseAreasX.m` (k·std·0.5, MATLAB N−1 std) | numpy / A | ✅ `patch_threshold_matches_reference` |
| patch_extraction | `AllenZhuang2017LabelOpenCloseDilate` | Allen RM.py 1089-1210 | scipy / A | 🟡 **component-complete** (`raw_patch_map_matches_genuine_nat_live`, `cross_morphology_matches_genuine_scipy_live`, `label4conn_matches_genuine_scipy_live`, `dilation_patches2_matches_genuine_nat_live`, `patch_sign_matches_genuine_snlc_getpatchsign_live`); no single chained-apply golden |
| patch_refinement | `AllenZhuang2017SplitMerge` | Allen RM.py 1247-1527 | scipy/Python / A | 🟡 **component-complete** (split `split2_matches_genuine_nat_live`, merge `merge_two_matches_genuine_nat_live`, visual-space `patch_visual_space_matches_genuine_nat_live`, `sigma_area_matches_genuine_nat_live`, `local_min_matches_genuine_nat_live`); no single chained-apply golden |
| patch_refinement | `Garrett2014SplitFuse` | SNLC `splitPatchesX.m` / `fusePatchesX.m` | MATLAB / A | ✅ **end-to-end** `split_fuse_match_genuine_snlc_matlab_live` (+ atomic SNLC goldens: `patch_com_matches_genuine_snlc_live`, `watershed_matches_genuine_reference_live`, `bwdist_matches_genuine_reference_live`, `interp2_spline_matches_genuine_reference_live`, `imimposemin_matches_genuine_reference_live`, `fft_gaussian_smooth_matches_genuine_reference_live`, …) |
| patch_refinement | `None` | — | — | — |
| eccentricity | `OpenIsiWholeCortexV1` | Allen `eccentricityMap` (RM.py 729-760) | Python / A | ✅ `eccentricity_matches_genuine_nat_eccentricitymap_live` (machine-precision f64) |
| eccentricity | `SnlcGetAreaBordersV1Center` | SNLC `getAreaBorders.m` (`getV1id` + `getPatchCoM`) | MATLAB / A | ✅ `compute_eccentricity_snlc_matches_get_area_borders` |
| baseline | `AllenAllFrameMean` | Allen `normalizeMovie('mean')` | numpy / A | ✅ `dff_matches_allen_normalize_movie_mean` |
| baseline | `AllenAllFrameMedian` | Allen `normalizeMovie('median')` | numpy / A | ✅ `median_baseline_matches_numpy` |
| baseline | `OpenIsiInterSweep{Mean,Median}` | OpenISI's own (falls back to the goldened all-frame mean when gapless) | — | — |
| magnification | determinant `\|det J\|` | Allen `_getDeterminantMap` | numpy / A | ✅ `magnification_matches_genuine_nat_determinant_map_live` |
| magnification | anisotropy (axis + distortion) | SNLC `getMagFactors.m` `prefAxisMF` + `Distrtion` | MATLAB / A | ✅ `magnification_anisotropy_matches_snlc_getmagfactors` (wrap-180 axis, κ-scaled) |

**Coverage conclusion (2026-06-18 reconciliation).** *Every source-named method
reaches an oracle golden.* The only residual is that the two most complex Allen
methods — `patch_extraction` and `patch_refinement::AllenZhuang2017SplitMerge` —
are validated **step-by-step** (every constituent op goldened) but lack a single
*chained* end-to-end `apply()` golden; both default-path equivalents
(`Garrett2014SplitFuse` for refinement) are end-to-end goldened, and the whole
default pipeline is bit-locked by `regression_oisi`. Methods marked `—` are
OpenISI's own or unpublished (no oracle exists to validate against), not gaps.

> **⚠ The remaining epistemic caveat.** This matrix is reconciled against the
> *implemented* methods. It does NOT prove the *oracle packages* contain no further
> automated method we haven't ported: the vendored `reference/corticalmapping/`
> (Python) and full `reference/ISI/` (MATLAB) have not been traversed
> function-by-function. A definitive "nothing is missing" needs that systematic
> enumeration (see the standing offer to run it as a parallel audit).

**Optional belt-and-suspenders worklist (not gaps — redundant with the
component goldens above):** a single chained-apply golden for (1) Allen
`patch_extraction` and (2) Allen `AllenZhuang2017SplitMerge`, each running the
verbatim Allen function end-to-end on a synthetic patch set.
