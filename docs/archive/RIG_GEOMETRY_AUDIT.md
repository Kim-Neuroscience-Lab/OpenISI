# Rig Geometry Audit — openisi-rust vs. canonical mouse-ISI convention

**Date:** 2026-05-28
**Purpose:** Establish how the canonical mouse-ISI rig geometry is defined in the literature and the reference Python implementation, then enumerate every divergence in our current code (`PARAM_DEFS`, `DisplayGeometry`, `stimulus.wgsl`) so we can align rather than reinvent. No code changes here — design audit only.

---

## 1. Sources

Four primary sources, read in full for the Methods / Visual stimulation / display-geometry sections:

| Source | Role |
|---|---|
| Marshel, Garrett, Nauhaus, Callaway 2011, *Neuron* (PMC3248795) + supplement | Original modern rig spec. Numerical defaults. |
| Garrett, Nauhaus, Marshel, Callaway 2014, *J Neurosci* (PMC4160785) | Same-lab follow-up. Reproduces Marshel. |
| Juavinett, Nauhaus, Garrett, Zhuang, Callaway 2017, *Nat Protocols* (PMC5381647) | Canonical protocol paper. Updates rig to 55″ TV; tilt 30°. |
| Zhuang, Ng, Williams, Valley, Li, Garrett, Waters 2017, *eLife* (PMC5218535) | Allen Institute version. Slightly different distances/envelope. |
| `zhuangjun1981/retinotopic_mapping` (`MonitorSetup.py`) | Canonical Python reference implementation. Pixel→(az, el) transform in code. |

---

## 2. Canonical rig geometry

### Physical model (consistent across all sources)

The rig is a **flat panel display offset to one hemifield** with the eye looking at it through a **calibrated perpendicular bisector**. The bisector is the ray from the eye normal to the monitor surface; where it intersects the monitor face defines **(az=0, el=0)** in stimulus space. The monitor is **yawed inward** so the screen plane becomes approximately parallel to the eye/retina (compensating for the lateral position of the mouse eye in the head).

Three independent physical quantities define the placement:

1. **`viewing_distance_cm`** — perpendicular distance from eye to monitor surface
2. **Bisector intercept on the monitor face**, in cm, in the monitor's own coordinates — usually expressed as `C2T_cm` (gaze→top) and `C2A_cm` (gaze→anterior edge), with the orthogonal pair derived as `C2B_cm = height - C2T_cm` and `C2P_cm = width - C2A_cm`. This is **rarely the geometric center**: Marshel places it 28 cm up from the bottom of a 121 cm-tall portrait screen (~23% up, horizontally centered); Zhuang's code defaults to `center_coordinates=(0°, 60°)` in (alt, az), meaning the gaze projection point is at 60° azimuth — i.e. heavily lateral.
3. **Monitor yaw** around the vertical axis (a body-to-monitor angle). Marshel = 20°, Juavinett/Garrett = 30°, Zhuang = "midline at ~30° to monitor plane" (same convention).

Plus the panel's own intrinsic facts: **`mon_width_cm`, `mon_height_cm`, `refresh_rate_hz`, `resolution_px`**, and a **`visual_field`** discriminator (`left` / `right`) for which hemifield is stimulated.

### Mathematical model (from `retinotopic_mapping/MonitorSetup.py`)

The pixel → visual-angle transform that the rest of the field uses, in cm-on-monitor coordinates with origin at the bisector intercept:

```python
# x in cm along monitor horizontal, y in cm along monitor vertical
azimuth_deg   = atan2_deg(x_cm, dis)
slant_dist    = sqrt(dis**2 + x_cm**2)        # eye-to-column distance
altitude_deg  = atan2_deg(y_cm, slant_dist)
```

Then both are biased by `center_coordinates=(alt0, az0)` so the bisector point lands at the chosen (az0, alt0) in *visual* coordinates — usually (0, 0), but the Zhuang code defaults to (0, 60°).

