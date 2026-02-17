# OpenISI Architecture Issues

Tracking document for architectural improvements identified during code audits.

---

## Reference Patterns (What's Working Well)

These serve as models for refactoring other areas:

- **Stimulus shaders** (`src/stimulus/shaders/`) - Good modularity, DRY, SoC:
  - Uses `.gdshaderinc` includes for shared code
  - `common.gdshaderinc` - uniforms, utilities
  - `projection.gdshaderinc` - coordinate transforms
  - `carriers.gdshaderinc` - pattern generation
  - `modulation.gdshaderinc` - temporal modulation
  - All 4 envelope shaders follow consistent structure

- **Theme constants SSoT** (`src/autoload/theme.gd`) - Single source of truth:
  - All colors, spacing, radii, shadows centralized
  - Preloaded shaders and fonts as constants
  - Good use of Godot theme type variations (LabelTitle, PanelWell, etc.)
  - Public API for dynamic styles (get_status_color, create_status_badge_style)

---

## How to Use This Document

**Priority Levels**: `P0` Critical | `P1` High | `P2` Medium | `P3` Low

**Status**: `[ ]` Open | `[~]` In Progress | `[x]` Complete

---

## 🎯 PRIORITY: Real-Time Timing

**Detailed audit:** `docs/audits/realtime_performance.md`

These issues affect scientific validity and must be fixed first. Must work for both MacBook webcam (development) and scientific cameras (production).

### Timing Architecture (Frame-Locking)

- [x] **P0** Wall-clock timing not frame-locked **RESOLVED**
  - Refactored to use frame counters instead of wall-clock time
  - All durations stored as frame counts (`_baseline_start_frames`, `_sweep_duration_frames`, etc.)
  - `advance_frame()` called at vsync boundary from StimulusDisplay
  - State transitions only occur when frame count reaches duration
  - **Files**: `stimulus_sequencer.gd`

- [x] **P0** Continuous progress not frame-quantized **RESOLVED**
  - During acquisition, progress comes from `_sequencer.get_state_progress()` (frame-quantized)
  - `_elapsed_sec` comes from `_sequencer.get_elapsed_time()` (frame-quantized)
  - Continuous calculation only used for preview mode (acceptable)
  - **Files**: `stimulus_display.gd` lines 562-568

- [x] **P1** Strobe phase not frame-locked **RESOLVED (2026-02-03)**
  - Added `_quantize_to_frame()` helper in `texture_renderer.gd`
  - Shader `time_sec` uniform now receives frame-quantized time
  - `get_paradigm_state()` strobe phase also uses quantized time
  - **Files**: `src/stimulus/renderers/texture_renderer.gd`

### Timestamp Precision

- [x] **P0** Millisecond precision for camera, microsecond for stimulus **RESOLVED**
  - Both CameraDataset and StimulusDataset now use microsecond timestamps
  - `src/camera/camera_dataset.gd`: `timestamps_us: PackedInt64Array`
  - `src/stimulus/dataset/stimulus_dataset.gd`: `timestamps_us: PackedInt64Array`
  - **Status**: Complete - uniform microsecond precision

- [x] **P0** CPU timestamps not GPU presentation time **RESOLVED**
  - Now using `RenderingServer.frame_post_draw` signal for timestamps
  - Timestamp captured AFTER frame rendering completes, closest to vsync
  - **Files**: `stimulus_display.gd`
  - **Fix**: Connected to `frame_post_draw` signal, capture timestamp in callback

- [x] **P0** Missing timestamp propagation **RESOLVED**
  - Hardware timestamps now available via `SharedMemoryReader.get_latest_timestamp_us()`
  - Camera and stimulus datasets both record hardware timestamps
  - TimingStatistics class provides uniform metric computation for both streams
  - **Files**: `stimulus_dataset.gd`, `camera_dataset.gd`, `timing_statistics.gd`
  - **Status**: Complete

- [x] **P0** macOS Apple Silicon has no software vsync timestamps **RESOLVED**
  - `VK_GOOGLE_display_timing` via MoltenVK returns garbage (3990µs jitter)
  - `MTLDrawable.presentedTime` is `API_UNAVAILABLE(macos)` - iOS/tvOS only
  - `CVDisplayLink` is just a timer, not actual vsync events
  - **Solution**: Photodiode-based hardware timestamps
    - Sync patch (50×50 white/black square) toggles each frame
    - Photodiode attached to screen detects actual photon emission
    - Arduino captures timestamp with µs precision
    - **Files**: `daemon/photodiode/`, `hardware/arduino_photodiode/`, `stimulus_display.gd`
    - **Docs**: `docs/design/hardware_timestamps.md`
  - **Status**: Complete - photodiode protocol and reference implementation ready

