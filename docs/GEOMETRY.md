# Rig Geometry & Visual-Angle Conventions

The single source on how OpenISI defines the physical rig geometry and the
pixel → visual-angle transform, aligned to the canonical mouse-ISI literature and
reference implementation. The scientific methods that consume these are in
`PIPELINE_METHODS.md`; the parameter *types* live in
`crates/openisi-params/src/config/{rig,experiment}.rs`.

> The point-in-time *audit* that established this alignment (divergence table,
> remediation steps, status) is archived at `archive/RIG_GEOMETRY_AUDIT.md`.

## Canonical sources

| Source | Role |
|---|---|
| Marshel, Garrett, Nauhaus, Callaway 2011, *Neuron* (PMC3248795) | Original modern rig spec; numerical defaults. |
| Garrett, Nauhaus, Marshel, Callaway 2014, *J Neurosci* (PMC4160785) | Same-lab follow-up. |
| Juavinett, Nauhaus, Garrett, Zhuang, Callaway 2017, *Nat Protocols* (PMC5381647) | Canonical protocol paper (55″ TV; 30° tilt). |
| Zhuang, Ng, Williams, Valley, Li, Garrett, Waters 2017, *eLife* (PMC5218535) | Allen Institute version. |
| `zhuangjun1981/retinotopic_mapping` (`MonitorSetup.py`) | Reference Python implementation of the pixel→(az, el) transform. |

## Physical model

A **flat panel offset to one hemifield**, viewed through a **calibrated perpendicular
bisector** — the ray from the eye normal to the monitor surface. Where it meets the
monitor face defines **(az = 0, el = 0)** in stimulus space (rarely the geometric
center). The monitor is **yawed inward** so its plane is roughly parallel to the
retina. Three physical quantities place it: the perpendicular **viewing distance**,
the **bisector intercept on the monitor face** (in cm), and the **monitor yaw**
(20–30° in the literature). Plus the panel's intrinsic facts (width/height cm,
resolution, refresh) and a **`visual_field`** discriminator (`left`/`right`) that
sets the azimuth sign convention.

## Mathematical model (planar-monitor spherical correction)

With `x_cm`, `y_cm` measured on the monitor from the bisector intercept and `dis` the
viewing distance:

```
azimuth_deg  = atan2_deg(x_cm, dis)
slant_dist   = sqrt(dis² + x_cm²)        # eye-to-column distance
altitude_deg = atan2_deg(y_cm, slant_dist)
```

then biased so the bisector point lands at the chosen visual-space center. OpenISI's
`DisplayGeometry::spherical_uv_to_angle` (`crates/openisi-stimulus/src/geometry.rs`)
reproduces these equations line-for-line with the reference implementation — the
projection math is verified canon and is *not* a place we diverge.

## OpenISI's parameter surface (current)

Two distinct, independent degrees of freedom — do not conflate them:

- **Rig geometry** (`RigConfig.geometry`, in `config/rig.json`) — the *physical
  placement*, calibrated per rig: `viewing_distance_cm`, `monitor_width_cm`,
  `monitor_height_cm`, `bisector_x_cm`, `bisector_y_cm`, `monitor_yaw_deg`,
  `monitor_pitch_deg`, `visual_field` (`left`/`right`). Monitor cm/px are EDID-sourced
  as a first-run default with provenance, then treated as calibrated rig facts.
- **Stimulus sweep geometry** (`ExperimentConfig.stimulus_geometry`, in
  `experiment.json`) — the experimenter-declared *swept range*, recorded into the
  `.oisi` and fed to `AcquisitionProperties` at analysis time: `rotation_k`,
  `azi_angular_range`, `alt_angular_range`, `offset_azi`, `offset_alt`.
- **Experiment rendering offsets** (`ExperimentConfig.geometry`): `horizontal_offset_deg`,
  `vertical_offset_deg`, `projection` (default `spherical`).

These coincide in a simple centered setup but diverge for asymmetric or
partial-hemifield sweeps — mirroring Zhuang's `MonitorSetup` distinction between
`center_coordinates` (visual-space center) and `C2A_cm`/`C2T_cm` (cm-on-monitor
bisector intercept). The sweep envelope is **binding in the renderer**: a bar/wedge/
ring pixel outside the declared `(az, el)` box renders as `background_luminance`
(fullfield calibration is intentionally unmasked).

Canonical defaults in use: Zhuang envelope **140° azimuth × 110° altitude**, bar 20°,
check 25°, strobe 6 Hz, sweep ~9°/s, spherical projection, 88×50 cm panel, 30° yaw,
right hemifield.

## Known deliberate divergences & limitations

- **Background luminance = 0.0 (black), by explicit rig decision** — the canon uses
  mean gray (0.5). Chosen for an unambiguous stimulus/FOV boundary on this rig; the
  larger bar-onset transient is an accepted tradeoff. The carrier `mean_luminance`
  stays 0.5.
- **Yaw/pitch math deferred.** `monitor_yaw_deg` / `monitor_pitch_deg` are declared,
  persisted, and UI-editable but **not yet consumed** by the projection — the shader
  assumes the monitor normal is the eye-perpendicular axis. Per the reference
  implementation, tilt is fully describable today via the asymmetric bisector
  intercept + visual-space center without explicit rotation; explicit yaw/pitch
  support in the transform is future work.
