# OpenISI Roadmap

OpenISI is an open-source intrinsic signal imaging system for retinotopic mapping. The goal is a single downloadable application that any neuroscience lab can use — no custom scripts, no expensive proprietary software, no building from source.

Work is organized into phases that must happen in order — each phase builds on the one before it. Within each phase, items are listed in recommended implementation order.

Release milestones:
- **Alpha** — Our lab uses it daily for real experiments. Data is trustworthy and complete.
- **Beta** — Another lab can download it and use it. Published alongside the paper.
- **v1.0.0+** — Community-driven. Multiple platforms, additional hardware, features requested by users.

---

## Phase 1: Architecture Refactor

The current codebase has accumulated debt from the Godot port: two types for the same concept (Protocol + StimulusConfig), integer enums, a god-object AppState, lossy conversion functions, and geometry in the wrong config file. Every new feature requires touching 4+ files. This must be fixed first.

### 1.1 Rename and restructure config files
`app.toml` → `rig.toml`. `stimulus.toml` → `experiment.toml`. Geometry moves from experiment to rig. Dead fields removed (`stimulus_width_cm`, `luminance_min`, `luminance_max`, `paradigm`, `inverted`, `daemon.*`).

*Done when:* Two config files exist: `rig.toml` (hardware, geometry, display, analysis, system, UI, paths) and `experiment.toml` (stimulus, presentation, timing). No other config files. All code reads from the new files.

### 1.2 Unify the data model
Replace `StimulusConfig` and `Protocol` with a single `Experiment` type. Remove `protocol_to_stimulus_config()`, `Protocol::from_stimulus_config()`, and all bridge functions. Saved experiment files (`.experiment.toml`) use the same type with optional metadata fields (name, description, timestamps).

*Done when:* One Rust type (`Experiment`) represents both the working experiment state and saved experiment files. Loading a saved experiment overwrites `experiment.toml`. Saving writes `experiment.toml` content to a named file. Zero conversion functions.

### 1.3 String enums everywhere
Replace all integer enum codes (`carrier = 1`, `projection_type = 1`, `envelope = 1`) with string enums (`carrier = "checkerboard"`, `projection = "spherical"`, `envelope = "bar"`). Integer-to-enum conversion functions (`from_shader_int`, `from_str`) are replaced by serde deserialization. The renderer converts enums to shader integers in exactly one place.

*Done when:* No integer enum codes in any config file, message type, or storage format. All enums use `#[serde(rename_all = "snake_case")]`.

### 1.4 Decompose AppState
Replace the god-object `AppState` with layered state: `rig` (Arc<Mutex<RigConfig>>), `experiment` (Experiment), `session` (Session with hardware/anatomical), `acquisition` (Option<Acquisition>), `threads` (ThreadHandles).

*Done when:* Each field in the top-level state has a clear owner and lifecycle. Session holds only volatile hardware state. Acquisition exists only during recording. No overlap between layers.

### 1.5 Self-contained thread messages
Replace `StimulusAcquisitionConfig` (bag of nested config objects) with `AcquisitionCommand` (flat, self-contained value). Replace `PreviewConfig` with `PreviewCommand`. Thread messages are assembled at the command boundary from current rig + experiment + session state.

*Done when:* Stimulus thread receives everything it needs in one message. No back-references to AppState. Adding a new parameter means adding it to one struct, not threading it through three.

### 1.6 Domain-specific frontend queries
Replace the single `get_session()` blob with focused queries: `get_rig_geometry()`, `get_hardware_state()`, `get_experiment()`, `get_acquisition_status()`.

*Done when:* Frontend calls domain-specific endpoints. Each returns a focused slice of state.

---

## Phase 2: Data Pipeline

The current system converts camera frames u16→f32, discards baseline frames, uses software timestamps for stimulus timing, has no camera-stimulus clock synchronization, and no per-camera-frame stimulus alignment. This must be fixed before the data can be trusted for publication.

### 2.1 DXGI hardware vsync timestamps
Replace QPC-after-WaitForVBlank with DXGI `GetFrameStatistics()` `SyncQPCTime` for true hardware vsync timestamps. Save DXGI `PresentCount` for GPU-level frame drop detection.

*Done when:* Every stimulus frame has a hardware vsync timestamp from DXGI, not a software approximation. Present count gaps are detected and recorded.

### 2.2 Clock synchronization
Record camera hardware clock + QPC at both first and last frame of acquisition. Save both sync points and QPC frequency in the .oisi file.

*Done when:* Analysis can convert between camera clock and QPC clock. Clock drift over the acquisition is detectable and correctable.

### 2.3 Raw frame storage
Save camera frames as raw u16, not f32. Save ALL frames in acquisition order (including baselines and inter-trial periods), as a single contiguous array.

*Done when:* `/acquisition/camera/frames` is u16 (T, H, W). No frames are discarded. File size is halved compared to f32.

### 2.4 Per-camera-frame stimulus alignment
For every camera frame, record the stimulus state at capture time (state, condition, sweep, progress). Computed during acquisition by interpolating stimulus state to camera frame times using the shared QPC clock.

*Done when:* `/acquisition/camera_stimulus_alignment/` has arrays of length T (same as camera frames) with per-frame stimulus state. Analysis can group camera frames by condition without post-hoc timestamp matching.

### 2.5 Quality metrics
Compute and save: camera frame deltas, camera sequence number gaps, stimulus frame deltas, stimulus present count gaps, mean pixel intensity per frame, total drop counts, acquisition completeness flag.

*Done when:* `/acquisition/quality/` group exists with all metrics. Incomplete acquisitions are flagged. Illumination drift is detectable from mean intensity.