### Camera Pipeline

- [x] **P0** Polling-based frame detection adds latency **PARTIALLY RESOLVED**
  - Reduced poll interval from 500µs to 100µs
  - Mean latency now ~50µs, jitter floor: 0-100µs
  - **Files**: `extension/src/lib.rs`
  - **Note**: Full fix (semaphore/condition variable) deferred; 100µs is acceptable

- [x] **P0** ~~Software timestamps only (not hardware)~~ **RESOLVED**
  - Hardware timestamps now flow through shared memory
  - `daemon/protocol.py` includes `latest_timestamp_us` (u64) in control region
  - `extension/src/lib.rs` exposes `get_latest_timestamp_us()` to GDScript
  - AVFoundation camera extracts CMSampleBuffer.presentationTimeStamp
  - CameraDataset and StimulusDataset both use `get_full_statistics()` via TimingStatistics
  - **Status**: Complete

- [ ] **P1** Mutex lock contention every frame
  - `extension/src/lib.rs:172-179` - worker thread locks on every frame
  - **Files**: `extension/src/lib.rs`
  - **Fix**: Use lock-free ring buffer

- [ ] **P1** Buffer reallocation per frame
  - `extension/src/lib.rs:174-175` - `clear()` + `extend_from_slice()` allocates
  - **Files**: `extension/src/lib.rs`
  - **Fix**: Pre-allocate and reuse buffer

### Synchronization

- [x] **P0** No hardware synchronization between camera and stimulus **RESOLVED (POST_HOC)**
  - Two sync modes defined: TRIGGERED (hardware) and POST_HOC (timestamp correlation)
  - POST_HOC mode fully implemented with comprehensive cross-stream sync analysis:
    - Nearest-neighbor offset analysis (handles different framerates)
    - Cross-correlation for optimal alignment detection
    - Relative clock drift measurement (ppm)
  - **Files**: `daemon/camera/interface.py`, `extension/src/lib.rs` (TimingAnalyzer class)
  - **Status**: POST_HOC mode complete; TRIGGERED pending hardware

- [x] **P0** Cross-stream sync analysis implemented **RESOLVED**
  - `extension/src/lib.rs`: TimingAnalyzer class with:
    - `compute_nearest_neighbor_offsets()` - finds nearest stimulus frame for each camera frame
    - `compute_cross_correlation()` - optimal alignment lag and correlation strength
    - `compute_relative_drift()` - relative clock drift in ppm
    - `analyze_sync()` - combined analysis with quality assessment
  - `src/core/timing_statistics.gd`: TimingStatistics class for uniform per-stream metrics
  - `src/ui/tools/timing_diagnostics.gd`: Full diagnostic UI for both streams and sync
  - **Status**: Complete

---

## Architecture: Workspace Refactor

Replace phase-based navigation with flexible workspace of composable panels.

### Phase to Screen Refactor (Completed 2026-02-03)

- [x] **P1** Rename phase infrastructure to screen
  - Renamed `src/ui/phases/` to `src/ui/screens/`
  - Renamed `base_phase.gd` to `base_screen.gd` (BasePhase → BaseScreen)
  - Deleted `phase_indicator/` component (replaced by NavigationBar)
  - Updated `Session.gd` - Screen enum retained for navigation

- [x] **P2** Remove "phase" terminology from codebase
  - `PHASE_CONTENT_PADDING_H` → `SCREEN_CONTENT_PADDING_H`
  - `PHASE_PILL_*` → `NAV_PILL_*` constants
  - `get_phase_color()` → deleted
  - `request_next_phase` signal → `request_next_screen`
  - Note: Kept mathematical "phase" (carrier_phase, strobe_phase, etc.)

### Panel Architecture

- [ ] **P1** Create composable panels organized by concern
  ```
  src/ui/panels/
  ├── camera/      # connection, preview, metrics
  ├── stimulus/    # config, preview
  ├── acquisition/ # run controls, progress, metrics
  ├── hardware/    # monitor, camera selection
  └── session/     # metadata, directory
  ```

- [ ] **P1** Extract panels from current monolith files
  - From `run_phase.gd` (1094 lines): camera preview/metrics, acquisition control, stimulus metrics
  - From `setup_phase.gd` (724 lines): monitor config, camera selector, session config
  - From `focus_phase.gd` (527 lines): camera preview, exposure controls

