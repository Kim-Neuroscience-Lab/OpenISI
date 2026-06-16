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
| 0 | `complex_maps` (DFT)  | raw frames, schedule             | per-direction F1 complex  |
| 0b| **`certainty`** (new) | raw frames / per-cycle phasors   | per-pixel validity field  |
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

## 10. The golden-test net

Every author/year-named method is validated by a golden test against its reference
implementation, across **both** reference ecosystems — Python/scipy
(`tests/golden/*.py`) and MATLAB via Octave (`tests/golden/*.m`) — with in-crate
golden tests in `compute/golden_vfs.rs` and `segmentation/golden_cortex_morph.rs` and
binary fixtures in `tests/golden/fixtures/`. The net both proves a claimed
reproduction is faithful and pins the exact defaults the presets need; the live
coverage matrix is §11.

## 11. Faithful-reproduction coverage matrix

Audited from the stage method enums (`methods/*.rs`). Policy (§6): a method named
after a source must reach **✅ golden** (Tier A) or a documented Tier-B
validation, or drop the attribution.

Legend: ✅ golden · 🟡 partial · ⬜ gap · — n/a (trivial / our own).
**Tier A** = runnable reference (Python/scipy, or MATLAB via Octave) → bit/precision golden.
**Tier B** = paper-only → transcription + property/analytic tests + shipped reference outputs (e.g. SNLC `Example Maps` R43).

| stage | method | source | ref/tier | status |
|---|---|---|---|---|
| complex_maps (F1 DFT) | single-bin DFT | Kalatsky-Stryker 2003 | numpy / A | 🟡 unit-tested vs numpy, no formal golden |
| cycle_combine | `KalatskyStryker2003DelaySubtraction` | `Gprocesskret.m` 88-99 | Octave / A | ✅ 2.6e-7 |
| cycle_combine | `UnweightedCycleAverage` | not published | — | — |
| phase_smoothing | `OpenIsiAmpWeightedPhasor` | ours; ≈ Allen `phaseFilter` @σ=1 (RM.py 269-296) | Python / A | ⬜ equivalence untested |
| vfs_computation | `OpenIsiChainRulePhasorGradient` | ours; ≈ Allen `visualSignMap` (RM.py 446-478) | Python / A | ✅ 1.3e-5 + wrap |
| sign_map_smoothing | `Gaussian` | Allen `_getSignMap` (RM.py 1016) | scipy / A | ⬜ **keystone** |
| cortex_source | `Reliability` | OpenISI's own (NO oracle for the mask). Coherence *metric* = Engel 1994 / Zhuang 2017, but Zhuang `RetinotopicMapping.py` does NO cortex restriction (full-frame, verified from source) | regression-lock only | ✅ resolved: OpenISI method; `>=` per ref threshold convention |
| cortex_source | `UserPolygon` | user input | — | — |
| cortex_source | `SnlcGarrett2014ImBound` | `getMouseAreasX.m` 76-95 | Octave / A | ✅ 0 diff (5 ops + e2e) |
| cortex_source | `NoRestriction` | Allen default (trivial) | — | — |
| patch_threshold | `AllenZhuang2017FixedSignMapThr` | Allen `_getRawPatchMap` (RM.py 1099-1103, 0.35) | Python / A | ⬜ |
| patch_threshold | `Garrett2014SigmaScaled` | `getMouseAreasX.m` | Octave / A | ⬜ (σ-thr covered inside cortex e2e) |
| patch_extraction | `AllenZhuang2017LabelOpenCloseDilate` | Allen RM.py 1089-1210 | scipy / A | 🟡 primitives only |
| patch_refinement | `AllenZhuang2017SplitMerge` | Allen RM.py 1247-1527 | Python / A | ⬜ (complex) |
| patch_refinement | `None` | — | — | — |
| eccentricity | `Garrett2014WholeCortexV1` | Allen `eccentricityMap` (RM.py 729-760) | Python / A | 🟡 distance formula only |

**Cross-cutting primitive:** `gaussian_smooth_f64` (`segmentation/morphology.rs`)
backs `sign_map_smoothing` *and* the amp-weighted phasor smooth — golden it first;
`gaussian_filter` truncation/border conventions are a classic drift point.

**Worklist order:** (1) gaussian smoothing primitive, (2) patch_threshold (both
variants — small), (3) patch_extraction full orchestration, (4) eccentricity
center-of-mass, (5) patch_refinement split/merge, (6) phase_smoothing
equivalence, (7) Reliability cortex (resolve Tier A vs B first), (8) F1 DFT
formal golden. Each ⬜→✅ is also a trustworthy *baseline* for old-vs-new
benchmarking (§1b); fair benchmarks need faithful baselines, not strawmen.
