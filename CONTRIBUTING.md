# Contributing to OpenISI

OpenISI aims to be a reference instrument for ISI retinotopy — software a lab can
trust with irreplaceable data and that the field can adopt as a standard. That
ambition sets the bar for contributions: correctness first, no "good enough."

## Start here

1. **Read [`docs/PRINCIPLES.md`](docs/PRINCIPLES.md)** — what OpenISI is, the
   invariants every change must uphold, and the objective Definition of Done.
   This is the contract; a change that violates an invariant is not merged, however
   convenient.
2. **Find the concern you're touching** in [`docs/README.md`](docs/README.md) —
   one source of truth per concern. Update that doc in the same change if you
   alter the behavior it describes; don't open a second doc for the same concern.
3. **Check the tool-vs-domain rule** in [`docs/TOOL_LEDGER.md`](docs/TOOL_LEDGER.md)
   before hand-rolling infrastructure — if an established tool fits, use it; only
   the science/hardware domain is hand-rolled.

## The gates (every change)

A change is not done until the workspace is pristine:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets   # zero warnings
cargo fmt --all --check
```

For any change that touches the analysis pipeline, the params it consumes, or the
`.oisi` format, the **bit-identical equivalence gate** must stay green — the
scientific output must not move unless the change is *deliberately* a math change
with an updated oracle:

```sh
cargo test -p isi-analysis --test regression_oisi -- --include-ignored
```

This run is the proof that a refactor changed structure or serialization, never the
science. If you intend to change a numerical result, say so explicitly in the PR and
update the corresponding golden/oracle in the same change.

## Style

- Match the surrounding code — naming, comment density, and idiom. New code should
  read like the file it lives in.
- Make invalid states unrepresentable where types can, and validate where they
  can't (PRINCIPLES Invariant 14).
- No silent fallbacks: when correctness is in doubt, fail loudly with a typed error
  or recompute — never serve a stale or guessed result.

## License

By contributing you agree your contributions are licensed under the
[MIT License](LICENSE).