**This is the standard planar-monitor spherical correction.** Our `DisplayGeometry::spherical_uv_to_angle` at `geometry.rs:239` produces the same equations modulo coordinate names — verified by hand-derivation — so the **math is fine**. The problem is **which parameters are exposed and where the origin actually sits**.

### Stimulus envelope

The bar sweeps over a **declared angular range** that is a recorded acquisition fact, *not* the full angular extent of the monitor. Numbers from the literature:

| Source | Azimuth range | Altitude range | Bar width | Drift | Checker | Carrier strobe |
|---|---|---|---|---|---|---|
| Marshel 2011 | 147° | 153° | 20° | 8.5–9.5°/s | 25° | 166 ms period (~6 Hz) |
| Zhuang 2017 | −10° to 130° (140°) | −50° to 60° (110°) | 20° | 9°/s | 25° | 6 Hz |
| Juavinett 2017 | defers to Marshel | defers to Marshel | defers | ~9°/s | defers | defers |

Mouse anatomical FOV (Wagor et al. 1980, quoted by Marshel) is ~140° horizontal × 110° vertical *per eye*. Marshel deliberately stimulates **beyond** this — the envelope they actually use exceeds anatomical FOV. So "bound the stimulus to the mouse FOV" is **not** what the canon does; the canon bounds it to an **explicit, calibrated, recorded angular range** chosen to cover the cortical area of interest — which happens to be similar in magnitude to anatomical FOV but is independently parameterized.

---

## 3. What openisi-rust has today

### Parameters in `PARAM_DEFS` (`crates/openisi-params/src/definitions.rs`)

**Rig.Display group** (lines 40–68):
- `ViewingDistanceCm` = 10.0 ✓
- `TargetStimulusFps` = 60 Hz
- `MonitorRotationDeg` = 180.0 — **image rotation**, not physical yaw

**Experiment.Geometry group** (lines 295–313, 395–405):
- `RotationK` = 0
- `AziAngularRange` = 100.0
- `AltAngularRange` = 100.0
- `OffsetAzi` = 0.0
- `OffsetAlt` = 0.0
- `HorizontalOffsetDeg` = 0.0
- `VerticalOffsetDeg` = 0.0
- `ExperimentProjection` = Spherical

**Experiment.Stimulus group** (lines 408–459):
- `StimulusWidthDeg` = 20.0 ✓
- `CheckSizeDeg` = 25.0 ✓
- `StrobeFrequencyHz` = 6.0 ✓
- `SweepSpeedDegPerSec` = **90.0** — canonical is 9°/s; suspect a typo, divergent by 10×
- `BackgroundLuminance` = 0.0 (canonical is mean gray = 0.5; black background is wrong outside the bar)

**Not in PARAM_DEFS at all (sourced at runtime from Win32 EDID via `MonitorInfo`):**
- `display_width_cm`, `display_height_cm`
- `display_width_px`, `display_height_px`

### `DisplayGeometry` (`crates/openisi-stimulus/src/geometry.rs`)

Built at `stimulus_thread.rs:401` using `(projection, viewing_distance_cm, center_az_deg, center_el_deg, monitor.width_cm, monitor.height_cm, …)`. The `center_az_deg` / `center_el_deg` come from `HorizontalOffsetDeg` / `VerticalOffsetDeg` — i.e. the bisector intercept is implicitly **expressed in degrees**, not centimeters, and computed against the *monitor center* as origin.

Spherical projection math at `geometry.rs:222-249` matches the Allen reference (verified). `flat_angle_to_uv` (`geometry.rs:202`) assumes the eye looks at the monitor's geometric center.

### Shader (`crates/openisi-stimulus/src/shaders/stimulus.wgsl`)

`bar_envelope` (line 266) uses `uniforms.visual_field_deg.x/.y` as the sweep extent — which is computed at `geometry.rs:106–133` purely from monitor cm / viewing distance, i.e. **whatever the monitor happens to subtend**. The Experiment-group `AziAngularRange` / `AltAngularRange` are written to the `.oisi` file as `AcquisitionProperties` but **never reach the shader**. No angular FOV mask is applied anywhere.

