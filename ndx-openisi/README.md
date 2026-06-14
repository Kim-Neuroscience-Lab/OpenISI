# ndx-openisi

An NWB extension for OpenISI intrinsic-signal-imaging retinotopy data — the parts
of an OpenISI recording that core NWB cannot hold faithfully.

## Neurodata types

- **`RetinotopyMaps`** (`NWBDataInterface`) — the retinotopy analysis result: the
  per-pixel maps (phase, amplitude, VFS, eccentricity, cortical magnification, SNR,
  reliability), the complex Fourier maps they derive from, the stimulus geometry
  that pins them to degrees of visual angle, and the analysis provenance. This is
  the replacement for the **deprecated** core `ImagingRetinotopy` type.
  - **`RetinotopyMap`** (`Image`) — one 2-D map carrying OpenISI render metadata
    (palette, display range, circular wrap period, NaN/zero sentinel semantics) so
    a reader reproduces the OpenISI figure without re-deriving conventions.
  - **`ComplexMap`** (`Data`) — a `(H, W, 2)` real/imag-split complex Fourier map
    (HDF5 has no portable native-complex convention; OpenISI documents this split).
- **`TimingForensics`** (`NWBDataInterface`) — the multi-clock acquisition timing
  record: the two physical clocks (camera hardware clock + the system QPC clock
  shared with stimulus vsync), their reconciliation (offset + drift), and the
  per-frame interval / dropped-frame / sequence-gap evidence. Core NWB's single
  timeline cannot represent two clocks and their offset.

## Regenerate the spec

The YAML in [`spec/`](spec/) is authored from the spec API (not hand-edited) so the
namespace is always internally consistent:

```bash
pip install pynwb
python create_extension_spec.py
```

## Use

The OpenISI exporter ([`tools/export_nwb`](../tools/export_nwb/)) loads this
namespace and embeds it in each exported file, so exported NWB files are
self-contained and valid without the extension being installed.

To use the types directly in Python:

```python
from pynwb import load_namespaces, get_class
load_namespaces("ndx-openisi/spec/ndx-openisi.namespace.yaml")
RetinotopyMaps = get_class("RetinotopyMaps", "ndx-openisi")
```

## Distribution (out of scope: embedded, not published)

This system validates for **conformance**; it does not publish to the public NWB
extensions catalog. The spec therefore ships **in-tree** and is **embedded in every
exported file** (see the exporter), so exported NWB files are self-contained and
fully valid — readable and validatable (`pynwb`, `nwbinspector`, `dandi validate`)
without `ndx-openisi` being installed anywhere. If public distribution is ever
wanted, the path is `ndx-template` packaging + a PR to
[`nwb-extensions/staged-extensions`](https://github.com/nwb-extensions/nwb-extensions.github.io).
