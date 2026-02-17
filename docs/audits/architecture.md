# OpenISI Architecture Audit

Audit Date: February 2026 (Comprehensive Update: 2026-02-03)

## 1. Overview

**Project**: OpenISI (Open Intrinsic Signal Imaging)

**Purpose**: Visual neuroscience stimulus presentation and widefield camera acquisition for intrinsic signal imaging

**Engine**: Godot 4.5 with Forward Plus rendering

**Languages**:

| Language    | Files  | Lines      |
|-------------|--------|------------|
| GDScript    | 72     | 18,087     |
| Python      | 15     | 3,587      |
| Rust        | 1      | 1,159      |
| GLSL/Shader | 11     | ~1,100     |
| **Total**   | **~99**| **~23,957** |

### Entry Point

- Main scene: `res://src/main.tscn`
- Splash scene: `res://src/ui/splash/splash.tscn`
- Window: 1280x800, per-pixel transparency enabled, vsync enabled

---

## 2. Architecture Layers

```text
+-------------------------------------------------------------+
|                      UI LAYER (src/ui/)                     |
|  Screens: Setup -> Focus -> Stimulus -> Run -> Analyze      |
|  Components: Cards, Buttons, InfoRows, StatusPills          |
+-------------------------------------------------------------+
                              |
                              v
+-------------------------------------------------------------+
|                 DOMAIN LAYER (src/domain/)                  |
|  AcquisitionController, SessionState                        |
+-------------------------------------------------------------+
                              |
                              v
+-------------------------------------------------------------+
|               STIMULUS LAYER (src/stimulus/)                |
|  Sequencer, Renderers, Shaders, Datasets, DisplayGeometry   |
+-------------------------------------------------------------+
                              |
                              v
+-------------------------------------------------------------+
|              INFRASTRUCTURE (src/autoload/)                 |
|  Settings, HardwareManager, AppTheme, Session, CameraClient |
|  ErrorHandler, DisplayValidator                              |
+-------------------------------------------------------------+
```

### Layer Descriptions

**UI Layer** (`src/ui/`)

- Screen-based workspaces implementing workflow areas
- Reusable components (17 component directories)
- Theme system with ceramic shader styling
- Programmatic UI construction (minimal .tscn files)

**Domain Layer** (`src/domain/`)

- AcquisitionController: coordinates stimulus + camera acquisition
- SessionState: typed container for session data

**Stimulus Layer** (`src/stimulus/`)

- StimulusSequencer: timing state machine
- Renderers: pluggable stimulus type renderers
- Shaders: GLSL for stimulus patterns
- Datasets: per-frame metadata recording
- DisplayGeometry: visual angle coordinate transformations

**Infrastructure Layer** (`src/autoload/`)

- Global singletons providing cross-cutting services
- Configuration management, hardware abstraction, theming

---

## 3. Autoload Singletons

### Line Count Summary

| Autoload        | Lines     | File                               |
|-----------------|-----------|-------------------------------------|
| AppTheme        | 1,093     | `src/autoload/theme.gd`             |
| Settings        | ~750      | `src/autoload/settings.gd`          |
| CameraClient    | 580       | `src/autoload/camera_client.gd`     |
| HardwareManager | 355       | `src/autoload/hardware_manager.gd`  |
| Session         | 290       | `src/autoload/session.gd`           |
| DisplayValidator| 258       | `src/autoload/display_validator.gd` |
| ErrorHandler    | 220       | `src/autoload/error_handler.gd`     |
| **Total**       | **3,546** |                                     |

---

### AppTheme (`src/autoload/theme.gd`) - 1,093 lines

**Role**: Centralized visual design system (largest autoload)

**Design System**: "Sleep Punk Night + Ceramic"

