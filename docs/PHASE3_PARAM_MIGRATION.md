# Phase 3 — Param-system consumer switch (line-level sub-plan)

Goal & gates: `docs/PRINCIPLES.md` → DoD "Standardized config & params". This is the
**coordinated consumer switch** that replaces the `define_params!` registry with the
typed serde config that already exists (`crates/openisi-params/src/config/`). The
foundation (typed structs + loader + equivalence proofs) is built and green. This
doc is the remaining work, sequenced so the workspace compiles + tests stay green at
every slice, with `regression_oisi` bit-identical as the end gate.

## Target architecture (the end-state, per PRINCIPLES invariants 3 & 4)

- **Live store** `ConfigStore` (new, in `openisi-params`) **replaces `Registry`** in
  `AppState`. Holds the three typed configs live: `rig: RigConfig`,
  `experiment: ExperimentConfig`, `analysis: AnalysisConfig`, plus `hardware:
  HardwareContext`, `shipped_dir`/`user_dir`, and the change `observer`. Methods:
  `load_*`/`save_*` (via `config::loader`), `inject_hardware`, `set`/`merge` (apply a
  sparse JSON overlay from the UI → garde-validate → clamp to dynamic hardware bounds
  → emit change event), and the computed accessors (geometry/luminance/sweep).
- **Snapshot = the typed structs themselves.** `RigConfig`/`ExperimentConfig`/
  `AnalysisConfig` are `Clone + Send + Sync + Serialize`, so thread messages
  (`AnalysisRequest`, `AcquisitionCommand`, `PreviewCommand`) carry a small
  `ConfigSnapshot { rig, experiment, analysis, hardware }` bundle **instead of
  `RegistrySnapshot`**. Deletes `snapshot.rs`.
- **Provenance = serde.** `.oisi` writes `serde_json::to_value(&rig/experiment/
  analysis)` directly; reads `serde_json::from_value` (sparse-merge for forward-compat
  handled by `#[serde(default)]`, strictness by `deny_unknown_fields`). Deletes
  `to_json_for_target` / `from_json_tree` dotted-path walking and `param_json.rs`.
- **Analysis consumes `AnalysisConfig` directly (UNIFY).** Move the per-stage method
  enums into `openisi-params` as the tagged enums (already done in `config/analysis.rs`);
  `isi-analysis` matches on them directly in each `methods/*` stage. **Deletes
  `bridge.rs`** and `analysis_kinds.rs`/`registry_param.rs` (`Tagged`). Re-verify the
  Merkle fingerprint covers every tunable after the switch.
- **Dynamic hardware constraints = garde `Context` + a thin clamp.** The only
  genuinely-custom validation: `RigConfig::validate_with(&HardwareCtx)` enforces
  exposure/binning/fps/stimulus-width against live hardware; `ConfigStore::merge`
  clamps on set + on `inject_hardware`. This is the one justified custom predicate
  (record in TOOL_LEDGER.md). Replaces `constraints.rs` `DynamicConstraint`.
- **IPC = the schemars schema.** `get_param_descriptors` → serve
  `schema_for!(RigConfig/ExperimentConfig/AnalysisConfig)` (cached in `LazyLock`) +
  current values + which-fields-are-hardware-bounded. `set_params` → apply overlay via
  `ConfigStore::merge`. `get_analysis_stages` → derive from the AnalysisConfig schema's
  `oneOf`. The JS form-gen (`ui/src/param-form.js`) consumes the schema (`oneOf` for
  tagged-enum method selection) instead of the bespoke descriptor list.

## Slice order (each ends green: `cargo test --workspace` + `cargo clippy`)

- **3a — `ConfigStore` + `ConfigSnapshot`** (additive; registry still present).
  Build the store, computed accessors (port `computed.rs`: geometry/luminance/sweep,
  taking `&RigConfig,&ExperimentConfig,&HardwareContext`), garde `Context` validation
  + clamp. Unit-test against the registry's numbers (reuse `computed.rs` test values).
- **3b — Analysis consumes `AnalysisConfig`** (UNIFY). Each `methods/*` stage matches
  the tagged enum directly; delete `bridge.rs`. Re-pin fingerprint coverage. Gate:
  `regression_oisi` bit-identical. Highest-value slice — proves the science is intact.
- **3c — Thread messages carry `ConfigSnapshot`.** Switch `messages.rs`,
  `analysis_thread.rs`, `commands/analysis.rs`, acquisition/preview producers; export
  accumulator + `.oisi` write/read use serde provenance. Delete `snapshot.rs`,
  `param_json.rs`.
- **3d — `AppState` holds `ConfigStore`.** Switch `state.rs`, `lib.rs` setup/shutdown,
  headless `main.rs`, every `commands/*.rs` `reg.foo()` call site to typed field
  access. Convert `config/*.toml` → `config/*.json` (+ `config/dev/*` overlays).
- **3e — IPC + frontend.** Rewrite `params/commands.rs` onto the schemars schema +
  `ConfigStore::merge`; port `ui/src/param-form.js` + views to the schema (`oneOf`).
- **3f — `migrate.rs`.** Old-`.oisi` param-provenance migration onto the typed structs
  (pre-2026 path already exists; adapt payload).
- **3g — Delete the macro & dead modules.** Remove `macros.rs`/`define_params!`,
  `definitions.rs`, `constraints.rs` (static half folds into garde attrs),
  `analysis_kinds.rs`, `registry_param.rs`, `registry.rs`, `toml_io.rs`,
  `computed.rs`, `labels.rs`. Grep proves `define_params!`/`PARAM_DEFS`/`RegistrySnapshot`
  return nothing. Final gate: `regression_oisi` bit-identical + clippy 0 workspace-wide.

## Invariants for this phase
- Bit-identical `regression_oisi` after 3b and at the end (it's serialization, not math).
- No commit until the entire standardization (Phase 3 + 4) is pristine — one commit.
- Each slice independently compiles + tests green; never land a half-flipped state.
