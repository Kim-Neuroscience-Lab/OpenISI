# Unified Compute & Concurrency Architecture

**Status:** Spec + roadmap. Supersedes the (now-deleted) `ANALYSIS_COMPUTE.md` and the compute-related portions of `ARCHITECTURE.md`.

**Implementation status (2026-06-04).** This document is partly built and partly roadmap. Read it with that split in mind:

- ✅ **BUILT and validated** — §1 Substrate (pure-Rust Burn via `burn-dispatch`, runtime device selection, CPU + CUDA validated); §3 State decomposition (the `AppState` god-mutex is replaced by co-access `parking_lot` lock groups; work evicted from locks — validated by build + unit/integration tests); §4 Pipeline DAG (the `Stage`/blackboard/`petgraph` orchestrator, with the blake3 + HDF5-resident incremental cache — retinotopy restores from `/results` when its input fingerprint matches; sentinel-tamper test proves disk restore; equivalence test proves bit-identical output); §11 Observability (`tracing` subscriber + per-stage timing + diagnostic migration); §10 Verification (real-SNLC-data baselines, cross-implementation + cross-backend equivalence harness, property tests, committed tolerances). Segmentation stages 4–10 run on host `ndarray` (the audit's correct call — inherently CPU work).
- ✅ **§5.1 algorithm fixes — done, but DATA-DIRECTED, not roadmap-directed.** The per-stage timing (§11) measured the real hotspot — `patch_refinement` at 943 ms, dominating everything else — and the fix landed there: O(N²)→O(N) memoization + rayon over the independent split/merge work brought it to **317 ms (3×)**, with `patch_extraction` 54→21 ms, both equivalence-gated bit-exact. The roadmap's *assumed* techniques (van Herk, integral-image, batched-GEMM) were measured to be aimed at the wrong place — the morphology kernels are tiny (no van Herk win) and the DFT/retinotopy is 171 ms on-device (no GEMM win). Applying them would be optimization without a measured target — which our "measure before optimize" principle forbids.
- ❌ **MEASURED & DELIBERATELY NOT BUILT (data-disconfirmed).** §5.2 device residency and §2 the GPU-stream / IO-prefetch / PNG thread zoo were each measured against the per-stage timing and **disconfirmed**, so they are intentionally *not* built — this is a recorded decision, not a backlog item:
  - **§5.2 residency:** the GPU-amenable segmentation stages (4 sign-smoothing 3.6 ms, 6 threshold 0.2 ms, 10 eccentricity 0.9 ms) are already negligible on the host; moving them on-device would add upload/download round-trip overhead *exceeding* the compute saved, and the rest of the tail (connectivity/morphology/`Vec<Patch>`) is inherently CPU and must round-trip back regardless. Net: no win, more complexity.
  - **§2 thread zoo:** the DFT runs *once per file* (the §4 cache skips it on every re-analysis), so GPU-stream concurrency only helps first-analysis, never the iterative loop; `write_results` is 12 ms and reads are sub-ms (IO-prefetch unwarranted); PNG encode is already throttled and moved out of the lock in §3 (PNG thread marginal). The only unknown is first-analysis DFT cost on real raw data, which can't be measured on the in-tree fixtures (they have no raw frames).
- 🚧 **ROADMAP — not built, not yet measured** — §6–§7 tensor/IO discipline, §13 Phases E–G. Described as intended end-state; revisit only if a measured need appears.

Where this document and the code disagree on a 🚧 section, the code is the current reality and the doc is the goal. Where they disagree on a ✅/❌ section, that's a bug in one of them — fix it. The ❌ sections are kept (not deleted) so the *reasoning* for not building them survives — if a future measurement changes the picture, the decision can be revisited with the data in hand.

**Scope.** The complete concurrency, compute, and data-flow architecture for OpenISI — analysis pipeline, Tauri shell threading, state ownership, IO model, verification. Out of scope: stimulus rendering (`crates/openisi-stimulus`, governed by display-timing constraints and documented separately), Tauri UI layout, configuration schema (already covered by `openisi-params` SSoT).

**Principle.** OpenISI is pre-1.0 with no user base. There is therefore no value in compatibility shims, dual-backend transitions, deprecation periods, or any other artifact of incremental migration. Every concern below has exactly one intended implementation. We do not carry parallel implementations of the same concept.

---

## 0. Non-negotiables

These are commitments, not preferences. Pull requests that violate any of them are rejected on principle, not negotiated.

1. **One tensor type for on-device analysis compute.** `burn_tensor::Tensor<Backend, D>` (✅). No `tch::Tensor`. No `Vec<f32>` masquerading as a row-major matrix. `ndarray::Array*` IS retained — deliberately — at the HDF5 boundary (the `hdf5-metno` API hands back `ndarray`), as the host representation for the segmentation stages (4–10, inherently CPU), and to name the `NdArrayDevice` device value. The device *tensor* pipeline (DFT + retinotopy) is Burn; ndarray is the host-side companion, not a parallel device tensor.
2. **One compute substrate.** Burn `=0.21` via `burn-dispatch` (✅) — its runtime multi-backend layer. One backend type (`Dispatch`); the device is a runtime `DispatchDevice` value (§1.4). The ndarray (CPU) backend is always compiled in; `--features cuda` adds the CUDA backend. No `tch`. No raw `cudarc` except in one named module (`crates/isi-analysis/src/cufft.rs`) if and only if we eventually need cuFFT.
3. **No god-mutex.** State is decomposed by purpose. `AppState` becomes a struct of `Arc<Mutex<…>>` fields whose contents are independently lockable. `state.lock()` does not exist as a pattern.
4. **No polling loops.** All inter-thread coordination is `crossbeam_channel::select!` or `crossbeam_channel::recv()`. Polling with `try_recv() + sleep` is forbidden in new code.
5. **No work inside locks.** Lock, snapshot, drop, work. The duration of any `Mutex::lock()` is bounded by reading or writing a small set of fields. HDF5 IO, tensor ops, PNG encoding, and JSON serialization never happen with a lock held.
6. **No silent fallbacks.** A missing CUDA device, an unavailable monitor, or a malformed config produces a typed error and surfaces. There is no "fall back to CPU if CUDA isn't there" branch in the analysis path. Device selection is explicit at startup; mismatch is fatal.
7. **No `String`-typed errors crossing module boundaries.** `thiserror`-derived enums everywhere. The only `String` is the *display* form for the UI.
8. **No magic numbers in algorithm code.** Every numerical constant that affects the science is a parameter in the `openisi-params` Registry, propagated through `RegistrySnapshot` → `AnalysisParams`. Constants in algorithm code are mathematical (π, e, 2.0) — never tuning knobs.
9. **Every algorithm choice cites its canonical reference.** A `// Kalatsky & Stryker 2003, Eq. 3` comment at the top of the function is mandatory wherever the math comes from the literature. Where multiple papers describe variations, the variant we implement is named explicitly (these names already live in the per-stage method enums).
10. **Verification is automated, not aspirational.** Per-stage equivalence tests against committed real-data baselines, plus property tests, run on every commit (✅). There is **no** external numerical oracle (no Python reference) — see §10 for why; the baseline-and-invariants approach replaced it. Untested code is incomplete code.
11. **One codebase, device-native at the substrate.** OpenISI is **one** Rust program — not a Windows version plus a macOS version plus a Linux version. Platform and device differences live exclusively at the substrate layer: `burn` abstracts compute device (the same `Tensor<B, 2>::matmul` runs on CUDA, Metal, WGPU, or LLVM-CPU); `wgpu` abstracts the graphics API; `tauri` abstracts the WebView; `hdf5-metno` abstracts file IO; `std` abstracts the OS. No business logic in `crates/isi-analysis` or `src-tauri` contains a `#[cfg(target_os)]` branch, queries "am I on CUDA?" to choose an algorithm, or maintains a parallel path per OS. **The one named exception** is the camera driver, because vendor SDKs are inherently platform-specific: `trait Camera` is the abstraction; `PcoPanda: impl Camera` is the only impl today (Windows-only, shipped by PCO as Windows DLLs); future `impl`s (GenICam on Linux, IIDC on macOS, etc.) plug in at the same boundary without touching the rest of the system. CI runs **the same test suite** on every (OS, device) pair we deploy to — passing on each confirms the substrate abstraction holds; a failure on one is fixed by repairing the leak in the substrate or our use of it, never by forking the code.
12. **Established crates first, hand-rolling requires written justification.** When an actively-maintained Rust crate solves the problem, we use it (see the concurrency-primitives table in §8 and the dependency list in §14). Hand-rolling is permitted only when: (a) no established crate covers the need — examples in our case: van Herk morphology on f32 fields, the ~50 LOC ready-set scheduler on top of petgraph; OR (b) every established alternative was evaluated and rejected with a cited reason — examples: `dagrs` (archived 2026-01), `cust` (4-year-old release), `tch` (libtorch C++ dep we're removing). Either justification is documented in this spec or in the relevant module's header comment. "I thought it'd be cleaner to write it ourselves" is not a justification.

---

## 1. Substrate decisions

### 1.1 Tensor library

**Burn `=0.21` via the granular crates `burn-tensor` + `burn-dispatch` (+ `burn-ndarray` always, `burn-cuda` behind `--features cuda`).** We depend on the granular crates, not the `burn` umbrella (whose default features pull `burn-tch`/libtorch — the native C++ dep we removed). `burn-dispatch` provides the single `Dispatch` backend type with runtime device selection (§1.4). Burn was chosen over Candle in a documented evaluation. The dominant reasons:

- Burn ships multi-stream concurrency in CubeCL (v0.19, May 2025): each tensor operation thread gets its own stream; cross-stream tensor safety is handled by CUDA/HIP events. Candle has no public streams API.
- Burn ships **a public pinned-memory user hook** (`ComputeClient::staging(&mut bytes, false)` in `cubecl-runtime`) plus internal staging pools. Mark bytes as pinned-eligible before `create_tensor()`; Burn does the H2D via pinned memory. No need to drop to `cudarc`. Candle does not have this.
- Burn's CubeCL kernel substrate compiles one kernel definition to CUDA + ROCm + Metal + Vulkan + WebGPU + LLVM-CPU. **Apple Silicon support is via the WGPU backend** (which auto-targets Metal), not via a separate `burn-metal` crate — there is no first-party `burn-metal`. The third-party `burn-mpsgraph` is at `0.0.1` and not usable. Candle requires per-backend kernel reimplementation.
- Burn explicitly positions itself as a tensor library first, DL framework second. Nathaniel Simard (Tracel CEO) presented at Scientific Computing in Rust 2025. Scientific compute is first-class.

**Substrate-property tradeoffs we accept (not workarounds — substrate design choices):**

- **Apple Silicon GPU via WGPU**, not raw MPSGraph. Lower performance than MPSGraph would deliver, acceptable because the rig is Linux+CUDA / Windows+CUDA; Apple Silicon support is for analysis-on-laptop scenarios where "good enough" is fine.
- **GPU concurrency = one stream per thread**, not explicit stream-handle dispatch from a single executor. Adopted as the GPU thread topology: N GPU worker threads, one per concurrent stream. Maps naturally to our existing camera/stimulus thread model.
- **Pinned-memory zero-bounce upload** (pre-allocated pinned buffer → tensor with no re-staging copy) is not yet public — staging via `ComputeClient::staging()` re-stages internally. Performance impact: one extra ~1MB memcpy per cycle. **Upstream PR #1334 (CubeCL) implements this exact API**; we petition for inclusion in 0.22.

**Substrate properties under upstream development (tracked):**

- CubeCL PR #1334 — `create_from_pinned_handle` for zero-bounce H2D. Draft, under active review by Nathaniel.
- Burn issue #4991 — explicit stream creation primitives. Opened by Nathaniel himself.
- See `docs/upstream/burn-petition.md` for our written-up petition.

### 1.2 What we don't use

- **`tch`.** Removed entirely. The libtorch C++ dependency, the delay-loaded `torch_cuda.dll`, the `SetDllDirectoryW` hacks in `lib.rs::run()` — all gone.
- **`tch::nn::*`.** We have no neural network. Burn's `nn` module is also unused for the same reason. Raw tensor ops only.

(`ndarray` is NOT removed — see non-negotiable #1: it's the host representation at the HDF5 boundary, for the CPU segmentation stages, and for the `NdArrayDevice` device value. The *device tensor* pipeline is Burn; ndarray is its host companion.)

### 1.3 Dtype convention

- **`f32` for all on-device analysis compute.** Phase, amplitude, gradients, smoothing kernels, FFT projections, sign maps. Matches Apple Metal's lack of `f64` support, halves memory bandwidth on CUDA, and provides ~7 decimal digits — beyond what retinotopy consumes.
- **`f64` only at named precision-critical points:** σ reductions where the variance of large frame counts can overflow `f32`'s exponent. Documented per-site with a comment naming why.
- **`i32` for integer labels** (connected components, patch IDs). Burn's integer tensor type.
- **`bool` for masks.** Burn's bool tensor type, not `u8`.
- **Complex numbers as paired (real, imag) `f32` tensors.** Burn has no native complex dtype. The pairing is a convention: `(re: Tensor<Backend, 2>, im: Tensor<Backend, 2>)` returned together from any function producing complex output. The `Complex2` type in `compute/complex.rs` wraps the pair with the operations the pipeline actually uses — `from_phase`, `real`, `imag`, `abs`, `angle`, `phase_shift`, `real_imag_sum`, `mul_scalar`. (It is non-generic: `Backend` is a single concrete type, `Dispatch` — see §1.4 — so the complex type doesn't need a `<B>` parameter.) Operations not used by any stage (complex `mul`/`conj`) are deliberately absent and added only if a stage needs them.

### 1.4 Device selection — runtime, via `burn-dispatch` (the PyTorch model)

There is **one backend type**, `burn_dispatch::Dispatch`, and the compute
**device is a runtime value**, not a compile-time type. This is Burn's
multi-backend dispatch layer — the equivalent of PyTorch's
`tensor.to('cuda')`. The entire pipeline is written against
`Tensor<Backend, D>` where `Backend = Dispatch`; the device is a
`DispatchDevice` value (`Cuda(..)`, `NdArray(..)`, …) selected at runtime.

Consequences — and why this is the unified system we wanted:

- **No `<B: Backend>` generic plumbing.** Ops are `Tensor<Backend, D>`,
  not `Tensor<B, D>` with a viral type parameter threaded through every
  function and struct.
- **No per-(device × OS) methods.** OS is handled inside each backend
  (CUDA spans Windows/Linux; the WGPU family handles Metal/Vulkan/DX12
  per-OS internally); device is one runtime enum. There is no matrix.
- **One binary, runtime selection.** A `--features cuda` binary compiles
  in *both* ndarray and CUDA and chooses at runtime — it can run on a
  CPU-only host or a GPU host without a rebuild.

**Cargo features choose what is *compiled in* (selectable at runtime), not
what runs.** `ndarray` (CPU) is always present. `--features cuda` adds the
CUDA backend to the dispatch set. A WGPU/Metal/Vulkan arm slots in the
same way: one feature + one `DispatchDevice` variant, **zero op-code
changes** (WGPU-family backends are mutually exclusive per `burn-dispatch`).

`compute::device()` returns the preferred device for the build: CUDA
when compiled in (the production GPU target), else ndarray CPU. Per the
no-silent-fallback rule the choice is explicit, not a "probe GPU and
swallow errors" path. A future UI/CLI device picker passes a chosen
`DispatchDevice` straight through — the type already supports it.

**Validation:** the `tests/equivalence.rs` pipeline passes identically
through `Dispatch→ndarray` (default build) and `Dispatch→CUDA` (cuda build,
device selected at runtime), at the same ~1e-4 cross-backend f32 drift —
proving the single-backend-type / runtime-device design produces correct
results on every device with no per-device code.

---

## 2. Resource topology

Threads and what runs on each. **As-designed (2026-06-03):** the responsiveness problem is solved by *state decomposition + evicting work from locks* (§3), not by a thread zoo. The GPU-stream / IO-prefetch / PNG-encoder threads below the line are **DEFERRED** — they are pure throughput optimizations of an already-correct, already-off-thread pipeline, to be added only if profiling shows a measured bottleneck. Building them now would be downstream work against an unsettled upstream.

```
        ┌───────────────────────────┐
        │        WebView (JS)       │  ← Tauri events, DOM, input
        └─────────────▲─────────────┘
                      │ Tauri IPC
        ┌─────────────┴─────────────┐
        │   Tauri Runtime (pool)    │  ← #[command] handlers: lock a field,
        │   handlers SYNC + TRIVIAL │     copy out, drop guard, return < 1 ms
        └──┬─────────┬──────────┬───┘
           │         │          │     (crossbeam channels)
       ┌───▼───┐ ┌───▼────┐ ┌───▼──────────┐
       │Camera │ │Stimulus│ │  Analysis    │
       │Thread │ │ Thread │ │  Worker      │  ← runs the DAG synchronously,
       │(PCO)  │ │ (wgpu) │ │  (cancel/    │     stage by stage, on this one
       └───┬───┘ └───┬────┘ │   preempt)   │     thread; emits progress
           │         │      └───┬──────────┘
           └────┬────┴──────────┘
            ┌───▼──────────────┐
            │ Event Forwarder  │  ← cached receivers; locks only the ONE
            │ (field-scoped    │     field it writes (frame_cache / session);
            │  locks, no work) │     no PNG, no HDF5, no whole-state lock
            └───┬──────────────┘
                │ emit() → WebView
─────────────── DEFERRED (perf only, gated on profiling) ───────────────
   N GPU-stream worker threads · IO prefetch/write thread · PNG encoder thread
```

### 2.1 Threads — as-designed (live)

| Thread | Owns | Responsibilities | Forbidden |
|---|---|---|---|
| **Tauri runtime workers** | Tauri's pool | Dispatch `#[command]` handlers: lock one field, copy out, drop, return | Holding a lock across IO/compute/channel-send; whole-state locks |
| **Camera thread** | PCO SDK handle | Frame acquisition, ring-buffer push | Tensor ops, IPC |
| **Stimulus thread** | wgpu surface | Render loop synced to vsync | Anything that misses vsync |
| **Analysis worker thread** | Pipeline state, cancel flag | Walk the DAG **synchronously**, run each stage, fingerprint + persist via the existing HDF5 layer, emit per-stage progress, honor cancel/preempt | (no restriction — it owns the compute) |
| **Event forwarder thread** | Cloned crossbeam receivers (cached at startup) | Drain channels, lock only the field being written, emit Tauri events | Holding a lock across PNG encode / HDF5 / channel send; whole-state lock |

Live long-lived threads: Tauri runtime + camera + stimulus + analysis worker + event forwarder, all fixed at startup. **Never** spawn-per-request or spawn-per-frame.

### 2.2 Deferred thread topology (perf, not fundamental)

The following are *optimizations* of a pipeline that already runs off the UI thread and already produces correct results. They are deferred until a profiler identifies them as the bottleneck — adding them speculatively is gold-plating:

- **N GPU-stream worker threads** — one Burn stream each, for intra-DFT concurrency (the 4 sweep directions). Today the DFT runs sequentially on one device queue and is fast enough; concurrency is a throughput win, not a correctness or responsiveness requirement.
- **IO prefetch/write thread** — overlap HDF5 reads/writes with compute. Today reads/writes are synchronous in the worker; the file is small enough that this isn't felt.
- **PNG encoder thread** — offload figure/preview encoding. Today encoding is moved *out of the lock* (§3) which is the responsiveness fix; a dedicated thread is a further refinement.

When (if) added, each slots in behind the existing channel boundaries without disturbing the `Stage`/DAG contract — which is exactly why deferring them is safe.

---

## 3. State decomposition

The current `Arc<Mutex<AppState>>` god-mutex (39 command lock-sites + the event forwarder locking 3×/10 ms) is replaced by **co-access lock groups**, not one lock per field. The grouping is derived from the measured access map: fields that are always written together in one critical section share a lock; fields accessed in isolation get their own. Over-splitting (one `Mutex` per field) is rejected — it would manufacture the multi-lock deadlock surface we want to eliminate.

```rust
// src-tauri/src/state.rs  — as-designed
use parking_lot::Mutex;   // no poisoning → infallible lock, no LockPoisoned

pub struct AppState {
    // Immutable after startup — no lock. Senders are Clone; hoist to Arc.
    pub threads: ThreadHandles,

    // Co-access groups, each its own parking_lot::Mutex:
    pub capture:     Arc<Mutex<Capture>>,        // latest_frame + timing_ring + acquisition
    pub session:     Arc<Mutex<SessionState>>,   // session + monitors
    pub handoff:     Arc<Mutex<Handoff>>,         // pending_save + last_summary + anatomical
    pub active_oisi: Arc<Mutex<Option<PathBuf>>>, // trivially independent
    pub registry:    Arc<Mutex<Registry>>,        // already inner-locked; drop the outer nesting
}
```

`AppState` is `Arc<AppState>` — never locked as a whole (no mutable outer fields). Why these groups (from the access map):

- **`capture`** — the 60–100 fps `CameraEvt::Frame` path writes `latest_frame`, the timing ring, and the acquisition accumulator in *one* critical section. Grouping them makes the hot path take **exactly one lock**, so its deadlock risk is eliminated structurally rather than by discipline.
- **`session`** — `monitors` is write-once at startup; only `select_display` co-accesses it with `session`. One lock, briefly held.
- **`handoff`** — `pending_save`/`last_summary`/`anatomical` are the low-frequency post-acquisition handoff, never co-accessed with hot fields.
- **`registry`** — already `Arc<Mutex<Registry>>`; after decomposition config commands lock it directly with no outer state lock.
- **`threads`** — `Sender`s are `Clone`; hoisted to `Arc` at setup so most `threads` access needs no lock at all.

### 3.1 Locking discipline

- **`parking_lot::Mutex`, not `std::sync::Mutex`.** No poisoning: `lock()` returns the guard directly. `lock_state` becomes infallible, `AppError::LockPoisoned` and ~25 `Err(_)`/`map_err`/`unwrap_or` poison branches are deleted. Trade-off honored: parking_lot deadlocks instead of poisoning on a panic-in-critical-section, so every critical section must be panic-free — verified safe because the heavy ops (HDF5, PNG, TOML) return `Result` rather than panicking.
- **Lock at most one group at a time.** With co-access grouping, the only remaining multi-group sites are documented and ordered: `CameraEvt::Frame` (single `capture` lock — no longer multi), `StimulusEvt::Complete` (`capture` → `handoff` → `session`), `select_display` (`session`, spawn moved out of lock). Order is fixed and recorded at each site.
- **No work inside locks.** The actual freeze culprits the map found — `capture_anatomical` PNG-encode + `fs::write`, 8× `save_rig()`/`save_experiment()` TOML writes, `select_display` thread-spawn, the per-frame accumulator copy — are each restructured to lock → copy out → drop → do the work. **This, not the lock split, is what stops the UI freezing.**

---

## 4. Pipeline DAG model

The analysis pipeline is a directed acyclic graph of stages. The orchestrator builds the DAG once per `analyze()` invocation, executes it with cancellation support, and emits per-stage progress events.

### 4.1 The `Stage` trait — uniform, over a blackboard

The stages are genuinely n-ary and heterogeneous (stage 8 consumes `Vec<Patch>` + 3 maps; stage 6 emits a mask + a scalar). A generic `Stage<Input, Output>` would force tuple-of-everything or associated-type gymnastics. The honest model — which is *what `compute_analysis` already is* (named intermediates threaded stage to stage) — is a **uniform trait over a shared `PipelineState` blackboard**. Heterogeneity lives in the blackboard's named fields; the trait stays uniform.

```rust
// crates/isi-analysis/src/pipeline/stage.rs
pub trait Stage {
    fn id(&self) -> StageId;
    fn deps(&self) -> &'static [StageId];                          // static edges → petgraph
    fn fingerprint(&self, ctx: &FingerprintCtx) -> Blake3Hash;     // over INPUTS, never outputs
    fn execute(&self, st: &mut PipelineState, ctx: &StageCtx) -> Result<(), AnalysisError>;
    fn restore(&self, st: &mut PipelineState, f: &OisiFile) -> Result<bool, AnalysisError>; // disk cache
    fn persist(&self, st: &PipelineState, f: &mut OisiFile) -> Result<(), AnalysisError>;
}

pub struct StageCtx<'a> {
    pub acq:      &'a AcquisitionProperties, // cross-cutting geometry/calibration inputs
    pub cancel:   &'a CancelToken,
    pub progress: &'a dyn ProgressSink,
}
```

No `Resource` enum and no `GpuExecutor`/`IoExecutor` in the context — those belong to the deferred thread zoo (§2.2). `execute()` **wraps** the existing `methods/*.rs` `apply()` impls; it never re-implements stage logic (SSoT — the method enums in `openisi-params` stay the single source).

**`StageId` (as-designed):** `Dft`, `Retinotopy`, `SignSmoothing`, `CortexSource`, `PatchThreshold`, `PatchExtraction`, `PatchRefinement`, `Labels`, `Eccentricity`, `DerivedMaps`. Retinotopy fuses former stages 1–3 (they exchange device tensors with no host boundary; separating them would force device↔host round-trips for zero benefit). The dead `quality_gate` (former stage 9, never invoked) is **removed** — from `StageId` and from the `openisi-params` registry.

### 4.2 The orchestrator — synchronous, incremental

One synchronous walk on the analysis worker thread (no async pool dispatch — that's deferred):

1. **Open** the `.oisi`; read `AcquisitionProperties` + the persisted `/analysis_state/<id>` fingerprints.
2. **Compute expected fingerprints** for every stage from (its param subset ⊕ acquisition subset ⊕ upstream fingerprints), in topological order.
3. **Find the restore frontier** — the latest *disk-cacheable* stage whose expected fingerprint matches its stored fingerprint and whose output dataset is present. Restore it into `PipelineState` from HDF5.
4. **Recompute everything downstream**, in topological order, calling each stage's `execute()`. Persist the disk-cacheable stages (§4.3) + their fingerprints; emit per-stage progress; check `cancel` at each stage boundary.
5. **On completion**, write `/analysis_params` provenance. Emit `Complete`.

### 4.3 Cost-aware cache boundaries

**Fingerprint every stage** (cheap), but **disk-persist + restore only where recompute-cost ≫ storage-cost**:

| Stage | Fingerprinted | Disk-cached | Why |
|---|---|---|---|
| `Dft` | ✅ | ✅ `/complex_maps` (exists) | Expensive; parameterless on raw frames |
| `Retinotopy` | ✅ | ✅ `/results/*` (+ **new `magnification_raw`**) | Expensive device path |
| `SignSmoothing`…`DerivedMaps` (tail) | ✅ | ❌ recompute from cached retinotopy | Cheap host-ndarray; persisting bulky `Vec<Patch>` to cache a sub-second stage is a pessimization |

This delivers the requested behavior — *tweak a segmentation param → DFT + retinotopy restored from disk, only the cheap tail reruns* — without bloating the file. **One persistence gap to close:** stage 8 needs the *unmasked* `magnification_raw`, which today is not in `/results`; the `Retinotopy` cache must add it.

### 4.4 Fingerprinting — engine and hazards

**Engine: hand-rolled on `petgraph` + `blake3` + the existing HDF5 layer** (research-justified, not cargo-culted). The crate survey is decisive: every incremental crate is either in-memory (`salsa`, `comemo`, `incremental-rs`) or caches into its own directory tree (`cacache`) — none caches *input-fingerprinted artifacts resident inside a foreign HDF5 container*, which is our hard constraint. Dirty-propagation over a fixed 10-node DAG is a ~30-LOC topological walk; `petgraph` supplies the graph + cycle check, `blake3` (already a dependency) the content hash. Fingerprint over **inputs, never outputs** — hashing f32 outputs would thrash on cross-backend drift.

**Fingerprint hazards** (inputs that affect a stage's output but aren't its declared params — must be folded into the hash or the cache is silently wrong):

- **`AcquisitionProperties`** (`rotation_k`, `azi/alt_angular_range`, `offset_azi/alt`, `um_per_pixel`) — cross-cutting geometry/calibration, **not in `AnalysisParams`**. Enter the fingerprint of `Retinotopy` (rotation, degree scaling, magnification) and `SignSmoothing` (`sigma_um`→px).
- **`Dft` has no params** → its fingerprint is purely raw-data identity (`created_at` + `animal_id` root attrs).
- **`CortexSource` file inputs** — reliability maps (raw-derived) and `/anatomical/cortex_roi` (user dataset) are neither params nor upstream-stage outputs; their identity enters the stage fingerprint.
- **Hardcoded constants** behaving like params — `4.0°` contour interval, `MAX_PATCHES=100` guards — are currently not registry params; flagged so that if they ever become params they must enter the relevant fingerprint.

### 4.5 Cancellation

`CancelToken` (`Arc<AtomicBool>`, already in `analysis_thread`) is checked at every stage boundary. On a param change mid-run the worker sets cancel, lets the current stage unwind, and restarts the walk with new params — already-cached stages (DFT, possibly retinotopy) restore instantly, so the restart resumes at the first dirtied stage rather than recomputing from scratch. This is the "kill and restart at the proper stage" behavior.

---

## 5. Per-stage resource assignment

The audit (`docs/PIPELINE_AUDIT.md`, to be written) identified per-stage placement gaps. The unified assignment:

| ID | Stage | Resource | Algorithm | Reference |
|---|---|---|---|---|
| 0 | DFT / phase extraction | **GPU**, batched GEMM over K cycles, 4 streams for 4 directions, pinned-host prefetch | `[K, T] @ [T, H·W]` batched matmul per direction; bin 1 only | Kalatsky & Stryker 2003 |
| 1 | CycleCombine | **GPU**, 2 streams (azi/alt) | Delay subtraction on complex phasors, native ops | Kalatsky & Stryker 2003, Eq. 3 |
| 2 | PhaseSmoothing | **GPU**, 2 streams | Separable Gaussian conv on amp-weighted real + imag | Marshel 2011 §Methods |
| 3 | VfsComputation | **GPU**, 2 streams | Chain-rule φ-gradient, `sin(θ_alt − θ_azi)` | Sereno 1995, Garrett 2014 |
| 4 | SignMapSmoothing | **GPU** (single stream) | Same separable Gaussian as stage 2 — calls the same kernel | Garrett 2014 |
| 5 | CortexSource | **CPU sequential** | Variant-dependent. Reliability: per-pixel min + flood fill + largest CC. SNLC: σ reduction + threshold + van Herk morphology + flood fill + largest CC. | Garrett 2014 (SNLC); Allen/Zhuang 2017 (reliability) |
| 6 | PatchThreshold | **GPU**, fuses with stage 4 output | σ reduction (or fixed scalar), elementwise compare, AND with cortex mask | Garrett 2014; Allen/Zhuang 2017 |
| 7 | PatchExtraction | **CPU**, BFS labeling sequential + rayon over patches for per-patch closing | 4-connected union-find; per-patch cross-SE close in parallel; Allen `dilation_patches2`; O(N²) adjacency via `rayon::par_iter` over pairs | Allen/Zhuang 2017 |
| 8 | PatchRefinement | **CPU**, rayon over patches | Per-patch watershed (sequential by definition); integral-image `uniform_filter`; parallel merge-candidate scan | Allen/Zhuang 2017 |
| 9 | QualityGate | trivial | No-op currently | (stub) |
| 10 | Eccentricity | **GPU** | Pointwise `atan(sqrt(tan² + tan²/cos²))` — transcendental intrinsics | Garrett 2014 V1-centric eccentricity |
| 11 | Write results | **IO**, async to next analysis | One HDF5 transaction per dataset group, writes don't block subsequent analysis | — |

### 5.1 Per-stage algorithm fixes

- **van Herk / Gil-Werman 1992** for disk morphology in stage 5. O(H·W) regardless of structuring element radius. Replaces the current naïve O(H·W·R²) neighborhood scan.
- **Integral image (Viola/Jones 2001)** for `uniform_filter_finite` in stage 8. O(H·W) regardless of filter size. Replaces the current O(H·W·size²) box scan.
- **Batched GEMM** for DFT. The current implementation does K sequential matmuls of `[1, T] @ [T, H·W]`. The unified implementation does one matmul of `[K, T] @ [T, H·W]` per direction. K-fold reduction in launch overhead.
- **Rayon over patches** for stages 7 and 8 per-patch work. Patches are independent; serializing them was waste.

### 5.2 Device-residency rules

```
[GPU] DFT → CycleCombine → PhaseSmoothing → VfsComp → SignMapSmoothing → PatchThreshold
                                                                              │
                                                                  download to host
                                                                              ▼
[CPU] CortexSource → PatchExtraction → PatchRefinement → QualityGate
                                                              │
                                                       upload to GPU
                                                              ▼
[GPU] Eccentricity
                                                              │
                                                      download to host
                                                              ▼
[IO]  Write results to HDF5
```

There is exactly **one GPU→CPU boundary** (after PatchThreshold) and **one CPU→GPU boundary** (before Eccentricity) per analysis run. Today there are several spurious boundaries; the unified design eliminates them.

---

## 6. Tensor & memory discipline

### 6.1 Raw frame ingestion

Raw camera frames live in HDF5 as `[T_total, H, W] u16`. They are loaded by the IO thread into a `Bytes` buffer (Burn's owned-bytes type, not a custom pinned-buffer type), marked as pinned-eligible via `ComputeClient::staging(&mut bytes, false)`, then handed to `compute_client.create_tensor()` which performs the pinned H2D. The current pattern (read entire `Array3<u16>` synchronously on the analysis thread, hold ~2.5 GB on host) is replaced by:

```rust
// IoExecutor::stream_raw_frames(path, direction, cycle) -> Receiver<FrameBatch>
//   - reads cycle-sized batches sequentially
//   - returns Bytes pre-marked via ComputeClient::staging() for pinned H2D
//   - returns a bounded-channel receiver
//   - prefetch depth: 2 (next cycle in flight while current cycle is on GPU)
```

The full cube is never held in host RAM; only the active prefetch window (~2 cycles × T_chunk × H × W × 2 bytes ≈ tens of MB).

**Note on H2D overlap:** Burn's `ComputeClient::staging()` provides pinned-memory bandwidth (~2–4× over non-pinned) but currently re-stages internally on the upload path (one extra ~1MB memcpy per cycle). Zero-bounce upload (pre-allocated pinned buffer → tensor with no re-staging) is the subject of upstream CubeCL PR #1334; we petition for inclusion. Performance impact of the current path is bounded and acceptable; revisit only if profiling shows H2D is the bottleneck.

### 6.2 Shared immutable data

Wherever raw frame data is shared between threads (e.g., the prefetch buffer the IO thread fills and the GPU thread consumes), it is held as `Arc<[u16]>` (or `Arc<Bytes>` after staging). Clones are refcount bumps. No `Vec::clone` of pixel data anywhere.

### 6.3 Burn tensor lifetime

Burn tensors are reference-counted (`burn_tensor::Tensor` is `Clone` and shares backing storage). Lifetimes within a stage are scope-bounded; tensors crossing stage boundaries are passed by value (the orchestrator owns them between stages). No `Arc<Mutex<Tensor>>` patterns — tensors are values, not shared mutable state.

---

## 7. IO discipline

### 7.1 The IoExecutor

A single thread owning HDF5 file handles. Receives `IoTask` over a channel:

```rust
pub enum IoTask {
    ReadComplexMaps    { path: PathBuf, reply: Sender<Result<ComplexMaps>> },
    ReadSnrMaps        { path: PathBuf, reply: Sender<Result<Option<SnrMaps>>> },
    ReadRawFrameBatch  { path: PathBuf, direction: Direction, cycle: u32, reply: Sender<Result<PinnedFrameBatch>> },
    WriteComplexMaps   { path: PathBuf, maps: ComplexMaps, reply: Sender<Result<()>> },
    WriteResultDataset { path: PathBuf, name: String, data: TypedArray, reply: Sender<Result<()>> },
    WriteAnalysisParams{ path: PathBuf, tree: serde_json::Value, reply: Sender<Result<()>> },
    InspectOisi        { path: PathBuf, reply: Sender<Result<OisiCapabilities>> },
}
```

Every HDF5 access in the program goes through this. No other thread opens HDF5 files. This eliminates the question "is this file open from two threads at once?" by construction — the answer is always no.

### 7.2 Prefetch discipline

The DFT stage subscribes to `stream_raw_frames(path, direction)` which returns a `Receiver<PinnedFrameBatch>` of bounded capacity 2. Cycle N+1's batch is read while cycle N is on GPU. The producer (IO thread) blocks when the consumer (GPU stage) is lagging — natural backpressure, no manual scheduling.

### 7.3 Write discipline

Result writes are dispatched as `WriteResultDataset` tasks and do not block the orchestrator from starting subsequent work. The final `analysis:complete` event fires after all write tasks have replied with success.

---

## 8. Concurrency primitives

| Need | Primitive | Forbidden alternative |
|---|---|---|
| Thread-to-thread message passing | `crossbeam_channel::{bounded, unbounded}` | `std::sync::mpsc`, `tokio::mpsc` |
| Wait on multiple channels | `crossbeam_channel::select!` | poll-with-sleep loops |
| Shared mutable state | `Arc<Mutex<T>>` (purpose-scoped, small critical section) | `Arc<RwLock<T>>` (unless reads dominate by 10:1), god-mutex |
| Reference-counted shared immutable | `Arc<T>` | `Arc<Mutex<T>>` for immutable data |
| Atomic counters / flags | `std::sync::atomic::{AtomicBool, AtomicUsize}` | `Mutex<bool>` |
| Cancellation token | `Arc<AtomicBool>` wrapped in `CancelToken` newtype | passing `&mut bool` |
| Parallel CPU work | `rayon::par_iter`, `rayon::scope` | manual thread spawning |
| GPU concurrency | Burn CubeCL streams via N GPU worker threads (one stream per thread) | manual CUDA via `cudarc` |
| One-shot async result | `crossbeam_channel::bounded(1)` reply channel | `oneshot`, `futures` |
| Pinned-memory H2D | `ComputeClient::staging(&mut bytes, false)` before `create_tensor()` | rolling your own `cudaMallocHost` via `cudarc` |
| Mutex / RwLock | `parking_lot::Mutex` | `std::sync::Mutex` (slower; poisoning we don't want) |
| DAG representation + toposort | `petgraph::Graph` + `petgraph::algo::toposort` + ~50 LOC ready-set scheduler on top | hand-rolled graph types; `dagrs` (archived 2026-01) |
| Property testing | `proptest` | hand-rolled fuzz, `quickcheck` |
| Numerical tolerance assertions | `approx::assert_relative_eq!` / `assert_abs_diff_eq!` (on slices via `&v[..]`) | hand-rolled epsilon comparisons |
| Builder pattern (when needed) | `bon` derive | `derive_builder` (runtime-checked), `typed-builder` |

No `tokio`. No `async fn`. The application is synchronous + thread-based. Tauri uses async internally; we present sync handlers to it via the sync command form. This decision is deliberate: scientific compute is dominated by long-running CPU/GPU work, not by waiting on millions of concurrent IO operations. The async model adds complexity without buying us anything.

---

## 9. Error model

```rust
// crates/isi-analysis/src/error.rs
#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    #[error("missing data: {0}")]
    MissingData(String),

    #[error("HDF5: {0}")]
    Hdf5(#[from] hdf5_metno::Error),

    #[error("compute: {0}")]
    Compute(String),

    #[error("cancelled")]
    Cancelled,

    #[error("validation: {0}")]
    Validation(String),

    #[error("pre-2026 file requires migration")]
    Pre2026Migration,

    #[error("device error: {0}")]
    Device(String),
}
```

`Result<T, AnalysisError>` is the only return type for fallible operations. No `Result<T, String>`. No `Box<dyn Error>`. No `anyhow` (it papers over type information). The Tauri command layer converts `AnalysisError` to a serializable `AppError` for the UI.

### 9.1 Cancellation is a first-class error

`AnalysisError::Cancelled` is not "failure" — the orchestrator distinguishes it explicitly and emits `analysis:cancelled` rather than `analysis:failed`. Stages return it as soon as they observe `cancel.is_set()`.

---

## 10. Numerical & verification

We do **not** depend on any external numerical oracle. No Python reference, no synthetic input generators, no bit-matching against another implementation in another language. The verification strategy uses only what we control: real scientific data we already trust, mathematical invariants derived from the algorithms themselves, and our own pre-refactor code as the cross-implementation baseline.

### 10.1 The verification corpus: real SNLC `.oisi` files

The repo (or a referenced data path for large files) contains a committed set of real `.oisi` acquisition files from SNLC. These are the test corpus — actual mouse retinotopy acquisitions with raw camera frames, the data we actually care about producing correct results on.

- `crates/isi-analysis/tests/fixtures/oisi/` — the small `.oisi` files committed directly (or a `fixtures.toml` pointing to a known data path on the rig for larger files).
- `crates/isi-analysis/tests/fixtures/baseline/` — per-stage intermediate outputs captured from the **current pre-refactor code** on each fixture: complex maps, smoothed phasors, VFS, sign maps, cortex masks, patches, eccentricity. Saved as HDF5 datasets parallel to the input file. **This snapshot is the cross-implementation equivalence target during the refactor.**

### 10.2 Cross-implementation equivalence — primary safety net

For each stage, a test asserts the new implementation's output matches the baseline on the same input within committed tolerance. Catches "the substrate swap broke this stage" or "the algorithm fix introduced a bug" immediately, on real scientific data, with no external dependency.

Written inline with each phase: when Phase A swaps stage 1 from tch to Burn, the test "Burn stage 1 on fixture X agrees with baseline stage 1 on fixture X within tolerance" lands in the same commit. The old code path is deleted only after agreement is demonstrated.

### 10.3 Property tests — mathematical invariants

Each stage has at least one property test. These don't depend on any input being synthetic — they hold on any input, including the real SNLC files:

- **PhaseSmoothing** with σ=0 must be identity within 1 ULP.
- **VfsComputation** of a phase field rotated 90° must equal the original VFS multiplied by −1.
- **Eccentricity** at the V1 foveal centre must be 0 ± 1e-6.
- **CycleCombine** of (φ, −φ) must produce (0, 0) phase output (delay subtraction cancels).
- **Gaussian smoothing** preserves total mass (Σ_after = Σ_before within FP precision).
- **Integral image + box query** equals direct neighborhood sum within FP precision.
- **van Herk morphology** equals naive disk morphology bit-exact on binary masks (no floating point involved).
- **Connected components** of an all-ones mask is 1 component with H·W pixels.

Implemented via `proptest` where parameterization helps, plain unit tests otherwise.

### 10.4 Cross-backend equivalence

Same test corpus run on CUDA, WGPU, and LLVM-CPU backends. Outputs must agree within per-stage tolerance. Catches substrate leaks (an op silently falling back to CPU on one backend, a kernel producing different ordering on another). Doesn't prove the algorithm is correct (the property tests do that) — proves the substrate isn't producing per-backend divergence.

### 10.5 Per-stage tolerance budgets

Tolerances live in `crates/isi-analysis/tests/fixtures/tolerances.toml`, one entry per stage:

```toml
[dft]
max_abs_err = 1e-5
rms_err     = 1e-6

[phase_smoothing]
max_abs_err = 1e-4
rms_err     = 1e-5
# ... etc per stage
```

Tolerances are **measured from cross-implementation drift on real data**, not theoretical worst-cases. The procedure: run the refactored stage against the baseline on every fixture, take the maximum observed drift, set the tolerance ≥ that value, commit. Any future change exceeding it is investigated, not absorbed by widening the bound.

### 10.6 Code review against canonical papers

Every algorithm function carries a citation comment: `// Kalatsky & Stryker 2003, Eq. 3`, `// Garrett 2014 §2.3`, `// van Herk 1992`. The reviewer's job at PR time is to check the code matches the equation. Manual but rigorous; this is how scientific software is normally validated when no machine-checkable oracle exists.

### 10.7 Visual inspection in the UI

Load a known SNLC `.oisi` in the app, look at the resulting maps. Compare by human eye against published retinotopic map figures (Garrett 2014 Fig. 3, Zhuang 2017 Fig. 2, etc.). Catches gross errors that pass numerical tests but produce nonsense maps (e.g., signs inverted, axes swapped).

### 10.8 Deliberate algorithm changes

When we deliberately change an algorithm (not just port it), the baseline must be regenerated with a documented diff. The diff names: the prior algorithm, the new algorithm, the scientific justification, the expected output difference, the tolerance update. This is a feature, not a regression — the verification keeps us honest about scientific changes vs accidental drift.

---

## 11. Observability

### 11.1 Logging

Replace all `eprintln!` calls with the `tracing` crate. Structured spans per stage:

```rust
let _span = tracing::info_span!("analyze.dft", direction = ?dir, cycle = k).entered();
```

Per-process configuration via `RUST_LOG`. Default in dev: `info`. Default in release: `warn`.

### 11.2 Per-stage timing

The orchestrator emits a structured trace event per stage completion:

```rust
tracing::info!(
    target: "analyze.timing",
    stage = ?stage_id,
    elapsed_ms = elapsed.as_millis(),
    resource = ?resource,
);
```

A `tracing-subscriber` formatter writes these to `~/.openisi/timing.jsonl` in dev mode. The benchmark tooling (`tools/bench/`) reads these to compare runs.

### 11.3 GPU utilization

Not directly exposed by Burn today. We rely on `nvidia-smi dmon -i 0 -s u` running externally during benchmark runs; if Burn ships GPU-utilization hooks, we adopt them.

---

## 12. Configuration

Every parameter that affects the science comes from the `openisi-params` Registry. The unified architecture changes nothing about the configuration SSoT — it already follows the right model. But:

- **No constants in algorithm code.** A value like "minimum patch size = 100 pixels" is a Registry param, not a `const PATCH_MIN: usize = 100`.
- **Tunables remain typed and validated** via `StaticConstraint` per the existing system.
- **Method-choice enums remain the source of truth** for algorithm dispatch (the `CycleCombineKind` etc. enums); algorithm modules dispatch on these.

---

## 13. Execution sequence

The refactor lands in one PR but is built up in this order so the working tree stays compilable throughout. **Verification tests are written inline with each phase, not deferred to a later phase.** Each phase's acceptance is "the corresponding tests pass on every backend."

### Phase 0 — Baseline capture

**Before any production code is touched.** Tests are the only thing built in this phase.

0a. Commit the SNLC fixture set to `crates/isi-analysis/tests/fixtures/oisi/` (or `fixtures.toml` pointing at a known data path).
0b. Write a `cargo xtask capture-baseline` (or `just capture-baseline`) that runs the **current** pipeline on every fixture and saves per-stage intermediates as HDF5 datasets to `tests/fixtures/baseline/<fixture-name>/<stage>.h5`.
0c. Run it once; commit the baseline snapshots (small) or document the data path (large).
0d. Write the property tests (§10.3) against the **current** code. They must all pass before any refactor work begins. This proves the property tests are correctly specified, not just that they'll pass against a broken implementation later.
0e. Write the cross-backend equivalence harness (§10.4) — empty stub asserting both backends produce baseline outputs. Will be activated per-stage during the refactor.

Phase 0 is complete when: baseline captured, property tests green on current code, cross-backend harness scaffolded.

### Phase A — Substrate (Burn in, tch out) — ✅ DONE

*As built, this differed from the original plan below: the backend is
`burn-dispatch` (runtime device selection), not compile-time `burn-cuda`/
`burn-wgpu` aliases; reflection padding is a `slice`+`cat`+`flip`
composition (Burn `conv2d` has zero-pad only); and `ndarray` is retained
(host boundary + CPU stages). The DFT lives in `compute/ops.rs`, complex in
`compute/complex.rs` — there is no `compute/dft.rs`. The original step list
is kept for history:*

1. Add the granular Burn deps + `petgraph`, `parking_lot`, `tracing`, `tracing-subscriber`, `blake3`, `imageproc`; test-only `proptest`, `approx`, `toml`.
2. Implement `compute::device()` for Burn. Reimplement the ops as Burn ops in `compute/{ops,complex,conversions,accumulator}.rs`. Reflection padding via `slice`+`flip`+`cat`.
3. **For each compute primitive moved**, a cross-implementation equivalence test (Burn op vs the committed baseline) lands in the same commit; the old path is deleted only after its replacement test is green.
4. Remove `tch` from `Cargo.toml` and the libtorch DLL hacks / vendored-libtorch build infra.
5. Adopt `burn-dispatch` for one-binary runtime device selection (CPU + CUDA validated).

### Phase B — State and threading

Pure refactor — no numerical changes. Verification is functional smoke tests (app starts, camera connects, params apply, file loads, analysis runs).

6. Decompose `AppState` into the per-field `Arc<parking_lot::Mutex<…>>` model.
7. Rewrite `events.rs::run_event_forwarder` to cache receivers locally and use `crossbeam_channel::select!`.
8. Split `PngEncoder` to its own thread. Spawn PNG tasks from camera event handling.
9. Introduce `IoExecutor` thread + `IoTask` enum. Migrate every HDF5 call site to use it.
10. Spawn N GPU worker threads (one per concurrent Burn stream); each receives `GpuTask` over a crossbeam channel.

### Phase C — Pipeline DAG

Stage outputs must match baseline within tolerance on every fixture (this is the cross-implementation equivalence test from Phase 0, now active per-stage as each stage is rewritten).

11. Define `Stage` trait, `Resource` enum, `StageCtx`. Build the orchestrator on top of `petgraph::Graph` + `petgraph::algo::toposort` in `pipeline/orchestrator.rs`. Add ~50 LOC in-degree-counter ready-set scheduler.
12. Rewrite each of the 11 stages as a `Stage` impl. Per the resource table above. **For each stage**: write the equivalence test against the baseline. Green test → land. Red test → fix or revert.
13. Implement per-stage fingerprinting (`pipeline/fingerprint.rs`) using `blake3::hash` over canonical-JSON of params + upstream fingerprints.
14. Wire HDF5 `/analysis_state/<stage>` persistence via the IO thread.

### Phase D — Algorithm fixes

Each algorithmic replacement asserts bit-identical output (binary morphology) or within-tolerance output (floating-point) against the baseline. Updates to the baseline are forbidden in this phase — these are algorithmic improvements that must produce equivalent results, not deliberate scientific changes.

15. Implement van Herk / Gil-Werman morphology in `segmentation/morphology.rs` (~150 LOC, canonical 1992 algorithm). Replace the disk-SE naïve loop. Use `imageproc::region_labelling::connected_components` for CC labeling (don't hand-roll). Use `imageproc::morphology` only for u8 binary masks; van Herk for f32 fields and large SEs. **Test**: van Herk output bit-matches naive output on every fixture for binary masks; matches within tolerance for f32.
16. Implement integral-image `uniform_filter` in `methods/patch_refinement.rs` via `imageproc::integral_image::sum_image_pixels` (the established primitive). Replace naïve box scan. **Test**: integral-image output matches naive output within FP tolerance on every fixture.
17. Implement batched-GEMM DFT in `compute/dft.rs`. Replace the per-cycle matmul loop. Use `ComputeClient::staging()` for pinned-H2D upload of cycle batches. **Test**: batched DFT output matches per-cycle DFT output within tolerance on every fixture.
18. Add `rayon::par_iter` over patches in stages 7 and 8. **Test**: parallel output bit-matches sequential output (rayon is deterministic over our reductions).

### Phase E — Verification consolidation

Tests already exist from Phases 0–D. This phase is finalization: run them on every backend, freeze tolerances, and decommission transition scaffolding.

19. Run the full test suite on every backend (CUDA / WGPU / LLVM-CPU). Cross-backend equivalence (§10.4) is the gate.
20. Freeze tolerances in `tests/fixtures/tolerances.toml` based on measured drift. Tighten any that are looser than necessary.
21. Decommission any "old vs new" cross-implementation tests that were transition-only. Property tests + cross-backend equivalence + baseline equivalence stay as the ongoing safety net.

### Phase F — Observability

22. Replace `eprintln!` with `tracing` throughout. Add per-stage spans.
23. Add timing-event subscriber writing to `~/.openisi/timing.jsonl`.

### Phase G — Cleanup

24. ✅ Delete `docs/ANALYSIS_COMPUTE.md` (superseded; its live dev-figures content moved to `docs/DEV_FIGURES.md`).
25. Update `docs/ARCHITECTURE.md` to reference this doc for compute/threading concerns.
26. Update `docs/INCREMENTAL_ANALYSIS_DESIGN.md` to reflect that fingerprinting is now in the DAG model (or fold it into this doc).
27. ✅ The `bridge.rs` code no longer marshals tensors (it maps the registry snapshot → method params); there is no `Array2<f32>` ↔ tch marshaling left to delete.

---

## 14. What this replaces / deletes

- **Files deleted in full:** `crates/isi-analysis/src/{math.rs}` (functions move to `compute/cpu_ops.rs` as Burn-tensor versions), `src-tauri/src/analysis_thread.rs` (replaced by the orchestrator).
- **Major rewrites:** `crates/isi-analysis/src/{lib.rs::analyze, io.rs, compute/*, methods/*, segmentation/*, bridge.rs}`. `src-tauri/src/{state.rs, events.rs}`.
- **Dependencies removed:** `tch` (+ `torch-sys`, and all vendored libtorch build infra).
- **Dependencies added (actual, as built):**
  - Substrate: `burn-tensor`, `burn-dispatch` (the runtime multi-backend), `burn-ndarray` (always), `burn-cuda` (behind `--features cuda`). NOT the `burn` umbrella, NOT `burn-cubecl`/`burn-wgpu` directly.
  - DAG (present, used when the orchestrator is built): `petgraph`
  - Locks: `parking_lot`
  - Hashing: `blake3`
  - Observability: `tracing`, `tracing-subscriber`
  - Segmentation primitives: `imageproc` (CC labeling + integral images only — NOT for morphology)
  - Test-only: `proptest`, `approx`, `toml`
- **Dependencies retained:** `ndarray` (host boundary + CPU segmentation — see #1), `num-complex`, `hdf5-metno`, `rayon`.
- **Dependencies rejected (verified):** `dagrs` (archived 2026-01-16); `cust` / `rustacuda` (stale / experimental); `candle` (no public streams API, no public pinned-memory hook).
- **Tauri shell changes:** all `state.lock()` call sites updated to lock the specific sub-field; `lib.rs::run()` DLL-search-path hacks removed; `analysis_thread.rs` deleted; new orchestrator/gpu/io thread spawns added to `lib.rs::run()`.

---

## 15. Acceptance criteria

The refactor is complete when:

1. **One test suite passes on every deployment target.** `cargo test --workspace` runs the *same* tests on Linux+CUDA, Windows+CUDA, macOS+Metal, WGPU on each platform, and LLVM-CPU as the no-GPU control. The test code is single-source and platform-agnostic — only the runner differs. A pass on each target confirms the substrate's device/OS abstraction holds.
2. **Cross-implementation equivalence on real SNLC data.** Every stage's output matches the Phase 0 baseline on every fixture within tolerance. Cross-backend equivalence holds within the same tolerance. Property tests (§10.3) green on every backend. Tolerances committed in `tests/fixtures/tolerances.toml`, measured from real drift not theoretical worst-case.
3. `tch` appears in `cargo tree -p isi-analysis` zero times (✅ — done). (`ndarray` remains — see non-negotiable #1.)
4. `grep -r "state.lock()" src-tauri/src/` returns zero matches (replaced by sub-field locks).
5. `grep -r "eprintln\!" crates/ src-tauri/` returns zero matches in non-test code (replaced by `tracing`).
5a. `grep -rE 'cfg\(target_os|cfg\(target_arch' crates/isi-analysis/ src-tauri/src/` returns zero matches outside the explicitly-named camera-driver factory (`src-tauri/src/camera/factory.rs`). Platform/device differences live in dependencies, not in our code.
6. A full re-analysis after a `phase_smoothing` param change runs in `< T_baseline / 3` (where `T_baseline` is the current end-to-end time) — the combined effect of cached complex maps, GPU-resident retinotopy stages, and DFT pipelining.
7. UI remains responsive (every frame paints within 16 ms, no IPC call awaits longer than 50 ms) during a full re-analysis. Verified by Playwright frame-time measurement in `tools/ui-bench/`.

When all seven hold, the refactor lands.

---

## 16. What this document is not

- Not a sales pitch for Burn or against tch. The choice is made and documented in the separate decision record.
- Not an implementation. The Rust code that realizes this lives in the repo under the modules named above.
- Not infinitely flexible. If a future requirement contradicts a non-negotiable in §0, the requirement is rejected unless the non-negotiable is renegotiated explicitly in this document first.
- Not uniformly built. Per the **Implementation status** block at the top: §1 and §10 are built and validated; §2–§9, §11, and Phases B–G are designed-here-but-not-yet-implemented roadmap. A 🚧 section describing something the code doesn't do yet is the goal, not a claim about the current tree.

For the ✅ (built) sections, this is the spec and code that doesn't match is a bug. For the 🚧 (roadmap) sections, this is the design we intend to build next.
