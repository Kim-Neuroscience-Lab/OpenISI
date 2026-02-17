# OpenISI Implementation Phases

This document outlines the phased implementation approach for OpenISI. Each phase builds on the previous, with clear deliverables and success criteria.

---

## Phase 0: Proof of Concept (Validation) ✅ COMPLETE

**Goal:** Validate the architecture before committing to full implementation.

### Results

All architecture assumptions validated:
- ✅ VSync timing: ~1ms jitter at 60fps (stable)
- ✅ Shared memory throughput: 30fps sustained via Rust GDExtension
- ✅ Multi-window: Reliable on dual-monitor setup
- ✅ UI responsiveness: No perceptible lag during acquisition

### Key Learnings

- Always use release builds for Rust GDExtension
- Struct layout must exactly match between Python and Rust
- 60fps more stable than 120fps for stimulus timing
- macOS window compositor causes occasional jitter in embedded mode

---

## Phase 1: Foundation ✅ COMPLETE

**Goal:** Establish project structure and core infrastructure.

### Completed Tasks

1. **Folder structure** ✅
   - `src/autoload/` - Config, AppTheme, Session, CameraClient, HardwareManager
   - `src/core/` - Protocol definitions, TimingStatistics
   - `src/stimulus/` - Stimulus display, sequencer, dataset
   - `src/ui/theme/` - Ceramic shaders and styling
   - `src/ui/components/` - 18 reusable component directories
   - `src/ui/phases/` - All phase screens

2. **Theme system** ✅
   - `AppTheme` autoload with colors, typography, spacing constants
   - Ceramic shader styling with rim highlights
   - HiDPI scaling support

3. **Component library** ✅
   - Cards with ceramic styling
   - Buttons with shader effects
   - Inputs, badges, status indicators
   - Layout components

4. **Session autoload** ✅
   - Phase enum (SETUP, FOCUS, CONFIRM, RUN, DONE)
   - FSM with transition guards
   - phase_changed signal

5. **Stub phase scenes** ✅
   - All five phases have stub scenes

---

## Phase 2: Shared Memory Infrastructure ✅ COMPLETE

**Goal:** Establish communication between Godot and Python daemon.

### Completed Tasks

1. **Rust GDExtension** ✅
   - `extension/` with godot-rust 0.4
   - `shared_memory` crate for cross-platform IPC
   - Threaded frame reading (background conversion)
   - Builds for macOS (other platforms ready)

2. **Shared memory protocol** ✅
   - 64-byte control region + frame ring buffer
   - Protocol defined in `daemon/protocol.py` and `extension/src/lib.rs`
   - Exact struct layout match between Python and Rust
   - **Hardware timestamps:** `latest_timestamp_us` (u64) in control region
   - `SharedMemoryReader.get_latest_timestamp_us()` exposed to GDScript

3. **Python daemon** ✅
   - `daemon/` with camera interface abstraction
   - MockCamera for development
   - OpenCvCamera for basic testing
   - AvFoundationCamera (macOS) with hardware timestamps
   - PcoCamera for scientific hardware (future)
   - Shared memory writer with timestamp support
   - **Minimal design:** Daemon only streams frames + timestamps; Godot owns acquisition logs

4. **Integration** ✅
   - Frames flow from Python daemon → Godot at 30fps
   - Dropped frame detection working

---

## Phase 3: UI Shell ✅ COMPLETE

**Goal:** Build the main application layout and base components.

### Completed Tasks

1. **Base components** ✅
   - Cards, Buttons, Inputs with ceramic shader styling
   - StatusPill, Badge for status indicators
   - InfoRow, InfoCard for key-value display
   - 18 component directories total

2. **Main layout** ✅
   - Header with phase indicator
   - Content area with phase switching
   - Footer with navigation buttons

3. **Phase switching** ✅
   - Session autoload manages navigation
   - SceneRegistry preloads all phases
   - Full cycle through phases working

### Deliverables

- [x] All base components implemented and styled
- [x] Components are reusable
- [x] Main layout functional
- [x] Phase switching works end-to-end

---

## Phase 4: Phase Implementation 🔄 IN PROGRESS

**Goal:** Build out each workflow phase.

### 4.1: SETUP Phase ✅ COMPLETE

**Purpose:** Configure session, verify hardware, validate settings.

- [x] Hardware cards (camera, display) with status badges
- [x] Camera/monitor selection with enumeration
- [x] Display geometry (EDID physical dimensions, user override)
- [x] Rig geometry inputs with visual field calculation
- [x] Display dimension hints (EDID status, manual entry guidance)
- [x] Refresh rate validation at display selection time
- [x] Session config (directory, name)
- [x] Validation and Continue guards

### 4.2: FOCUS Phase ✅ COMPLETE

**Purpose:** Set camera exposure, position ROI, capture anatomical.

- [x] Live camera preview (frames from shared memory)
- [x] Exposure controls
- [x] Anatomical capture
- [x] Preview of captured anatomical

### 4.3: STIMULUS Phase ✅ COMPLETE

**Purpose:** Configure stimulus parameters.

- [x] Envelope selection (bar, wedge, ring)
- [x] Carrier selection (checkerboard, solid)
- [x] Parameter controls with validation
- [x] Condition selection
- [x] Sequence configuration (order, repetitions)
- [x] Live stimulus preview

### 4.4: RUN Phase ✅ COMPLETE

**Purpose:** Monitor acquisition progress.

