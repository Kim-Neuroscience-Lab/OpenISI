# Incremental Re-Analysis Design

**Status.** Proposal. Not yet implemented.

**Scope.** This document covers incremental re-execution of the analysis pipeline in `crates/isi-analysis` — turning today's always-rerun-from-scratch `analyze()` into a DAG walk that re-executes only the stages whose inputs (params + upstream outputs) actually changed. It does **not** propose any change to the scientific methods themselves, to the `.oisi` schema's `/results/*` data layer, or to the substrate principles in `UNIFIED_COMPUTE_ARCHITECTURE.md` (one Burn backend type, f32 device pipeline, runtime device selection, method preservation). It introduces a per-stage **fingerprint sidecar** as a new annotation on the `.oisi` file.

The current cost is concrete: a user tuning a single morphological dilation iter (a Stage 7 tunable) currently waits for the per-cycle DFT (Stage 0) to re-run on raw frames. On the regression file this is minutes per tweak. The fix is to recognise that Stages 0–6 are unchanged and only re-run Stages 7+.

---

## 1. Survey of established incremental-recomputation architectures

For each system below we ask the same five questions: how is a stage's input fingerprint computed; where is the cache stored; how is invalidation cascaded; what's the failure / partial-result story; what's the UX when params change.

### Make / CMake / Ninja — mtime-based dependency graphs

Make compares the mtime of each target against the mtime of its declared prerequisites; if any prereq is newer, the recipe runs. Ninja adds a build log that also records the **command line** of the previous build, so changing a compiler flag forces a rebuild even when no source file's mtime moved. CMake re-generates Ninja files when `CMakeLists.txt` changes, which usually flips touched-file mtimes downstream.

- **Fingerprint:** mtime + (in Ninja) command-line string.
- **Cache:** the build directory; outputs *are* the cache.
- **Invalidation:** pull-based, eager when `make` runs. Cascades down the DAG via mtime ordering.
- **Failure:** if a recipe crashes mid-run, the (partial) target file is left on disk with a fresh mtime, which is **wrong** — the next build wrongly considers it up to date. Make's `.DELETE_ON_ERROR` opt-in and Ninja's `restat` rule patch around this.
- **UX:** none — Make is invoked manually.

**Applicability:** weak. Our problem is not "the file on disk changed", it's "the *parameter* the stage was given changed." Make-style mtime tracking would force us to touch the param file every time, then somehow communicate which stages care — which is exactly the dependency graph we'd have to write anyway.

### Bazel / Buck2 — content-addressed action cache

Bazel constructs an **action fingerprint** from a structured representation of each action: the executable, the command line, the digests of every declared input file, the environment, and the platform. The fingerprint is a SHA-256; the cache (local disk, optionally remote) maps fingerprint → output set. A change in any input — even a comment in a source file — alters the input file's digest, which alters every dependent action's fingerprint, which invalidates the whole transitive cone.

- **Fingerprint:** SHA-256 of canonicalised action protobuf (command + env + input digests + platform).
- **Cache:** content-addressed store, local + optionally remote.
- **Invalidation:** lazy / pull. The action is recomputed on demand if its fingerprint isn't in the cache.
- **Failure:** atomic. An action either produces its full declared output set and is cached, or it produces nothing and isn't.
- **UX:** invisible — the user just runs `bazel build` and changed bits run.

**Applicability:** strong. The "action" abstraction maps directly to our "stage", and the fingerprint formula (command + inputs) maps to (params + upstream outputs).

### DVC — data-version-control pipelines

DVC's `dvc.yaml` declares stages with `cmd`, `deps`, `outs`, and `params`. On `dvc repro`, DVC reads `dvc.lock`, which records per-stage: the dependency file hashes, the **resolved param values** (not just names), and the output hashes. A stage re-runs iff any tracked entry's current value differs from the lock.