- Deep night backgrounds (#13111a base)
- Cream/lavender accents
- Amber highlights for actions
- Ceramic shader styling with rim highlights

**Provides**:

- 14+ typography styles (LabelTitle, LabelHeading, LabelCaption, etc.)
- Panel/card variations (PanelWell, PanelInfoCard, PanelModal, etc.)
- Input styling
- Status color functions
- Preloaded fonts and shaders
- 50+ spacing/sizing constants

---

### Settings (`src/autoload/settings.gd`) - ~750 lines

**Role**: Single Source of Truth (SSoT) for persistent application settings

**Note**: Runtime state (camera/display selection) is now in Session autoload.

**Manages**:

- `hardware.json`: Camera, display, daemon settings
- `preferences.json`: Window state, UI preferences
- `stimulus.json`: Stimulus parameters, timing, presentation

**Key Features**:

- Parameter validation contracts (min/max/step/unit)
- Auto-initialization of user files from bundled defaults
- Computed properties (visual field angles, total duration, sweep duration)
- Change signals with section/key/value payloads
- Snapshot save/load for protocols

**API Surface**: ~80+ accessor properties

---

### CameraClient (`src/autoload/camera_client.gd`) - 554 lines

**Role**: Interface to Python camera daemon

**Communication**: Shared memory via GDExtension (Rust)

**Daemon Lifecycle**:

- Async spawn with configurable startup delay
- Orphaned daemon cleanup
- Graceful shutdown (SIGTERM -> SIGKILL)

**Connection**:

- Exponential backoff retry (500ms -> 1s -> 2s)
- Non-blocking async operations
- Frame drop detection via counter

**Signals**: `connection_changed`, `daemon_state_changed`, `connection_failed`, `connection_attempt_complete`

---

### HardwareManager (`src/autoload/hardware_manager.gd`) - 320 lines

**Role**: Hardware enumeration and detection

**Cameras**:

- Async enumeration via Python daemon subprocess
- Supports Mock, Webcam, AVFoundation, PCO Panda types
- Thread-safe background enumeration

**Monitors**:

- DisplayServer API + native MonitorInfo extension
- EDID/CoreGraphics/WinAPI for physical dimensions (via Rust extension)
- **No DPI-based fallback** — if EDID fails, user must enter dimensions manually
- Refresh rate detection (validated before acquisition)

**Physical Dimension Sources**:
- `"edid"`: From display EDID via MonitorInfo
- `"user_override"`: User-entered values (original EDID preserved)
- `"none"`: No physical dimensions available (requires manual entry)

**Signals**: `cameras_enumerated`, `monitors_enumerated`, `enumeration_failed`

---

### Session (`src/autoload/session.gd`) - 290 lines

**Role**: Navigation, session state, and runtime hardware selection

**Screens**: SETUP, FOCUS, STIMULUS, ACQUIRE, RESULTS (+ future SETTINGS, SESSION_BROWSER)

**Navigation**:

- `navigate_to(screen)`: Direct navigation
- `navigate_next()` / `navigate_back()`: Sequential
- No validation gating - users navigate freely

**Runtime Hardware State** (moved from Config):

- `_selected_camera: Dictionary` - Camera selection from HardwareManager
- `_selected_display: Dictionary` - Display selection with validation state
- Computed properties: camera_width_px, camera_fps, display_refresh_hz, etc.
- Signals: `camera_selected`, `display_selected`

**State Container**: SessionState object holding:

- Anatomical image capture
- Hardware snapshot at acquisition start
- Stimulus configuration snapshot
- Acquisition results

**Signal**: `screen_changed`

---

### DisplayValidator (`src/autoload/display_validator.gd`) - 258 lines

**Role**: Display validation and refresh rate measurement

**Responsibilities**:

- Validates display configuration before acquisition
- Measures actual display refresh rate (not just reported rate)
- Ensures vsync is working correctly
- Provides frame timing validation

**Signals**: `validation_completed`, `validation_failed`

---

### ErrorHandler (`src/autoload/error_handler.gd`) - 220 lines

**Role**: Centralized error handling with user feedback and history tracking

**Features**:

- Severity levels: INFO, WARNING, ERROR, CRITICAL
- Category classification: HARDWARE, CAMERA, DISPLAY, CONFIG, ACQUISITION, STIMULUS, EXPORT
- Error codes for programmatic handling
- Error history tracking (last 100 errors)
- Convenience methods: `report_camera_error()`, `report_hardware_error()`, etc.

**Signals**: `error_occurred`, `error_dismissed`

**UI Component**: ErrorDialog (`src/ui/components/error_dialog/error_dialog.gd`)

- Modal dialog with severity-colored icons
- Expandable details section
- Dismiss and optional Retry buttons
- Error queue management

---

## 4. UI Screen System

### Screen Count: 5

| Screen   | File                    | Lines | Purpose                                            |
|----------|-------------------------|-------|----------------------------------------------------|
| Setup    | `setup_screen.gd`       | 1,063 | Camera/monitor selection, session params           |
| Focus    | `focus_screen.gd`       | 526   | Live preview, exposure control, anatomical capture |
| Stimulus | `stimulus_screen.gd`    | 590   | Protocol design, live preview                      |
| Run      | `run_screen.gd`         | 1,142 | Active acquisition, real-time metrics              |
| Analyze  | `analyze_screen.gd`     | 300   | Results summary, export info                       |

### Stimulus Screen Supporting Components

The stimulus screen has 6 supporting card components:

| Component               | Lines | Purpose                           |
|-------------------------|-------|-----------------------------------|
| `composition_card.gd`   | 211   | Stimulus composition settings     |
| `geometry_card.gd`      | 183   | Display geometry configuration    |
| `parameters_card.gd`    | 207   | Stimulus parameter controls       |
| `preview_controller.gd` | 84    | Preview window management         |
| `sequence_card.gd`      | 446   | Sequence/timing configuration     |
| `timing_card.gd`        | 186   | Timing parameter controls         |

**Total stimulus screen ecosystem**: 1,907 lines (590 + 1,317 supporting)

### BaseScreen Lifecycle

All screens extend `BaseScreen` (48 lines) and implement:

```text
_ready()
    |
    v
_build_ui()        # Construct UI elements
    |
    v
_connect_signals() # Wire event handlers
    |
    v
_load_state()      # Populate from Config/Session
    |
    v
_validate()        # Check prerequisites, emit validation_changed
```

### Navigation Architecture

- `main.gd` (372 lines) listens to `Session.screen_changed`
- Pre-loads all screen scenes via SceneRegistry
- Footer buttons trigger `Session.navigate_next/back()`
- Screens emit `validation_changed` to enable/disable Continue button

---

## 5. Stimulus Pipeline

### Rendering Flow

```text
Config (parameters)
    |
    v
StimulusSequencer (timing state machine)
    |
    v
StimulusDisplay (orchestrator, 823 lines)
    |
    v
TextureRenderer (unified renderer, 258 lines)
    |
    v
Shaders (GLSL)
    |
    v
StimulusDataset (per-frame recording)
```

### StimulusSequencer States (7 states)

```text
IDLE -> BASELINE_START -> SWEEP -> INTER_STIMULUS -> ... -> INTER_DIRECTION -> BASELINE_END -> COMPLETE
```

- Manages timing durations from Config
- Generates sweep sequence (sequential/interleaved/randomized)
- Emits: `state_changed`, `sweep_started`, `sweep_completed`, `direction_changed`, `sequence_started`, `sequence_completed`, `progress_updated`

### Stimulus Types (4 types)

| Type            | File                     | Lines |
|-----------------|--------------------------|-------|
| Checkerboard    | `checkerboard_type.gd`   | 106   |
| Drifting Bar    | `drifting_bar_type.gd`   | 110   |
| Rotating Wedge  | `rotating_wedge_type.gd` | 131   |
| Expanding Ring  | `expanding_ring_type.gd` | 139   |

**Base class**: `stimulus_type_base.gd` (240 lines)

**Envelopes**: NONE (fullfield), BAR (drifting), WEDGE (rotating), RING (expanding)

**Carriers**: CHECKERBOARD, SOLID

**Projections**: Cartesian, Spherical (Marshel correction), Cylindrical

### Dataset Recording

Every rendered frame records:

- Timestamp (microseconds, via `RenderingServer.frame_post_draw`)
- Condition, sweep index, progress
- Sequencer state
- **Sequence-agnostic metadata**:
  - `condition_occurrence`: Nth time this condition shown (1-indexed)
  - `is_baseline`: True if baseline/inter-trial frame
- Paradigm-specific state (envelope position, carrier phase, etc.)

**Display Geometry** captured in metadata:
- Resolution, physical dimensions, physical source
- Visual field angles, viewing distance
- Center azimuth/elevation, projection type

**Dataset Recording Components**:

- `stimulus_dataset.gd`: 487 lines - per-frame metadata management
- `dataset_exporter.gd`: 261 lines - export to JSON/binary formats

Export format: `metadata.json` + `stimulus_frames.bin` + `schema.json`

---

## 6. Camera Integration

### Architecture

```text
CameraClient (GDScript)
    | spawns
    v
Python Daemon (daemon/main.py)
    | writes frames to
    v
Shared Memory (ring buffer)
    | read by
    v
SharedMemoryReader (Rust GDExtension)
    | returns
    v
PackedByteArray (grayscale uint8)
```

### Python Daemon Structure (`daemon/`)

| File               | Lines     | Purpose                          |
|--------------------|-----------|----------------------------------|
| `main.py`          | 282       | Entry point, main loop           |
| `shm.py`           | 161       | Shared memory writer             |
| `protocol.py`      | 118       | Memory layout definitions        |
| `config.py`        | 41        | Configuration dataclasses        |
| `setup_system.py`  | 256       | System configuration             |
| `camera_test.py`   | 671       | Camera testing infrastructure    |
| `camera/`          |           | Camera implementations           |
| - `interface.py`   | 607       | Camera interface + SyncMode      |
| - `avfoundation.py`| 393       | macOS AVFoundation camera driver |
| - `pco.py`         | 283       | PCO scientific camera driver     |
| - `enumerate.py`   | 200       | Camera enumeration script        |
| - `__init__.py`    | 55        | Camera factory                   |
| `photodiode/`      |           | Timing synchronization           |
| - `correlator.py`  | 207       | Timestamp correlation            |
| - `reader.py`      | 296       | Photodiode serial reader         |
| **Total**          | **3,587** |                                  |

Note: OpenCV camera driver has been removed.

### Rust GDExtension (`extension/src/lib.rs`) - 1,159 lines

**Classes**:

- `SharedMemoryReader`: Background thread for reading camera frames
- `MonitorInfo`: Cross-platform physical monitor dimension detection (macOS, Windows, Linux)
- `TimingAnalyzer`: Cross-stream sync analysis (~550 lines)
  - `compute_nearest_neighbor_offsets()`: Finds nearest stimulus frame for each camera frame
  - `compute_cross_correlation()`: Optimal alignment lag and correlation strength
  - `compute_relative_drift()`: Relative clock drift in ppm
  - `analyze_sync()`: Combined analysis with quality assessment
  - `check_sync_quality()`: Threshold-based quality flags

### Frame Acquisition

- CameraClient polls `get_frame_count()` each `_process()`
- Detects dropped frames when count jumps > 1
- Frame data: grayscale uint8, dimensions from Config

---

## 7. Configuration System

### SSoT Pattern

All parameters flow through Config autoload:

- No hardcoded defaults in code
- Missing JSON values = bug to fix in JSON
- Components read Config directly

### JSON Files (user://)

```text
user://
+-- hardware.json      # Camera, display, daemon
+-- preferences.json   # Window state, UI prefs
+-- stimulus.json      # All stimulus parameters
+-- protocols/         # Saved snapshots
```

### Parameter Contracts

Config stores validation metadata:

- `_min`, `_max`, `_step` suffixes for ranges
- `_unit` suffix for display
- Used by UI for spinbox configuration

---

## 8. File Organization

```text
src/
+-- main.gd                # 372 lines - app shell (entry point)
+-- autoload/              # 6 global singletons (3,298 lines total)
|   +-- config.gd          # 897 lines - configuration management
|   +-- theme.gd           # 1,093 lines - visual design system
|   +-- camera_client.gd   # 554 lines - daemon interface
|   +-- hardware_manager.gd# 320 lines - hardware enumeration
|   +-- display_validator.gd# 258 lines - display validation
|   +-- session.gd         # 176 lines - navigation/state
+-- core/                  # Shared definitions
|   +-- protocol.gd        # 74 lines - shared memory protocol
|   +-- timing_statistics.gd  # 95 lines - uniform timing metric computation
+-- camera/                # Camera subsystem
|   +-- camera_dataset.gd  # 242 lines - per-frame camera timing data
+-- domain/                # Domain logic
|   +-- acquisition_controller.gd  # 361 lines
|   +-- session_state.gd           # 96 lines
+-- stimulus/              # Stimulus subsystem
|   +-- dataset/           # Recording and export
|   |   +-- stimulus_dataset.gd    # 487 lines
|   |   +-- dataset_exporter.gd    # 261 lines
|   |   +-- texture_frame_data.gd  # 100 lines
|   |   +-- element_frame_data.gd  # 106 lines
|   |   +-- media_frame_data.gd    # 82 lines
|   +-- shaders/           # GLSL stimulus shaders
|   |   +-- includes/      # Shared shader functions
|   |   +-- texture_*.gdshader
|   +-- renderers/         # Stimulus renderers
|   |   +-- texture_renderer.gd    # 258 lines
|   |   +-- stimulus_renderer_base.gd  # 247 lines
|   |   +-- renderer_factory.gd    # 102 lines
|   +-- types/             # Stimulus type definitions
|   |   +-- stimulus_type_base.gd      # 240 lines
|   |   +-- stimulus_type_registry.gd  # 120 lines
|   |   +-- checkerboard_type.gd       # 106 lines
|   |   +-- drifting_bar_type.gd       # 110 lines
|   |   +-- rotating_wedge_type.gd     # 131 lines
|   |   +-- expanding_ring_type.gd     # 139 lines
|   |   +-- parameter_traits.gd        # 83 lines
|   +-- texture/           # Texture components
|   |   +-- carriers.gd    # 97 lines
|   |   +-- envelopes.gd   # 129 lines
|   |   +-- modulations.gd # 22 lines
|   +-- protocol/          # (empty - placeholder)
|   +-- stimulus_display.gd    # 823 lines
|   +-- stimulus_sequencer.gd  # 391 lines
|   +-- display_geometry.gd    # 448 lines
|   +-- direction_system.gd    # 88 lines
|   +-- stimulus_window.gd     # 49 lines
+-- ui/
|   +-- components/        # 17 reusable component directories
|   |   +-- app_footer/
|   |   +-- app_header/
|   |   +-- badge/
|   |   +-- base_card/
|   |   +-- button/
|   |   +-- card/
|   |   +-- checkbox_tile/
|   |   +-- divider/
|   |   +-- info_card/
|   |   +-- info_row/
|   |   +-- input/
|   |   +-- layout/
|   |   +-- navigation_bar/
|   |   +-- section_header/
|   |   +-- smooth_scroll_container/
|   |   +-- status_pill/
|   |   +-- styled_input/
+-- ui/
|   +-- screens/           # Screen implementations
|   |   +-- base_screen.gd # 48 lines - abstract base
|   |   +-- setup/         # 1,063 lines
|   |   +-- focus/         # 526 lines
|   |   +-- stimulus/      # 590 lines + 1,317 lines supporting cards
|   |   +-- run/           # 1,142 lines
|   |   +-- analyze/       # 300 lines
|   +-- tools/             # Diagnostic tools
|   |   +-- timing_diagnostics.gd  # 950 lines - timing validation UI
|   +-- splash/            # Splash screen
|   |   +-- splash.gd      # 83 lines
|   +-- theme/             # UI shaders
|   |   +-- shaders/       # button.gdshader, ceramic.gdshader, etc.
|   +-- scene_registry.gd  # 37 lines - preloaded scenes
+-- util/                  # (empty)
+-- utils/                 # Utilities
    +-- file_utils.gd      # 107 lines
    +-- format_utils.gd    # 40 lines

daemon/                    # Python camera daemon (3,587 lines total)
+-- main.py                # 282 lines - entry point
+-- shm.py                 # 161 lines - shared memory writer
+-- protocol.py            # 118 lines - memory layout
+-- config.py              # 41 lines - configuration
+-- setup_system.py        # 256 lines - system configuration
+-- camera_test.py         # 671 lines - camera testing infrastructure
+-- camera/                # Camera implementations
|   +-- interface.py       # 607 lines - interface + SyncMode
|   +-- avfoundation.py    # 393 lines - macOS AVFoundation
|   +-- pco.py             # 283 lines - PCO scientific camera
|   +-- enumerate.py       # 200 lines
|   +-- __init__.py        # 55 lines - factory
+-- photodiode/            # Timing synchronization
    +-- correlator.py      # 207 lines - timestamp correlation
    +-- reader.py          # 296 lines - photodiode serial reader

extension/                 # Rust GDExtension
+-- src/
    +-- lib.rs             # 1,159 lines - SharedMemoryReader + MonitorInfo + TimingAnalyzer
```

---

## 9. Anti-Patterns & Code Smells

### God Objects (CRITICAL)

Large classes handling too many responsibilities:

| File | Lines | Responsibilities |
|------|-------|------------------|
| `setup_screen.gd` | 1,063 | Hardware enumeration, monitor/camera selection, display validation, session config, scrollbar quirks |
| `run_screen.gd` | 1,142 | Stimulus window lifecycle, camera preview, metrics display, dataset export, acquisition sync |
| `theme.gd` | 1,093 | Massive StyleBox factory with mechanical boilerplate |

**Symptoms**:
- Difficult to understand full scope of each class
- Changes to one area may affect others unexpectedly
- Hard to test individual behaviors in isolation
- Mixed abstraction levels (UI + domain logic)

**Locations**:
- `src/ui/screens/setup/setup_screen.gd`
- `src/ui/screens/run/run_screen.gd`
- `src/autoload/theme.gd`

---

### ~~Silent Error Handling~~ (RESOLVED 2026-02-03)

**Resolution**: Created `ErrorHandler` autoload with modal `ErrorDialog` component.

- Severity levels (INFO, WARNING, ERROR, CRITICAL)
- Category classification for filtering
- Error history for diagnostics
- User-facing modal dialogs for ERROR/CRITICAL
- Migrated key error sites in camera_client, hardware_manager, run_screen

---

### ~~Config SSoT Violation~~ (RESOLVED 2026-02-03)

**Resolution**: Split Config into Settings (persistent) and Session (runtime):

| Data Type | Location | Examples |
|-----------|----------|----------|
| Persistent settings | `Settings` autoload | viewing_distance_cm, display_fps_divisor |
| Runtime state | `Session` autoload | _selected_camera, _selected_display, validation state |
| Computed values | Stay with their data source | visual_field_width_deg (in Settings, reads Session) |

- `Config` renamed to `Settings` (clarifies persistent-only role)
- Runtime hardware selection moved to `Session` with `camera_selected`/`display_selected` signals
- Clear API: Settings for persistent, Session for runtime

---

### Feature Envy (MODERATE)

**Observation**: Screens directly access 5-6 autoloads, creating tight coupling.

**Example** - SetupScreen uses:
- HardwareManager
- DisplayValidator
- CameraClient
- Settings
- Session
- AppTheme
- SceneRegistry

**Symptoms**:
- If Config API changes, 10+ files need updates
- Hidden dependencies (not visible in function signatures)
- Difficult to test without full autoload initialization

**Example** (in run_screen.gd):

```gdscript
# Camera jitter calculation - should this be in CameraClient or a MetricsService?
var sum := 0.0
var sum_sq := 0.0
for i in range(_cam_frame_times_ms.size() - count, _cam_frame_times_ms.size()):
    var delta := _cam_frame_times_ms[i]
    sum += delta
    sum_sq += delta * delta
```

---

### Inappropriate Intimacy (MODERATE)

**Observation**: Components reach into each other's internals rather than using clean interfaces.

**Examples**:
- SetupScreen directly manipulates Config runtime state
- RunScreen uses `has_method()` checks instead of interfaces
- StimulusDisplay reads Config in hot loop (every frame)

**Symptoms**:
- Tight coupling between unrelated components
- Changes ripple unexpectedly
- No clear contracts between layers

---

### Duplicated Code (MODERATE)

**Observation**: Similar patterns repeated throughout the codebase.

**Examples**:
- InfoRow creation pattern repeated 10+ times
- Metrics display has near-identical patterns for camera and stimulus
- Validation logic duplicated across screens

**Mitigation**: Factory functions would reduce this significantly.

---

### Long Methods (MODERATE)

**Observation**: Several methods exceed reasonable length for comprehension.

| Method | Lines | Location |
|--------|-------|----------|
| `_build_ui()` methods | 150+ | Multiple screens |
| `_update_stimulus_metrics()` | 120+ | run_screen.gd |

**Symptoms**:
- Hard to understand at a glance
- Multiple responsibilities in single method
- Should be split into focused helpers

---

### State Management Issues (MODERATE)

**Observation**: Multiple sources of truth for runtime state create confusion.

**Examples**:
- `_running` in RunScreen is local (lost if screen closes)
- No way to query acquisition state from other screens
- Camera connection state is implicit in CameraClient

**Problems**:
- State can become inconsistent across components
- No single authoritative source for "is acquisition running?"
- Recovery from unexpected states is difficult

---

### Signal Complexity (MODERATE)

**Observation**: Three-level signal chains make debugging difficult.

**Example**: Sequencer -> Controller -> Screen

**Symptoms**:
- Hard to trace why screen shows certain state
- Trending toward spaghetti architecture
- Disconnection logic must mirror connection locations
- Signal connections spread across `_ready()`, `_connect_signals()`, `_load_state()`, and inline

---

### Primitive Obsession

**Observation**: Frame data passed as Dictionary rather than typed class.

**Example**:

```gdscript
var frame_data := dataset.get_current_frame_data()  # Returns Dictionary
var condition: String = frame_data["condition"]     # Runtime key access
```

**Symptoms**:
- No compile-time key validation
- IDE cannot provide autocomplete
- Typos in keys fail at runtime

---

### Magic Strings for States

**Observation**: Sequencer states stored as strings in dataset.

**Example**:

```gdscript
data["state"] = "baseline_start"  # String literal
data["state"] = "stimulus"
data["state"] = "inter_stimulus"
```

**Symptoms**:
- Typos not caught at compile time
- No autocomplete for valid values

---

### No Test Coverage

**Observation**: No test files found in codebase.

**Symptoms**:
- Behavior undocumented except through code reading
- Refactoring carries high risk
- Regression detection relies on manual testing

---

### Housekeeping Issues

**Empty/Duplicate Directories**:
- `src/util/` - empty
- `src/utils/` - contains file_utils.gd and format_utils.gd
- `src/stimulus/protocol/` - empty placeholder

---

## 10. What's Working Well

Despite the issues above, the architecture has several strong foundations:

### Clean Daemon/Godot Split

The shared memory boundary between Python daemon and Godot is well-designed:
- Clear protocol definition in both languages
- Ring buffer abstraction hides complexity
- Rust GDExtension provides efficient, thread-safe access
- Daemon lifecycle management is robust (spawn, monitor, cleanup)

### Good Naming Conventions

Consistent naming throughout the codebase:
- Files match their class names
- Methods use clear verb prefixes (`_build_`, `_update_`, `_on_`)
- Signals follow `noun_verb` pattern
- Private members use underscore prefix

### Parameter Contracts

Config stores validation metadata alongside values:
- `_min`, `_max`, `_step` suffixes for ranges
- `_unit` suffix for display
- Used by UI for automatic spinbox configuration
- Self-documenting parameter constraints

### Renderer Factory Pattern

The stimulus rendering system uses appropriate abstraction:
- `RendererFactory` creates correct renderer for stimulus type
- Renderers implement common interface
- Easy to add new stimulus types
- Shader system is well-organized

### Right-Sized Domain Layer

Domain layer is appropriately thin (not bloated with unnecessary abstractions):
- `AcquisitionController` coordinates what needs coordinating
- `SessionState` holds session data without overengineering
- Business logic is close to where it's used

### BaseScreen Template Pattern

All screens extend `BaseScreen` with consistent lifecycle:
- `_build_ui()` -> `_connect_signals()` -> `_load_state()` -> `_validate()`
- Easy to understand new screens
- Consistent behavior across screens

### Consistent Signal Conventions

Signal usage follows predictable patterns:
- Config signals include section/key/value payload
- Sequencer signals provide relevant state
- Screens emit `validation_changed` for navigation

### Theme as Centralized Style SSoT

Despite its size, `theme.gd` provides real value:
- Single place to change visual design
- Consistent styling across all components
- Programmatic approach allows design system enforcement

### Well-Separated Dataset Classes

Data recording is cleanly separated:
- `CameraDataset` for camera timing data
- `StimulusDataset` for per-frame stimulus metadata
- `DatasetExporter` handles serialization
- Clear schema definitions for exports

---

## 11. Recommended Refactoring

### Priority 1 - Critical (Do First)

#### 1. Fix Silent Errors (2-3 weeks)

Create centralized error handling with user feedback:

**Implementation**:
- Create `ErrorHandler` autoload
- Define error severity levels (info, warning, error, critical)
- Propagate errors to UI with appropriate dialogs
- Allow retry or abort for recoverable errors
- Log all errors for debugging

**Files to modify**:
- Create `src/autoload/error_handler.gd`
- Update `stimulus_display.gd`, `run_screen.gd`, `setup_screen.gd`
- Add error dialog component to UI

#### 2. Split Config (2 weeks)

Separate Config into distinct responsibilities:

**New structure**:
```
Settings (persistent, saved to JSON)
  - camera_type, viewing_distance, stimulus params

SessionState (runtime, cleared on exit)
  - _selected_camera, _selected_display, validation state

ComputedValues (calculated on demand)
  - total_sweeps, visual_field_width_deg
```

**Benefits**:
- Clear API contract about persistence
- Runtime state explicitly transient
- Type safety for each category

---

### Priority 2 - High (Next)

#### 3. Split RunScreen (1.5 weeks)

Extract focused controllers from 1,142-line monolith:

| Component | Lines | Responsibility |
|-----------|-------|----------------|
| `RunScreen` (coordinator) | ~400 | Layout, lifecycle, delegation |
| `CameraPreviewController` | ~150 | Camera preview display and updates |
| `MetricsDisplayController` | ~200 | FPS, jitter, drift display |
| `StimulusWindowManager` | ~150 | Stimulus window creation/management |
| `DatasetExportController` | ~150 | Export workflow and progress |

#### 4. Split SetupScreen (1.5 weeks)

Extract focused components from 1,063-line monolith:

| Component | Lines | Responsibility |
|-----------|-------|----------------|
| `SetupScreen` (coordinator) | ~400 | Layout, lifecycle, delegation |
| `HardwarePanel` | ~300 | Camera/monitor enumeration and selection |
| `SessionPanel` | ~150 | Session configuration options |
| `ValidationController` | ~200 | Display validation workflow |

---

### Priority 3 - Medium (Polish)

#### 5. Extract Metrics Formatting (3 days)

Create `MetricsFormatter` class:
- Centralize FPS, jitter, drift formatting
- Share between camera and stimulus metrics
- Consistent precision and units

#### 6. Generate Theme StyleBoxes (1 week)

Replace mechanical boilerplate with data-driven generation:
- Define style variants in data structure
- Generate StyleBoxes programmatically
- Reduce theme.gd from 1,093 lines to ~400

#### 7. Create Formal Interfaces (1 week)

Define explicit contracts:
- `IStimulusDisplay` - renderer interface
- `IDataset` - dataset recording interface
- `IMetricsProvider` - metrics source interface

---

### Priority 4 - Nice to Have

#### 8. Unit Tests (2-3 weeks)

Add test coverage for core logic:
- Config parameter validation
- Sequencer state machine
- DisplayGeometry calculations
- Metrics computation

#### 9. Integration Tests (2-3 weeks)

Add end-to-end workflow tests:
- Setup -> Focus -> Stimulus -> Run -> Analyze flow
- Hardware enumeration scenarios
- Export verification

---

## Architecture Health Summary

| Metric | Value |
|--------|-------|
| **Overall Score** | 7.2/10 |
| **Trajectory** | Needs attention before screens become unmaintainable |
| **Technical Debt Level** | Moderate, mounting |
| **Refactoring Timeline** | ~10 weeks for significant improvement |

**Strengths**: Good foundations, clean boundaries, consistent conventions

**Weaknesses**: God objects, silent errors, config confusion

**Recommendation**: Address Priority 1 items (silent errors, config split) before adding new features. The codebase is at a tipping point where continued feature development without refactoring will accelerate technical debt accumulation.

---

## 12. Signal Catalog

### Config Signals

| Signal                | Payload                                          | Purpose                   |
|-----------------------|--------------------------------------------------|---------------------------|
| `hardware_changed`    | `section: String, key: String, value: Variant`   | Hardware config updated   |
| `stimulus_changed`    | `section: String, key: String, value: Variant`   | Stimulus params updated   |
| `preferences_changed` | `section: String, key: String, value: Variant`   | Preferences updated       |
| `config_loaded`       | none                                             | Initial load complete     |
| `config_saved`        | `path: String`                                   | File saved                |
| `snapshot_saved`      | `name: String, path: String`                     | Protocol snapshot saved   |
| `snapshot_loaded`     | `name: String`                                   | Protocol snapshot loaded  |
| `snapshot_deleted`    | `name: String`                                   | Protocol snapshot deleted |

### Session Signals

| Signal           | Payload  | Purpose             |
|------------------|----------|---------------------|
| `screen_changed` | `Screen` | Navigation occurred |

### CameraClient Signals

| Signal                        | Payload  | Purpose                   |
|-------------------------------|----------|---------------------------|
| `connection_changed`          | `bool`   | Connected/disconnected    |
| `daemon_state_changed`        | `bool`   | Daemon running/stopped    |
| `connection_failed`           | `String` | Connection error message  |
| `connection_attempt_complete` | `bool`   | Async connection finished |

### HardwareManager Signals

| Signal                | Payload             | Purpose            |
|-----------------------|---------------------|--------------------|
| `cameras_enumerated`  | `Array[Dictionary]` | Camera list ready  |
| `monitors_enumerated` | `Array[Dictionary]` | Monitor list ready |
| `enumeration_failed`  | `String`            | Enumeration error  |

### StimulusSequencer Signals

| Signal               | Payload               | Purpose                 |
|----------------------|-----------------------|-------------------------|
| `state_changed`      | `State, State`        | State transition        |
| `sweep_started`      | `int, String`         | Sweep began             |
| `sweep_completed`    | `int, String`         | Sweep finished          |
| `direction_changed`  | `String, String`      | Direction change        |
| `sequence_started`   | none                  | Sequence began          |
| `sequence_completed` | none                  | All sweeps done         |
| `progress_updated`   | `float, float, float` | Elapsed, total, percent |

### AcquisitionController Signals

| Signal                    | Payload                   | Purpose                    |
|---------------------------|---------------------------|----------------------------|
| `state_changed`           | `State`                   | Controller state changed   |
| `progress_updated`        | `float, float, float`     | elapsed, total, percent    |
| `sweep_started`           | `int, String`             | Sweep began                |
| `sweep_completed`         | `int, String`             | Sweep finished             |
| `direction_changed`       | `String, String`          | Direction change           |
| `sequencer_state_changed` | `State, State`            | Sequencer state changed    |
| `acquisition_started`     | none                      | Acquisition began          |
| `acquisition_completed`   | `AcquisitionMetrics`      | Acquisition finished       |
| `acquisition_stopped`     | `AcquisitionMetrics`      | User stopped early         |

### StimulusDisplay Signals

| Signal           | Payload  | Purpose               |
|------------------|----------|-----------------------|
| `sweep_completed`| `String` | Sweep direction       |
| `state_changed`  | `String` | New state name        |
| `frame_recorded` | `int`    | Frame index           |

### StimulusDataset Signals

| Signal              | Payload | Purpose              |
|---------------------|---------|----------------------|
| `frame_recorded`    | `int`   | Frame index          |
| `recording_started` | none    | Recording began      |
| `recording_stopped` | none    | Recording ended      |

### BaseScreen Signals

| Signal               | Payload | Purpose                      |
|----------------------|---------|------------------------------|
| `validation_changed` | `bool`  | Screen validity changed      |
| `request_next_screen`| none    | Request navigation forward   |

---

## 13. Scene and Resource Files

| Type         | Count |
|--------------|-------|
| .tscn        | 25    |
| .tres        | 0     |
| .gdshader    | 11    |
| .gdshaderinc | 4     |

---

End of Architecture Audit
