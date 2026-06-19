# NWB Interoperability & Export

The single source on how OpenISI data reaches the field's interchange standard.
See `PRINCIPLES.md` (Invariant 9, "By standard, not by adapter") for *why*; this
doc is *how*.

**Goal:** OpenISI data is NWB-/DANDI-**conformant** and NWB-ecosystem-readable,
with conformance *guaranteed by the reference validator* (PyNWB + `nwbinspector` +
`dandi validate`), not hand-asserted. Scope is validation + conformance, run
locally; uploading to DANDI and publishing the NDX extension are out of scope.

## Architecture decision: transform-only export (not native-layout change)

Export is a **transform**, not a repackaging of an NWB-shaped native layout. Aligning
the native `.oisi` layout to NWB so the bridge could merely repackage is the wrong
trade:

- The native `.oisi` is consumed by the analysis pipeline, whose **bit-identical
  `regression_oisi` gate** reads committed fixtures. Renaming/reshaping native
  datasets (e.g. `/acquisition/camera/frames` → an `OnePhotonSeries`-shaped path)
  would churn the read/write paths, the migration, and the fixtures — risking the
  crown-jewel gate — for no functional gain.
- A valid NWB file is a *transformation* regardless (NWB has its own neurodata
  types, `/general` conventions, and namespace machinery). The native format does
  not need to match NWB for the export to be correct.

So `export_nwb` is a **pure transformation**: read the existing `.oisi` (`h5py`),
write a new `.nwb` (PyNWB). The native format is untouched; the regression gate
stays green by construction. A native Rust NWB writer remains a "prove-it-later"
option once the layout + extension are settled and if a no-Python export becomes a
hard requirement.

## Tooling

Export is a Python bridge in [`tools/export_nwb`](../tools/export_nwb/) — Python is
a runtime dependency **at export time only**; acquisition and analysis stay pure
Rust. The Rust side exposes it as `openisi-headless export-nwb` (shells out to the
bridge). PyNWB is the reference implementation; `nwbinspector` is the validator.

## Mapping

| `.oisi` source | NWB target | Type |
|---|---|---|
| `/created_at`, `/notes`, institution/lab | `NWBFile` fields + `/general` | core |
| `/animal_id` (+ metadata sidecar: species/sex/age) | `Subject` | core |
| `/hardware` camera/monitor | `Device` ×2 | core |
| camera calibration (`rig.camera.um_per_pixel`) | `ImagingPlane.grid_spacing` | core |
| `/acquisition/camera/frames` `(T,H,W)` + `timestamps_sec` | `OnePhotonSeries` | core |
| `/acquisition/schedule/*` + `sweep_sequence` | `TimeIntervals` "sweeps" (+ `direction` col) | core |
| `/anatomical` | `GrayscaleImage` in `Images` | core |
| `/results/area_labels` + `area_signs` | `PlaneSegmentation` (ROI `image_mask` + `sign` col) | core |
| `/results/*` per-pixel maps (phase/amp/VFS/ecc/mag/SNR/reliability/contours/masks) | `RetinotopyMaps.retinotopy_maps` (`RetinotopyMap`) | **ndx-openisi** |
| per-map render meta (palette/units/display range/wrap/sentinels) | `RetinotopyMap` attributes | **ndx-openisi** |
| `/complex_maps/{azi,alt}_{fwd,rev}` `(H,W,2)` | `RetinotopyMaps.complex_maps` (`ComplexMap`) | **ndx-openisi** |
| stimulus geometry (rotation_k, azi/alt range, offsets) | `RetinotopyMaps` attributes | **ndx-openisi** |
| `/analysis_params` (tagged `AnalysisConfig`) + `software_version` | `RetinotopyMaps` attributes | **ndx-openisi** |
| `/acquisition/{camera,stimulus,quality}` timing + `/acquisition/clock_sync` | `TimingForensics` | **ndx-openisi** |

The [`ndx-openisi`](../ndx-openisi/) extension covers only what core NWB cannot
hold: the deprecated-`ImagingRetinotopy` replacement (`RetinotopyMaps`), the
complex Fourier maps, the render-metadata contract, and the two-physical-clock
timing record (`TimingForensics`). See its README for the type definitions.

### Metadata sidecar

DANDI requires fields the `.oisi` does not capture (subject age/sex/species,
experimenter, experiment description, Allen-CCF imaging-plane location). These ride
in an optional `--metadata` JSON sidecar (the standard NWB-conversion pattern); the
export never fabricates them. Fields the `.oisi` carries are filled automatically.
This is the one durable *acquisition-metadata gap* the export surfaces: to be
DANDI-complete without a sidecar, the `.oisi` capture path should record subject
species/sex/age going forward.

## Scope: validation + conformance, not upload/publish

The goal is that exported data **conforms** to the NWB + DANDI standards — proven
by the reference validators, all run **locally with no account**. Uploading to the
public DANDI archive and publishing `ndx-openisi` to the NWB extensions catalog are
identity-bound publish actions and are **explicitly out of scope**. Consequently
the `ndx-openisi` namespace is **embedded in every exported file**, so exports are
self-contained and valid without the extension being installed anywhere.

## Conformance gate (CI)

`python tools/export_nwb/validate_export.py` builds a full-surface synthetic
fixture (a real analysis-output `.oisi` augmented with a realistic `/acquisition`
group), exports it, and asserts:

1. `nwbinspector` finds **no issues** (NWB best-practices);
2. the round-trip is **byte-for-byte lossless** (every map, complex map,
   segmentation area + sign, anatomical image, raw frame, sweep, and provenance
   field, via `roundtrip_check.py`);
3. **`dandi validate`** finds no errors — DANDI's own metadata-conformance check,
   run locally (organize into a Dandiset layout → validate). No account needed.

Exit 0 = pass. **Status: PASSING.** A second gate, the Rust integration test
`src-tauri/tests/nwb_export_e2e.rs`, drives the export against a **real**
`write_oisi` output (not a synthetic fixture) and asserts a lossless round-trip —
both gates skip gracefully if Python/`pynwb` is unavailable.

Optional, not done: **MatNWB** read-back for the MATLAB ecosystem.

## Follow-on (durable, in-repo, not blocking the export)

- Keep `docs/oisi.schema.json` (the format SSoT) reconciled with what the code
  actually writes — ideally **generated from the Rust schema source** so it cannot
  drift (the I/O-boundary + schema-SSoT frontier in `PRINCIPLES.md`). The exporter's
  structural paths should consume that same generated schema, eliminating the last
  hand-copied schema knowledge across the Rust↔Python boundary.