- **Fingerprint:** content hash for files, literal value for params, all recorded per-stage in `dvc.lock`.
- **Cache:** content-addressed object store (`.dvc/cache/`) plus the `dvc.lock` sidecar.
- **Invalidation:** lazy. `dvc status` reports staleness; `dvc repro` runs the minimum set.
- **Failure:** the lock is updated only after a successful stage run, so a crash leaves the previous lock entry intact and the stage is correctly considered stale on retry.
- **UX:** manual reproduction (`dvc repro`) plus a status check.

**Applicability:** very strong, and ideologically the closest analog. The `dvc.lock` convention — separate sidecar of per-stage fingerprints, params recorded as resolved values — is essentially what we want.

### Snakemake / Nextflow — bioinformatics DAGs

**Snakemake 7.8+** changed its trigger model from pure mtime to a multi-source decision: mtime, **params**, code, software environment, and the set of input files all participate in the "needs rerun" decision. Each can be selectively disabled via `--rerun-triggers`.

**Nextflow** computes a per-task hash from: full input file path, last modified timestamp, file size (default), script body, container image, params. Hash hits the task work directory cache; `-resume` skips matching tasks.

- **Fingerprint:** structured (Snakemake) or hash-of-everything (Nextflow).
- **Cache:** Snakemake — the declared `output:` files. Nextflow — per-task `work/<hash>/` directory.
- **Invalidation:** lazy.
- **Failure:** Nextflow leaves the failing task's work dir in place but won't mark it cached.
- **UX:** explicit re-execute commands; both have a known footgun around false rerun-positives.

**Applicability:** the multi-source trigger model is honest — params and code are first-class invalidators — but the failure modes (false positives on lambda-in-params, mtime drift) are warnings for us.

### Dask delayed / Prefect / Airflow — DAG schedulers with caching

These are predominantly **in-memory** lazy-graph evaluators. Dask's `delayed` builds a task graph, `compute()` realises it, and intermediate `.persist()` keeps results in cluster memory. Prefect's result-caching key is user-supplied; cache backends are pluggable. Airflow's "task instance" model treats each run as an event log entry; XCom transfers small results, but heavy caching is the user's problem.

- **Fingerprint:** user-supplied (Prefect `cache_key_fn`), or implicit graph identity (Dask).
- **Cache:** memory (Dask), pluggable file/object store (Prefect), opaque (Airflow).
- **Failure:** task-level retry policies; partial graphs survive.
- **UX:** programmer-mediated.

**Applicability:** weak. These are general schedulers; the caching story is bolted on rather than the primary feature.

### Adapton / self-adjusting computation

Acar's line of work introduces dynamic dependence graphs (DDGs) and change propagation: every read of an input is recorded at the call site, and on input mutation a propagator walks affected nodes only. miniAdapton is a Scheme implementation in ~100 lines. The asymptotic theory is excellent — for many problems re-execution is O(δ) where δ is the perturbation.

- **Fingerprint:** implicit, via recorded read-edges.
- **Cache:** in-memory DDG.
- **Invalidation:** push-from-changed-input, pull-on-demand.
- **Failure:** the entire graph lives in memory; crash = lose everything.
- **UX:** transparent if it works.

**Engineering trade-off:** reported 2–30× constant-factor overhead on the *initial* run, because every read becomes a graph-node creation. For our 10-stage pipeline this is severe — first-run latency is already the headline complaint.

**Applicability:** instructive (Salsa, below, is Adapton-flavoured for Rust) but the runtime DDG model is overkill when our DAG is statically known and finite.

### Salsa (Rust) — what rust-analyzer uses

Salsa is the practical Rust-flavoured Adapton. Computations are declared as **queries** (pure functions); the framework records read-edges between queries and stores per-query revision numbers. When an input changes, only the transitive query closure that depended on the changed input is re-executed — and Salsa's *early cutoff* further short-circuits when a re-run produces the same value as the cached one. rust-analyzer uses it to recompile a file's IDE state on every keystroke.

