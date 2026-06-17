# Onboarding — getting productive on OpenISI

For a developer taking over or contributing — including someone newer to Rust or to
this codebase. Read `CONTRIBUTING.md` and `PRINCIPLES.md` first (what OpenISI is, the
invariants, the Definition of Done); this is the practical tour on top of them.

## Build & run

Setup, build, and the sample-data quickstart are in the root [`README.md`](../README.md).
In short: run the setup script, then `cargo tauri dev` for the app. The headless CLI
lives in `src-tauri/src/bin/headless` (`cargo run -p openisi --bin headless -- <cmd>`).
The first build is slow (it compiles HDF5, wgpu, Tauri); later builds are fast.

## The safety net is your most important tool

The tests and the type system are the senior engineer you may not have. They are how a
less-experienced maintainer changes this code safely. Before **and** after any change:

```sh
cargo test --workspace                                                   # all green
cargo clippy --workspace --all-targets                                   # zero warnings
cargo test -p isi-analysis --test regression_oisi -- --include-ignored   # BIT-IDENTICAL
```

The last one is load-bearing: if you touch the analysis pipeline, the params it reads,
or the `.oisi` I/O, the scientific output **must not move**. If `regression_oisi` goes
red, you changed the science — revert, or, if you *meant* to, update the golden and say
so explicitly in the change. If you can't explain why a change is safe, the net can.
Trust it; never bypass it.

If you touch an **oracle golden** (the reference fixtures in
`crates/isi-analysis/tests/golden/`), regenerate and re-check it through the harness
instead of editing a `.bin` by hand:

```sh
cargo xtask goldens --check    # regenerate every fixture from its oracle, diff vs committed
cargo xtask goldens <name>     # regenerate just the matching generators (then commit the .bin)
```

One-time toolchain setup (Octave + a pinned Python env) is in
[`../tools/golden/README.md`](../tools/golden/README.md). The app/release build needs
none of it — only this dev harness does.

## The map

| Crate | What it is |
|---|---|
| `crates/openisi-stimulus` | Stimulus design + the wgpu/WGSL renderer |
| `crates/openisi-params` | Typed config (serde + schemars + garde) — the parameter SSoT |
| `crates/isi-analysis` | The analysis pipeline **and** the `.oisi` format I/O (`io.rs`) |
| `crates/pco-sdk` | PCO camera FFI (libloading over the vendor DLLs) |
| `src-tauri` | The app: acquisition orchestration, threads, IPC, the headless CLI, capture-write (`export.rs`) |

Full structure + dependency direction: [`ARCHITECTURE.md`](ARCHITECTURE.md). Compute and
the threading/lock model: [`COMPUTE.md`](COMPUTE.md).

## Where to add things

- **A new analysis method** → a tagged-enum variant in `crates/isi-analysis/src/methods/`,
  plus a golden test against its reference. See [`PIPELINE_METHODS.md`](PIPELINE_METHODS.md).
- **A new camera** → implement `trait Camera` (the abstraction); the PCO impl is the
  pattern. See [`ARCHITECTURE.md`](ARCHITECTURE.md).
- **A new config parameter** → the typed structs in `openisi-params::config`. The UI
  descriptors and the frontend error-code catalog are *generated* — don't hand-edit them.

## Gotchas

- **The pipeline is incremental** (a Merkle cache inside the `.oisi`). Changing a stage's
  inputs invalidates it and everything downstream — see [`INCREMENTAL_ANALYSIS_DESIGN.md`](INCREMENTAL_ANALYSIS_DESIGN.md).
- **No silent failures.** Errors surface as typed, source-preserving values. Do not
  `.ok()` / `unwrap_or_default()` an I/O read whose absence would change a result — fail
  loud or handle it explicitly.
- **Acquisition is Windows-only today** (PCO + DXGI vsync + QPC timing); analysis and
  visualization are cross-platform.
- **The `.oisi` carries a format version**; `analyze()` refuses a file whose version it
  doesn't recognize rather than misreading it.
- **CPU is canonical** for reproducible results; GPU (CUDA) is optional acceleration
  within a characterized bound (see `PRINCIPLES.md` → Platform).

## The why

`PRINCIPLES.md` is the contract: what OpenISI is, the invariants every change must hold,
and the objective Definition of Done. When a change feels like it's fighting the
structure, re-read the relevant concern doc (the index is [`README.md`](README.md)) —
the structure is deliberate.

For the **foundation state** — what the senior-required invariants are (crash/disk-full
integrity, error surfacing, re-entrancy, determinism, schema drift), why they matter, how
each was verified, and the known residual risk you inherit — read
[`FOUNDATION_AUDIT.md`](FOUNDATION_AUDIT.md). It also has the exact commands to re-verify
the foundation holds after your change.
