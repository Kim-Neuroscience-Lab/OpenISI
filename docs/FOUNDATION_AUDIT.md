# Foundation audit (2026-06-16)

A bedrock audit of the senior-engineer-required invariants, before stacking more
features on top. Each finding is evidence-ordered (file:line) and tiered:
**A** = real foundational gap to fix, **B** = smaller "no silent failure" gap,
**C** = audited and found sound (documented so a successor knows it was checked,
not skipped). Claims here were verified against source, and two sub-agent
overstatements were corrected (noted inline) — do the same before trusting any
line.

## A — fix (bedrock-up, highest blast radius first)

### A1 · Analysis writes mutate the `.oisi` in-place, non-atomically
`analyze()` (`crates/isi-analysis/src/lib.rs:475-503`) performs 4–5 separate
`io::write_*` open→write→close cycles directly on the live file
(`io.rs` `open_readwrite`). HDF5 B-tree/superblock updates are not atomic, so a
crash / power-loss / disk-full **mid-write can corrupt the whole file** — which
also holds the (often irreplaceable) raw `/acquisition` frames. Witnessed this
session: the `capture_baseline` strip-then-reanalyze sequence left a stripped,
result-less baseline when migration failed mid-way.

The fix already exists in-repo and is *not* used here: the **acquisition** writer
(`src-tauri/src/export.rs:482`) writes to a `.partial` temp + flush + atomic
rename. The analysis path should update via the same copy-temp → mutate →
fsync → atomic-rename discipline, so a failed write leaves the original intact.

Verification note: a sub-agent claimed "fingerprint persists before data → cache
silently restores garbage." **False** — the write order is data-first
(`lib.rs:476` complex_maps → `477` projection fp; `484` results → `501` tail
fps), so a crash yields a *missing* fingerprint (→ safe recompute), never a
premature one. The real risk is whole-file corruption from non-atomic in-place
mutation, not stale-cache garbage.

Secondary (same area): 2D `/results` datasets lack the `fletcher32` checksums
that 1D arrays and acquisition frames get (`io.rs` `write_f64`/`write_mask` vs
`write_checked_1d`); disk-full is not surfaced as a distinct, actionable error.

## B — smaller "no silent failure" gaps

- **B1** `src-tauri/src/analysis_thread.rs:45,52` — `let _ = handle.join()`
  swallows a panicked analysis thread; the operator never learns the run died
  abnormally. Log/propagate the join error.
- **B2** `src-tauri/src/commands/acquire.rs:206` — `let _ =
  std::fs::create_dir_all(...)` masks a save-directory failure; the user then
  sees a confusing downstream "file not found / rename failed" instead of the
  real cause (permissions / disk-full). Surface it.
- **B3** `crates/isi-analysis/src/io.rs:280` — a corrupt stage fingerprint is
  silently treated as a cache miss. Safe (recompute) but undiagnosed; overlaps
  A1. Trace it at least.
- Not a bug: the 13× `let _ = app.emit(...)` in `events.rs` are fire-and-forget
  by design (emit fails ⇒ the frontend is already gone). Worth a consistency
  pass to `trace!` on failure, not a correctness fix.

## C — audited, sound (no fix; recorded so it isn't re-audited blindly)

- **C1 · Re-entrancy / restart.** Structurally sound in code: the stimulus
  thread is spawn-once-reuse; `DropMonitor::reset()` runs on every
  `StartAcquisition`; the catastrophic-drop abort no longer `return`s out of the
  thread (the 0e03d05 root cause) but finalizes and `continue`s. 14 unit tests
  cover the reset + the phantom-gap failure mode
  (`src-tauri/src/stimulus_thread.rs:1618-1726`). Remaining gap is **live-rig
  2-run** confirmation (hardware, out of code scope).
- **C2 · Cross-backend determinism.** No output-affecting `HashMap`/`HashSet`
  iteration and no unseeded RNG were found; within-backend reductions are
  fixed-order (why the equivalence test passes bit-exact run-to-run). A sub-agent
  flagged many "nondeterminism" sites, but all are **cross-backend** float-drift
  that *could* flip a discrete decision — a known, documented limitation
  (`tolerances.toml` acknowledges cross-backend drift; `bit_exact` is asserted
  same-backend only), not a within-run regression. No fix; the caveat is the
  documented contract.
- **C3 · Schema drift.** Solved: the `.oisi` layout is declared once in
  `oisi_schema.rs` (`SCHEMA`), `docs/oisi.schema.json` is generated under a
  golden test, and a contract test checks real files both ways. The render
  contract (`io/meta.rs`) is a *parallel* per-dataset source keyed by name — a
  minor cleanliness item (fold into the `SCHEMA` SSoT), not a foundational gap.

## Fix order

A1 → B1/B2/B3 → (resume oracle-coverage gaps). Each fix is its own green-gated
commit; `regression_oisi` stays bit-identical (these change serialization/
error-paths, never the science). C-tier is documented above, not changed.