- [x] Progress bar with elapsed/remaining time
- [x] Current sweep info (direction, occurrence)
- [x] Live metrics (stimulus fps, camera fps, jitter)
- [x] Dropped frame detection
- [x] Stop Early button
- [x] Stimulus dataset recording with:
  - Microsecond timestamps via `frame_post_draw`
  - Sequence-agnostic metadata (`condition_occurrence`, `is_baseline`)
  - Full display geometry capture

### 4.5: ANALYZE Phase ✅ COMPLETE

**Purpose:** Show results, provide next actions.

- [x] Completion summary
- [x] Quality metrics display
- [x] Export information

### Deliverables

- [x] All phases implemented
- [x] Validation working on SETUP
- [x] Live preview working on FOCUS
- [x] Progress tracking working on RUN
- [x] Full workflow completable end-to-end

---

## Phase 5: Stimulus ✅ COMPLETE

**Goal:** Implement stimulus rendering on secondary display.

### Completed Tasks

1. **Stimulus window** ✅
   - [x] Window node for secondary display
   - [x] Fullscreen, VSync configuration
   - [x] Monitor detection/selection via HardwareManager

2. **Pattern renderers** ✅
   - [x] TextureRenderer (unified for all texture-based stimuli)
   - [x] Envelopes: BAR (drifting), WEDGE (rotating), RING (expanding), NONE (full-field)
   - [x] Carriers: CHECKERBOARD, SOLID
   - [x] Modulations: sweep, rotate, expand, static
   - [x] Strobe (contrast reversal)
   - [x] Projection types: Cartesian, Spherical, Cylindrical

3. **Timing and dataset recording** ✅
   - [x] `StimulusDataset` with microsecond timestamps
   - [x] Hardware timestamps via `RenderingServer.frame_post_draw`
   - [x] Per-frame recording of all stimulus state
   - [x] `TimingStatistics` for uniform metrics
   - [x] Dropped frame detection
   - [x] `get_full_statistics()` for comprehensive timing analysis

4. **Sequencer** ✅
   - [x] `StimulusSequencer` state machine
   - [x] States: IDLE, BASELINE_START, SWEEP, INTER_STIMULUS, INTER_DIRECTION, BASELINE_END, COMPLETE
   - [x] Sequence generation: sequential, interleaved, randomized
   - [x] Condition occurrence tracking for sequence-agnostic analysis
   - [x] `is_baseline()` accessor for frame classification

5. **Dataset export** ✅
   - [x] `DatasetExporter` with JSON + binary format
   - [x] Full display geometry in metadata
   - [x] Sequence-agnostic fields (`condition_occurrences`, `is_baseline`)
   - [x] Schema for HDF5 conversion

### Deliverables

- [x] Stimulus window opens on secondary monitor
- [x] All required patterns render correctly
- [x] Timing recorded with microsecond precision
- [x] Coordination with acquisition working

---

## Phase 6: Integration 🔄 IN PROGRESS

**Goal:** Connect real hardware, polish, ship.

### Completed Tasks

1. **Camera framework** ✅
   - [x] AVFoundation camera with hardware timestamps (macOS)
   - [x] OpenCV camera for development/testing
   - [x] Mock camera for testing
   - [x] PCO camera interface defined (awaiting hardware)

2. **Timing infrastructure** ✅
   - [x] Hardware timestamps flow through shared memory
   - [x] Cross-stream sync analysis (TimingAnalyzer in Rust)
   - [x] Uniform per-stream metrics (TimingStatistics)
   - [x] Timing diagnostics UI

3. **Display geometry** ✅
   - [x] EDID physical dimension detection (MonitorInfo Rust extension)
   - [x] User override tracking with original EDID values preserved
   - [x] No unsafe DPI-based fallbacks

### Remaining Tasks

4. **Frame-locking** ⏳
   - [ ] Frame-lock sequencer state machine (wall-clock → frame counter)
   - [ ] Quantize stimulus progress to frame boundaries
   - [ ] Frame-lock strobe phase

5. **TRIGGERED sync mode** ⏳
   - [ ] Hardware trigger support when camera available
   - [ ] Currently POST_HOC mode only

6. **Polish** ⏳
   - [ ] Error messages and recovery flows
   - [ ] Edge cases and robustness
   - [ ] Documentation completion

### Deliverables

- [x] Camera framework complete
- [x] Timing infrastructure complete
- [x] Display geometry capture complete
- [ ] Frame-locking complete
- [ ] Ready for publication/release

---

## Phase Dependencies

```
Phase 0 (PoC)
    │
    ▼
Phase 1 (Foundation) ──────────────────┐
    │                                  │
    ▼                                  │
Phase 2 (Shared Memory) ◄──────────────┤
    │                                  │
    ▼                                  │
Phase 3 (UI Shell) ◄───────────────────┘
    │
    ├──────────────────┐
    ▼                  ▼
Phase 4 (Phases)   Phase 5 (Stimulus)
    │                  │
    └────────┬─────────┘
             ▼
      Phase 6 (Integration)
```

- Phase 0 must complete and pass before starting Phase 1
- Phases 1, 2, 3 are sequential (each builds on previous)
- Phases 4 and 5 can be worked in parallel after Phase 3
- Phase 6 requires both Phase 4 and Phase 5 complete

---

## Milestone Summary

| Phase | Milestone | Status |
|-------|-----------|--------|
| 0 | Architecture validated | ✅ Complete |
| 1 | Foundation complete | ✅ Complete |
| 2 | IPC working | ✅ Complete |
| 3 | UI shell complete | ✅ Complete |
| 4 | Phases complete | ✅ Complete |
| 5 | Stimulus complete | ✅ Complete |
| 6 | Ship ready | 🔄 In Progress (frame-locking, polish remaining) |
