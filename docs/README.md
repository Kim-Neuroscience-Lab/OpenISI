# OpenISI Documentation

The documentation obeys the same rules as the code: **one source of truth per
concern (SSoT)**, and **each document owns exactly one concern (SoC)**. There is no
second doc covering the same thing; docs are named by *concern*, never by *phase* or
work item. If you find two docs describing one concern, that is debt — consolidate.

Start with **`PRINCIPLES.md`** (what OpenISI is, its invariants, and the objective
Definition of Done). Everything else is the *how* for one concern:

| Concern | Source of truth |
|---|---|
| **Contract** — what it is, invariants, Definition of Done | [`PRINCIPLES.md`](PRINCIPLES.md) |
| Tool-vs-domain enforcement (the contract's appendix) | [`TOOL_LEDGER.md`](TOOL_LEDGER.md) |
| **System architecture** — crates, the self-contained-core ↔ standard-at-the-boundary model, data flow | [`ARCHITECTURE.md`](ARCHITECTURE.md) |
| Compute & concurrency substrate (Burn backends, the threading/lock model) | [`COMPUTE.md`](COMPUTE.md) |
| Incremental re-analysis cache (the demand-driven Merkle DAG) | [`INCREMENTAL_ANALYSIS_DESIGN.md`](INCREMENTAL_ANALYSIS_DESIGN.md) |
| Analysis pipeline & scientific methods (per stage) | [`PIPELINE_METHODS.md`](PIPELINE_METHODS.md) |
| Rig geometry & visual-angle conventions | [`GEOMETRY.md`](GEOMETRY.md) |
| **`.oisi` data format** — the on-disk contract | [`oisi.schema.json`](oisi.schema.json) |
| Interoperability — NWB / DANDI export | [`INTEROP_NWB.md`](INTEROP_NWB.md) |
| Scientific-validation status (against the field's oracles) | [`VALIDATION_SCORECARD.md`](VALIDATION_SCORECARD.md) |
| UI architecture (the build-less vanilla-JS frontend) | [`UI_ARCHITECTURE.md`](UI_ARCHITECTURE.md) |
| Release roadmap & milestones (Alpha / Beta / v1) | [`ROADMAP.md`](ROADMAP.md) |
| Dev workflows (generated figures, etc.) | [`DEV_FIGURES.md`](DEV_FIGURES.md) |
| Upstream contributions (Burn / cubecl petitions) | [`upstream/`](upstream/) |
| Superseded / point-in-time documents | [`archive/`](archive/README.md) |

**Conventions.** The `.oisi` format contract (`oisi.schema.json`) is the machine
SSoT and is to be *generated* from the Rust schema source so it cannot drift; docs
cross-reference rather than restate (e.g. method docs cite `PRINCIPLES.md` for the
invariant, not re-argue it). When a doc completes its purpose or stops describing
reality, it moves to `archive/` with an entry there — it is not left to rot in the
live set.
