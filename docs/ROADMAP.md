# OpenISI Roadmap

The release trajectory. *What* OpenISI must be and the objective gates that define
"done" live in [`PRINCIPLES.md`](PRINCIPLES.md) (the contract + Definition of Done);
this doc is the *sequence* — the milestones the field will adopt against. Detailed
in-flight engineering work is tracked in the project plan + memory, not here.

## Milestones

- **Alpha — our lab uses it daily.** OpenISI runs real experiments on our rig;
  acquisition is reliable and the data is trustworthy and complete; analysis
  reproduces known results. *Bar:* the scientific-correctness, instrument, and
  data-integrity invariants are green (`PRINCIPLES.md`).
- **Beta — another lab can download and use it.** A self-contained, signed installer;
  zero-setup acquisition + analysis for a non-developer; data exports cleanly to
  NWB/DANDI; published alongside the paper. *Bar:* the self-containment, stewardship
  (docs/installers), and interoperability gates are green.
- **v1.0.0+ — community-driven.** Multiple platforms, additional cameras/displays via
  the hardware abstraction, and features requested by adopting labs. *Bar:* the
  extensibility and openness invariants are green; the platform-reach trajectory
  (cross-platform acquisition, cross-vendor GPU) is delivered as demand warrants.

## Where we are

The foundations are in place and validated: the typed config/param system, the
analysis pipeline with the bit-identical regression gate and oracle validation, the
incremental cache, and reference-validated NWB/DANDI export. The codebase is **between
Alpha and Beta**.

The current frontier — the work that carries it to a shippable Beta — is the
**instrument-architecture program** in `PRINCIPLES.md`'s Definition of Done: the
single owned I/O boundary + schema-SSoT, `thiserror` source-preservation, cache/data
separation, **self-containment (bundling the camera DLLs + the export runtime)**, the
camera/display abstraction, and the stewardship gates (docs, observability, semantic
versioning, supply-chain, signed installers).
