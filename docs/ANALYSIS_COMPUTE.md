# Analysis Compute Architecture

**Scope.** This document covers the compute architecture for the **analysis pipeline only** — the numerical work inside `crates/isi-analysis` that turns acquired frames into phase maps, SNR, retinotopy, and segmentation outputs. It does not govern any other compute-heavy part of the system:

- **Stimulus rendering** (`crates/openisi-stimulus`) uses wgpu and has its own GPU story tied to display timing and DXGI vsync. Out of scope here.
- **Camera acquisition** (`src-tauri/src/camera_thread.rs`, `crates/pco-sdk`) is hardware I/O and frame buffering, not numerics. Out of scope here.

The principles below apply only to `crates/isi-analysis` and its callers.

Analysis numerics run on whichever hardware backend the user's machine provides — CUDA on NVIDIA, Metal on Apple Silicon, otherwise CPU. There is **one** implementation of every analysis operation. Hardware dispatch is libtorch's job, not ours.

## Principles

1. **One backend for analysis.** `tch` (libtorch) is an unconditional dependency of `crates/isi-analysis`. There is no parallel ndarray implementation of analysis operations. The `gpu` feature flag on the analysis crate is removed.

2. **One numerical representation: `f32`.** All on-device analysis uses `Kind::Float`. This is **the single deliberate methodology variance** introduced by this rewrite — the current pipeline computes in `f64`, and the embedded `/results/` ground truth in our regression dataset was produced in `f64`. We accept the change for three reasons:

   - **MPS support.** Apple Silicon's Metal backend does not implement `f64`. Without dropping to `f32`, MPS is unreachable.
   - **Throughput.** CUDA and MPS both run materially faster at `f32`; memory bandwidth halves; tensor sizes halve.
   - **Phase precision is not load-bearing.** Phase maps are angles modulo 2π and amplitudes are normalized in downstream use. `f32` provides ~7 decimal digits — well beyond the precision retinotopy actually consumes.

   This is not a free claim. The regression test (see below) measures the actual `f64 → f32` drift against the existing ground truth. Tolerances are **provisional until first run** — placeholder values written from prior intuition — and **become commitments after first run**, set from the measured drift. Any future change that exceeds a committed tolerance is a regression that gets diagnosed before merge, not absorbed by widening the bound. If the initial measured drift is larger than scientifically acceptable on a particular map, we fix that op (e.g., accumulate in `f64` even when storage is `f32`) rather than enshrine a loose tolerance.

