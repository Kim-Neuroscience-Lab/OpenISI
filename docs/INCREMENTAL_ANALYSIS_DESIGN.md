# Incremental Re-Analysis

The analysis pipeline re-executes **only the stages whose inputs changed**. Tuning a
late-stage tunable — a morphological dilation iteration in patch refinement, say —
restores the per-cycle DFT, retinotopy, and every other unaffected stage from the
`.oisi` file and recomputes only the dirtied tail. Without this, every parameter
tweak re-runs the whole pipeline from raw frames (minutes per tweak on a real
acquisition); with it, the cost of a tweak is the cost of the stages it actually
affects.

This document owns the **incremental cache** concern: the fingerprint contract, where
cache state lives in the file, and the demand-driven cut that decides what to restore
versus recompute. The pipeline's stage set, the `Stage` trait, and the orchestrator
walk belong to [`COMPUTE.md`](COMPUTE.md); the invariants this enforces
("Reproducible by record", "No silent stale results") belong to
[`PRINCIPLES.md`](PRINCIPLES.md).

## The model: a Merkle DAG over stage inputs

The pipeline is a fixed directed acyclic graph of stages (`pipeline/stages.rs`):

```
Baseline → Projection → Retinotopy → SignSmoothing → CortexSource →
PatchThreshold → PatchExtraction → PatchRefinement → Labels → Eccentricity → DerivedMaps
```

Each stage has a **fingerprint**: a BLAKE3 Merkle key over that stage's *direct
inputs* — its parameter slice, the cross-cutting acquisition inputs its math reads,
the identity of any raw data it consumes, and the fingerprints of its upstream
stages. Because each key folds in its dependencies' keys, a change to any input
propagates to every stage downstream of it and to nothing upstream — the same
"derivation key" shape Bazel and DVC use. The keys are computed at the I/O boundary
in topological order before any stage runs (`pipeline/fingerprint.rs`).

## The fingerprint contract

**Fingerprint inputs, never outputs.** The key hashes what goes *into* a stage, not
what comes out. Hashing f32 outputs would thrash the cache on cross-backend
summation-order drift (the same params can produce bit-different outputs on CPU
vs CUDA within tolerance) for no scientific benefit. Inputs are stable; outputs are
not bit-stable across devices.

What enters a stage's fingerprint:

- **Its parameter slice** of the typed `AnalysisParams` (constructed from
  `AnalysisConfig`). Because the analysis methods are tagged enums, only the
  *selected* method's tunables exist to be hashed — an unselected variant's tunables
  are structurally absent, so toggling a method back and forth (`A → B → A`) is a
  cache hit by construction, with no `active_when` predicate to maintain.
- **Cross-cutting acquisition inputs** the stage's math actually reads — geometry and
  calibration from `AcquisitionProperties` (`rotation_k`, `azi/alt_angular_range`,
  `offset_azi/alt`, `um_per_pixel`). These are not parameters but they change the
  result, so they are folded into the fingerprint of the stages that read them
  (`Retinotopy`, `SignSmoothing`). A migration that rewrites a recorded geometry
  value therefore correctly invalidates the dependent stages.
- **Raw-data identity** for inputs not produced by an upstream stage. The first stage
  has no parameters; its fingerprint is the identity of the acquisition recording
  (root provenance attributes), not a rehash of tens of GB of frames. Inputs
  that aren't parameters — e.g. raw-derived reliability maps — enter the
  fingerprint of the stage that consumes them, gated on the variant that uses them.
- **The upstream stages' fingerprints**, in dependency order.

**An algorithm change is a fingerprint change.** The key includes a version tag the
pipeline bumps whenever a stage's math changes in a way that alters output for
unchanged inputs. Changing the tagged-enum structure of `AnalysisConfig` (which
tunables exist under which method) is likewise a version bump, because that structure
is part of the contract. This is the one case where the fingerprint must be moved by
hand: a code change the hash cannot see otherwise.

## Where cache state lives

Cache state lives **inside the `.oisi` file**, so the file remains the single
self-contained artifact ("Reproducible by record"):

- **`/analysis_state`** — one attribute per stage, keyed by the stage's
  `fingerprint_key()`, holding the fingerprint that produced that stage's currently
  stored output.
- **`/cache`** — the non-result intermediates a restore needs but which are not part
  of `/results` (the binary candidate-patch mask and the applied `|VFS|` threshold
  that `PatchThreshold` produces and later stages consume).

Only the expensive tail is worth persisting: `Projection`'s complex maps and the nine
`Retinotopy`-through-`DerivedMaps` results are disk-cacheable; the cheap host-side
stages recompute from the restored retinotopy rather than bloat the file with bulky
intermediates. The cache writes back into the same file the results live in.

## The cut: restore the deepest matching tail, recompute the rest

On each analysis the I/O boundary:

1. Computes the wanted fingerprint for every stage, in topological order.
2. Reads the stored `/analysis_state` fingerprints.
3. Finds the **restore frontier** — the deepest cacheable stage whose wanted
   fingerprint matches its stored fingerprint *and* whose output is present in the
   file — and seeds the pipeline state with that stage's outputs (and the `/cache`
   intermediates) read from disk.
4. Hands the orchestrator the set of restored stages. The walk
   (`pipeline/orchestrator.rs`) runs every stage that is neither restored nor
   already seeded, in topological order, checking the cancel token at each stage
   boundary, and writes each recomputed stage's output and fingerprint back.

## Invariants

**No silent stale results.** Any doubt recomputes. A missing fingerprint attribute,
an unreadable or unparseable one, a fingerprint mismatch, or a stage output absent
from the file all force recomputation of that stage and its descendants — never a
trust-by-default. A wrong cache hit is worse than a slow correct recompute, so the
cache is conservative at every ambiguity.

**Cancellation restarts at the right cut.** A parameter change mid-run sets the
cancel token; the current stage unwinds and the walk restarts with the new
parameters. The stages whose inputs did not change restore instantly, so the restart
resumes at the first dirtied stage rather than from scratch — "kill and restart at the
proper stage."

## Deliberate non-goals

- **No early cutoff.** A dirtied stage invalidates all its descendants, even when a
  descendant's input from it happens to be unchanged. Salsa-style early cutoff would
  require also hashing each stage's *output*, doubling per-stage I/O and adding an
  output-canonicalization contract. The fixed eleven-stage DAG does not need it.
- **No cross-file or remote cache.** The cache lives inside each `.oisi`; two files
  with identical inputs both compute. A shared content-addressed store (eviction,
  locking, a daemon) is foreign to a per-recording desktop instrument and buys
  nothing for the single-user workflow.

## Where this lives in the code

- `crates/isi-analysis/src/pipeline/stages.rs` — the stage table; `graph.rs` derives
  the topological order.
- `crates/isi-analysis/src/pipeline/fingerprint.rs` — the Merkle key over inputs.
- `crates/isi-analysis/src/pipeline/orchestrator.rs` — the walk that skips restored
  and seeded stages.
- `crates/isi-analysis/src/io.rs` — `read_stage_fingerprint` /
  `write_stage_fingerprint` / `read_all_stage_fingerprints` over `/analysis_state`,
  and the `/cache` intermediate reads and writes.
