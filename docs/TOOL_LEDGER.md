# Tool-vs-Domain Ledger

The enforcement appendix to `PRINCIPLES.md` Invariant 10 ("Tool for infrastructure;
hand-roll only the domain"). It records the running answer to: *is every remaining
piece of custom code genuine domain, or is something tool-able hiding in it?*

**Decision rule.** For any *infrastructure* responsibility, use the established tool.
Hand-roll **only the domain** — the logic a tool could provide only by encoding this
project's specific science/hardware. The test:

> Am I reimplementing serialization / schema / validation-dispatch /
> reactivity-transport / hashing / IO / parsing? → **use the tool.**
> Am I writing the domain rule the tool *invokes* (e.g. "exposure ≤ this camera's
> reported max")? → **that's the app**, and no tool can supply it.

Cargo-culting a mismatched tool (an untyped KV store where typed structs are the
point; a TS-binding generator for a build-less JS frontend) is as much a failure as
hand-rolling. Don't pull a new dependency where an existing one already serves.

---

## State (2026-06-15)

The two tool-able hand-rolled islands the earlier sweep identified are **both
resolved**, and the codebase is now fully tool-based. What remains custom is all
justified domain.

1. **Param system — RESOLVED.** The `define_params!` macro DSL + manual
   serialization/validation/overlay/observer is **deleted**. It is now typed `serde`
   structs + `schemars`-derived JSON Schema + `garde` validation (incl. context-aware
   for dynamic hardware constraints) + `json-patch` (RFC 7386) sparse overlay +
   Tauri-events reactivity (`crates/openisi-params/src/config`). The error-code
   contract is `strum`-derived and generated into the frontend (`error-codes.generated.js`).
2. **`.oisi` format — REFRAMED, not a hand-roll to eliminate.** `.oisi` is the
   instrument's **native working format** (mutable, incremental, pure-Rust,
   self-contained) — a justified domain format, *not* a failure to standardize.
   Interoperability is met by **exporting to NWB/DANDI via the reference tools**
   (`pynwb` / `nwbinspector` / `dandi`) at a deliberate boundary, with `ndx-openisi`
   for what core NWB cannot hold. See `ARCHITECTURE.md` (core ↔ boundary) and
   `PRINCIPLES.md` Invariants 6 & 9. The remaining work on it is *structural* (one
   owned I/O boundary + a schema SSoT), not tool-vs-domain.

## Already tooled

`serde`/`serde_json` (serialization), `schemars` (derived JSON Schema), `garde`
(validation), `json-patch` (RFC 7386 merge), `thiserror` + `strum` (typed errors +
stable enum-string codes), `tracing` (logging), `clap` (CLI), `tauri` (IPC, events,
path resolution), `crossbeam-channel` (channels), `parking_lot` (locks), `blake3`
(content hashing), `hdf5-metno` + Fletcher32/gzip (HDF5 + integrity), `ndarray` /
`num-complex` / `ndarray-stats` (numerics), `petgraph` (DAG), `rayon` (parallelism),
`burn-*` (tensor compute, runtime-dispatched CPU/CUDA), `wgpu` / `glyphon`
(rendering), `png` (encoding), `libloading` (DLL FFI), `flate2`, `bytemuck`, `rand`,
`pollster`. Export boundary: `pynwb` / `nwbinspector` / `dandi` (the NWB reference
toolchain).

## Justified hand-rolls — keep (no tool fits, or an ecosystem gap)

| Hand-roll | Why it stays |
|---|---|
| The `.oisi` native working format (`isi-analysis/src/io.rs`, `export.rs`) | The instrument's mutable, incremental, self-contained working store. NWB is an archive/interchange format and there is no Rust NWB writer; standardization is met by *exporting* to it, not adopting it as the native format. Domain. |
| MATLAB v5 `.mat` SNLC importer (`mat5.rs`) | No production-grade Rust v5 reader (cell-arrays/structs/compression). Legacy one-way import; genuine ecosystem gap. |
| Param/`.oisi` migration (`migrate.rs`) | JSON tree reshaping + named legacy renames; DB-migration frameworks don't apply. Domain, well-tested. |
| Incremental-cache Merkle DAG (`pipeline/fingerprint.rs`) | `blake3` *is* the hashing tool; the **persist-across-sessions** cache is domain — `salsa`/build systems are in-memory only. |
| MapMeta render hints, timestamp unification, clock-drift forensics, per-frame QA, atomic-HDF5-export protocol | Domain provenance/visualization contracts; no off-the-shelf equivalent. |
| PCO FFI, win32/wgpu/DXGI stimulus, EDID parsing, ISI timing math, the camera/display abstraction | Platform/hardware domain. |

## Open structural debt (not tool-vs-domain — tracked in PRINCIPLES' frontier)

Some justified-domain code is currently *organized* poorly (the HDF5 boundary is
duplicated across `io.rs` + `export.rs`; error sources are stringified at scattered
sites). These are **SoC / SSoT** issues — one owned I/O boundary, sources preserved
once — not tool substitutions. They are the I/O-architecture frontier in
`PRINCIPLES.md`, not entries for this ledger.
