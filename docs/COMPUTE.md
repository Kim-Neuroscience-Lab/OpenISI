# Compute & Concurrency

The compute substrate, the threading and state-ownership model, and the verification
strategy for OpenISI's analysis path. Out of scope (owned elsewhere): the incremental
cache ([`INCREMENTAL_ANALYSIS_DESIGN.md`](INCREMENTAL_ANALYSIS_DESIGN.md)), stimulus
rendering (`crates/openisi-stimulus`, governed by display-timing constraints),
configuration (the `openisi-params` typed-config SSoT), and validation *status*
([`VALIDATION_SCORECARD.md`](VALIDATION_SCORECARD.md) — this doc states the strategy,
the scorecard states what is currently validated). For *why* these choices uphold the
field-standard contract, see [`PRINCIPLES.md`](PRINCIPLES.md).

Every concern below has **exactly one** implementation. There are no compatibility
shims, dual-backend transitions, or parallel paths for the same concept.

## Non-negotiables

These are commitments. Code that violates one is a bug in the code, not a tradeoff.

1. **One tensor type for on-device compute** — `burn_tensor::Tensor<Backend, D>`. No
   `tch::Tensor`, no `Vec<f32>` masquerading as a matrix. `ndarray` is retained
   *deliberately* at the HDF5 boundary (the `hdf5-metno` API hands back `ndarray`), as
   the host representation for the segmentation stages (inherently CPU), and to name
   the `NdArrayDevice` value. The device-tensor pipeline is Burn; ndarray is its host
   companion, not a parallel device tensor.
2. **One compute substrate** — Burn via its runtime-dispatch backend. One backend
   type; the device is a runtime value. ndarray (CPU) is always compiled in;
   `--features cuda` adds the CUDA backend.
3. **No god-mutex.** State is decomposed by co-access; `state.lock()` as a
   whole-state pattern does not exist.
4. **No polling loops.** Inter-thread coordination is `crossbeam_channel::select!` /
   `recv()`, never `try_recv()` + sleep.
5. **No work inside locks.** Lock, snapshot, drop, work. HDF5 I/O, tensor ops, PNG
   encoding, and serialization never run with a lock held.
6. **No silent fallbacks.** A missing CUDA device, an unavailable monitor, or a
   malformed config produces a typed error and surfaces. Device selection is explicit
   at startup; a mismatch is fatal, never a silent fall back to CPU.
7. **No `String`-typed errors across module boundaries.** `thiserror` enums
   everywhere; `String` is only the display form for the UI.
8. **No magic numbers in algorithm code.** Every constant that affects the science is
   a typed `AnalysisConfig` parameter. Constants in code are mathematical (π, e) —
   never tuning knobs.
9. **Every algorithm choice cites its canonical reference** — a
   `// Kalatsky & Stryker 2003, Eq. 3` comment wherever the math comes from the
   literature, naming the specific variant (those names live in the method enums).
10. **Verification is automated.** Per-stage equivalence against committed real-data
    baselines plus property tests run on every commit. Untested code is incomplete.

## Substrate

### Tensor library — Burn `=0.21`, granular crates

We depend on `burn-tensor` + `burn-dispatch` (+ `burn-ndarray` always, `burn-cuda`
behind `--features cuda`), **not** the `burn` umbrella (whose default features pull
`burn-tch`/libtorch — a native C++ dependency we do not take). Burn is chosen over
Candle because:

- It ships multi-stream concurrency in CubeCL and a public pinned-memory hook
  (`ComputeClient::staging`) for fast host→device transfer; Candle exposes neither.
- Its CubeCL kernel substrate compiles one kernel definition to CUDA, ROCm, Metal
  (via the WGPU backend — there is no first-party `burn-metal`), Vulkan, WebGPU, and
  LLVM-CPU. Candle requires per-backend kernel reimplementation.
- It positions itself as a tensor library first, DL framework second; scientific
  compute is first-class.

We do **not** use `tch`/libtorch, and we have no neural network, so Burn's `nn`
module is unused — raw tensor ops only. Open upstream items we track (pinned-memory
zero-bounce upload, explicit stream primitives) are written up in
[`upstream/`](upstream/); none blocks the current design.

### Device selection — runtime, one backend type

There is one backend type, `burn_dispatch::Dispatch`, and the device is a **runtime
value** (`DispatchDevice`), the equivalent of PyTorch's `tensor.to('cuda')`. The whole
pipeline is written against `Tensor<Backend, D>` where `Backend = Dispatch`:

- **No `<B: Backend>` generic plumbing** threaded through every function and struct.
- **No per-(device × OS) methods** — OS lives inside each backend (CUDA spans
  Windows/Linux; the WGPU family handles per-OS graphics internally); device is one
  runtime enum.
