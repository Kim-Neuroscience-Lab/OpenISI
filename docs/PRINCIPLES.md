# OpenISI Principles & Definition of Done

This is the codebase's architectural contract. It states what OpenISI *is*, the
invariants every change is held to, and the objective gates that define "done."
It is not aspirational prose: the [Definition of Done](#definition-of-done) is a
checklist of commands and checks that each pass or fail. The goal is achieved
exactly when every gate is green — no judgment call.

`docs/TOOL_LEDGER.md` is this document's enforcement appendix (the tool-vs-domain
record). When the two disagree, fix the code, not the doc.

## Thesis

OpenISI is **the reference implementation of an ISI retinotopy pipeline** — a tool
another lab can adopt, trust the numbers from, extend, and exchange data with.
Spotless, pristine, modern, standardized, validated scientific software. Held to a
single bar: **100% correct and complete — exactly the right size and scope.** Never
"good enough," never a throwaway, and never enterprise-bloat. Both
under-engineering (hand-rolling a standard) and over-engineering (ceremony the
domain doesn't need) are failures against the same bar.

## Invariants

1. **Scientific faithfulness is inviolable.** A given config + input yields
   bit-identical output, forever. Every change is a serialization/format/structure
   change that must pass through the equivalence gate, never move the science.
   This is the backstop every other invariant defers to.

2. **Interoperability by standard, not by adapter.** Data leaves the tool in a form
   the field already reads (NWB / DANDI). Config and provenance are self-describing,
   schema-bearing JSON. "Standardized" means the *recognized* standard with its
   *reference* validator — not a bespoke format with a hand-written checker.

3. **Tool for infrastructure, hand-roll only the domain.** For any infrastructure
   responsibility — serialization, schema, validation-dispatch, reactivity
   transport, hashing, IO, parsing — use the established tool. Hand-roll *only* the
   logic a tool could supply solely by encoding this project's specific
   science/hardware (the predicate body, not the dispatch). Cargo-culting a
   mismatched tool is as much a failure as hand-rolling. This is the decision rule;
   `TOOL_LEDGER.md` is its running record. The custom surface must equal the domain
   surface — nothing tool-able hiding inside it.

4. **Make invalid states unrepresentable where the type system can express it;
   validate where it can't.** The template is the tagged enum that collapses
   `active_when` into the variant structure — a tunable cannot exist unless its
   method is selected. Prefer this wherever a type *correctly* expresses the
   invariant. Use runtime validation (e.g. garde) only for what types genuinely
   cannot — dynamic hardware-driven constraints. Sized by correctness, not by code
   cost in either direction: not maximal-type-level for its own sake, not a runtime
   shortcut to save typing.

5. **Bounded, justified custom surface.** "Architecturally debt-free" has a precise
   meaning: every remaining custom island is named and justified in
   `TOOL_LEDGER.md`, and the custom surface equals the domain surface. Zero
   tool-able code is the target; zero custom code is not.

6. **Pristine at every step.** No commit until pristine; commits land on `main` when
   asked; never push without asking. Each landed change is tested and clippy-clean
   workspace-wide.

## Definition of Done

The codebase reaches its goal when **all** of the following are green. Until then,
work continues.

### Faithfulness
- [ ] `regression_oisi` is **bit-identical** (`cargo test -p isi-analysis --include-ignored`, ~98s).
- [ ] All per-stage goldens and synthetic-ground-truth pipeline tests pass.

### Standardized config & params (Phase 3)
- [ ] Config and params are typed serde structs with `schemars`-derived JSON Schema and `garde` validation.
- [ ] `active_when` is collapsed into `#[serde(tag="method")]` tagged enums (invalid method+tunable combinations are unrepresentable).
- [ ] Dynamic hardware constraints are the *only* runtime-validated config (garde `Context`), justified in `TOOL_LEDGER.md`.
- [ ] Config files are JSON (`config/*.json`); no TOML remains in the config/param path.
- [ ] The `define_params!` macro DSL and the dead modules it subsumes (`toml_io`, `param_json`, `snapshot`, `definitions`, the transitional `bridge`) are **deleted** — proven by grep returning nothing.
- [ ] The IPC/UI is driven by the schemars-derived schema, not `PARAM_DEFS`.

### Interoperable data format (Phase 4)
- [ ] The native `.oisi` layout is NWB-aligned; existing files migrate forward.
- [ ] An `ndx-openisi` NDX extension exists for what core NWB cannot hold, and is **published to the NDX catalog**.
- [ ] `export_nwb` output passes **`nwbinspector` clean**.
- [ ] Exported files round-trip through `pynwb` (and MatNWB) with all maps/metadata intact.
- [ ] A sample dataset is **accepted by DANDI validation**.

### Tool-vs-domain & rigor
- [ ] `TOOL_LEDGER.md` reconciles: every custom island named + justified; no tool-able code in the custom surface.
- [ ] `cargo test --workspace` green; `cargo clippy --workspace --all-targets` **zero warnings**.

## Status

Tracked in the standardization roadmap (plan file) and the project memory entry
`standardization-roadmap-2026-06-13`. Phase 3 foundation is built and green;
remaining work is the coordinated consumer switch (Phase 3) then NWB conformance
(Phase 4). All uncommitted by policy until the entire goal is pristine.
