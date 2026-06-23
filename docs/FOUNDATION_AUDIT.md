# Foundation audit (2026-06-16)

> **Dated point-in-time snapshot.** Findings, file:line citations, and commit
> SHAs below record the state at the 2026-06-16 audit commit (`3ca1582`) and are
> preserved as-is. Some paths/identifiers have since moved (notably the `.oisi`
> schema + I/O were extracted into the `oisi` crate on 2026-06-19, `9736718`);
> such drift is flagged inline with "(now …)" notes and does **not** invalidate
> the finding. Line numbers are as-of the snapshot and are not re-synced.

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

**FIXED** (`e74362c`): `io::atomic_update(path, mutate)` — copy → mutate temp →
fsync → atomic rename — now wraps `analyze()`'s entire write block. A crash
leaves the original intact; output is byte-identical (regression bit-identical).
Locked by two crash-safety tests in `io.rs`. (Now: `atomic_update` and those
two tests live in `crates/oisi/src/io.rs` after the oisi-crate extraction;
`analyze()` still calls `io::atomic_update` at `lib.rs:475`.) The provenance `/analysis_params`
stamp was a residual separate in-place write; **also FIXED** (`e932f19`) by
folding it into the same transaction via `analyze`'s `params_tree` argument.

Secondary, still open (lower severity): 2D `/results` datasets lack the
`fletcher32` checksums that 1D arrays and acquisition frames get (`io.rs`
`write_f64`/`write_mask` — since refactored into `write_results`; cf.
`write_checked_1d`, now in `crates/oisi/src/io.rs`) — a corruption-*detection* gap,
not a corruption-*creation* one; disk-full is surfaced (it propagates as a typed
HDF5/Io error) but not specially classified. See "Residual risk" below.

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
  `oisi_schema.rs` (`SCHEMA`) (now `crates/oisi/src/schema.rs` post-extraction),
  `docs/oisi.schema.json` is generated under a golden test, and a contract test
  checks real files both ways. The render contract (`io/meta.rs`) is a
  *parallel* per-dataset source keyed by name — a minor cleanliness item (fold
  into the `SCHEMA` SSoT), not a foundational gap.

## Status

A1 ✅ (`e74362c` + residual `e932f19`) · B1–B3 ✅ (`3011f99`) · C1–C3 audited
green (no change). All foundation fixes are committed, green-gated, and
bit-identical-preserving.

## How to verify the foundation (successor checklist)

Run from the repo root; all must pass before trusting a change:

```sh
cargo test --workspace                                                  # all green
cargo clippy --workspace --all-targets                                  # zero warnings
cargo test -p isi-analysis --test regression_oisi -- --include-ignored  # BIT-IDENTICAL (~100s)
cargo test -p isi-analysis --test equivalence                           # re-analyze R43, all leaves
cargo xtask goldens --check                                             # oracle goldens reproducible
```

Per-finding evidence the fix holds:
- **A1 atomicity** — `cargo test -p oisi --lib atomic_update` (snapshot-era:
  `-p isi-analysis`, before the oisi-crate extraction) proves a
  failed mutation leaves the original byte-for-byte intact and cleans the temp,
  and a success publishes atomically. The bit-identical `regression_oisi` proves
  the atomic path didn't move the science.
- **B-tier** — these are error-path changes (a panicked worker is now logged, a
  save-dir failure surfaces with its path, a corrupt fingerprint errors instead
  of being read as a cache miss); covered by the io + incremental suites.
- **C1 re-entrancy** — `cargo test -p openisi stimulus_thread` runs the 14
  lifecycle/reset tests. **C2 determinism** — `regression_oisi` passing
  bit-identical run-to-run *is* the within-backend determinism proof.

## Residual risk (known, accepted — not silent)

- **`fletcher32` on 2D `/results`.** Detection-only gap: a post-write disk bit-rot
  in a result map isn't checksum-flagged on read (1D arrays + frames are). It does
  not *create* corruption. Add it to the 2D result writes in `write_results`
  (snapshot-era `write_f64`/`write_mask`) if/when result-map integrity-on-read
  is required.
- **`capture_baseline` strip-then-reanalyze** does an in-place
  `strip_derived_outputs` before the (atomic) analyze. The window is **dev/baseline
  only** — `capture_baseline` regenerates a throwaway baseline; production
  `analyze()` never strips. Make it atomic too if baselines become precious.
- **Cross-backend discrete-flip** (C2): a segmentation bit-exact output *could*
  differ between CPU and CUDA at a numerically degenerate pixel. CPU is canonical
  (`PRINCIPLES.md` → Platform); `bit_exact` is asserted same-backend.
- **Live-rig items** (out of code scope): re-entrancy 2-run on real hardware;
  ring-calibration UI verification.

C-tier is documented above, not changed; the residuals are recorded here so a
successor inherits the *known* risk surface rather than discovering it.
