# OpenISI → NWB export

Convert an OpenISI `.oisi` file into a reference-valid, DANDI-submittable
[NWB](https://www.nwb.org/) file.

This is a pure **transformation**: it reads the existing `.oisi` (via `h5py`) and
writes a new `.nwb` (via [PyNWB](https://pynwb.readthedocs.io/), the reference NWB
implementation). The native `.oisi` format is **never modified**, so the
bit-identical analysis-regression gate is untouched. Conformance is guaranteed by
the reference implementation + [`nwbinspector`](https://nwbinspector.readthedocs.io/),
not by hand-asserted structure.

Python is a runtime dependency **at export time only** — acquisition and analysis
are pure Rust.

## Install

```bash
pip install -r tools/export_nwb/requirements.txt
```

## Use

From the headless CLI (recommended — locates the bridge for you):

```bash
openisi-headless export-nwb path/to/recording.oisi [out.nwb] --metadata my_metadata.json
```

Or directly:

```bash
python tools/export_nwb/export_oisi_to_nwb.py recording.oisi recording.nwb \
    --metadata tools/export_nwb/metadata.example.json
```

### Metadata sidecar

NWB / DANDI require a few fields the `.oisi` format does not capture (subject age,
sex, species; experimenter; experiment description; the Allen-CCF imaging-plane
location). Supply them via `--metadata <file.json>` — see
[`metadata.example.json`](metadata.example.json). Without it the export still
succeeds and is schema-valid, but `nwbinspector` will flag the missing
DANDI-required fields. Fields the `.oisi` *does* carry (session time, animal id,
notes, rig/experiment/analysis provenance) are filled automatically.

## What maps where

| `.oisi`                                   | NWB target |
|-------------------------------------------|------------|
| session / animal / institution            | `NWBFile` + `Subject` + `/general` |
| raw camera frames `(T,H,W)`               | `OnePhotonSeries` (core) |
| sweep schedule (start/stop + direction)   | `TimeIntervals` "sweeps" (core) |
| anatomical reference image                | `GrayscaleImage` in `Images` (core) |
| visual-area segmentation (labels + signs) | `PlaneSegmentation` (core) |
| retinotopy result maps + complex maps     | `ndx-openisi` `RetinotopyMaps` |
| multi-clock acquisition timing forensics  | `ndx-openisi` `TimingForensics` |

The custom types live in the [`ndx-openisi`](../../ndx-openisi/) extension — only
the parts core NWB cannot hold (the deprecated-`ImagingRetinotopy` replacement, the
complex Fourier maps, the render metadata, and the two-physical-clock timing
record).

## Validate (the conformance gate)

```bash
python tools/export_nwb/validate_export.py
```

Builds a full-surface synthetic fixture, exports it, and asserts:

1. `nwbinspector` finds **no** issues (NWB best-practices);
2. the round-trip is **byte-for-byte lossless**;
3. **`dandi validate`** finds no errors (DANDI's own metadata-conformance check —
   run locally, **no account required**).

Exit 0 = pass. Run this in CI. There is also a Rust `cargo test` gate
(`src-tauri/tests/nwb_export_e2e.rs`) that drives this against a **real**
`write_oisi` output, and `roundtrip_check.py SOURCE.oisi EXPORT.nwb` checks
fidelity of any pair.

## Scope: validation + conformance, not upload

This project's goal is that exported data **conforms** to NWB + DANDI — proven by
the validators above, all of which run locally. We do **not** upload to the public
[DANDI archive](https://dandiarchive.org/) or publish `ndx-openisi` to the NWB
extensions catalog; those are identity-bound publish actions outside this system's
scope. Consequently the `ndx-openisi` namespace is **embedded in every exported
file** — exports are self-contained and fully valid without the extension being
installed anywhere. (Optional, not done: read-back through **MatNWB** for the
MATLAB ecosystem.)