- [ ] **P2** Create workspace layout system
  - `main.gd` orchestrates panel arrangement
  - Predefined layouts for common tasks (or user-configurable later)

---

## Architecture: Autoloads

### God Objects

- [x] **P1** `Config` SSoT violation - runtime state mixed with persistent settings **RESOLVED (2026-02-03)**
  - Runtime state (camera/display selection) moved to `Session` autoload
  - `Config` renamed to `Settings` to clarify it holds only persistent settings
  - Settings now ~750 lines (hardware, preferences, stimulus params only)
  - Session holds runtime state with `camera_selected`/`display_selected` signals

- [ ] **P1** `Session` mixes two responsibilities
  - Screen routing/navigation (lines 90-136)
  - Session state management (lines 142-177)
  - **Fix**: Split into `NavigationRouter` and `SessionStateManager`

### State Machine Issues

- [x] **P1** `CameraClient` state machine uses 6 booleans **RESOLVED (2026-02-03)**
  - Created `ConnectionState` enum (IDLE, STARTING_DAEMON, POLLING_SHM, CONNECTED, RETRYING, FAILED, CLEANUP)
  - Added `_transition_to()` with transition validation
  - Added `get_state()` and `get_state_name()` methods
  - **Files**: `src/autoload/camera_client.gd`

### Duplication

- [x] **P1** Python path resolution duplicated 3x **RESOLVED (2026-02-03)**
  - Created `PythonUtils` utility class in `src/utils/python_utils.gd`
  - Methods: `get_venv_python_path()`, `get_script_path()`, `venv_exists()`, `get_shell_exe()`
  - Updated `camera_client.gd` and `hardware_manager.gd` to use PythonUtils
  - **Files**: `src/utils/python_utils.gd`, `src/autoload/camera_client.gd`, `src/autoload/hardware_manager.gd`

### Thread Safety

- [x] **P2** Config reads in threads not safe **RESOLVED (2026-02-03)**
  - Refactored `_start_daemon_async()` to snapshot all config values on main thread
  - Pass snapshotted dictionary to `_start_daemon_threaded()` via `.bind(config)`
  - Thread function no longer accesses Session/Settings autoloads
  - **Files**: `src/autoload/camera_client.gd`

- [x] **P1** HardwareManager thread safety issues **RESOLVED (2026-02-03)**
  - Added mutex protection for `_camera_devices` access
  - Fixed race condition when checking/accessing camera list
  - Changed thread cleanup to non-blocking pattern with `is_alive()` check
  - All accessor methods now return thread-safe copies
  - **Files**: `src/autoload/hardware_manager.gd`

- [ ] **P2** Thread cleanup duplicated and racy
  - Multiple wait points for same thread could deadlock
  - **Fix**: Centralize thread lifecycle management

- [ ] **P2** `HardwareManager` cache never invalidates
  - No support for hardware hot-plug
  - **Fix**: Add `refresh()` method and cache invalidation

---

## Architecture: State Management

- [x] **P0** Window state persistence not implemented **RESOLVED (2026-02-03)**
  - Added `Settings.apply_window_state()` call in `main.gd _ready()`
  - Added `Settings.update_window_state()` call in `main.gd _cleanup()`
  - **Files**: `src/main.gd`

- [ ] **P1** Session metadata not persisted
  - `session_state.gd:8-10` defines `session_name`, `session_dir`, `created_at`
  - Never saved to Config or JSON
  - **Fix**: Persist to preferences, implement session browser

- [x] **P1** Stimulus snapshot never captured (dead code) **RESOLVED (2026-02-03)**
  - Added `Session.state.capture_stimulus_snapshot()` call at start of `_start_acquisition()` in run_screen.gd
  - **Files**: `src/ui/screens/run/run_screen.gd`

- [x] **P2** `SessionState.hardware` never populated **RESOLVED (2026-02-03)**
  - Added `capture_hardware_state()` method to SessionState
  - Called in `run_screen.gd _start_acquisition()` alongside stimulus snapshot
  - Captures: camera_index, camera_type, monitor_index, exposure_us
  - **Files**: `src/domain/session_state.gd`, `src/ui/screens/run/run_screen.gd`

---

## Architecture: Signals

- [x] **P1** Settings changes don't propagate to UI **RESOLVED (2026-02-03)**
  - Connected stimulus_screen to `Settings.stimulus_changed` signal
  - Added re-entrancy guard (`_updating_settings`) to prevent feedback loops
  - Full UI refresh on bulk updates (empty section = `set_stimulus_data()` called)
  - **Files**: `src/ui/screens/stimulus/stimulus_screen.gd`