- **Fingerprint:** revision number per input + the recorded input-set of each query.
- **Cache:** in-process, per-query-key hashmap.
- **Invalidation:** demand-driven (pull); on query call, walk recorded dependencies and recompute only the queries whose declared-input revisions exceeded the cached one.
- **Failure:** queries are pure; a panic in one query doesn't poison others.
- **UX:** invisible — every query call is "compute or retrieve".

**Applicability:** the model is excellent. The implementation cost is real: Salsa is ~10k lines of framework code and demands every computation be expressible as `fn(key) -> value` with no side effects.

### React / MobX / Solid — reactive UI frameworks

UI frameworks ship the same primitives in miniature: a signal/observable holds a value, a `computed` derives one signal from others by recording reads, and a `reaction` re-runs when an observed dep changes. MobX and Solid are **push** from changed source through derived to effect; React is **pull** (set state, schedule re-render, the next render reads). All assume in-memory single-process state.

**Applicability:** the conceptual vocabulary (push vs pull) is useful; the implementations target UI render budgets, not minute-scale numerics.

### Spark RDD lineage / Catalyst

A Spark transformation is lazy — the RDD/DataFrame stores a recipe, and `action()` materialises it through the Catalyst optimiser. `cache()`/`persist()` keeps an intermediate in cluster memory; `checkpoint()` writes it to durable storage **and breaks lineage** (so the cache is the source of truth). Cache invalidation on Delta Lake tables is automatic when the source is overwritten via the same cluster — but **not** across clusters.

- **Fingerprint:** lineage object identity.
- **Cache:** cluster memory or HDFS.
- **Invalidation:** lazy, recompute from lineage if the cache is dropped.
- **Failure:** partition-level retry, the cluster-level orchestrator picks up.
- **UX:** programmer calls `.cache()` deliberately.

**Applicability:** Spark's lazy DAG mirrors what we want, but the in-cluster-memory story doesn't fit a desktop Tauri app.

---

## 2. Requirements for our case

Distilled from the project principles, the `.oisi` schema, and the actual user flow.

**R1. Stage granularity is fixed.** The 10 stages in `compute_analysis` are well-defined and won't multiply. Whatever we build should not be a general-purpose framework; a hand-rolled per-stage table is the right size.

**R2. Stage params are already in a registry.** `PARAM_DEFS` (`crates/openisi-params/src/definitions.rs`) declares which param belongs to which stage via the `GroupId` (e.g. `CycleCombine`, `PhaseSmoothing`, …). The mapping from `param_id` → `stage` is a property of the param definition, not something we need to invent.

**R3. Outputs already live at known HDF5 paths.** `write_results` writes a flat `/results/<name>` layout. A stage's output set is enumerable in code; we don't need a Bazel-style "declare outputs" DSL.

**R4. The `.oisi` file is canonical.** Per the "Complete provenance" principle in `DATA_FORMAT.md`, the file contains everything needed to reproduce. Any sidecar we add must either live inside the `.oisi` (as HDF5 attrs) or be safely regenerable from it.

**R5. No silent stale results.** Per the project's "no silent fallbacks" line: **when in doubt, recompute**. A wrong cache hit is worse than a slow correct recompute. Every fingerprint mismatch — including "fingerprint attr missing", "fingerprint format unrecognised", "upstream output dataset missing" — must invalidate, never trust-by-default.

**R6. Bit-determinism within `f32` tolerance.** Same params + same input → same output, every run. Floating-point summation order on the device may permute bits at the cross-backend tolerance level (the per-dataset budgets in `tests/fixtures/tolerances.toml`); this is **not** a fingerprint input — we fingerprint *inputs to the computation*, not the computation's output digest, so summation-order noise doesn't trigger spurious invalidations.

**R7. The user UX is "edit a param, see the result."** No `Force Rerun` button on the happy path. A `--force` escape hatch in the CLI is fine (and useful for debugging an alleged cache bug), but the UI must not require it.

**R8. Cancellation already exists.** `compute_analysis` threads an `AtomicBool` cancel flag through every stage. Partial-work semantics around cancellation generalise to crash semantics: a half-finished stage must leave the file in a state where the next run **knows** the stage is stale.

