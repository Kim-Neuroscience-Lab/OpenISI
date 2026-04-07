# Architecture

This document defines the target architecture for OpenISI. The current codebase predates this design and will be migrated to match it.

## Domains

The system has five natural domains. Each has clear boundaries, its own state, and its own persistence rules.

### Rig
The physical installation — monitor, camera, geometry, system tuning. Changes when you rearrange hardware. Persists across everything.

**File:** `rig.toml`
**Rust type:** `RigConfig`
**Mutability:** Read at startup. Written back on explicit user changes (e.g., changing viewing distance, adjusting exposure).

### Experiment
What stimulus to present and how — envelope, carrier, parameters, presentation order, timing. Portable between rigs, shareable between labs.

**File:** `experiment.toml` (current working state), `*.experiment.toml` (saved/named experiments)
**Rust type:** `Experiment`
**Mutability:** Loaded at startup from `experiment.toml` or from last-used experiment file. Overwritten when user loads a saved experiment or edits parameters.

### Session
One sitting at the rig. Hardware connections, validation results, anatomical capture. Begins when the user clicks "New Session." Ends when they explicitly end it or start a new one.

**File:** None — volatile, in-memory only.
**Rust type:** `Session`
**Mutability:** Built up incrementally as the user connects hardware and validates.

### Acquisition
One recording run. Has a definite start and end. Produces one `.oisi` file. Only exists while recording is in progress.

**File:** `*.oisi` (output)
**Rust type:** `Acquisition`
**Mutability:** Created at acquisition start, consumed at acquisition end.

### Analysis
Post-hoc processing of acquisition data. Can happen any time, independently of hardware. Loads a `.oisi` file, computes maps, writes results back.

**File:** Results written into the `.oisi` file.
**Rust type:** `AnalysisState` (loaded file, computed maps, current parameters)
**Mutability:** Created when user opens a file for analysis.

## File landscape

| File | Purpose | Persists | Schema |
|------|---------|----------|--------|
| `rig.toml` | Physical installation config | Across everything | `RigConfig` |
| `experiment.toml` | Current working experiment | Across app restarts | `Experiment` (no metadata) |
| `*.experiment.toml` | Saved experiment definitions | Permanently | `Experiment` (with metadata) |
| `*.oisi` | Acquisition data + analysis results | Permanently | HDF5 |

No other config files. No `stimulus.toml`, no `app.toml`, no JSON config files.

## Persistence rules

### Persists across app restarts (rig properties)
- Camera defaults (exposure, gain, target FPS)
- Rig geometry (viewing distance, offsets, projection)
- Display settings (target stimulus FPS, monitor rotation)
- Analysis defaults
- Window state, UI preferences, paths
- System internals (poll intervals, timeouts, thresholds)
- Current working experiment (`experiment.toml`)
- Path to last-used experiment file

### Resets each app launch (session state)
- Which monitor is selected
- Display validation results
- Camera connection
- Camera FPS validation results
- Anatomical capture
- Acquisition state (progress, save path, animal ID, notes)

## Config file schemas

### rig.toml

```toml
[camera]
exposure_us = 33000
gain = -1
target_fps = 0.0

[geometry]
viewing_distance_cm = 10.0
horizontal_offset_deg = 30.0
vertical_offset_deg = 0.0
projection = "spherical"

[display]
target_stimulus_fps = 60
monitor_rotation_deg = 0.0

[analysis]
smoothing_sigma = 2.0
rotation_k = 0
azi_angular_range = 100.0
alt_angular_range = 100.0
offset_azi = 0.0
offset_alt = 0.0
epsilon = 1e-10

[system]
camera_frame_send_interval_ms = 33
camera_poll_interval_ms = 1
camera_first_frame_timeout_ms = 5000
camera_first_frame_poll_ms = 10
display_validation_sample_count = 150
preview_width_px = 320
preview_interval_ms = 100
preview_cycle_sec = 10.0
idle_sleep_ms = 16
fps_window_frames = 10
drop_detection_warmup_frames = 10
drop_detection_threshold = 1.5

[ui]
show_debug_overlay = false
show_timing_info = true

[window]
maximized = false
x = 100
y = 100
width = 1280
height = 800

[paths]
data_directory = ""
protocols_directory = ""
last_experiment_path = ""
```

