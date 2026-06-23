# Architecture

The system-level structure of OpenISI: the crates and their responsibilities, the
core-vs-boundary model, and how data and control flow through them. This is the map;
each concern's detail lives in its own doc (see [`README.md`](README.md)). For *why*
the structure is what it is — the invariants — see [`PRINCIPLES.md`](PRINCIPLES.md).

## The shape: a self-contained core, a standard at the boundary

OpenISI is an **instrument**: a Tauri desktop app that runs on a rig, drives a
camera and a stimulus display in real time, captures irreplaceable data, analyzes
it, and visualizes the result. Its architecture has a **hot core that is bulletproof
and self-contained** and a **cold interoperability boundary that may be heavier**:

- **Core (pure Rust, zero external runtime):** acquisition → a lean Rust-native
  working format (`.oisi`) → the incremental analysis pipeline → visualization.
- **Boundary (the standard, via its reference tool):** export to NWB / DANDI through
  `pynwb` / `nwbinspector`, off the hot path, with the machinery bundled and invisible.

The native format is *not* NWB and is *not* written by an external tool; the standard
is reached by *exporting to* it. (Full rationale: `PRINCIPLES.md` → Architecture.)

## Crate map

```
src-tauri  (the application: Tauri shell, IPC commands, acquisition orchestration,
            the camera/stimulus/analysis threads, the headless CLI)
  ├── isi-analysis      the analysis pipeline; analysis-semantic .oisi read/write
  │     │                  (re-exports the `oisi` crate, the app's path to the format)
  │     ├── oisi             the .oisi working format: schema SSoT + HDF5 I/O + types + import
  │     └── openisi-params   typed config (ConfigStore) — shared with the app
  │           └── openisi-stimulus   stimulus design enums + display geometry
  ├── openisi-params    (also used directly by the app for the live ConfigStore)
  ├── openisi-stimulus  stimulus dataset/sequencer/geometry + the wgpu/WGSL renderer
  └── pco-sdk           Rust FFI (libloading) over the PCO camera DLLs
```

Dependency direction is strictly downward (no cycles). The numerical/scientific core
(`isi-analysis`) and the config SSoT (`openisi-params`) are **platform-neutral and
GUI-free** — they build and run on Windows/macOS/Linux. The Windows-bound surface
(PCO, DXGI vsync, QPC timing) lives in `src-tauri` + `pco-sdk` and is what makes
*acquisition* Windows-only today (analysis/visualization/export are cross-platform —
see `PRINCIPLES.md` → Platform).

## Configuration

All parameters are typed `serde` structs in `openisi-params::config`
(`RigConfig` / `ExperimentConfig` / `AnalysisConfig` / `UiStateConfig`) behind a
single live **`ConfigStore`** (the `define_params!` registry it replaced is deleted).
`schemars` derives the JSON Schema that drives the UI descriptors; `garde` validates
static bounds; the one runtime-validated concern is the dynamic hardware constraints.
Config persists as `config/*.json` (shipped baseline + a sparse user/dev overlay,
merged via RFC-7386). The analysis stages are internally-tagged enums, so a tunable
cannot exist unless its method is selected — these same enums *are* the pipeline's
method types (no bridge). Detail: the config structs + `PRINCIPLES.md` Invariants 10–11.

## Data & control flow

**Acquisition** (in `src-tauri`, real-time): the camera thread pulls frames via
`pco-sdk`; the stimulus thread renders the sweep via `wgpu`/WGSL and times it against
DXGI vsync + QPC. A frozen `ConfigSnapshot` + the accumulated frames + the realized
sweep schedule are written by `write_oisi` into the `.oisi` file (atomic temp-write +
rename).

**The `.oisi` working format** (HDF5, owned by the dedicated light `oisi` crate —
schema + I/O + types + import — which `isi-analysis` composes for analysis-semantic
read/write) is the canonical store between stages: raw frames, stimulus schedule,
multi-clock timing forensics, per-direction complex maps, results, and the provenance
(typed config + software version). Its schema is declared once in the `oisi` crate
and the contract [`oisi.schema.json`](oisi.schema.json) (the format SSoT) is generated
from it.

**Analysis** (`isi-analysis`): a demand-driven DAG over the pipeline stages
(`ΔF/F → per-cycle DFT → cycle combine → phase smoothing → VFS → sign-map smoothing →
cortex source → patch threshold/extraction/refinement → eccentricity`). Compute runs
on the runtime-dispatched Burn backend (CPU canonical, CUDA optional). A Merkle
fingerprint per stage drives the **incremental cache**, so only stages whose inputs
changed re-run; results are written back into the same `.oisi`. Methods:
[`PIPELINE_METHODS.md`](PIPELINE_METHODS.md); cache: [`INCREMENTAL_ANALYSIS_DESIGN.md`](INCREMENTAL_ANALYSIS_DESIGN.md);
substrate: [`COMPUTE.md`](COMPUTE.md).

**Visualization & export:** the vanilla-JS webview reads result maps over Tauri IPC
and renders them ([`UI_ARCHITECTURE.md`](UI_ARCHITECTURE.md)); a deliberate
`export-nwb` step transforms a `.oisi` into a reference-validated NWB file
([`INTEROP_NWB.md`](INTEROP_NWB.md)).

## Process & concurrency

One process. The Rust backend runs the long-lived **camera**, **stimulus**, and
**analysis-worker** threads, communicating with the IPC layer over `crossbeam`
channels. Shared mutable state is **decomposed by co-access** — no god-mutex; each
group (`config`, `session`, `capture`, `handoff`, `active_oisi`) has its own
`parking_lot` lock, with a documented lock order. The threading/lock model and the
Burn backend are detailed in [`COMPUTE.md`](COMPUTE.md).

## Errors

Typed `thiserror` enums per crate; `AppError` is the Tauri-IPC façade that unions
them via `#[from]` and serializes a structured `AppErrorWire` (stable `category` +
`code` + message). The `E_*` codes are `strum`-derived on the variants and generated
into the frontend (`error-codes.generated.js`) so the JS cannot drift from the
backend. (The one owned HDF5 I/O boundary + source-preservation is the current
frontier — `PRINCIPLES.md`.)

## Cross-references (no duplication)

This doc states structure once and defers everything else: invariants & Definition of
Done → `PRINCIPLES.md`; tool-vs-domain → `TOOL_LEDGER.md`; the format contract →
`oisi.schema.json`; methods → `PIPELINE_METHODS.md`; compute/threads → `COMPUTE.md`;
cache → `INCREMENTAL_ANALYSIS_DESIGN.md`; geometry → `GEOMETRY.md`; interop →
`INTEROP_NWB.md`; UI → `UI_ARCHITECTURE.md`; validation → `VALIDATION_SCORECARD.md`.
