# Oracle golden-fixture harness

OpenISI validates its Rust pipeline against the field's reference
implementations ("oracles"): the SNLC/Garrett MATLAB code (`reference/ISI`, run
under **genuine MATLAB**) and the Allen/Zhuang Python code
(`reference/corticalmapping`, transcribed in the `gen_*_golden.py` scripts). Each
oracle runs **offline** to emit committed golden fixtures under
`crates/isi-analysis/tests/golden/fixtures/*.bin`; the test suite validates the
Rust output against those committed blobs.

**A normal `cargo test`, and every user/release build, needs no MATLAB
/ Python** — they only read the committed fixtures. The toolchain here is for
*dev* regeneration and the freshness gate only.

## The harness (one command, no script-hunting)

The dev-only [`xtask`](../../xtask) crate drives every generator through the
right interpreter:

```sh
cargo xtask goldens            # regenerate every golden fixture
cargo xtask goldens combine    # only generators whose name contains "combine"
cargo xtask goldens --check    # regenerate into a sandbox + diff vs committed
                               #   (the freshness gate; restores the tree)
cargo xtask figures            # run the render_*.py comparison-figure tools
cargo xtask figures oracle_state  # just the oracle-state gallery
```

`xtask` is its own workspace member that nothing depends on, so the app/release
build never compiles it and never acquires a Python/MATLAB dependency.

Most generators are Python (`gen_*_golden.py`, run via `OPENISI_PYTHON`). Two are
MATLAB — `gen_cortex_full_golden.m` and `gen_magroi_golden.m` — which call MATLAB
Image Processing Toolbox functions and so run under genuine MATLAB (the harness
invokes `matlab -batch <stem>`, selected via `OPENISI_MATLAB`). When MATLAB is
absent, `cargo xtask goldens` **skips the two `.m` generators with a logged
notice** and still regenerates/checks every Python fixture; the `.m` fixtures'
freshness is then covered by the self-hosted MATLAB CI job (see [CI](#ci)).

### The oracle-state gallery

`cargo xtask figures oracle_state` writes one figure per (dataset, group) under
`target/oracle_state/` (overwritten each run). Each is a grid: every row is a
method/leaf in pipeline-DAG order, columns are `[oracle|reference | OpenISI |
difference]`, colormapped by data kind (periodic `twilight` / diverging `RdBu_r`
/ sequential `viridis`, a 4-way {both, oracle-only, ours-only, neither}
categorical map for boolean masks, and a differ-highlight for integer label
maps).

**Two paths** (the dump example `oracle_state.rs` runs first; both by default):

- **synthetic** (`*_{Allen,SNLC,NumLib}_oracle_state.png`) — each method on its
  committed per-op golden fixture vs the verbatim reference output. Column 1 is a
  true external **oracle**, recomputed live through the same public op the golden
  test exercises. The **NumLib** group holds methods whose oracle is a canonical
  numerical-library primitive (numpy.fft, scipy.gaussian_filter, numpy.median)
  rather than an Allen/SNLC science method — full coverage, distinct origin.

- **r43** (`r43_{Maps,Segmentation}_oracle_state.png`) — the full pipeline
  re-run on the real `R43_smoke.oisi` recording, every `/results` leaf vs the
  committed `R43_smoke.baseline.oisi` (the equivalence harness's reference, so
  column 1 is the **reference** baseline). This is the real-data regression view;
  "for the ones possible" = the leaves present in the file. Skipped automatically
  if the R43 fixture/baseline are absent.

Run one path alone with `cargo run -p isi-analysis --example oracle_state --
synthetic` (fast; good for iterating on figure style) or `-- r43`. Re-running
just `render_oracle_state.py` re-renders from the last dump without recomputing.

## One-time toolchain setup

**Python** (for `gen_*_golden.py` / `render_*.py` — numpy/scipy/scikit-image/
matplotlib), pinned in [`requirements.txt`](requirements.txt):

```sh
python -m venv tools/golden/.venv
# Windows
tools/golden/.venv/Scripts/pip install -r tools/golden/requirements.txt
$env:OPENISI_PYTHON = "tools/golden/.venv/Scripts/python.exe"
# Unix
tools/golden/.venv/bin/pip install -r tools/golden/requirements.txt
export OPENISI_PYTHON="tools/golden/.venv/bin/python"
```

**MATLAB** (for the `*.m` generators): install MATLAB with the Image Processing
Toolbox, then either put `matlab` on `PATH` or set `OPENISI_MATLAB` to its full
path (e.g. `C:/Program Files/MATLAB/R2025b/bin/matlab.exe`). If MATLAB is absent,
the harness skips the two `.m` generators with a logged notice (their freshness
is covered by the self-hosted MATLAB CI job).

The harness discovers interpreters in this order:
`OPENISI_MATLAB` env override → bare `matlab` on `PATH` → known install locations
(e.g. `C:/Program Files/MATLAB/R2025b/bin/matlab.exe`); and `OPENISI_PYTHON` →
`python3` / `python` on `PATH` → actionable error.

## Why pin the toolchain

Float-valued fixtures depend on library version (reduction order, special-
function implementations). The pins make regeneration reproducible and let
`--check` mean "the committed fixtures still match what the declared oracles +
toolchain produce." Bumping a pin is a deliberate change that may require
regenerating and reviewing fixtures — the same discipline as editing an oracle.

## CI

`.github/workflows/goldens.yml` installs the Python toolchain on Linux and runs
`cargo xtask goldens --check` so fixture/generator drift is caught
automatically, while the main hermetic test job stays toolchain-free. This job
runs without MATLAB, so it covers every Python fixture and skips the two `.m`
generators. Their freshness is covered by the self-hosted MATLAB CI job
(`goldens-matlab`), gated on the repo variable `SELF_HOSTED_MATLAB=true`.
