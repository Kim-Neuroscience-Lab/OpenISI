# SNLC / Garrett oracle environment

The genuine SNLC (Sereno / Garrett et al. 2014 lineage) retinotopy reference is
**MATLAB** code, vendored byte-pristine under `reference/ISI/*.m`. This oracle runs
it **live via genuine MATLAB** — `bridge.m` `addpath`s the real `.m` and calls it
directly; no transcription, no frozen fixtures.

## Environment (harness-managed, pinned)

- **Genuine MATLAB R2025b** (the runtime; set `OPENISI_MATLAB` to the `matlab`
  executable). The bridge is invoked as `matlab -batch bridge`.
- **Image Processing Toolbox** — the SNLC reference uses MATLAB Image Processing
  Toolbox functions (`bwlabel`, `imopen`/`imclose`/`imfill`/`imdilate`, `fspecial`,
  `watershed`, `bwdist`, `roifilt2`), which are built into MATLAB with the Image
  Processing Toolbox installed. No package-load step is required.
- The reference (`reference/ISI/`) is **never modified**.

## Proven

`bridge.m` marshalling is de-risked (a non-square identity round-trips exactly,
column-major handled), and it runs the genuine reference end-to-end: `getPatchCoM`
on a two-patch mask returns the correct centroids. The Rust side
(`test_support::oracle::snlc`) invokes the bridge through genuine MATLAB, behind the
`oracle_live` feature. SNLC live tests skip cleanly when `OPENISI_MATLAB` is unset.

## Status

The environment + bridge + Rust harness are built and proven. Genuine MATLAB R2025b
passes the full SNLC oracle suite locally. The per-method migrations — matching
OpenISI ops against the genuine SNLC functions (`getMouseAreasX` cortex,
`getMagFactors` anisotropy, `getPatchSign`, `splitPatchesX`/`fusePatchesX`,
`Gprocesskret` combine/delay, `getAreaBorders`/`getV1id` V1-centre) — follow.
Because `roifilt2` is built into genuine MATLAB, `splitPatchesX`/`fusePatchesX` run
live. Several SNLC functions are *composite* (internal smoothing/thresholding), so
each migration matches inputs carefully rather than assuming a 1:1 op boundary.