- [x] **P1** No navigation guard during acquisition **RESOLVED (2026-02-03)**
  - Added `acquisition_running: bool` flag to Session autoload
  - Added guard in `navigate_to()` to block navigation when acquisition is active
  - RunScreen sets flag on acquisition start/stop
  - **Files**: `src/autoload/session.gd`, `src/ui/screens/run/run_screen.gd`

- [ ] **P2** Excessive polling instead of events
  - RunPhase polls `CameraClient.get_frame()` in `_process()`
  - Should emit `frame_available` signal
  - **Fix**: Convert to event-driven architecture

- [ ] **P2** Signal duplication between Sequencer and Controller
  - Both emit `sweep_started(index, direction)`
  - **Fix**: Controller should wrap, not duplicate

---

## Code Quality: UI Shaders

### Duplication

- [x] **P1** `rounded_rect_sdf()` duplicated in 6 shaders **RESOLVED (2026-02-03)**
  - Extracted to `src/ui/theme/shaders/includes/sdf.gdshaderinc`
  - Updated all 5 shaders that use it: ceramic, ceramic_gradient, button, input_field, rounded_mask
  - **Files**: `src/ui/theme/shaders/includes/sdf.gdshaderinc`

- [x] **P1** `dither()` duplicated in 5 shaders **RESOLVED (2026-02-03)**
  - Extracted to `src/ui/theme/shaders/includes/common.gdshaderinc`
  - Also includes `aa_edge()` utility function
  - Updated all shaders that use it: ceramic, ceramic_gradient, button, input_field, scroll_fade
  - **Files**: `src/ui/theme/shaders/includes/common.gdshaderinc`

- [ ] **P1** Rim highlight logic duplicated with variations
  - Same pattern in 4 shaders with slight differences
  - **Fix**: Extract to `includes/rim_highlights.gdshaderinc`

- [x] **P1** Create `src/ui/theme/shaders/includes/` directory **RESOLVED (2026-02-03)**
  - Created `src/ui/theme/shaders/includes/` with sdf.gdshaderinc and common.gdshaderinc
  - All UI shaders now use `#include` for shared code
  - **Files**: `src/ui/theme/shaders/includes/`

### Organization

- [ ] **P2** `button.gdshader` is 270-line monolith
  - Secondary mode, Nightlight mode, 4 button states, neumorphic effects
  - Consider splitting into composable parts

- [ ] **P2** Inconsistent rect size acquisition
  - Some use `uniform vec2 rect_size`, some use `1.0 / TEXTURE_PIXEL_SIZE`
  - **Fix**: Standardize on one approach

---

## Code Quality: Components

### Duplication

- [ ] **P1** NavigationBar/PhaseIndicator duplication (~180 lines each)
  - Nearly identical tab/pill creation logic
  - **Fix**: Extract generic `PillContainer` or `TabBar` base class

- [ ] **P1** Ceramic shader setup duplicated
  - base_card.gd, status_pill.gd have identical ~10 lines
  - **Fix**: Extract to utility function in AppTheme

- [ ] **P2** Styled wrapper pattern duplicated 4x
  - StyledButton (329), StyledLineEdit (110), StyledOptionButton (135), StyledSpinBox (149)
  - All: ColorRect background + shader + inner control
  - **Fix**: Extract `ShaderWrappedControl` base class

### Coupling

- [x] **P1** CameraClient ↔ Session bidirectional dependency **RESOLVED (2026-02-03)**
  - Removed direct Session modification from CameraClient (format mismatch handling)
  - CameraClient now only emits `format_mismatch_detected` signal
  - SetupScreen handles signal and updates Session properties
  - Used `call_deferred` for auto-retry so signal handlers run first
  - **Files**: `src/autoload/camera_client.gd`, `src/ui/screens/setup/setup_screen.gd`

- [x] **P1** Settings → Session implicit coupling **RESOLVED (2026-02-03)**
  - Created `GeometryCalculator` domain class with pure static functions
  - Removed `visual_field_width_deg` and `visual_field_height_deg` properties from Settings
  - Settings.sweep_duration_sec now uses GeometryCalculator with explicit Session values
  - Session reads are now contained to single point in Settings computed properties
  - **Files**: `src/domain/geometry_calculator.gd`, `src/autoload/settings.gd`