---

## 4. Divergences from the canon

| Concept (canonical) | Status | Notes |
|---|---|---|
| `monitor_width_cm`, `monitor_height_cm` as **calibrated rig parameters** with provenance | **Missing.** EDID is sourced silently at runtime via `MonitorInfo`. | EDID is famously unreliable; the canon treats these as measured rig facts that get recorded in the file. |
| Bisector intercept in **cm** (`C2T_cm`/`C2A_cm` or equivalent) | **Missing.** | We have only `HorizontalOffsetDeg`/`VerticalOffsetDeg`, which are *angular*. This conflates "where the eye physically sits relative to monitor center" (a rig fact, in cm) with "where the analysis is centered" (a visual-space concept). |
| Monitor yaw around vertical axis (20–30°) | **Missing.** | `MonitorRotationDeg=180°` is the *image* rotation (portrait flip), not the physical orientation. There is no representation of how the monitor plane sits relative to the eye normal. |
| Monitor pitch around horizontal axis (e.g. 20° when headframe is tilted) | **Missing.** | Same gap. |
| `visual_field` discriminator (left/right) | **Missing.** | The canon's transform inverts X sign depending on hemifield. |
| Explicit `azi_angular_range` / `alt_angular_range` consumed by the **renderer** | **Half-present.** Declared as Experiment params (100°/100°), but the shader reads `visual_field_deg` (derived from monitor cm) instead. Defaults also wrong: canonical 140–147° azi, 110–153° alt. | This is the primary "stimulus not bounded to declared FOV" bug. |
| Bisector intercept treated as origin (az=0, el=0) | **Wrong by default.** `geometry.rs` puts origin at monitor center unless `HorizontalOffsetDeg`/`VerticalOffsetDeg` are set; the canon places it at the bisector intercept in cm and derives the rest. | The math is identical *if* the user enters the right degree value, but there's no way to enter the rig measurement in cm and have the system compute the rest. |
| Sweep speed | **Divergent (10×).** `SweepSpeedDegPerSec` = 90.0; canonical 8.5–9.5°/s. | Either a literal typo or intentional that I don't understand. Flag for review. |
| Background luminance | **Divergent.** `BackgroundLuminance = 0.0` (black). Canonical: mean gray (0.5). | Marshel/Zhuang use mean gray outside the bar; black background makes the bar onset transient much larger than the canonical protocol. |
| Mouse anatomical FOV as a **separate** quantity | **N/A — neither the canon nor we use anatomical FOV as a mask.** | The canon uses a *declared sweep envelope* (Marshel: 147×153°, Zhuang: 140×110°) that is similar to anatomical FOV in magnitude but parameterized independently. Your concern is met by parameterizing + enforcing the sweep envelope, not by adding a separate "mouse FOV" param. |
| `flat_angle_to_uv` / spherical equations | **Math is correct**, matches retinotopic_mapping line-for-line. | No change needed to projection math itself. |
| Spherical projection enum present and default | ✓ | `ExperimentProjection = Spherical` is right; canon uses spherical. |

---

## 5. Proposed aligned design

This is the **minimal** delta to bring the rig geometry parameter surface into alignment with the canon. Three steps; each independently testable.

### Step A — promote monitor + bisector to calibrated Rig parameters

Add to `PARAM_DEFS` (Rig group, geometry section):

```text
MonitorWidthCm:   F64,   "geometry.monitor_width_cm",   Rig, Display, ...
MonitorHeightCm:  F64,   "geometry.monitor_height_cm",  Rig, Display, ...
BisectorXCm:      F64,   "geometry.bisector_x_cm",      Rig, Display, ...   // gaze→anterior edge offset (signed)
BisectorYCm:      F64,   "geometry.bisector_y_cm",      Rig, Display, ...   // gaze→top edge offset (signed)
MonitorYawDeg:    F64,   "geometry.monitor_yaw_deg",    Rig, Display, ...   // physical yaw around vertical axis
MonitorPitchDeg:  F64,   "geometry.monitor_pitch_deg",  Rig, Display, ...   // physical pitch around horizontal axis
VisualField:      enum { Left, Right }, "geometry.visual_field", Rig, Display, ...
```