---

## 3. Concrete proposal

### 3.1 Stage IR

```rust
/// One node in the analysis DAG.
pub struct Stage {
    pub id: StageId,                // enum: CycleCombine, PhaseSmoothing, ...
    pub upstream: &'static [StageId],
    pub param_groups: &'static [GroupId], // which GroupId(s) this stage reads from
    pub outputs: &'static [&'static str], // /results/<name> datasets it writes
}
```

The full DAG is a `static` table of 10 entries declared in `crates/isi-analysis/src/stages.rs`. Edges and output-name sets are wrong-once: a new stage means a new row + a recompile, which is correct for a 10-stage system.

`compute_analysis` is refactored into 10 `fn run_<stage>(ctx: &mut StageCtx) -> Result<()>` functions, each pulling its inputs from `ctx` (which holds the in-memory intermediate results plus the file handle for upstream reads from `/results/*`). The orchestrator owns the `for stage in topo_order { ... }` loop.

### 3.2 Fingerprint definition

A stage's fingerprint is a BLAKE3-256 of a **canonical** byte string composed of:

1. A `u32` schema version (bump when the fingerprint formula itself changes).
2. The stage's `StageId` discriminant.
3. The set of param `(key, value)` pairs belonging to the stage's `param_groups`, serialised in sorted-by-key JSON canonical form (`serde_json::to_vec` with sorted keys via `BTreeMap`). This explicitly includes the method-choice param (`*.method`) and the active-tunable subtree for the selected variant; **inactive** variants' tunables are excluded (they don't affect the result).
4. The fingerprints of every upstream stage's outputs, in declared upstream order.
5. The **input-data identity** for inputs not produced by other stages: for `cycle_combine` (Stage 0), this is `(start_camera_clock_us, end_camera_clock_us, frame_count)` from `/acquisition/clock_sync` — three i64s that uniquely identify the acquisition recording without rehashing 30 GB of frames. For the complex-maps-import path it's a digest of the four `/complex_maps/*` datasets (small, ~16 MB at 512²).
6. The `AcquisitionProperties` (rig + experiment params) JSON, when the stage's math reads them (`compute_retinotopy` uses `azi_angular_range`, `offset_azi`, `um_per_pixel`, etc).

Canonicalisation is load-bearing. JSON object key order, float string formatting (`ryu`), and numeric tag values must be byte-stable. We borrow `serde_json::ser::PrettyFormatter` with sorted-keys preprocessing and the `f64::to_string` round-trip rule; tests pin the formula by hashing known param trees and comparing against checked-in expected digests.

### 3.3 Where fingerprints live

**In-file, as HDF5 attrs on a per-stage group.** New layout addition:

```
/analysis_state/
    cycle_combine                attrs: fingerprint (str), status ("ok"|"failed"), schema_version (u32)
    phase_smoothing              attrs: fingerprint, status, schema_version
    ... (one group per StageId)
    write_lock                   attr: pid + start_time_us — present iff a write is in flight
```

Rationale:
- **In-file** satisfies R4 (single canonical artefact).
- **Per-stage group** keeps individual stage updates atomic at the HDF5 attribute level.
- A separate `write_lock` attr (created at stage start, removed at stage success) gives us crash detection (next run sees the lock, treats the stage as `failed`, invalidates). HDF5 itself is *not* multi-writer safe; the lock is paranoia, not concurrency. This satisfies R5 and R8.
- `schema_version` lets us tighten the fingerprint formula later without poisoning old files — a version mismatch is treated as "stale", same as a hash mismatch.

A `--no-cache` orchestrator flag (and the equivalent UI debug toggle) **deletes the `/analysis_state` group** before running, forcing a full rerun. This is the only escape hatch.

### 3.4 The `analyze()` algorithm

```text
fn analyze_incremental(path, registry_snapshot, progress, cancel) -> Result<()>:
    let caps = io::inspect(path)
    let stages = build_static_dag()                       # 10-entry table, topo-sorted

    # Phase A — fingerprint every stage in topo order.
    let mut want: BTreeMap<StageId, Fingerprint> = {}
    for stage in stages.in_topo_order():
        let upstream_fps = stage.upstream.map(|u| want[u])
        want[stage.id] = fingerprint(stage, registry_snapshot, upstream_fps, caps)

    # Phase B — read the in-file fingerprints + status.
    let have = io::read_analysis_state(path)              # may be empty / partial

    # Phase C — decide which stages to run.
    let mut run_set: Set<StageId> = {}
    for stage in stages.in_topo_order():
        let is_stale =
            stage.upstream.any(|u| u in run_set)          # cascade
            or have[stage.id].status != Some("ok")        # crashed last time / never ran
            or have[stage.id].fingerprint != Some(want[stage.id])
            or have[stage.id].schema_version != CURRENT_SCHEMA
            or any(stage.outputs).missing_from_file(path) # outputs got nuked externally
        if is_stale: run_set.add(stage.id)

    if run_set.is_empty():
        progress.set_stage("Cached — all stages up to date")
        return Ok(())

    # Phase D — execute the stale stages in topo order.
    let mut ctx = StageCtx::new(path, caps, registry_snapshot)
    for stage in stages.in_topo_order():
        if stage.id not in run_set:
            ctx.load_from_disk(stage.outputs)             # read previous outputs as inputs
            continue
        if cancel.load(): return Err(Cancelled)
        write_status(path, stage.id, "running")           # write_lock
        match run_stage(stage.id, &mut ctx):
            Ok(()):
                write_outputs(path, stage.outputs, ctx)   # /results/<name>
                write_fingerprint(path, stage.id, want[stage.id], "ok")
            Err(e):
                write_status(path, stage.id, "failed")    # explicit failure marker
                return Err(e)

    Ok(())
```

Pseudocode notes:
- **Cascade is explicit and pull-based**: if I'm stale, all my descendants are stale. We don't push from changed inputs because we don't have a long-lived in-process graph; each `analyze()` invocation reconstructs both `want` and `have` from scratch.
- **The cache key is the *fingerprint*, not the param tree.** Two different param-tree edits that compute to the same fingerprint (none, in practice, because we hash the params byte-for-byte) hit the same cache slot, which is the correct semantics.
- **Output-missing-from-file** is checked explicitly so that an externally-edited `.oisi` (someone deletes `/results/vfs`) re-runs from the right cut point.

### 3.5 Partial failure handling

Three failure modes, three behaviours, no silent recovery:

1. **Stage panics or returns `Err`.** `status = "failed"` written; the run aborts; the next run treats the stage as stale (the `status != "ok"` check) and re-runs from there. Upstream `"ok"` outputs are kept; that's the entire point.
2. **Process crashes mid-stage write.** The `write_lock` attr is present without a corresponding `"ok"` status. On next start, the orchestrator's Phase B treats `write_lock present` as `status = "failed"` for the locked stage and any of its descendants whose outputs may have been partially written. Conservative: invalidate the locked stage **and** all descendants (descendants will cascade-invalidate anyway because of the upstream check).
3. **HDF5 dataset corruption.** Fletcher32 checksums are already on every dataset (per `DATA_FORMAT.md` "Data integrity"). A checksum-failed read triggers the same `status = "failed"` path on the *consumer* stage, which then re-runs and fixes itself only if the upstream stage's output is also re-derivable. If `/acquisition/camera/frames` itself is corrupt, we error out and stop — there is no recovery from raw-frame corruption.

There is no "salvage partial outputs" mode. A failed stage's outputs are not trusted, period.

### 3.6 `--force` and `--no-cache` flags

- `--force` (CLI + headless): tactical, force-rerun **one named stage** and cascade. Useful when debugging an alleged cache miss.
- `--no-cache` (CLI + UI debug): nuke `/analysis_state` and re-run everything. Hard reset.

Neither is on the happy path. The UI does not expose them in normal use; they're behind a developer-mode toggle.

### 3.7 Where this lives in the code

- `crates/isi-analysis/src/stages.rs` — new file: `Stage`, `StageId`, the static DAG, the param-group-to-stage mapping.
- `crates/isi-analysis/src/fingerprint.rs` — new file: canonical-serialise + BLAKE3.
- `crates/isi-analysis/src/io.rs` — add `read_analysis_state`, `write_stage_fingerprint`, `clear_analysis_state` (for `--no-cache`).
- `crates/isi-analysis/src/lib.rs` — `analyze()` becomes the Phase A–D orchestrator; the body of today's `compute_analysis` is split into 10 `run_*` functions consumed by it.

---

## 4. Tradeoffs

**Adopting fingerprints adds a serialisation contract.** The canonical-JSON form of params, the active-variant exclusion rule, the upstream-output-fp ordering — these are part of the fingerprint definition. Any drift silently invalidates everyone's caches. We mitigate by `schema_version` bumping (explicit, visible) and by checked-in expected-digest tests.

**HDF5 attributes are not atomically updated.** A power loss between "write fingerprint" and "fsync" leaves an attr-of-undefined-value. The `write_lock` plus `status` discipline catches this — on restart the lock is present, the status is not `"ok"`, the stage is stale. The cost is one extra attribute write per stage run (negligible).

**We do not reuse outputs across files.** Unlike Bazel's content-addressed store, our cache lives *inside* the `.oisi`. Two files with identical inputs both re-run. This is intentional: a shared cache is operationally complex (eviction, locking) and not motivated by the per-user / per-recording workflow.

**The fingerprint excludes inactive method variants.** A param subtree under an unselected method-choice variant is not part of the fingerprint. If the user toggles methods back and forth (`A → B → A`), the second `A` is a cache hit. This is the correct semantics by definition but worth flagging: it implies `PARAM_DEFS.active_when` is part of the fingerprint contract; changing an `active_when` predicate is a schema-version bump.

**`f32` summation order is not part of the fingerprint.** Per R6 we fingerprint *inputs to the computation*, not *outputs of the computation*. A run that produced `vfs[42, 100] = 0.4231` on the CUDA backend and `vfs[42, 100] = 0.4232` on the ndarray-CPU backend for the same params is correctly considered a cache hit if it happens to be the cache occupant — within the cross-backend tolerance committed in `tests/fixtures/tolerances.toml`. The alternative (fingerprint the output, treat any bit-flip as invalidation) would create thrashing across machine moves with no scientific benefit.

**Cascade is conservative.** Any stale stage invalidates all its descendants, even when the descendant's input from the stale stage *might* be unchanged (Salsa's "early cutoff"). Implementing early cutoff requires hashing each stage's output too, which doubles the per-stage I/O and adds a new contract (output canonicalisation). Defer until measured-need.

