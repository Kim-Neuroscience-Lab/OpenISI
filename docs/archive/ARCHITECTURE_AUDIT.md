# Architecture Audit & Issue Register

**Date:** 2026-06-12 · **Status at audit:** full workspace test suite green, clippy clean,
all work uncommitted on `main`.

## Why this document exists

This is a handoff artifact. It records every known structural issue (dead code, DRY /
SoC / SSoT violations, consistency gaps) found in a deliberate whole-codebase sweep, so a
new maintainer can see the real state at a glance instead of rediscovering it. Each finding
carries a **location**, the **principle** it violates, a **severity**, a **confidence**
(Verified = checked directly; Reported = surfaced by the sweep, plausible, spot-check before
acting), and a **one-line fix**. None of these are correctness bugs in the analysis math —
the pipeline output is golden-validated. They are maintainability / clarity issues.

How to read severity: **High** = a drift hazard that has bitten us or will bite the next dev;
**Med** = real but contained, fix when you next touch the area; **Low** = polish.

---

## Remediation status (2026-06-12)

Active remediation pass underway. Resolved so far (full workspace green + clippy clean after each):

- **H-1** ✅ UI stage list now served by the `get_analysis_stages` Tauri command,
  derived from PARAM_DEFS (`analysis_stage_groups()` + `GroupId::card_title`); JS
  hardcode removed. Pinned by `analysis_stage_groups_are_the_eleven_pipeline_stages_in_order`.
- **H-2** ✅ Corrected (was over-flagged) — invariant documented + test-locked. See row below.
- **M-1** ✅ Unused `imageproc` dependency removed.
- **M-4** ✅ Corrected (was over-flagged) — misleading config header fixed + end-to-end
  shipped-config load/bridge guard added. See row below.
- **L-1** ✅ Method entry points unified to `apply()`; `BaselineMethod::apply` returns a
  `BaselineResult { f0, floor }`; `combine`/`resolve` → `apply`.
- **L-2** ✅ `CortexSource` → `CortexSourceMethod` (now consistent with all other method enums).
- **L-3** ✅ `parse_group_id` replaced by a `strum::EnumString` derive on `GroupId`
  (`group_id_from_str_snake_case` pins it).
- **L-4** ✅ `CycleAverageMethod::apply` expresses the non-empty invariant via `reduce`/`?`
  (no `.expect()`).
- **L-5** ✅ `PatchRefinementMethod::None` carries the convention's "why no citation" doc.

- **M-2** ✅ **Narrowed** after a scope review (the literal 5-way split was over-engineering —
  the read/write/inspect core is cohesive). Extracted only the two genuinely-separable concerns:
  `io/meta.rs` (rendering metadata) and `io/import.rs` (SNLC `.mat` import). `io.rs` 2107 → 1617.
- **M-3** ✅ **Reframed** — `meta_for_f64` is a readable name→meta dispatch with an intentional,
  documented default arm, not a problematic god-function; a static table would fight the
  percentile-from-`data` arms. Pinned the render contract with `meta_for_f64_pins_explicit_render_contract`
  + `classify_result_type_routes_known_datasets` instead.

A **scope review** (prompted by "is the partitioning right for this project's size?") produced two
course-corrections worth recording for the next maintainer: (1) the audit over-indexed on `io.rs`;
the real god-file is **`src-tauri/src/bin/headless.rs` (3794 lines, 53 fns)** — added as a finding
to split next. (2) Large files like `patch_refinement.rs` are large only because of idiomatic
inline golden tests — NOT a partitioning problem; leave them.

- **L-7** ✅ Stale "current MPS pipeline" comment in `regression_oisi.rs` corrected (the MPS
  references are historical fixture provenance; only the "current" wording was wrong).
- **L-8** ✅ `.bak`/`.orig` gitignore rule added (the flagged `.bak` was untracked clutter in the
  vendored `reference/` dir — not ours to delete).
- **C** ✅ Task #55 closed as not-reproducible (see "Open / unverified" below).

- **M-5** ✅ Duplicated golden-test byte-loaders + diff helpers consolidated into a single
  `#[cfg(test)] test_support` module (`load_f64`/`load_f32`/`load_i32`/`count_differing`).
- **L-6** ✅ Resolved by *documenting* where the 5 dispatcher-only method modules are actually
  validated (in `methods/mod.rs`), rather than adding redundant cargo-cult unit tests.

