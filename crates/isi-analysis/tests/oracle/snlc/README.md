# SNLC / Garrett oracle environment

The genuine SNLC (Sereno / Garrett et al. 2014 lineage) retinotopy reference is
**MATLAB** code, vendored byte-pristine under `reference/ISI/*.m`. This oracle runs
it **live via Octave** — `bridge.m` `addpath`s the real `.m` and calls it directly;
no transcription, no frozen fixtures.

## Environment (harness-managed, pinned)

- **Octave 11.2.0** (the runtime; set `OPENISI_OCTAVE` or put `octave-cli` on PATH).
- **Octave `image` package** — the SNLC reference uses MATLAB Image Processing
  Toolbox functions (`bwlabel`, `imopen`/`imclose`/`imfill`/`imdilate`, `fspecial`,
  `watershed`, `bwdist`); `bridge.m` does `pkg load image`. The package must be
  installed in the Octave used.
- The reference (`reference/ISI/`) is **never modified**.

Unlike the NAT Python env (fully `uv.lock`-pinned), Octave + its packages are a
system install that cannot be lockfile-pinned the same way; the versions above are
the **documented, required** toolchain. This is the SNLC analogue of NAT's
period-correct-reconstruction caveat.

## Irreducible gap (stated, never assumed away)

**Octave is not MATLAB.** Octave's IPT functions match MATLAB to high precision but
are not guaranteed bit-identical. SNLC oracle comparisons are therefore held at an
ε-grounded tolerance (not bit-equality), and this gap is flagged here and at the
top of `bridge.m`. It is the one irreducible approximation in the SNLC oracle.

## Proven

`bridge.m` marshalling is de-risked (a non-square identity round-trips exactly,
column-major handled), and it runs the genuine reference end-to-end: `getPatchCoM`
on a two-patch mask returns the correct centroids. The Rust side
(`test_support::oracle::snlc`) invokes the bridge through Octave, behind the
`oracle_live` feature.

## Status

The environment + bridge + Rust harness are built and proven. The per-method
migrations — matching OpenISI ops against the genuine SNLC functions (`getMouseAreasX`
cortex, `getMagFactors` anisotropy, `getPatchSign`, `splitPatchesX`/`fusePatchesX`,
`Gprocesskret` combine/delay, `getAreaBorders`/`getV1id` V1-centre) — follow.
Several SNLC functions are *composite* (internal smoothing/thresholding), so each
migration matches inputs carefully rather than assuming a 1:1 op boundary.