**No remote cache.** We are a desktop scientific instrument, not a distributed build system. Even if a lab had a NAS, the per-`.oisi` workflow and the "every file is self-contained" principle make a remote cache foreign to the model.

**`/analysis_state` is part of the `.oisi` schema and bumps `version`.** Adding the group is a non-breaking additive change (older readers ignore the unknown group), but the orchestrator now writes attrs that older versions don't know about. We bump `version` from `"2.0"` to `"2.1"` and document it in `DATA_FORMAT.md`.

---

## 5. Open questions

1. **Does the cortex ROI count as a param for fingerprinting?** Currently the user-drawn polygon is at `/anatomical/cortex_roi`. Editing the polygon should invalidate Stage 5 (cortex source) and downstream — but only when `CortexSourceKind::UserPolygon` is the selected method. Proposal: hash the polygon dataset bytes, include in Stage 5's fingerprint **only when** `cortex_source.method == UserPolygon`. This is the same active-variant discipline as for params.

2. **What about `BaselineMode`?** `BaselineMode` is a Stage 0 (raw frame processing) param. A switch from `AllFrames` to `OutsideSweepWindows` will, correctly, invalidate every stage downstream of Stage 0. Confirm `BaselineMode` is in `PARAM_DEFS` (under a `GroupId::CycleCombine` group) before the design lands — current grep returns nothing under that name. If it's an `AnalysisParams` field not yet promoted to the registry, lift it first.