`MonitorInfo` (EDID) becomes a **sourced default** with explicit provenance (record `monitor_width_cm.source = "edid" | "calibrated"` into `/rig_params`) rather than the silent ground truth.

### Step B — derive angular origin from physical params in `DisplayGeometry`

`DisplayGeometry::new` stops taking `center_azimuth_deg`/`center_elevation_deg` as user input and instead computes them from `(bisector_x_cm, bisector_y_cm, viewing_distance_cm, monitor_yaw_deg, monitor_pitch_deg)`. The current `HorizontalOffsetDeg` / `VerticalOffsetDeg` either go away or get repurposed as an *experiment-level* additional visual-space offset (e.g. for masked-stimulus offsets — Marshel doesn't use one).

This is the step that **introduces tilt support** in the math: yaw and pitch rotate the monitor normal away from the eye-perpendicular axis, which changes the pixel→(az,el) transform. The Zhuang code doesn't do this — it absorbs tilt into `center_coordinates` + asymmetric `C2A_cm`/`C2T_cm`. We can adopt the same convention (don't model rotation explicitly, just let the bisector + asymmetric offsets express it) or we can model it explicitly. The former is simpler and matches the reference impl; recommend that.

### Step C — make the sweep envelope binding in the shader

In `stimulus.wgsl::bar_envelope` and friends, **replace** `sweep_extent = uniforms.visual_field_deg.x/.y` with a new `uniforms.swept_range_deg: vec2<f32>` plumbed from `AziAngularRange` / `AltAngularRange`. At the top of each envelope function, discard (return `background_luminance`) any pixel whose `(az, el)` falls outside `[offset_az ± azi_range/2, offset_el ± alt_range/2]`. Defaults move from 100°/100° to canonical: choose either Marshel (147°/153°) or Zhuang (140°/110°) — Marshel/Garrett is more historical, Zhuang is more practical for cortex coverage. Recommend Zhuang's 140°/110° as the default since the code already cites Zhuang heavily in the analysis stages.

Also fix `SweepSpeedDegPerSec` default to 9.0 (or whatever value is actually run on the rig — confirm) and `BackgroundLuminance` to 0.5.

### What does *not* change

- The spherical-correction math (`geometry.rs:222-249`) — matches canon exactly.
- `ExperimentProjection = Spherical` default — matches canon.
- Stimulus pattern params already aligned (bar 20°, check 25°, strobe 6 Hz, mean luminance 0.5).
- The `AcquisitionProperties` plumbing into analysis — already structured correctly; it just needs the renderer to honor the same numbers it records.

---

## 6. Note on the two `geometry` sections (correction)

An earlier draft of this audit flagged the coexistence of `[geometry]` (rig-rendering) and `[stimulus_geometry]` (sweep-envelope) in `config/experiment.toml` as a "design smell" — claiming `HorizontalOffsetDeg`/`VerticalOffsetDeg` and `OffsetAzi`/`OffsetAlt` were duplicates. **That claim was wrong**, as the file's own header comment (`config/experiment.toml:1-15`) makes explicit:

> `[geometry]` is **RIG-level** — describes the projection model used to warp the flat stimulus onto the viewer's visual field … and the rendering offsets from the monitor center.
>
> `[stimulus_geometry]` is **STIMULUS-SWEEP-level** — describes the extent and zero-offset of the sweep itself … feeds `AcquisitionProperties` at analysis time.

These are two distinct degrees of freedom — the rig physical placement vs. the experimenter-declared swept range — that coincide in simple centered setups but diverge for asymmetric or partial-hemifield sweeps. Mirrors Zhuang's `MonitorSetup.Monitor` distinction between `center_coordinates` (visual-space angular center of the gaze projection) and `C2A_cm`/`C2T_cm` (cm-on-monitor bisector intercept). Both are needed.