### experiment.toml

```toml
[stimulus]
envelope = "bar"
carrier = "checkerboard"

[stimulus.params]
contrast = 1.0
mean_luminance = 0.5
background_luminance = 0.0
check_size_deg = 25.0
check_size_cm = 1.0
strobe_frequency_hz = 6.0
stimulus_width_deg = 20.0
sweep_speed_deg_per_sec = 9.0
rotation_speed_deg_per_sec = 15.0
expansion_speed_deg_per_sec = 5.0
rotation_deg = 0.0

[presentation]
conditions = ["LR", "RL", "TB", "BT"]
repetitions = 10
structure = "blocked"
order = "sequential"

[timing]
baseline_start_sec = 5.0
baseline_end_sec = 5.0
inter_stimulus_sec = 0.0
inter_direction_sec = 5.0
```

### Saved experiment files (*.experiment.toml)

Same schema as `experiment.toml` with additional metadata fields at the top:

```toml
name = "Standard Retinotopy - Bar"
description = "4-direction bar mapping, azimuth + elevation"
created = "2024-03-15T10:30:00Z"
modified = "2024-03-15T14:22:00Z"

[stimulus]
# ... identical to experiment.toml
```

Loading a saved experiment overwrites `experiment.toml`. Saving an experiment writes `experiment.toml` content to a named file with metadata added.

## Rust type system

### Enums

All enums use string serialization. No integer codes anywhere in config, messages, or storage.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Envelope { Bar, Wedge, Ring, Fullfield }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Carrier { Solid, Checkerboard }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Projection { Cartesian, Spherical, Cylindrical }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Structure { Blocked, Interleaved }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Order { Sequential, Interleaved, Randomized }
```

The renderer converts enums to shader integers in exactly one place — the render config builder. Nowhere else.

### Rig config (rig.toml)

```rust
pub struct RigConfig {
    pub camera: CameraDefaults,
    pub geometry: RigGeometry,
    pub display: DisplaySettings,
    pub analysis: AnalysisDefaults,
    pub system: SystemTuning,
    pub ui: UiPreferences,
    pub window: WindowState,
    pub paths: Paths,
}

pub struct CameraDefaults {
    pub exposure_us: u32,
    pub gain: i32,
    pub target_fps: f64,
}

pub struct RigGeometry {
    pub viewing_distance_cm: f64,
    pub horizontal_offset_deg: f64,
    pub vertical_offset_deg: f64,
    pub projection: Projection,
}

pub struct DisplaySettings {
    pub target_stimulus_fps: u32,
    pub monitor_rotation_deg: f64,
}

pub struct AnalysisDefaults {
    pub smoothing_sigma: f64,
    pub rotation_k: i32,
    pub azi_angular_range: f64,
    pub alt_angular_range: f64,
    pub offset_azi: f64,
    pub offset_alt: f64,
    pub epsilon: f64,
}

pub struct SystemTuning {
    pub camera_frame_send_interval_ms: u32,
    pub camera_poll_interval_ms: u32,
    pub camera_first_frame_timeout_ms: u32,
    pub camera_first_frame_poll_ms: u32,
    pub display_validation_sample_count: u32,
    pub preview_width_px: u32,
    pub preview_interval_ms: u32,
    pub preview_cycle_sec: f64,
    pub idle_sleep_ms: u32,
    pub fps_window_frames: usize,
    pub drop_detection_warmup_frames: usize,
    pub drop_detection_threshold: f64,
}

pub struct UiPreferences {
    pub show_debug_overlay: bool,
    pub show_timing_info: bool,
}

pub struct WindowState {
    pub maximized: bool,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub struct Paths {
    pub data_directory: String,
    pub protocols_directory: String,
    pub last_experiment_path: String,
}
```

### Experiment (experiment.toml + saved experiment files)

One type for both the working experiment and saved experiment files. Metadata fields are `Option` — present in saved files, absent in the working file.

```rust
pub struct Experiment {
    pub name: Option<String>,
    pub description: Option<String>,
    pub created: Option<String>,
    pub modified: Option<String>,