3. **Does the `software_version` attribute participate in the fingerprint?** Bazel includes the toolchain in action keys. We currently don't — but a bug fix in `compute_snr` could silently serve stale results from before the fix. Two options:
   - Strict: bump `schema_version` on every analysis-crate release. Forces a full re-run on every update; expensive but correct.
   - Permissive: include `software_version` in every stage's fingerprint by default. Same behaviour, less ceremony. Recommendation: permissive.

4. **In-process memoisation across `analyze()` calls?** Each `analyze()` invocation today reconstructs state from disk. For a UI workflow where the user is tweaking one param every few seconds, an in-process `StageCtx` cache (per-`.oisi`-handle, dropped on file change) would skip the disk re-read entirely. Worth a follow-up after the on-disk caching is shipped and measured.

5. **Debounce in the UI?** Right now there's no debounce — a slider drag would fire `analyze()` on every value change. With incremental analysis the *cost* of each fire drops dramatically but the *floor* (load upstream outputs from disk + recompute the last stage) is still non-zero. The UI should debounce param edits (~300 ms) regardless; that's a UI-layer concern, not an analysis-layer one.

6. **What about acquisition-time params changing?** `AcquisitionProperties` (rig + experiment params, recorded at capture) is technically immutable per file. But the `/rig_params` and `/experiment_params` JSON attrs *can* be overwritten by a migration command. We include them in the fingerprint of stages that read them (Stage 2 `compute_retinotopy` reads `um_per_pixel`, `azi_angular_range`, etc), so a migration that changes a value correctly invalidates the dependent stages.