- [ ] **P2** Navigation components coupled to Session
  - AppHeader, AppFooter, NavigationBar directly subscribe
  - **Fix**: Inject navigation state or use event bus

- [ ] **P2** Inconsistent AppTheme access
  - Some use `AppTheme.CONSTANT`, some use `get_node_or_null()`
  - **Fix**: Standardize on direct autoload reference

### Missing

- [ ] **P2** SceneRegistry incomplete
  - Missing: StyledLineEdit, StyledOptionButton, StyledSpinBox, InfoCard, SmoothScrollContainer
  - **Fix**: Add to registry or remove preloading pattern

---

## Code Quality: Theme

- [ ] **P2** `theme.gd` is 1100-line monolith
  - Color palette, spacing, typography, shadows, ceramic styling, animation timing, StyleBox factories
  - **Fix**: Split into focused modules (palette.gd, spacing.gd, shadows.gd)

- [ ] **P2** Rim color construction repeated
  - `Color(AppTheme.CREAM.r, ..., AppTheme.RIM_LIGHT_ALPHA)` pattern
  - **Fix**: Pre-compute `RIM_TOP_COLOR`, `RIM_BOTTOM_COLOR` constants

- [ ] **P3** Status enum defined twice
  - StatusBadge.Status enum vs StatusPill.status string
  - **Fix**: Consolidate to single Status enum

---

## Recently Completed

### Display Geometry (2026-02-03)

- [x] **P0** EDID physical dimension detection via MonitorInfo Rust extension
- [x] **P0** User override tracking (preserves original EDID values when overridden)
- [x] **P0** Removed unsafe DPI-based fallback (OS returns logical DPI, not physical)
- [x] **P0** Display dimension hints in Setup UI (EDID status, manual entry guidance)
- [x] **P0** Full display geometry captured in dataset metadata

### Sequence-Agnostic Metadata (2026-02-03)

- [x] **P0** `condition_occurrence` tracking in StimulusSequencer
- [x] **P0** `is_baseline` accessor for frame classification
- [x] **P0** Both fields exported in dataset (binary and schema)
- [x] **P0** `record_frame()` signature updated to include new fields

### Nameless Protocol Architecture (2026-02-03)

- [x] **P1** Removed protocol name/description from StimulusDataset
- [x] **P1** Protocols only named when explicitly saved to share

---

## Backlog

Empty sections for future issues:
- Type Safety
- Testing
- Documentation

---

## Recently Completed (Continued)

### Error Handling Infrastructure (2026-02-03)

- [x] **P1** Created `ErrorHandler` autoload with:
  - Severity levels (INFO, WARNING, ERROR, CRITICAL)
  - Category classification (HARDWARE, CAMERA, DISPLAY, CONFIG, ACQUISITION, STIMULUS, EXPORT)
  - Error codes for programmatic handling
  - Error history tracking for diagnostics
  - Signals for UI integration (`error_occurred`, `error_dismissed`)

- [x] **P1** Created `ErrorDialog` component:
  - Modal dialog with severity icons and color coding
  - Expandable details section
  - Dismiss and optional Retry actions
  - Error queue management (shows one at a time)

- [x] **P1** Migrated key error sites:
  - `camera_client.gd` - Connection, daemon, shared memory errors
  - `hardware_manager.gd` - Enumeration failures
  - `run_screen.gd` - Acquisition and export errors
  - `main.gd` - Display selection errors

- [x] **P1** Standardized domain error handling **RESOLVED (2026-02-03)**
  - Updated `acquisition_controller.gd` - Not initialized error uses ErrorHandler
  - Updated `stimulus_display.gd` - Critical vsync timestamp errors use ErrorHandler with CRITICAL severity
  - Updated `stimulus_sequencer.gd` - Empty sweep sequence error uses ErrorHandler
  - **Files**: `src/domain/acquisition_controller.gd`, `src/stimulus/stimulus_display.gd`, `src/stimulus/stimulus_sequencer.gd`

### Config/Settings Refactor (2026-02-03)

- [x] **P1** Renamed `Config` to `Settings` (clarifies persistent-only role)
- [x] **P1** Moved runtime state to `Session`:
  - `selected_camera` and `selected_display` dictionaries
  - Related computed properties (camera_width_px, display_refresh_hz, etc.)
  - Validation state (display_refresh_validated, measured_refresh_hz)
  - Signals: `camera_selected`, `display_selected`
- [x] **P1** Updated all references (~100 files) from Config to Settings

---

End of Issues Document