- **One binary, runtime selection.** A `--features cuda` build compiles in *both*
  ndarray and CUDA and chooses at runtime; it runs on a CPU-only or a GPU host with no
  rebuild. `compute::device()` returns the preferred device for the build (CUDA when
  compiled in, else ndarray CPU) — explicit, never a "probe-GPU-and-swallow-errors"
  path. A device picker passes a chosen `DispatchDevice` straight through.

The `tests/equivalence.rs` pipeline passes identically through `Dispatch→ndarray` and
`Dispatch→CUDA` at the same ~1e-4 cross-backend f32 drift, proving the
single-backend-type / runtime-device design is correct on every device with no
per-device code.

### Dtype conventions

- **`f32` for all on-device compute** — phase, amplitude, gradients, kernels, FFT
  projections, sign maps. Matches Metal's lack of `f64`, halves CUDA bandwidth, and
  gives ~7 digits (beyond what retinotopy consumes).
- **`f64` only at named precision-critical points** — σ reductions where large
  frame counts can overflow `f32`'s exponent. Documented per-site.
- **`i32` for integer labels** (connected components, patch IDs); **`bool` for masks**
  (Burn's bool tensor, not `u8`).
- **Complex as paired `(re, im)` `f32` tensors** — Burn has no complex dtype. The
  `Complex2` type in `compute/complex.rs` wraps the pair with exactly the operations
  the pipeline uses (`from_phase`, `abs`, `angle`, `phase_shift`, …); unused ops are
  absent until a stage needs them.

### One codebase, device-native at the substrate

OpenISI is **one** Rust program, not a per-OS fork. Platform and device differences
live exclusively at the substrate: `burn` abstracts the compute device, `wgpu` the
graphics API, `tauri` the WebView, `hdf5-metno` file I/O, `std` the OS. No business
logic in `crates/isi-analysis` or `src-tauri` carries a `#[cfg(target_os)]` branch or
asks "am I on CUDA?" to pick an algorithm. **The one named exception** is the camera
driver, because vendor SDKs are inherently platform-specific: `trait Camera` is the
abstraction and `PcoPanda` is the only impl today (Windows DLLs); future GenICam/IIDC
impls plug in at the same boundary. CI runs the *same* test suite on every (OS,
device) pair — a failure is fixed by repairing the substrate leak, never by forking.

## State & threading

### State decomposition — co-access lock groups

Shared mutable state is decomposed by **co-access**: fields always written together in
one critical section share a lock; fields accessed in isolation get their own.
Over-splitting (one `Mutex` per field) is rejected — it would manufacture the
multi-lock deadlock surface the grouping exists to eliminate.

```rust
// src-tauri/src/state.rs
use parking_lot::Mutex;   // no poisoning → infallible lock

pub struct AppState {
    pub threads:     ThreadHandles,                  // immutable after startup — no lock
    pub capture:     Arc<Mutex<Capture>>,            // latest_frame + timing_ring + acquisition
    pub session:     Arc<Mutex<Session>>,            // session + monitors
    pub handoff:     Arc<Mutex<Handoff>>,            // pending_save + last_summary + anatomical
    pub active_oisi: Arc<Mutex<Option<PathBuf>>>,    // independent
    pub config:      Arc<Mutex<ConfigStore>>,        // config commands lock it directly
}
```

`AppState` is `Arc<AppState>`, never locked as a whole. The grouping follows the
access map: the 60–100 fps `CameraEvt::Frame` path writes `latest_frame`, the timing
ring, and the accumulator in *one* critical section, so the hot path takes exactly one
lock and its deadlock risk is eliminated structurally.

**Locking discipline.** `parking_lot::Mutex` (no poisoning; `lock()` returns the guard
directly), so every critical section must be panic-free — satisfied because the heavy
ops (HDF5, PNG, serialization) return `Result` rather than panicking. Lock at most one
group at a time; the few remaining multi-group sites have a fixed, documented order
recorded at each site. The freeze culprits — anatomical PNG-encode + write, config
saves, display thread-spawn, the per-frame accumulator copy — are each restructured to
lock → copy out → drop → do the work. That, not the lock split, is what keeps the UI
responsive.

### Threads

Long-lived threads, all fixed at startup — never spawn-per-request or spawn-per-frame:

| Thread | Owns | Responsibilities |
|---|---|---|
| **Tauri runtime workers** | Tauri's pool | Dispatch `#[command]` handlers: lock one field, copy out, drop, return (< 1 ms) |
| **Camera thread** | PCO SDK handle | Frame acquisition, ring-buffer push |
| **Stimulus thread** | wgpu surface | Render loop synced to vsync |
| **Analysis worker** | Pipeline state, cancel flag | Walk the DAG synchronously, run each stage, fingerprint + persist, emit progress, honor cancel/preempt |
| **Event forwarder** | Cloned crossbeam receivers | Drain channels, lock only the field being written, emit Tauri events |

Command handlers are synchronous and trivial; the heavy work lives on the camera,
stimulus, and analysis threads, reached over `crossbeam` channels.

GPU-stream worker threads, an I/O-prefetch thread, and a dedicated PNG-encode thread
are **deliberately not built**: the pipeline already runs off the UI thread, and
per-stage timing shows no current bottleneck they would relieve (the DFT runs once per
file and is then cache-skipped; writes are milliseconds; PNG encode is already evicted
from the lock). They are throughput optimizations behind the existing channel
boundaries, to be added only if profiling identifies a measured need — adding them
speculatively is gold-plating.

## Pipeline

The analysis pipeline is a directed acyclic graph of stages. `isi_analysis::analyze`
(called on the analysis worker thread, with preemption + a cancel token) opens the
`.oisi`, builds the DAG, walks it in topological order, and assembles the result.

### The `Stage` trait over a blackboard

The stages are genuinely n-ary and heterogeneous (one consumes `Vec<Patch>` + three
maps; another emits a mask + a scalar). Rather than force a generic
`Stage<Input, Output>` into tuple-of-everything gymnastics, the model is a **uniform
trait over a shared `PipelineState` blackboard** — heterogeneity lives in the
blackboard's named fields, the trait stays uniform. A stage declares its dependency
edges (`petgraph` builds the DAG and checks acyclicity), computes its input
fingerprint, and executes by *wrapping* the `methods/*.rs` implementations — it never
re-implements stage logic (the method enums in `openisi-params` stay the single
source). The orchestrator (`pipeline/orchestrator.rs`) owns the topological walk,
skipping stages restored from the incremental cache and checking the cancel token at
each stage boundary.

The eleven stages:

```
Baseline → Projection → Retinotopy → SignSmoothing → CortexSource →
PatchThreshold → PatchExtraction → PatchRefinement → Labels → Eccentricity → DerivedMaps
```

### Device / host placement

The complex-valued front of the pipeline — baseline removal, the per-cycle DFT
projection, and retinotopy — runs on the Burn device backend (`compute/*`,
`methods/phase_smoothing.rs`, `methods/cycle_average.rs`). The segmentation tail —
cortex source, patch threshold/extraction/refinement, labels, eccentricity, derived
maps — runs on host `ndarray`: it is inherently CPU work (connected components,
morphology, per-patch `Vec<Patch>` operations) and is negligible against the device
stages, so moving it on-device would add upload/download round-trips that exceed the
compute saved. Per-stage timing identified the real hotspot — patch refinement — and
the fix landed there: an O(N²)→O(N) reformulation plus `rayon` over the independent
per-patch work, equivalence-gated bit-exact. We do not apply optimizations (van Herk
morphology, batched GEMM, GPU residency) that measurement shows are aimed at stages
that are not the bottleneck; "measure before optimize" is the rule.

### Cancellation

The cancel token (`Arc<AtomicBool>`) is checked at every stage boundary. A parameter
change mid-run sets it; the current stage unwinds and the walk restarts with the new
parameters, restoring unaffected stages from the cache so the restart resumes at the
first dirtied stage rather than from scratch. The incremental cache (fingerprints,
`/analysis_state`, the restore cut) is specified in
[`INCREMENTAL_ANALYSIS_DESIGN.md`](INCREMENTAL_ANALYSIS_DESIGN.md).

## Error model

```rust
// crates/isi-analysis/src/lib.rs — each variant carries a strum-derived stable code
#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    #[error("I/O error: {0}")]          Io(#[from] std::io::Error),  // E_IO
    #[error("HDF5 error: {0}")]         Hdf5(String),                // E_HDF5
    #[error("Invalid .oisi file: {0}")] InvalidPackage(String),      // E_INVALID_PACKAGE
    #[error("Missing data: {0}")]       MissingData(String),         // E_MISSING_DATA
    #[error("Compute: {0}")]            Compute(String),             // E_COMPUTE
    #[error("Validation: {0}")]         Validation(String),          // E_VALIDATION
    #[error("Analysis cancelled")]      Cancelled,                   // E_CANCELLED
}
```

`Result<T, AnalysisError>` is the only fallible return type — no `Result<T, String>`,
no `Box<dyn Error>`, no `anyhow`. Each variant carries a stable `E_*` code derived on
the variant via `strum` `EnumDiscriminants` (`code()`), the single source the IPC wire
and frontend share. `Cancelled` is a first-class outcome, not a failure: the
orchestrator emits `analysis:cancelled`, not `analysis:failed`. The Tauri command
layer unions `AnalysisError` into the serializable `AppError` façade for the UI (see
[`ARCHITECTURE.md`](ARCHITECTURE.md) → Errors).

## Numerical verification strategy

OpenISI depends on **no external numerical oracle** — no Python reference, no
cross-language bit-matching. Verification uses only what we control:

- **Cross-implementation equivalence** — each stage's output matches a committed
  baseline captured from trusted code on real SNLC `.oisi` acquisitions, within a
  per-stage tolerance. This is the primary safety net for any substrate or algorithm
  change.
- **Cross-backend equivalence** — the same corpus run on ndarray-CPU and CUDA must
  agree within the same tolerance, catching substrate leaks (an op diverging per
  backend).
- **Property tests** — mathematical invariants that hold on any input: σ=0 smoothing
  is identity; a 90°-rotated phase field negates the VFS; foveal eccentricity is 0;
  Gaussian smoothing preserves total mass; an all-ones mask is one connected
  component.
- **Per-stage tolerance budgets** in `crates/isi-analysis/tests/fixtures/tolerances.toml`,
  measured from observed cross-implementation drift on real data — not theoretical
  worst-cases. A future change that exceeds a budget is investigated, not absorbed by
  widening the bound.
- **Citation review + visual inspection** — every algorithm function carries a paper
  citation the reviewer checks against the equation, and known acquisitions are eyed
  in the UI against published map figures to catch errors that pass numerically but
  produce nonsense maps.

A *deliberate* algorithm change regenerates the baseline with a documented diff naming
the prior and new algorithm, the scientific justification, and the tolerance update —
keeping deliberate scientific changes distinct from accidental drift. What is
currently validated against which oracle is tracked in
[`VALIDATION_SCORECARD.md`](VALIDATION_SCORECARD.md).

## Observability

All logging is `tracing` (no `eprintln!` in non-test code), with a structured span per
stage. The orchestrator emits a per-stage timing event
(`stage`, `elapsed_ms`, `resource`); a `tracing-subscriber` formatter writes these to
a JSONL timing log in dev, which the benchmark tooling reads to compare runs. GPU
utilization is not yet exposed by Burn; it is read externally (`nvidia-smi dmon`)
during benchmark runs until Burn ships a hook.

## Concurrency primitives

| Need | Primitive |
|---|---|
| Thread-to-thread messaging | `crossbeam_channel::{bounded, unbounded}` (not `std`/`tokio` mpsc) |
| Wait on multiple channels | `crossbeam_channel::select!` (not poll-with-sleep) |
| Shared mutable state | purpose-scoped `parking_lot::Mutex` with a small critical section (not a god-mutex) |
| Reference-counted shared immutable | `Arc<T>` (not `Arc<Mutex<T>>` for immutable data) |
| Atomic flags / cancellation | `std::sync::atomic::AtomicBool`, wrapped in a `CancelToken` newtype |
| Parallel CPU work | `rayon` (not manual thread spawning) |
| DAG + toposort | `petgraph::Graph` + `petgraph::algo::toposort` |
| Content hashing | `blake3` |
| Property testing | `proptest`; numerical assertions via `approx` |

The application is **synchronous + thread-based** — no `tokio`, no `async fn` in our
code (Tauri's internal async is presented sync handlers). Scientific compute is
dominated by long-running CPU/GPU work, not by waiting on many concurrent I/O
operations, so the async model would add complexity without buying anything.

## Dependencies

**Used:** the granular Burn crates (`burn-tensor`, `burn-dispatch`, `burn-ndarray`,
`burn-cuda` behind `--features cuda`); `petgraph` (DAG); `parking_lot` (locks);
`blake3` (hashing); `crossbeam-channel`; `rayon`; `tracing` + `tracing-subscriber`;
`ndarray` / `num-complex` / `ndarray-stats` (host numerics at the HDF5 boundary and
the CPU stages); `hdf5-metno`; test-only `proptest` / `approx`.

**Not used:** `tch` / `torch-sys` / libtorch (the native C++ dependency and its
DLL-search hacks); `anyhow` (erases type information); `tokio` / `async`.

**Evaluated and rejected, with cause:** `candle` (no public streams API or
pinned-memory hook); `dagrs` (archived); `cust` / `rustacuda` (stale / experimental).
Established crates come first; hand-rolling requires a written justification in the
module header — the only standing hand-roll is the incremental cache's
fingerprint/restore logic, which no crate provides (every incremental crate is either
in-memory or caches into its own directory tree, never input-fingerprinted artifacts
resident inside a foreign HDF5 container).