---

## Sources

- [DVC: Defining Pipelines](https://doc.dvc.org/user-guide/pipelines/defining-pipelines)
- [DVC: dvc.yaml Files](https://doc.dvc.org/user-guide/project-structure/dvcyaml-files)
- [DVC: repro command reference](https://dvc.org/doc/command-reference/repro)
- [Bazel: Persistent Action Cache (sluongng)](https://sluongng.hashnode.dev/bazel-caching-explained-pt-4-persistent-action-cache)
- [Bazel: Using Remote Cache Service for Bazel (ACM Queue)](https://queue.acm.org/detail.cfm?id=3287302)
- [Snakemake: Change in rerun behavior in 7.8.0](https://github.com/snakemake/snakemake/issues/1677)
- [Snakemake CLI: --rerun-triggers](https://snakemake.readthedocs.io/en/stable/executing/cli.html)
- [Nextflow: Caching and resuming](https://www.nextflow.io/docs/latest/cache-and-resume.html)
- [Adapton: composable, demand-driven incremental computation (Hammer et al., PLDI 2014)](https://dl.acm.org/doi/10.1145/2666356.2594324)
- [Acar: Self-Adjusting Computation (CMU thesis)](https://www.cs.cmu.edu/~rwh/students/acar.pdf)
- [Salsa: overview](https://salsa-rs.github.io/salsa/overview.html)
- [Salsa: algorithm explained (Lakhin)](https://medium.com/@eliah.lakhin/salsa-algorithm-explained-c5d6df1dd291)
- [rust-analyzer: Durable Incrementality](https://rust-analyzer.github.io/blog/2023/07/24/durable-incrementality.html)
- [Spark: Catalyst Optimizer transforms lazy evaluation into execution](https://medium.com/@shlokjp/spark-how-the-catalyst-optimizer-transforms-lazy-evaluation-into-execution-7e61c6591f46)
- [Databricks: Expensive transformation on DataFrame is recalculated even when cached](https://kb.databricks.com/python/expensive-transformation-on-dataframe-is-recalculated-even-when-cached)