    pub stimulus: StimulusSpec,
    pub presentation: PresentationSpec,
    pub timing: TimingSpec,
}

pub struct StimulusSpec {
    pub envelope: Envelope,
    pub carrier: Carrier,
    pub params: StimulusParams,
}

pub struct StimulusParams {
    // Carrier
    pub contrast: f64,
    pub mean_luminance: f64,
    pub background_luminance: f64,
    pub check_size_deg: f64,
    pub check_size_cm: f64,
    pub strobe_frequency_hz: f64,

    // Envelope (all present; only relevant ones used at runtime based on envelope type)
    pub stimulus_width_deg: f64,
    pub sweep_speed_deg_per_sec: f64,
    pub rotation_speed_deg_per_sec: f64,
    pub expansion_speed_deg_per_sec: f64,
    pub rotation_deg: f64,
}

pub struct PresentationSpec {
    pub conditions: Vec<String>,
    pub repetitions: u32,
    pub structure: Structure,
    pub order: Order,
}

pub struct TimingSpec {
    pub baseline_start_sec: f64,
    pub baseline_end_sec: f64,
    pub inter_stimulus_sec: f64,
    pub inter_direction_sec: f64,
}
```

No geometry in `Experiment`. An experiment defines what to show and how to sequence it. Where the monitor is positioned is a rig concern.

### Session (volatile, in memory)

```rust
pub struct Session {
    pub hardware: HardwareState,
    pub anatomical: Option<AnatomicalCapture>,
}

pub struct HardwareState {
    pub display: Option<DisplayState>,
    pub camera: Option<CameraState>,
}

pub struct DisplayState {
    pub monitor: MonitorInfo,
    pub validation: Option<DisplayValidation>,
}

pub struct CameraState {
    pub info: CameraInfo,
    pub fps_validation: Option<FpsValidation>,
}

pub struct AnatomicalCapture {
    pub pixels: Vec<u16>,
    pub width: u32,
    pub height: u32,
    pub saved_path: Option<PathBuf>,
}
```

Session holds only hardware connection state and the anatomical capture. No config, no experiment, no acquisition state mixed in.

### Acquisition (in-flight only)

```rust
pub struct Acquisition {
    pub save_path: PathBuf,
    pub animal_id: String,
    pub notes: String,
    pub accumulator: AcquisitionAccumulator,
}
```

Created when the user starts acquisition. Consumed when acquisition ends (either saved or discarded). Does not exist at any other time.

### App state (layered)

```rust
pub struct AppState {
    // Rig config — read at startup, written on user changes
    pub rig: Arc<Mutex<RigConfig>>,

    // Current working experiment — loaded at startup, modified by user
    pub experiment: Experiment,
    pub experiment_path: Option<PathBuf>,

    // Session — volatile hardware state
    pub session: Session,

    // In-flight acquisition — only exists during recording
    pub acquisition: Option<Acquisition>,

    // Thread handles
    pub threads: ThreadHandles,

