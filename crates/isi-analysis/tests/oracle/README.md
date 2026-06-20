# Genuine-oracle validation harness

This is the **validation instrument**: OpenISI's correctness claims are proven by
comparison against the field's *genuine reference code, executed live* in a
per-oracle locked environment — never against our own re-reading of a method.
A green oracle gate is therefore independent proof of faithfulness, not a tautology.

See the pinned goal: *An independently-validated, reproducible correctness
foundation*.

## How it works

- **Per-oracle, locked, isolated environments.** Each reference codebase has its own
  directory here with a pinned, reproducible environment the harness provisions —
  never the machine's ambient toolchain.
  - `nat/` — **NeuroAnalysisTools 3.1.0** (Allen/Zhuang retinotopy), Python, via
    [`uv`](https://docs.astral.sh/uv/): `pyproject.toml` + committed `uv.lock`
    pin Python 3.10 + a period-correct stack (numpy 1.23.5, scipy 1.9.3,
    scikit-image 0.19.3, …). In that era `np.int` exists, so the vendored
    reference runs **natively — no shim**.
  - `snlc/` *(pending)* — SNLC/Garrett MATLAB, via a pinned Octave.
- **The reference code is byte-pristine.** Nothing here modifies
  `reference/…`; the env is shaped to fit the reference, not the reverse.
- **Live, not frozen.** `bridge.py` (per oracle) is a pure caller — it marshals
  arrays across the process boundary and a dispatch table maps a function id to a
  **direct call of the genuine reference function** (module functions and class
  methods, the latter constructed in the bridge and driven as the real method).
  No per-golden scripts, no committed output fixtures: the oracle is computed on
  every run.
- **Rust side.** `test_support::oracle::{nat, nat_raw}` invokes the bridge through
  `uv run --project <oracle>`. The validation tests live in-crate (they need
  crate-internal compute), gated behind the `oracle_live` Cargo feature, so the
  default `cargo test` needs no interpreter.

## Running

```
# requires `uv` on PATH (or OPENISI_UV=<path to uv>)
cargo test -p isi-analysis --features oracle_live genuine_nat -- --nocapture
```
The locked env is provisioned automatically by `uv` from the committed `uv.lock`,
so a clean machine / CI reproduces the same result.

## Irreducible gaps (stated, never assumed away)

- **Period-correct reconstruction, not the authors' exact machine.** NAT's
  `requirements.txt` is *unversioned*, so the locked env is a reconstruction from
  the code's hard constraints (`np.int` ⇒ numpy<1.24) + the 3.1.0 era — defensible
  and reproducible, but not provably the authors' exact toolchain (which is
  unknowable). Pins live in `nat/pyproject.toml` / `nat/uv.lock`.
- **Octave ≈ MATLAB, not identical.** The SNLC reference is MATLAB; we execute it
  via Octave (the only open, scriptable runtime). Octave's IPT functions match
  MATLAB's to high precision but are not bit-identical. This will be flagged
  per-method when `snlc/` lands.

## Divergence ledger

Every difference between OpenISI and the genuine oracle is recorded here — fixed in
OpenISI, or stated as a justified deviation. No loosened tolerances, no cropping, no
skipped cases.

| Method | Oracle | Result | Note |
|---|---|---|---|
| `visualSignMap` (VFS) | NAT `RM.visualSignMap` | agree to 1.3e-5 (interior) | Discretization-order difference: our chain-rule phasor gradient vs Allen's `np.gradient`-of-wrapped-phase. Within the documented `5e-2` discretization tolerance; ours is the more wrap-stable variant (separate wrap test). **Not a defect.** |
| `dilationPatches2` | NAT `RM.dilationPatches2` | bit-identical (0 px) | — |
| `_getRawPatchMap` | NAT `RetinotopicMappingTrial._getRawPatchMap` | bit-identical (0 px) | Driven as the genuine class method (signMapThr=0.5 reproduces our binary input). |
| `eccentricityMap` | NAT `RM.eccentricityMap` | agree to f64 machine precision | — |
| `is_adjacent` | NAT `core.ImageAnalysis.is_adjacent` | bit-identical (all pairs × border-widths) | — |
| `_getDeterminantMap` | NAT `RetinotopicMappingTrial._getDeterminantMap` | agree to f32 precision (`\|det J\|`) | Driven as the genuine class method. f32 gradient + det-cancellation vs genuine f64. |
| `localMin` | NAT `RM.localMin` | bit-identical (integer marker maps) | — |
| `getSigmaArea` | NAT `Patch.getSigmaArea` | bit-identical incl. NaN cases | The audit *suspected* a NaN-handling divergence; the live oracle **disproved** it — ours propagates `0·NaN = NaN` (NaN outside the mask) exactly like genuine numpy. No divergence. |
| `getPatchSign` (signs) | **SNLC** `getPatchSign` (Octave) | region-wise identical (non-zero-mean) | **Documented deviation, zero-mean only:** MATLAB `sign(mean)=0` gives an *undefined* patch sign at exactly zero mean; ours takes a deterministic `+1` tie-break. Justified (a patch must get a sign). Separately: our `label_4conn` is row-major, MATLAB `bwlabel` column-major — different label *order*, identical signs; compared label-invariantly (per-pixel), so not a divergence. |

*(Updated as each method migrates.)*

## Honest labels (regression-locks vs oracles)

Methods with **no external reference code** are regression-locks (they pin
OpenISI's own current behaviour), **not** oracles — and are labelled as such at the
source, never dressed as validated:

| Method | Label site | Why no oracle |
|---|---|---|
| `spectral_snr` | `compute/responsiveness.rs:10,48` — *"OpenISI heuristic, NO external oracle… regression-lock"* | OpenISI's own SNR ratio rule; nothing upstream computes it. |
| `cortex_from_reliability` | `segmentation/mod.rs:101`, `golden_cortex_morph.rs:611` — *"UNVALIDATED (regression-lock only)"* | Zhuang segments full-frame; the reliability-mask cortex restriction is OpenISI's. |
| `derive_visual_grid` | `methods/patch_refinement.rs:1466` — *"regression-lock, NOT an oracle"* | OpenISI rendering choice. |
| `compute_eccentricity` V1-center | `golden_cortex_morph.rs:643` — *"regression-lock"* | OpenISI V1-center selection differs from SNLC by design. |

The cross-cycle `reliability` coherence is the Engel 1994 / Zhuang 2017 **published
formula** computed via numpy primitives (`sum`/`abs`) — a formula pin, labelled as
such (it has no canonical reference *code* to execute), not claimed as a code oracle.

## Reproducibility (condition 7)

The harness is reproducible **by construction**: the genuine env is materialised
only from the committed `nat/uv.lock` (+ `pyproject.toml`), and `uv sync` rebuilds
the identical environment on any machine. The *execution* on a genuinely second
machine is the CI workflow **`.github/workflows/oracle.yml`**, which on a clean
GitHub runner installs `uv`, materialises the NAT env from the committed lock with
`uv sync --locked` (fails if the lock is stale → proves "from the committed lock
alone"), installs Octave + the image package, and runs the live suite
(`cargo test --features oracle_live`). That workflow IS the second-machine
reproducibility gate; it runs off the dev host on every change to the oracle/
harness/reference files.