### 2.6 Provenance and integrity
Save software version, gamma correction flag, Fletcher32 checksums on all datasets.

*Done when:* Every .oisi file records which OpenISI version produced it. Checksums catch silent corruption. Gamma state is recorded for reproducibility.

### 2.7 Sweep schedule
Save the realized sweep schedule: ordered list of conditions as actually presented, with precise start/end timestamps per sweep.

*Done when:* `/acquisition/schedule/` contains the actual execution order and timing. Analysis uses this rather than reconstructing it from per-frame state arrays.

---

## Phase 3: UI Rebuild

The current tab-based UI doesn't match the workflow and has no data management. This phase implements the target UI architecture: icon bar navigation, Library landing page, Session workflow rail, Analysis view, dark theme.

### 3.1 Dark theme and layout shell
Implement the base layout: icon bar (left), main content area (center), live preview panel (right), status bar (bottom). Dark theme CSS. Three icons: Library, Session, Analysis.

*Done when:* The layout structure matches the UI architecture doc. Dark theme. Icons enable/disable based on state (Session disabled until created, Analysis disabled until file loaded).

### 3.2 Library view
File browser for the configured data directory. Shows .oisi files with metadata (date, animal ID, stimulus type, duration, has analysis results). Actions: Open for Analysis, New Session, Import, Export.

*Done when:* Library is the landing page. User can browse, select, and open files. New Session creates a session and enables the Session icon.

### 3.3 Session workflow rail
Collapsible sections: Setup, Focus, Protocol, Acquire, Analyze (inline). Each section header shows a one-line status summary and completion indicator. Accordion behavior for Focus (camera preview expands).

*Done when:* All five sections exist with controls mapped to the correct domain-specific backend endpoints. Sections show status when collapsed.

### 3.4 Analysis view
Opens a single .oisi file. Displays phase maps, amplitude maps, VFS overlaid on anatomical image. Parameter panel (smoothing, rotation, angular ranges, offsets). Export buttons.

*Done when:* Analysis icon activates when a file is loaded from Library. Maps are displayed. Parameters are editable with re-run. Export produces TIFF/PNG, CSV, MATLAB .mat.

---

## Phase 4: Alpha Features

With clean architecture, correct data pipeline, and the new UI in place, these features complete the system for daily use in our lab.

### Data Safety

**Partial acquisition save** — Abort (user-initiated or error) writes accumulated frames to an .oisi file marked as incomplete.

**Save prompt on acquisition complete** — Dialog: "Save acquisition?" with Save / Save As / Discard. No file written until confirmed.

**Camera disconnect recovery** — Disconnect during acquisition triggers partial save, clear error in UI, return to known state.

### Hardware Validation

**Display validation: independent measurement context** — Independent Vulkan FIFO or equivalent for true per-monitor vsync measurement.

**Display validation: statistical quality checks** — Warmup skip, 95% CI, mismatch detection (measured vs reported). Thresholds from config.

**Camera FPS validation** — Two-phase: measure at min exposure → derive readout model → validate target FPS → compute exposure bounds.

**Physical dimension override UI** — Editable width/height in cm, labeled source (EDID vs override), hint to measure.

### Core Workflow

**Anatomical image capture and save** — Save 16-bit PNG/TIFF, embed in .oisi file, record path in session.

**Focus screen** — Full-size live preview, exposure ±buttons, anatomical capture side-by-side.

**Input validation** — Frontend validates against ParamDescriptor ranges. Backend returns errors instead of panicking. Plausibility warnings for implausible geometry, exposure, baseline.

**Session notes and metadata** — Animal ID and notes fields on Acquire section, saved in .oisi metadata.

### Analysis

**Analysis UI wiring** — Open .oisi, run pipeline with progress, display maps. Parameters from config, editable.

**Map overlay on anatomy** — Semi-transparent colormapped overlay, opacity slider, colorbar.

**Data export** — TIFF/PNG, CSV, MATLAB .mat with metadata.

---

## Phase 5: Beta Polish

Everything from alpha, plus the system is polished enough for a first-time user. Published alongside the paper.

### Robustness

**Crash recovery** — Incremental temp file during acquisition. Finalized on completion. Recovered on next startup if found.

### UX

**Acquisition confirmation dialog** — Shows estimated duration, sweep count, save path. Must confirm.

**Sequence timeline preview** — Visual block diagram of full acquisition schedule. Updates live as parameters change.

**Recent files** — Last 10 .oisi files and experiments tracked. Shown in Library.

**Head ring overlay** — Configurable ring on camera preview for craniotomy alignment.

**Live histogram** — Real-time pixel value distribution in Focus section.

### Camera

**Gain, binning, and ROI** — Exposed in Focus section. Persisted in rig config. Affects frame dimensions in accumulator and .oisi.

### Preview

**Condition-specific preview** — Dropdown to preview individual conditions.

**Monitor rotation preview** — Sidebar preview reflects applied rotation.

### Analysis

**Map comparison across runs** — Load multiple .oisi files, side-by-side or overlaid, derived maps.

### Release

**Installer and packaging** — CI pipeline, signed Windows installer, bundles binary + default config + PCO redistributables.

**Bundled default config** — Default rig.toml + experiment.toml copied to config dir on first launch.

**User documentation** — User guide: supported hardware, installation, first-run, acquisition workflow, analysis, troubleshooting.

---

## v1.0.0+

Community-driven. Not required for publication.

**Cross-platform support** — Linux and macOS with platform-appropriate display validation and camera backends.

**Additional camera SDKs** — Camera backend trait. At least one additional backend (e.g., Spinnaker).

**Multi-session animal tracking** — Group .oisi files by animal ID, session history, previous anatomical as reference.
