# Tool-vs-Domain Ledger

**Decision rule.** For any *infrastructure* responsibility, use the established
tool. Hand-roll **only the domain** — the logic a tool could provide only by
encoding this project's specific science/hardware. The test for whether a piece
is a hand-rolling failure:

> Am I reimplementing serialization / schema / validation-dispatch /
> reactivity-transport / hashing / IO / parsing? → **use the tool.**
> Am I writing the domain rule the tool *invokes* (e.g. "exposure ≤ this
> camera's reported max")? → **that's the app**, and no tool can supply it.

Cargo-culting a mismatched tool (e.g. an untyped KV store where typed structs are
the point) is as much a failure as hand-rolling. Don't pull a new dependency
where an existing one already serves.

---

## Conclusion (comprehensive sweep, 2026-06-13)

A full sweep of every infrastructure responsibility across all crates found the
codebase is **already overwhelmingly tool-based**. There are **exactly two
tool-able hand-rolled islands**, and **both are already on the standardization
roadmap**:

1. **Param system** — the `define_params!` macro DSL + manual
   serialization/validation/overlay/observer (`crates/openisi-params`). →
   **Phase 3**: serde structs + schemars (derived JSON Schema) + garde (incl.
   context validation) + serde `Option` overlay + a watch/Tauri-events reactive
   layer. *(This also subsumes the config TOML→JSON change for free.)*
2. **`.oisi` private HDF5 schema** — a bespoke format where NWB is the field
   standard. → **Phase 4**: NWB-aligned layout + an `ndx-openisi` extension +
   reference-validated (`pynwb`/`nwbinspector`) export.

**Everything else is already-tooled or genuine domain.** No other splits remain
to catch.

---

## Already tooled (the ~95%)

`serde`/`serde_json` (serialization), `thiserror` (errors), `tracing`
(logging), `clap` (CLI), `tauri` (IPC, events, path resolution),
`crossbeam-channel` (channels), `parking_lot` (locks), `blake3` (hashing),
`hdf5-metno` + Fletcher32/gzip filters (HDF5 + integrity), `ndarray` /
`num-complex` / `ndarray-stats` (numerics), `petgraph` (DAG), `rayon`
(parallelism), `burn-*` (tensor compute), `strum` (enum strings/iteration),
`wgpu` / `glyphon` (rendering), `png` (encoding), `libloading` (DLL FFI),
`flate2` (zlib), `bytemuck`, `rand`, `pollster`.

## Justified hand-rolls — keep (no tool fits, or an ecosystem gap)

| Hand-roll | Why it stays |
|---|---|
| MATLAB v5 `.mat` SNLC importer (`mat5.rs`) | No production-grade Rust v5 reader (`matlab_io`/`mat4-rs` inadequate for cell-arrays/structs/compression). Legacy one-way import; genuine ecosystem gap. |
| Param migration (`migrate.rs`) | JSON param-tree reshaping + named legacy renames; DB-migration frameworks don't apply. Domain, well-tested. |
| Incremental-cache Merkle DAG (`pipeline/fingerprint.rs`) | `blake3` *is* the hashing tool; the **persist-across-sessions** cache is domain — `salsa`/build-systems are in-memory only. |
| MapMeta render hints, timestamp unification, clock-drift forensics, per-frame QA metrics, atomic-HDF5-export protocol | Domain provenance/visualization contracts; no off-the-shelf equivalent. |
| PCO FFI, win32/wgpu/DXGI stimulus, EDID parsing, ISI timing math | Platform/hardware domain. |

## Note on the param sub-pieces

The audits flagged smaller param-system hand-rolls (dotted-path tree walking,
two-layer overlay, batch queue, the `ParamChangeObserver`). These are **not**
patched piecemeal: Phase 3 replaces the whole *runtime-registry* design with
typed serde structs, at which point serde/schemars/garde subsume them and they
cease to exist. Evaluating them in isolation ("not worth a dep") misses that the
design itself is what changes.

*Out of scope here:* the frontend (webview) — but Phase 3's schemars-derived JSON
Schema is exactly what a standard form generator consumes, so it standardizes
that boundary too.