    // Last acquisition result (persists until next run or app close)
    pub last_result: Option<AcquisitionSummary>,
}
```

Each field has clear ownership and lifecycle. No overlap between layers.

## Thread communication

Threads receive self-contained value messages. No back-references to `AppState`.

### Stimulus thread commands

```rust
pub enum StimulusCmd {
    StartAcquisition(AcquisitionCommand),
    Preview(PreviewCommand),
    StopPreview,
    Stop,
    Shutdown,
}

pub struct AcquisitionCommand {
    pub stimulus: StimulusSpec,
    pub geometry: RigGeometry,
    pub presentation: PresentationSpec,
    pub timing: TimingSpec,
    pub monitor: MonitorInfo,
    pub display: DisplaySettings,
    pub measured_refresh_hz: f64,
    pub quality: QualitySettings,
}

pub struct PreviewCommand {
    pub stimulus: StimulusSpec,
    pub geometry: RigGeometry,
    pub monitor: MonitorInfo,
}

pub struct QualitySettings {
    pub drop_detection_warmup_frames: usize,
    pub drop_detection_threshold: f64,
    pub fps_window_frames: usize,
}
```

`AcquisitionCommand` is assembled at the boundary — in the Tauri command handler — from the current rig config, experiment, and session state. The stimulus thread receives everything it needs in one message.

### Camera thread

```rust
pub enum CameraCmd {
    Enumerate,
    Connect { index: u16, exposure_us: u32 },
    Disconnect,
    SetExposure(u32),
    Shutdown,
}
```

Camera thread receives rig camera defaults at spawn time for timing parameters (poll intervals, timeouts).

## Frontend queries

Instead of one `get_session()` returning a flat blob, domain-specific queries:

```
get_rig_geometry()        → { viewing_distance_cm, ... }
get_display_settings()    → { target_stimulus_fps, monitor_rotation_deg }
get_hardware_state()      → { display: { monitor, validation }, camera: { info, fps_validation } }
get_experiment()          → { stimulus, presentation, timing }
get_acquisition_status()  → { is_active, progress, metrics } or null
```

Each returns a focused slice of state. The frontend knows exactly what it's getting and doesn't parse irrelevant fields.

## What this design eliminates

**Types that go away:**
- `StimulusConfig` — replaced by `Experiment`
- `Protocol` — replaced by `Experiment` with optional metadata
- `GeometryConfig` in stimulus config — geometry is rig-only
- `StimulusAcquisitionConfig` — replaced by `AcquisitionCommand`
- `PreviewConfig` — replaced by `PreviewCommand`
- `ConfigManager` — replaced by a simpler rig config loader

**Conversion functions that go away:**
- `protocol_to_stimulus_config()` — one type, no conversion needed
- `Protocol::from_stimulus_config()` — one type, no conversion needed
- `Carrier::from_shader_int()` / `Projection::from_shader_int()` — no integer enums
- `Structure::from_str()` / `SweepOrder::from_str()` — serde handles it

**Dead fields that go away:**
- `stimulus_width_cm` — always 0, never used
- `luminance_min` / `luminance_max` — computed from contrast + mean_luminance
- `paradigm` — always "periodic", never read
- `inverted` — redundant with `monitor_rotation_deg = 180`
- `daemon.*` — dead code from Godot's Python daemon architecture

**Principles maintained:**
- Config files are the only SSoT
- No hardcoded defaults in code
- No fallbacks — fail loudly on invalid config
- No backward compatibility — only the current format exists
- All enums use strings, not integers
- Threads receive self-contained messages, never reach back to shared state

## .oisi file contents

The `.oisi` file snapshots everything at acquisition time for complete reproducibility:

```
/version                    attr: "1.0"
/created_at                 attr: ISO-8601 string
/animal_id                  attr: string
/notes                      attr: string

/rig/                       group — snapshot of rig config
  geometry                  attrs: viewing_distance_cm, offsets, projection
  display                   attrs: monitor name, resolution, physical size, measured_refresh_hz, rotation
  camera                    attrs: model, resolution, exposure_us, gain

/experiment/                group — snapshot of experiment definition
  stimulus                  attrs: envelope, carrier, all params
  presentation              attrs: conditions, repetitions, structure, order
  timing                    attrs: baselines, intervals

/anatomical                 dataset: u16 (H, W) — optional

/acquisition/               group
  frames/<cycle_name>       dataset: u16 (T, H, W) chunked+gzip
  timestamps/<cycle_name>   dataset: f64 (T,)
  stimulus/                 group — per-frame stimulus state
    timestamps_us           dataset: i64
    state_ids               dataset: u8
    condition_indices        dataset: u8
    sweep_indices            dataset: u32
    progress                dataset: f32
    frame_deltas_us         dataset: i64
    dropped_frame_indices   dataset: u32

/complex_maps/              group — computed by analysis
  azi_fwd                   dataset: f64 (H, W, 2)
  azi_rev, alt_fwd, alt_rev

/results/                   group — computed by analysis
  azi_phase                 dataset: f64 (H, W)
  alt_phase, azi_phase_degrees, alt_phase_degrees
  azi_amplitude, alt_amplitude, vfs

/analysis_params            attr: JSON string
```

The file is self-contained. You can interpret it without the original config files.