---

## 7. Implementation status (2026-05-28)

The fix landed in two parts. **Step A + C are done**, **Step B is partial**, yaw math is deferred.

**Step A — calibrated rig parameter surface (done):**
- Added `MonitorWidthCm`, `MonitorHeightCm`, `BisectorXCm`, `BisectorYCm`, `MonitorYawDeg`, `MonitorPitchDeg`, `StimulusVisualField` to `PARAM_DEFS` as `Rig` / `Display` group, with Zhuang canonical defaults (88×50 cm panel, 30° yaw, right hemifield).
- Added the `VisualField {Left, Right}` enum with full plumbing (lib.rs, macros.rs, param_json.rs, snapshot.rs, src-tauri/params/commands.rs).
- Fixed two Godot-port-carryover defaults: `SweepSpeedDegPerSec` 90.0 → 9.0 (canonical Marshel/Zhuang ~9°/s) and `BackgroundLuminance` 0.0 → 0.5 (canonical mean gray). Also updated in shipped `config/experiment.toml` so the dev baseline carries the corrected values.
- Updated `AziAngularRange`/`AltAngularRange` defaults 100/100 → 140/110 (Zhuang canonical envelope).

**Step C — sweep envelope binding in the shader (done):**
- Added `swept_range_deg` and `swept_center_deg` to `StimulusUniforms` (Rust + WGSL, struct grew 104 → 120 bytes).
- `bar_envelope` now uses `swept_range_deg` as `sweep_extent` instead of monitor-derived `visual_field_deg`, anchored at `swept_center_deg` — bars enter and exit at the declared envelope edges, not the monitor edges.
- `fs_main` masks any bar/wedge/ring pixel outside the declared `(az, el)` box to `background_luminance`. Fullfield (used for calibration) is intentionally unmasked.
- Plumbed through `stimulus_thread::build_renderer_config` from `AziAngularRange`/`AltAngularRange`/`OffsetAzi`/`OffsetAlt`.

**Step B — consume the new rig params in `DisplayGeometry` (in progress):**
- `MonitorWidthCm`/`MonitorHeightCm` from registry replace EDID-sourced `MonitorInfo` cm values as the ground truth. EDID becomes the first-run sourced default with provenance, not the silent ground truth.
- `BisectorXCm`/`BisectorYCm` shift the angular origin within the monitor's coordinate system.
- **Yaw math deferred** — `MonitorYawDeg`/`MonitorPitchDeg` are declared, persisted, and UI-editable but not yet consumed. Wiring them requires rotating the monitor plane out of perpendicular in the WGSL projection (currently the shader assumes the monitor normal is the eye-perpendicular axis). The Zhuang reference implementation handles this via asymmetric `C2A_cm`/`C2T_cm` + `center_coordinates` without explicit rotation, so as long as users follow that convention the rig is fully describable today. Explicit yaw support is a future change.

**Verified on the rig (2026-05-28):**
Background luminance fix visible on stimulus monitor (mean gray, not black). wgpu accepted the new uniform layout. Display validation 179.95Hz, clean shutdown with session persistence. Geometry-group descriptor fetch returns new envelope defaults.

---

## 8. Addendum (2026-06-04) — background reverted to black by rig decision

The §7 canon-alignment change set `BackgroundLuminance = 0.5` (mean gray). Per an explicit rig decision it is now **`0.0` (black)** again — a deliberate divergence from the Marshel/Zhuang mean-gray convention, chosen for a clean, unambiguous stimulus/FOV boundary on this rig. Changed in the `PARAM_DEFS` default (`definitions.rs`) and `config/experiment.toml`. The tradeoff (a larger bar-onset luminance transient than the canon) is accepted. `MeanLuminance` (the carrier mean) is unchanged at 0.5.