3. **One device, picked at startup, with an override hook for validation.** `compute::device()` is a `OnceLock<Device>` resolved on first call. Default priority:

   ```
   tch::Cuda::is_available()  → Device::Cuda(0)
   tch::utils::has_mps()       → Device::Mps
   else                        → Device::Cpu     (libtorch CPU: MKL on x86, Accelerate on Apple)
   ```

   Once chosen, every tensor created via `Tensor::from_slice(...).to(device())` joins that device. Tensor operations propagate device automatically — there is no need to thread a device parameter through call signatures or wrap it in a `ComputeContext`.

   **Override hook.** The selection honors the environment variable `OPENISI_ANALYSIS_DEVICE` (values: `cpu`, `cuda`, `mps`). This exists solely for cross-device validation — running the regression test on each available device to verify they all produce results within tolerance. It is not exposed to users or the UI; the production path always uses auto-selection. Setting the variable to a device that is not available on the host is an error, not a silent fallback (per the project's no-fallbacks principle).

4. **Two ndarray↔tensor boundaries, bounded device memory.** ndarray exists at exactly two points: HDF5 read (necessary — `hdf5-metno` produces `ndarray::Array3`) and the segmentation hand-off (necessary — segmentation is morphological/region-based work with no clean libtorch equivalent). Between those, data is `Tensor` on the active device for dF/F, DFT, SNR, smoothing, gradients, phase, amplitude, and VFS. After segmentation, the derived maps (`vfs_thresholded`, `eccentricity`, `magnification`, contours) compute on host `ndarray` because they consume segmentation's integer label output anyway; the device round-trip would buy little and cost a copy.

   Within the tensor region, **the full frame stack never lives on the device.** Host RAM holds the raw `Array3<u16>` (3.7 GB for our 7,049-frame 512×512 regression file — well within host budgets at any realistic acquisition length). Baseline is computed *on host* as `ndarray.mean(Axis(0))` over frames selected by `BaselineMode` (see [Baseline computation modes](#baseline-computation-modes)), parallelized via rayon. Only the small `[H, W]` f32 baseline tensor is uploaded. For each sweep, only that sweep's frames are uploaded as `f32` (typically 100–200 frames × H × W × 4 bytes ≈ a few hundred MB), processed through dF/F → DFT → SNR → accumulator, then dropped. Peak device memory is bounded to `O(sweep_size + accumulator_size + baseline_size)` ≈ ~200 MB for 512×512 data, **independent of acquisition length**. This works on any GPU regardless of VRAM (8 GB consumer NVIDIA, 16 GB Apple unified, 24 GB workstation). Per the project's no-fallbacks principle, per-sweep streaming is the only model — there is no "bulk-upload when memory permits, stream otherwise" branch.

5. **Method preservation.** Modulo the `f32` representation defined in Principle 2, this rewrite is a pure architectural refactor. The scientific methods — Kalatsky & Stryker Fourier retinotopy, Marshel delay-corrected mapping, the segmentation algorithm, the SNR definition, the gradient scheme — stay exactly as implemented today. Same operation, same inputs → same answer, bit-for-bit when run at the same dtype on the same device, and within the `f32` regression tolerances otherwise. Architectural improvements do not get to silently change methodology beyond the one variance that Principle 2 names explicitly.

6. **Express each operation in its most idiomatic tch form.** Method preservation (Principle 5) is the *constraint*, not the starting shape. The default is to look at what a given operation actually is, mathematically, and ask which tch primitive computes that thing — matmul, einsum, conv2d, a complex op, an in-place arithmetic op. The legacy code's shape is informational, not prescriptive.

   Concrete consequences:
   - **Single-frequency non-uniform DFT projection is a matrix product**, not a broadcast+reduce. `Σₜ dff[t,:,:] · kernel[t]` is `kernel @ dff_flat` — one `matmul` call that hits cuBLAS / Accelerate / MPS-BLAS. Both forms compute the same answer; matmul is the natural shape.
   - **SNR's noise computation is `[n_noise, n] @ [n, H·W]`**, a single batched matmul, not a 4D broadcast. Same answer, much faster path.
   - **Combine directions (`Z = fwd · conj(rev)`)** is one complex tensor multiply, not four real multiplies plus an add and a sub. Phase = `angle()`, amplitude = `abs()` — both native tch ops on complex tensors. See Principle 7.
   - **In-place arithmetic where ownership permits.** `f_add_`, `f_sub_`, `f_mul_`, `f_div_` mutate in place; consecutive `Tensor` allocations in a forward chain are wasted. Currently used in `CycleAccumulator`; should be used in the dF/F chain too.

   The one operation we deliberately do *not* use is `Tensor::fft_rfft`: it assumes uniform sampling, the pipeline does not (camera timestamps include hardware jitter), and switching would change the method, not just the implementation. Same answer would *not* result. This is the only place "method preservation" outranks "idiomatic tch."

7. **Native complex (`Kind::ComplexFloat`) is the default representation for complex maps.** Once a DFT projection produces a complex result, it stays as a `Kind::ComplexFloat` tensor through the rest of the on-device pipeline (smoothing, combine, gradients, phase extraction, amplitude extraction). The `(re, im)` f32 pair representation is reserved as a per-operation fallback if a specific op turns out to be missing or numerically divergent on native complex on a supported backend (CUDA / MPS / CPU). The regression test (Task #10) catches any backend-specific divergence.

   Why this matters concretely: complex multiply is one fused op instead of four real multiplies plus add/sub; `angle()` and `abs()` are native ops; cuBLAS and Apple Accelerate both expose complex BLAS kernels (`cgemv`, `cgemm`, etc.) which back tch's `matmul` on complex tensors. The pair representation drops all of that on the floor.

## Device-side data flow

```
HDF5 → Array3<u16>  ──── host RAM (stays here for the whole pipeline)
        │
        │ baseline_indices = select_by(BaselineMode, sweep_schedule, state_ids)
        │ baseline_host   = frames.select(Axis(0), &baseline_indices).mean(Axis(0))
        │
        ▼ (one small upload: just the [H,W] baseline tensor, ~1 MB)
   Tensor baseline [H, W] f32 on device
        │
        ├── for each sweep:
        │     ──► upload only this sweep's frames: Tensor [n_sweep, H, W] f32
        │         dff    = (sweep_frames - baseline) / (baseline + eps)
        │         kernel = Tensor::from_slice(timestamps).to(device()).{cos,sin}()
        │         complex = (dff * kernel).sum(dim=0)        // Tensor [H,W] complex
        │         accumulator.add(complex, direction)        // in-place tensor add
        │         (on first forward sweep per direction) snr = non_uniform_dft_snr(...)
        │     ──► sweep tensor dropped (device memory released)
        │
        └── retinotopy on accumulator outputs (all on device):
              smooth(complex)        // separable conv2d w/ Gaussian
              gradients(complex)     // same central-difference scheme
              phase, amplitude       // atan2, hypot
              vfs = sin(atan2(...) - atan2(...))

  ─────────────────────── device → CPU (one download per output map) ───────────────────────

retinotopy outputs as ndarray ───► segmentation (ndarray, skimage-style)
                                 │
                                 └─► derived maps (ndarray pointwise/local):
                                       vfs_thresholded, eccentricity,
                                       magnification, contours_azi, contours_alt

HDF5 ← f64 arrays
```

Peak device residency at any moment: one sweep's frames (~180 MB at 512×512, ~175 frames) + 4 complex accumulator tensors (~8 MB) + baseline (~1 MB) + SNR scratch + retinotopy intermediates. Total ~200 MB. Bounded by the largest sweep, not the acquisition length.

## Module shape

```
crates/isi-analysis/src/
├── compute/
│   ├── mod.rs              device selection (with OPENISI_ANALYSIS_DEVICE override),
│   │                       dtype constants, public API
│   ├── ops.rs              all tensor operations: baseline, dF/F, non-uniform DFT,
│   │                       SNR, gaussian smoothing, phase gradients, VFS,
│   │                       phase angle, amplitude
│   ├── conversions.rs      Array3<u16> → Tensor; Tensor → Array2<f64>/Complex64
│   └── accumulator.rs      on-device CycleAccumulator
├── io.rs                   HDF5 read/write only; calls compute::
├── math.rs                 host-side derived maps that consume segmentation output:
│                           vfs_thresholded, eccentricity, magnification, contours.
│                           No DFT, no SNR, no smoothing, no gradients — those moved
│                           wholesale into compute::ops.
├── segmentation.rs         unchanged (skimage-style image ops on ndarray)
└── lib.rs                  public API, orchestrator
```

## Per-operation port plan

Each row records the current method, what changes architecturally, and what stays mathematically identical. The "What changes" column is non-empty only when the change is provably method-preserving (the same answer for the same inputs, within `f32` quantization).

| Operation | Method (preserved) | tch idiom |
|-----------|-------------------|-----------|
| Baseline (temporal mean) | mean over selected frame indices; `BaselineMode` picks which frames | computed *on host* via `ndarray.mean(Axis(0))` — bandwidth-bound, no benefit from device round-trip; only the `[H,W]` result is uploaded |
| dF/F | `(frame - baseline) / (baseline + eps)` per-pixel | tensor broadcast then **in-place** chain via `f_sub_` + `f_div_` (sweep tensor is owned, no need for intermediate copies) |
| DFT projection at stim frequency | non-uniform DFT at one frequency `1 / (t_last - t_first)`; sign flips for forward/reverse; uses actual camera timestamps | kernel built on-device by uploading timestamps as f32 then `cos`/`sin`; **projection is `kernel_complex @ dff_flat` via `matmul`** (BLAS-backed); result is a `Kind::ComplexFloat` `[H,W]` tensor |
| SNR | signal at stim freq + 20 noise bins (skipping harmonics 2–4, Nyquist-capped, evenly subsampled if more available); `signal_power / mean(noise_power)` with `1e-20` floor | single batched matmul `[n_noise, n] @ [n, H·W]` for noise; one matmul each for signal real/imag parts (no broadcast-then-reduce); same bin-selection rule, harmonic skipping, Nyquist cap, division floor |
| Combine directions `Z = fwd · conj(rev)` | identical formula | **one complex tensor multiply** `&fwd * &rev.conj()` on `Kind::ComplexFloat` — replaces four real multiplies + add + sub |
| Gaussian smoothing | separable convolution; kernel radius `ceil(3σ)`, normalized; reflection padding | `conv2d` with `reflection_pad2d`; kernel built on-device via `Tensor::arange` + `exp`. For complex inputs: view as real `[H, W, 2]` and conv both channels (or run twice if a backend lacks 2-channel complex conv) |
| Phase gradients | central differences in interior; forward at left/top edge, backward at right/bottom edge; `dφ/dx = Im{conj(Z) · ∂Z/∂x}` | tensor `narrow`/`copy_` preserving the exact edge scheme; complex multiply for `conj(Z) · ∂Z`; `.imag()` for the imaginary-part extraction. Deliberately *not* a generic `Tensor::diff` or Sobel conv2d — those have different edge semantics |
| Phase | `arg(Z) = atan2(im, re)` | **`complex_tensor.angle()`** native op (replaces `im.atan2(&re)`) |
| Amplitude | `|Z| = √(re² + im²)` | **`complex_tensor.abs()`** native op (replaces `(&re*&re + &im*&im).sqrt()`) |
| VFS | `sin(θ_alt − θ_azi)` where `θ = atan2(dy, dx)` | unchanged formula; runs as tensor ops on f32 |

The single deliberate non-use is `Tensor::fft_rfft`: it assumes uniform sampling, our timestamps don't, and the substitution would change the method (see Principle 6).

## CycleAccumulator

Holds one `Kind::ComplexFloat` tensor per direction, plus per-orientation SNR tensors:

```rust
struct CycleAccumulator {
    azi_fwd: Option<(Tensor /* complex [H,W] */, u32 /* count */)>,
    azi_rev: Option<(Tensor, u32)>,
    alt_fwd: Option<(Tensor, u32)>,
    alt_rev: Option<(Tensor, u32)>,
    snr_azi: Option<Tensor>,   // f32 [H,W]
    snr_alt: Option<Tensor>,
}
```

`add(complex_map, direction)` does in-place complex-tensor add (`f_add_`). `finalize()` divides by counts, downloads the four complex maps and two SNR tensors to host once, and produces the `ComplexMaps` + `SnrMaps` the analysis API expects. Averaging behavior (per-direction mean across repetitions) is identical to the legacy `CycleAccumulator` — only the storage type changes.

## Baseline computation modes

`AnalysisParams` gains a new field:

```rust
pub enum BaselineMode {
    /// Mean over every camera frame in the acquisition. Matches the current
    /// pipeline's behavior. Default for the architectural-migration PR so the
    /// regression test against existing ground truth passes.
    AllFrames,

    /// Mean over camera frames whose timestamps fall OUTSIDE every recorded
    /// sweep window. Includes pre/post baseline periods and inter-direction
    /// gaps. Excludes contamination from stimulus-driven response.
    OutsideSweepWindows,

    /// Mean over camera frames whose nearest stimulus state is
    /// `baseline_start` or `baseline_end`. Strictest interpretation —
    /// only the dedicated pre/post-acquisition baseline windows.
    DedicatedBaselinePeriods,
}
```

**Why this matters.** Inspecting the regression dataset shows the current `AllFrames` baseline averages 7,049 camera frames, of which 6,799 (96.5%) fall inside sweep windows. F₀ is therefore contaminated by the stimulus response itself, biasing dF/F numerators and denominators in a spatially structured way (responsive pixels bias themselves down). This is a likely cause of the noisy phase maps and weak amplitudes currently observed.

The data needed to fix this is already in the `.oisi` file:
- `/acquisition/schedule/sweep_start_sec`, `sweep_end_sec` — sufficient for `OutsideSweepWindows`
- `/acquisition/stimulus/state_ids` (per stimulus frame) plus `/acquisition/stimulus/timestamps_sec` for camera-frame mapping — sufficient for `DedicatedBaselinePeriods`

No format change, no acquisition change, no new attributes.

**Selection happens on host**, in `io.rs` (or the new `compute::conversions` module), before any tensor upload — see the data-flow diagram. `BaselineMode` selects camera-frame indices; `ndarray.select(Axis(0), &indices).mean(Axis(0))` produces the baseline tensor; that tensor is what gets uploaded. The compute layer is unaware of which mode produced its input baseline.

**Default.** For the architectural-migration PR, default is `AllFrames` — see two-PR sequencing below.

### Acquisition-side bugs (tracked separately)

Two known bugs on the acquisition side share a common architectural root, neither addressed by this rewrite:

- **`/experiment/timing/` attributes are not written** into produced `.oisi` files. The group is created but empty, even though the values (`baseline_start_sec`, `baseline_end_sec`, `inter_stimulus_sec`, `inter_direction_sec`) exist in `experiment.toml` and are loaded into the in-memory `Experiment` struct at startup.
- **Loose `anatomical_<ts>.png` files are left in the data directory** after acquisition completes. The anatomical *is* correctly embedded into `/anatomical` in the .oisi, but the transient session-time PNG (written by `ui/src/views/session.js:410`) is never cleaned up. The PNG is redundant data sitting next to the canonical .oisi.

Both are symptoms of the same architectural looseness: there is no single acquisition-finalization step that takes the complete in-memory state and projects it into the .oisi with a completeness check. Multiple write paths exist — the session-time PNG write, the experiment.toml load at startup, the .oisi serializer at acquisition end — and the principle "the .oisi is the sole on-disk product of an acquisition" (stated in `DATA_FORMAT.md`'s "Complete provenance" principle) is not enforced by code or test. These belong in a future `ACQUISITION_ARCHITECTURE.md` pass, not in this analysis-compute rewrite.

Neither blocks this rewrite: the sweep schedule and state IDs in the `.oisi` are sufficient for `BaselineMode::OutsideSweepWindows`, and the analysis crate reads the anatomical from `/anatomical`, not from any side PNG.

## Two-PR sequencing

Two distinct claims, two distinct PRs, no entanglement between architectural correctness and methodological correction.

**PR 1 — Architectural migration, pure refactor.**
- Default `BaselineMode::AllFrames` (preserves current behavior).
- All six principles above land: unified backend, f32, runtime device with override, bounded device memory via per-sweep streaming, method preservation, tch-primitive selectivity.
- Regression test passes Claim 1 (method preservation vs current f64 ground truth) on every available device.
- Regression test passes Claim 2 (cross-device unification).
- Merges when both claims green.

**PR 2 — Switch baseline default, regenerate ground truth.**
- Default `BaselineMode::OutsideSweepWindows`.
- Visually validate phase maps look better on the regression file (the science check that motivated this whole thread).
- Regenerate `/results/*` in the regression dataset using the new default.
- Regression test now pins method preservation to the *corrected* baseline. The `AllFrames` mode remains available but is no longer the ground truth.
- Merges when the science check is satisfying and the new regression baseline is committed.

This keeps the architectural refactor honest ("we didn't break anything") and the methodological fix honest ("we improved the science"), with no chance of one masking the other.

## Dev workflow: generated figures

For dev debugging without launching the UI, the headless binary's existing `--figures` flag exports per-result-map PNGs — jet colormap for scalars, black/white for boolean masks, red/blue by area sign for label maps, anatomical as grayscale.

**Location.** Output lands in `dev_figures/<oisi_stem>/<run_tag>/` at the repo root, separate from the user's data directory so dev artifacts don't intermingle with real recordings. `dev_figures/` is gitignored.

**Run tag.** `<baseline_mode>-<device>-<UTC-timestamp>`, e.g., `allframes-mps-20260519T1145`. Tag components are pulled from `params.baseline_mode`, `compute::device()` after resolution, and the current UTC minute. Different runs never overwrite each other; side-by-side comparison across baseline modes or devices is `ls dev_figures/<stem>/`.

**meta.json.** Each run directory contains a JSON file recording the full reproduction context. Uses portable identifiers from the .oisi root attributes (`animal_id`, `created_at`), not absolute paths, so a `dev_figures/` directory is shareable across machines:

```json
{
  "source": {
    "filename": "5_14_2026_test5_1778801597.oisi",
    "animal_id": "5/14/2026_test5",
    "created_at": "1778801597"
  },
  "device": "Apple Metal (MPS)",
  "baseline_mode": "OutsideSweepWindows",
  "baseline_frame_count": 250,
  "git_sha": "350aa2d",
  "git_dirty": true,
  "git_branch": "main",
  "timestamp_utc": "2026-05-19T11:45:00Z",
  "analysis_params": { ... }
}
```

`source.created_at` is the acquisition's unix timestamp — globally unique to the recording, survives renames and copies, and identifies the source without needing a content hash.

**CLI.** `--figures` with no path auto-tags into `dev_figures/<oisi_stem>/<auto_tag>/`. Explicit `--figures <path>` honors a custom path (no auto-tag) for one-off comparisons.

## UI surface

`get_analysis_backend` Tauri command returns the device string for the analysis pipeline ("CUDA device 0", "Apple Metal (MPS)", "CPU (libtorch)"). Displayed read-only in the analysis sidebar. No override toggle — the best available device is always used. The command name is analysis-scoped to leave room for separate device status from other subsystems (stimulus, camera) if those ever need to surface their own backend info.

## Regression test

The imported `5_14_2026_test5_1778801597.oisi` is the regression dataset. It contains complete `/results/*` arrays produced by the current pipeline. The equivalence test pins two claims with distinct tolerances — method preservation and cross-device unification — because they have different sources of drift and conflating them would let one hide the other.

### Claim 1: Method preservation (device vs. f64 ground truth)

The new pipeline, run on any single device with `BaselineMode::AllFrames` (the migration default — see [Two-PR sequencing](#two-pr-sequencing)), reproduces the embedded `/results/*` from the current f64 pipeline within a tolerance that bounds **`f64 → f32` quantization**.

The only legitimate sources of drift in this claim are:
- `f64 → f32` quantization in tensor ops (bounded, predictable)
- Floating-point summation order (parallel reduction vs the current sequential ndarray accumulation)

These tolerances are provisional until first run, then **become commitments** — measured drift sets each bound, and any future change that exceeds it is a regression that must be diagnosed before merge.

| Map | Provisional tolerance |
|-----|----------------------|
| `azi_phase`, `alt_phase` (radians, modulo 2π) | `1e-4` after unwrapping |
| `azi_amplitude`, `alt_amplitude` | relative `1e-4` |
| `vfs` (bounded to [-1, 1]) | `1e-4` |
| `snr_azi`, `snr_alt` | relative `1e-3` |
| `area_labels` (integer segmentation output) | exact equality |
| `area_borders` | exact equality |

### Claim 2: Cross-device unification (device vs. device)

The new pipeline, run on the same input across every device available on the host, produces results that agree across devices to a **much tighter** tolerance — because cross-device drift has only one source: floating-point summation order (parallel reduction shape differs between CUDA / MPS / CPU). No dtype change is involved; both sides are running f32 with the same kernel.

| Map | Cross-device tolerance |
|-----|------------------------|
| All f32 result maps | `1e-6` (relative for amplitudes/SNR, absolute for bounded maps like VFS and phase) |
| `area_labels`, `area_borders` | exact equality |

If devices diverge by more than this, that is a real unification bug — not f32 drift — and gets diagnosed before merge.

### How the test runs

Test lives at `crates/isi-analysis/tests/regression_oisi.rs`. It is `#[ignore]` by default (1.9 GB file, not in CI) and run manually via `cargo test --test regression_oisi -- --ignored`. The CI sample-data path covers the smaller SNLC dataset for continuous coverage.

The test enumerates `Device::Cpu` plus whichever of `Cuda(0)` / `Mps` are present on the host, drives each via the `OPENISI_ANALYSIS_DEVICE` override, and applies Claim 1 to each device individually, then Claim 2 across the set. On a developer Apple Silicon machine that's two devices (CPU + MPS); on a CUDA workstation, two or three.

## tch capabilities the architecture leverages

Enumerating these explicitly so the design choices are visible. Each entry says *what* tch offers and *why* we use it (or commit to using it during the refactor in Task #13).

- **Native complex tensors (`Kind::ComplexFloat`).** Used end-to-end for complex maps. Enables single-op complex multiplication, `angle()`, `abs()`, `conj()` and lets BLAS-backed matmul use complex kernels (`cgemv`/`cgemm`) where the backend provides them. See Principle 7.
- **`matmul` (and `mm` / `bmm` / `addmm`).** Used for the DFT projection and the SNR noise computation. The mathematical shape of "project a per-frame signal onto a per-frame kernel" *is* a matrix product; expressing it as `matmul` rather than broadcast+reduce hits cuBLAS / Accelerate / MPS-BLAS, which are aggressively optimized.
- **`conv2d` with `reflection_pad2d`.** Used for separable Gaussian smoothing. The right primitive — no in-Rust convolution loop.
- **`Tensor::arange` / `Tensor::from_slice`.** Kernel construction (Gaussian radii, timestamp arrays) happens on-device, not as CPU `Vec<f64>` intermediates.
- **In-place arithmetic (`f_add_`, `f_sub_`, `f_mul_`, `f_div_`).** Used in `CycleAccumulator` and in the dF/F chain. Avoids transient tensor allocations on the per-sweep hot path.
- **`Tensor::cos` / `sin` / `atan2` / `sqrt`.** Pointwise math on-device. Used everywhere we need trigonometric or magnitude operations.
- **Device dispatch.** `tch::Cuda::is_available()`, `tch::utils::has_mps()`, `Device::Cuda(n)`, `Device::Mps`, `Device::Cpu`. The basis of Principle 3.

## tch capabilities deliberately deferred

Available in tch 0.24 but not part of this pass. Named so the choice is visible:

- **TorchScript JIT (`CModule::load`, `forward_ts`, `create_by_tracing`).** Could compile the per-sweep pipeline once and execute it as a single fused module across all sweeps, eliminating per-op Rust↔libtorch dispatch overhead. Real perf win for repeated identical compute, but adds build/distribution complexity (a `.pt` module file, or a tracing step at startup). Deferred until the per-op dispatch overhead becomes a measurable bottleneck on the regression file.
- **Autograd (`requires_grad`, `backward`, `grad`).** Our retinotopy pipeline has six scalar parameters (`smoothing_sigma`, `azi_angular_range`, `offset_azi`, `offset_alt`, segmentation thresholds). With autograd, those could be fit by gradient descent against a loss — useful for rig calibration or per-mouse parameter tuning. Not a current product requirement; named here so it's visible we have the capability and chose not to use it yet.
- **Multi-GPU dispatch (`Device::Cuda(n)` for `n > 0`).** Only `Cuda(0)` is targeted. Sweep-level parallelism across multiple GPUs is straightforward (each direction's sweeps are independent), but not justified for current acquisition sizes.
- **Mixed precision (bf16, f16).** We chose `f32` end-to-end per Principle 2. bf16 could halve memory again on supported GPUs at the cost of more aggressive precision loss. Out of scope until f32 is shown insufficient.
- **`Tensor::fft_*` family.** Specifically *not* used — see Principle 6. The non-uniform DFT is the methodologically correct shape; rfft would be a method change.

## Known risks and follow-ups

- **MPS op coverage.** Apple's MPS backend in libtorch has been improving rapidly but is not feature-complete relative to CUDA. A specific op we depend on (e.g., a particular form of `narrow`+`copy_`, a complex-tensor matmul, a `conv2d` with certain stride/padding) may be missing or numerically divergent on MPS. The cross-device regression test on the developer's Apple Silicon machine is where we'll discover this; the remediation is a targeted per-op CPU bounce for the offending tensor — or, where it matters, fall back to the `(re, im)` pair representation for that op only. We do not pre-emptively scatter such bounces; we add them only when the regression test points at one.

- **Libtorch distribution.** Project-managed for **development** (handled): `scripts/setup.sh` and `scripts/setup.ps1` download a pinned libtorch (currently `2.11.0`, matching tch 0.24) into `vendor/libtorch/`, and `.cargo/config.toml` forces `LIBTORCH` to that path so `cargo build` is hermetic with respect to system or Python-installed libtorch. Still **open for end-user distribution**: how libtorch lands inside the Tauri installer / app bundle for shipping to neuroscientists is a separate question intersecting packaging (Tauri sidecar, CI release workflow) and belongs in its own doc.

  Known caveat on the dev side: `torch-sys` switches to a Python-torch path the instant `LIBTORCH_USE_PYTORCH` is *set* in the environment, regardless of value. Cargo's `[env]` table can override values but cannot unset a variable. The setup scripts detect this and instruct the developer to remove `LIBTORCH_USE_PYTORCH` from their shell rc. Subsequent `cargo` invocations rely on the variable being absent.

## What gets deleted

- `crates/isi-analysis/src/compute/fallback.rs` (stub)
- `[features]` block in `crates/isi-analysis/Cargo.toml` (`gpu` flag and `default = ["gpu"]`)
- All `#[cfg(feature = "gpu")]` / `#[cfg(not(feature = "gpu"))]` in `io.rs` and `math.rs`
- `math::dft_projection`, `math::compute_snr_map`, `math::gaussian_smooth_complex`, `math::phase_gradients`, `math::compute_vfs` (the ndarray duplicates of operations already implemented in `compute::`)
- `compute::gpu_*` prefixes — there's only one implementation, no prefix needed
- Per-sweep `Tensor → Array2<Complex64>` round-trips (`CycleAccumulator` now accumulates on-device)
- CPU-side `Vec<f64>` kernel construction for DFT/SNR/Gaussian (rebuilt on-device, same formula)
