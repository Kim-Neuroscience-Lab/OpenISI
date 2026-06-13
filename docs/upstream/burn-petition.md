# Upstream Petition: Zero-Bounce Pinned H2D + Explicit Stream Primitives

**Target repos:** `tracel-ai/cubecl`, `tracel-ai/burn`
**Status:** draft for posting as a single combined GitHub issue cross-referencing existing work
**Author:** OpenISI maintainers
**Related upstream work:** [cubecl#1334](https://github.com/tracel-ai/cubecl/pull/1334) (draft), [burn#4991](https://github.com/tracel-ai/burn/issues/4991) (open, by @nathanielsimard)

---

## Summary

We are building **OpenISI**, a scientific GPU-compute application in Rust (Intrinsic Signal Imaging analysis for systems neuroscience). We selected Burn 0.21 + CubeCL 0.10 as our compute substrate after evaluating Burn against Candle, `tch`, and several lower-level alternatives. Burn is the right call for our use case and we want to stay on it long-term.

We need two related primitives that are currently in flight but not yet shipped, and are writing this issue both to (a) add a concrete scientific-compute use case to the discussion already happening on [cubecl#1334](https://github.com/tracel-ai/cubecl/pull/1334) and [burn#4991](https://github.com/tracel-ai/burn/issues/4991), and (b) offer concrete numbers from our pipeline to help motivate prioritization.

**The two primitives:**

1. **Zero-bounce pinned-buffer → tensor upload.** A way to take a host buffer we have already pinned (or had Burn pin) and hand it to `ComputeClient` without re-staging through Burn's internal pool. The public `ComputeClient::staging(&mut bytes, false)` API gets us pinned-memory bandwidth but adds one extra ~chunk-sized memcpy on the upload path. Eliminating that bounce is exactly what [cubecl#1334](https://github.com/tracel-ai/cubecl/pull/1334) proposes (`create_from_slice_pinned`, `create_tensors_from_slices_pinned`).

2. **Explicit CUDA-stream creation + record/wait primitives.** Per Burn's current model, "thread = stream" — we spawn N worker threads to get N concurrent streams, which is fine for steady-state compute concurrency. But for **producer-consumer overlap between H2D copies and compute kernels on the same logical work item** (cycle N+1's H2D in flight while cycle N is computing), we need either explicit `Stream::record_event` / `Stream::wait_event` primitives or a `create_from_pinned_handle` that pipelines internally on its own non-default stream. This is exactly [burn#4991](https://github.com/tracel-ai/burn/issues/4991)'s scope.

## Why this matters — concrete OpenISI use case

Our analysis pipeline includes a **Fourier-retinotopy stage** that, for each of 4 sweep directions, processes K~10 cycles of T~500 raw frames at 512×512×u16. Per direction this means:

- Sequential HDF5 read of `[K·T, H, W] u16` ≈ 250 MB
- Per cycle: host→f32 conversion + H2D ≈ 50 MB transferred at PCIe 4.0 bandwidth (~12 GB/s pinned, ~3 GB/s pageable)
- GPU compute per cycle: single batched matmul `[1, T] @ [T, H·W]` → tens of milliseconds on a 4070-class card

In the current Burn 0.21 path (using `ComputeClient::staging()`):

- Pinned-memory bandwidth: ✅ achieved
- Per-cycle re-staging memcpy: adds ~5 ms × K × 4 ≈ 200 ms per analysis run
- H2D and GPU compute serialization on the default stream: adds ~K × 4 ms (H2D wait) per analysis run ≈ 160 ms

**With the proposed primitives:**

- Zero-bounce upload: saves the ~200 ms of re-staging
- Explicit stream record/wait for H2D/compute overlap: saves the ~160 ms of serialization
- Combined per-analysis savings: **~360 ms or ~20-30% of typical retinotopy-stage wall-clock** on our reference dataset

For a working scientist iterating on analysis params, this is the difference between sub-second and multi-second feedback after every parameter tweak.

## Microbenchmark we can contribute

We are prepared to contribute a **standalone microbenchmark** to `cubecl/benches/` or `burn/benches/` measuring:

- Pinned vs pageable H2D bandwidth on a configurable buffer size (matches the existing benchmark style in `cubecl/crates/cubecl-runtime/benches/`)
- Pipelined-vs-sequential H2D + compute on a synthetic GEMM workload (matches the DFT pattern)
- Both run on CUDA, with reproducible numbers we can pair against the implementation PRs

We can do this independently of whether the API design lands; it stands as a reproducible perf baseline for the discussion.

## On API placement

We have read @nathanielsimard's review comments on [cubecl#1334](https://github.com/tracel-ai/cubecl/pull/1334), particularly the concern that `reserve_staging` and `create_from_slice_pinned` may not be "at the right level" because `create_from_slice` is primarily a testing path. We agree the API surface deserves care. From our use case, the **minimum useful primitives** are:

1. A way to allocate a pinned host buffer of known size, owned by user code (so we can fill it from an external source like an HDF5 read).
2. A way to create a tensor from that buffer with a non-blocking H2D copy that records a CUDA event on completion.
3. A way for subsequent GPU operations on that tensor to wait on that event (implicitly or explicitly).

We don't have a strong opinion on whether this lives on `ComputeClient`, on a new `StreamingBuffer<T>` type, on `Tensor::from_pinned_handle`, or as part of a broader async-tensor-creation API. We care about the semantics, not the spelling. Happy to discuss any preferred shape.

## What we are doing in the meantime

For our 0.21 release of OpenISI, we are:

- Using `ComputeClient::staging(&mut bytes, false)` to get pinned-memory bandwidth (~2-4× over pageable).
- Spawning N worker threads to get N concurrent streams for stage-level parallelism (4 sweep directions × 2 orientations).
- Accepting the ~360 ms of preventable overhead per analysis run, documented in our internal architecture spec.

We are not dropping to `cudarc` directly — we believe staying within Burn's substrate is the right architectural call, and we'd rather petition for the API than fragment our compute layer.

## What we are asking

1. **Acknowledge our use case** as a concrete data point in [cubecl#1334](https://github.com/tracel-ai/cubecl/pull/1334) and [burn#4991](https://github.com/tracel-ai/burn/issues/4991).
2. **Indicate a likely release target** (0.22? 0.23?) for zero-bounce pinned upload + explicit stream primitives, so downstream projects can plan around it.
3. **If helpful, accept our microbenchmark contribution** so the perf discussion has reproducible numbers.

We are not asking for special treatment, urgency, or API decisions on our behalf. We just want our needs documented and the timing legibility a roadmap commitment provides.

Thank you for the work on Burn and CubeCL. The substrate is the only credible Rust path to a unified GPU compute story in 2026, and we're committed to it for the long haul.

---

## Reference: relevant existing work

- [cubecl PR #1334](https://github.com/tracel-ai/cubecl/pull/1334) — feat(runtime): pinned-host-buffer fast path for create_from_slice uploads (draft, by @lilith, opened 2026-05-17). Microbenchmark in the PR: 48 MB upload 9 ms → 3 ms.
- [burn issue #4991](https://github.com/tracel-ai/burn/issues/4991) — Explicit Stream Creation (open, by @nathanielsimard, opened 2026-05-21).
- [cubecl issue #539](https://github.com/tracel-ai/cubecl/issues/539) — CUDA: Use pinned/page-locked memory when possible (closed by #885; H2D portion deferred).
- [cubecl PR #885](https://github.com/tracel-ai/cubecl/pull/885) — Pinned Memory (D2H only, shipped 2025-09).
- [cubecl PR #1030](https://github.com/tracel-ai/cubecl/pull/1030) + [burn PR #4016](https://github.com/tracel-ai/burn/pull/4016) — pinned-memory staging plumbing (shipped 2025-11).
- [Burn 0.19.0 release notes](https://burn.dev/blog/release-0.19.0/) — introduces the thread=stream model and internal pinned staging.

---

## How to post this

Post as a single new issue on **`tracel-ai/cubecl`** (since both primitives live below the Burn layer at the CubeCL runtime level), with title:

> Zero-bounce pinned H2D + explicit stream primitives — scientific-compute use case

Cross-link from a comment on [burn#4991](https://github.com/tracel-ai/burn/issues/4991). Cross-link from a comment on [cubecl#1334](https://github.com/tracel-ai/cubecl/pull/1334) (without nagging — Nathaniel is aware).