**All 13 original findings + the `io.rs` split are complete; full workspace green, clippy clean.**

**`headless.rs` / rendering — reframed after a "are we reinventing a framework?" review.** The
binary's PNG encoding already uses the `png` crate (correct). But the scalar-map RENDERER was
reinvented THREE times — `headless.rs` (MapMeta-aware), `commands/analysis.rs::export_map_png`
(its own `jet_colormap`, auto-fit min/max, **ignoring MapMeta** — a latent bug: the GUI's "export
PNG" disagreed with its own on-screen view), and the JS canvas renderer. **DONE (Piece A):**
extracted one shared `src-tauri/src/render.rs` (`render_map` + `hsv_circular`/`hot`/`jet` +
`write_rgba_png`), used by BOTH Rust renderers; `export_map_png` is now MapMeta-faithful (bug
fixed); colormaps golden-pinned (`render::tests::colormaps_pinned_at_anchor_points`). `headless.rs`
3794 → 3601. Verdict on frameworks: `clap` is the one clear standard-tool win for the CLI; `plotters`
would be over-engineering for these bespoke faithful-output validation figures — keep the hand-rolled
(now deduplicated) renderer.

**DONE (Piece B):** adopted `clap` derive for the CLI — the hand-rolled `match args[1]` dispatch +
40-line `print_usage` are replaced by a declarative `Commands` enum (single source of truth for the
command surface), with typed args, auto-generated `--help`/usage/version, and arg validation. All 14
subcommands preserve their original CLI (verified via `--help` + a live `inspect` run).

**DONE (Piece C):** split the binary into `src/bin/headless/main.rs` (2168 lines — CLI + command
handlers) and `src/bin/headless/figures.rs` (1441 lines — figure-generation: `export_all_figures`,
the `compare_*` / threshold-grid family, the bitmap label font, `write_meta_json` & friends). Pure
relocation (the only logic touch was restoring a `#[derive(Copy, Clone)]` the cut clipped); `figures.rs`
reaches the bin-root helpers via `super::`, the binary-root reaches the figure entry points via
`pub(crate)`. The `[[bin]]` path was updated to the multi-file layout. Zero-warning build, CLI verified.

**`headless.rs` is fully resolved (A + B + C).** The whole audit — all 13 original findings, the
`io.rs` split, the renderer dedup + `export_map_png` bug fix + colormap goldens, the `clap` CLI, and
this split — is complete. Full workspace green, clippy clean.

## TL;DR verdict

The architecture is **fundamentally sound**: a proper declared-dependency DAG executor, a
single macro-driven parameter SSoT (`define_params!`), a clean crate-dependency direction,
a real compute framework (Burn) rather than a hand-rolled tensor library, and disciplined
golden-backed faithful replication of named oracles. It is **not** built on a malformed
base. The issues below are a finite, mostly-mechanical punch-list — the largest theme is a
handful of **silent cross-boundary mirrors** (Rust↔JS, code↔config) that lack compile-time
enforcement and can therefore drift.

---

## A. Verified clean — do NOT re-investigate these

A new maintainer can trust these; they were checked and are in good shape:

- **Crate dependency direction is acyclic and clean:** `openisi-params` ← `isi-analysis` ←
  `src-tauri`; `openisi-stimulus` and `pco-sdk` standalone. No inverted edges.
- **Boundary discipline holds:** `analyze()` (`crates/isi-analysis/src/lib.rs:321`) owns all
  file I/O; the pipeline core (`pipeline/`, `methods/`, `compute/`) never touches the
  filesystem/HDF5. `openisi-params` pulls in no ndarray/hdf5/tauri/tch.
- **The DAG is real and proper:** explicit `Stage` trait (`pipeline/stage.rs:99`), deps
  declared as data, `petgraph` toposort with cycle detection, typed `PipelineState`
  blackboard with `*_ref() -> Result<&T, MissingData>` accessors (no unwraps in stage code).
- **Hand-rolled numerics are principled, not sprawling:** every reimplemented primitive
  (gaussian, morphology, labeling, skeletonize, DFT, eccentricity, Jacobian) cites a named
  oracle and carries a golden test; off-the-shelf crates would silently lose fidelity on
  border/SE/tie-break conventions. OpenISI-original methods are honestly labeled.
- **The tch→Burn migration is complete and clean:** no lingering torch/libtorch/MPS code
  (only dated comments noting the removal). Burn is the genuine compute substrate.
- **No god-objects in the compute core; no commented-out dead blocks; no active
  TODO/FIXME blockers** in production paths.

---

## B. Findings register

### High severity

| ID | Finding | Location | Principle | Conf. | Fix |
|----|---------|----------|-----------|-------|-----|
| **H-1** | The analysis UI stage list is hardcoded in JS, mirroring the Rust `GroupId` enum with no link. Has already drifted once this session (missing Baseline card). | `ui/src/views/analysis.js:73-85` (STAGES) vs `crates/openisi-params/src/lib.rs:195-206` (GroupId) | SSoT / DRY | Verified | Add a Tauri command that returns the stage list `[{key,title}]` from the backend; build STAGES from it at startup. |
| **H-2** | ~~Fingerprint/bridge arms are silent mirrors not compiler-enforced.~~ **CORRECTED 2026-06-12 — over-flagged.** On inspection: bridge.rs and fingerprint.rs have NO `_` wildcards and destructure tunables fully (`{ sigma_px }`, no `..`), so same-crate exhaustiveness already makes both a new variant AND a new tunable a hard compile error. The method enums are already `#[non_exhaustive]`. The premise was wrong; `#[non_exhaustive]` would add nothing. **RESOLVED:** made the invariant explicit in the fingerprint module doc and locked it with per-input sensitivity tests (`tests/incremental.rs`: `fingerprint_sensitive_to_cycle_average`, `fingerprint_sensitive_to_phase_smoothing_tunable`) so a future `_`/`..` regression fails the suite. | `crates/isi-analysis/src/pipeline/fingerprint.rs`, `crates/isi-analysis/src/bridge.rs` | SSoT / correctness-adjacent | Verified | DONE — invariant documented + test-locked. |

### Medium severity

| ID | Finding | Location | Principle | Conf. | Fix |
|----|---------|----------|-----------|-------|-----|
| **M-1** | `imageproc = "0.26"` declared but never used (the adjacent comment even says morphology is hand-rolled instead). Actively misleads a reader into thinking we depend on it. | `crates/isi-analysis/Cargo.toml:69` | Dead code | Verified | Delete the dependency line. |
| **M-2** | `io.rs` is ~2100 lines mixing HDF5 read, HDF5 write, MapMeta/display metadata, SNLC `.mat` import, and helper utilities. High cognitive load; unclear which parts are stable. | `crates/isi-analysis/src/io.rs` | SoC | Verified | Split into `io/{inspect,read,write,meta,import}.rs`, re-export the public API. |
| **M-3** | `meta_for_f64` is a ~160-line god-function hand-mapping palette/units for 30+ result datasets. A new leaf silently falls back to `jet`/`unitless`; no compile-time check that every `AnalysisResult` field has an entry. | `crates/isi-analysis/src/io.rs` (`meta_for_f64`, ~L1195-1357) | SoC / SSoT | Reported | Move to a data table keyed by dataset name; assert at compile/test time that every result field is covered. |
| **M-4** | ~~Config restates PARAM_DEFS defaults → drift.~~ **CORRECTED 2026-06-12 — premise was wrong.** `config/analysis.toml` is this lab's *deliberately-tuned working state* (e.g. `cortex_source = reliability` vs default `snlc_garrett2014_im_bound`; `merge_overlap_thr = 0.01`), self-documented as "current values for THIS machine". It is SUPPOSED to differ from defaults, so "must match PARAM_DEFS" is the wrong contract. The only real drift risk is a *stale key* after a rename. **RESOLVED:** fixed the genuinely-misleading header comment (it falsely claimed "Defaults shown match PARAM_DEFS" and pointed at a stale schema path), and added the correct guard `bridge::tests::shipped_analysis_config_loads_and_bridges` — the shipped config must still load (fail-loud on unknown keys) AND bridge to a valid `AnalysisParams` end-to-end. | `config/analysis.toml`, `crates/isi-analysis/src/bridge.rs` | SSoT / docs | Verified | DONE — misleading doc fixed + end-to-end load/bridge guard added. |
| **M-5** | Golden-test byte-decoder helpers (`load_f64`/`load_f32`/`load_i32`/`diff_u8`/`differing_px`) are copy-pasted across ≥4 test modules. | `compute/golden_vfs.rs:45`, `methods/patch_refinement.rs:923`, `segmentation/golden_cortex_morph.rs:29`, `openisi-stimulus/tests/golden_spherical_marshel.rs` | DRY | Verified | Extract a shared `golden_helpers` test-support module. |

### Low severity

| ID | Finding | Location | Principle | Conf. | Fix |
|----|---------|----------|-----------|-------|-----|
| **L-1** | Method entry-point names diverge: `apply()` (7 methods), `combine()` (cycle_average), `resolve()` (cortex_source), `baseline()`+`floor()` (baseline). | `crates/isi-analysis/src/methods/*.rs` | Consistency | Verified | Standardize on `apply()` where the signature allows; for baseline, return a `{f0, floor}` result struct from one `apply()`. |
| **L-2** | `CortexSource` is the only method enum without the `Method` suffix (10/11 have it). | `crates/isi-analysis/src/methods/cortex_source.rs:23` | Naming | Verified | Rename `CortexSource` → `CortexSourceMethod`. |
| **L-3** | `parse_group_id` is a 15-arm hand-written string→GroupId match with no codegen link to the enum. | `src-tauri/src/params/commands.rs:392-417` | DRY | Verified | Derive `strum::EnumString` on `GroupId` and call `from_str`. |
| **L-4** | `CycleAverageMethod::combine` uses `.expect("n > 0 checked")` twice. Guarded-unreachable (the `n == 0` early-return precedes them), so not a real hazard — style only. | `crates/isi-analysis/src/methods/cycle_average.rs:50,72` | Error-handling style | Verified | Optional: restructure so the invariant is expressed without `expect`. |
| **L-5** | `PatchRefinementMethod::None` lacks the provenance docstring every other variant carries. | `crates/isi-analysis/src/methods/patch_refinement.rs:23` | Docs consistency | Verified | Add a one-line "OpenISI passthrough; no published method" doc. |
| **L-6** | Five method files have no in-module unit tests (`baseline`, `cycle_combine`, `eccentricity`, `vfs_computation`, `patch_extraction`). They ARE covered by the equivalence/regression integration tests, so this is optional coverage, not a gap. | `crates/isi-analysis/src/methods/` | Test consistency | Reported | Add minimal shape/edge unit tests, or a comment noting integration coverage. |
| **L-7** | Stale comment references the retired MPS backend in a fixture-provenance note. | `crates/isi-analysis/tests/regression_oisi.rs:~52` | Dead doc | Reported | Update the comment to name Burn. |
| **L-8** | A `.bak` file is tracked under the vendored reference dir. Third-party clutter, not our code. | `reference/KimLabISI/docs/archive/...md.bak` | Repo hygiene | Reported | Remove; gitignore `*.bak`. |

---

## C. Open / unverified

- **Task #55 "Fix acquisition-layer principle violations (audit findings)"** — **CLOSED
  2026-06-12 as not-reproducible.** The original audit detail was never captured, and a static
  sweep of the camera/stimulus/acquisition code did not reproduce a concrete violation (real-time
  threading is sound — see the completed camera/stimulus threading work; frames are pre-allocated;
  acquisition/analysis SoC is clean). If a real-time issue is later observed empirically (drops,
  jitter, missed deadlines on hardware), open a fresh task with the concrete symptom rather than
  reviving this detail-less one.

---

## D. Suggested remediation order

1. **H-1** (UI STAGES from backend) and **M-1** (drop `imageproc`) — small, high signal.
2. **H-2** (`#[non_exhaustive]` + coverage) — converts the silent science-relevant mirrors
   into compiler errors; do this before adding any further methods.
3. **M-5** + **L-1/L-2/L-3/L-5** — mechanical consistency pass over `methods/` and test utils.
4. **M-2** (`io.rs` split) and **M-3** (MapMeta table) — larger refactors; do when next
   touching the I/O layer.
5. **M-4** (config↔PARAM_DEFS) — add the drift-guard test.
6. **C** (task #55) — resolve or close.

None of these block continued development; they reduce the surface a new maintainer must hold
in their head, and they close the silent-drift paths (H-1, H-2, M-3, M-4) that could otherwise
introduce real bugs later.
