# UI Architecture

## Layout

```
┌──────┬────────────────────────────────┬──────────────────┐
│      │                                │                  │
│ ICON │   Main content area            │  Live preview    │
│ BAR  │                                │  (camera, when   │
│      │                                │   connected)     │
│  ○   │                                │                  │
│  ○   │                                │                  │
│  ○   │                                │                  │
│      │                                │                  │
│      │                                │                  │
├──────┴────────────────────────────────┴──────────────────┤
│ Status bar                                               │
└──────────────────────────────────────────────────────────┘
```

**Icon bar** — thin, left edge, icons only with tooltips. Always visible.

**Main content area** — changes based on which icon is selected.

**Live preview panel** — right side, persistent when camera is connected. Collapses when no camera. During Focus, expands to full height. During Acquire, shows monitoring metrics alongside preview.

**Status bar** — bottom edge, always visible. Shows connected hardware, current activity, timing info.

**Dark theme** — the scientist works in a dark room. Bright UI contaminates imaging and is painful to look at.

## Navigation

Three icons:

### Library (default on launch)
Always enabled. This is the home screen.

Shows `.oisi` files from the configured data directory as a table or card grid. Metadata columns: date, animal ID, stimulus type, conditions, duration, whether analysis results exist, notes excerpt.

**Actions from Library:**
- Select a file → "Open for Analysis" → loads file, enables Analysis icon
- "New Session" button → enables Session icon, switches to Session view
- "Import" button → brings in external data (SNLC `.mat`, etc.), converts to `.oisi`, appears in Library
- "Export" on selected file → export maps/data to TIFF, CSV, MATLAB
- Delete, rename (basic file management)

Sort and filter by date, animal ID, stimulus type.

### Session (disabled until "New Session")
Activates when the user clicks "New Session" in Library. Stays active until the user explicitly ends the session or starts a new one.

A session spans an entire sitting — the scientist may run multiple acquisitions on the same animal without ending the session. Hardware stays connected, anatomical stays captured, protocol can be adjusted between runs.

**Starting a new session when one is active:** prompts "A session is in progress. End current session and start new?" No silent overwriting.

**Content:** the workflow rail — collapsible sections, accordion-style. Each section header shows a one-line status summary and a green checkmark when complete.

## Workflow sections

Each section owns a specific set of configurable values. The principle: values live where the scientist thinks about them, not where they happen to be stored in config files.

### Setup — the physical rig

Everything about the hardware and physical arrangement. Set once per session, rarely changes between acquisitions.

**Monitor:**
- Monitor selection (dropdown of detected monitors)
- Physical dimensions — width/height in cm (EDID-detected, user-overridable)
- Source label ("EDID" vs "user override") with hint to measure if uncertain
- Display validation button → measured refresh Hz, jitter, sample count, confidence
- Target stimulus FPS

**Geometry:**
- Viewing distance (cm) — distance from animal's eye to monitor center
- Horizontal offset (degrees) — azimuth of monitor center from animal's midline
- Vertical offset (degrees) — elevation of monitor center
- Projection type — cartesian / spherical / cylindrical
- Monitor rotation — 0° / 90° / 180° / 270°

**Camera:**
- Camera enumeration + selection
- Target camera FPS
- FPS validation button → measured FPS, jitter, exposure bounds

Collapsed summary: `✓ Display: 60.0Hz  ✓ Geometry: 25cm, 30° azi  ✓ Camera: PCO panda @ 30fps`

**Why geometry is here, not in Protocol:** The monitor's physical position relative to the animal doesn't change between stimulus protocols. You set it once when you position the rig. Protocols define *what* to present (bar at 9 deg/s), not *where* the monitor is. The geometry converts between monitor pixels and visual degrees — that's a property of the physical setup.

### Focus — camera adjustment

Hands-on step. Camera preview expands to full height. Scientist is physically adjusting the microscope and camera.

**Controls:**
- Exposure (slider + fine increment ±buttons)
- Gain (when supported)
- Live histogram (pixel value distribution)
- Head ring overlay (toggle, radius, center position)
- Anatomical capture (button → save dialog → side-by-side with live view)

Collapsed summary: `✓ Exposure: 33000µs  ✓ Anatomical captured` (with thumbnail)

### Protocol — stimulus design

What stimulus to present and how. This is what changes between acquisitions on the same animal.

**Stimulus:**
- Envelope type — bar / wedge / ring / fullfield
- Carrier type — solid / checkerboard
- Carrier parameters — contrast, mean luminance, background luminance, check size, strobe frequency
- Envelope parameters — width, speed (fields change based on envelope type)

**Presentation:**
- Conditions (auto-populated from envelope type, e.g. LR/RL/TB/BT for bar)
- Repetitions
- Structure — blocked / interleaved
- Order — sequential / interleaved / randomized

**Timing:**
- Baseline start (seconds)
- Baseline end (seconds)
- Inter-stimulus interval (seconds)
- Inter-direction interval (seconds)

**Timeline visualization:** visual block diagram of the full acquisition schedule. Baseline blocks, sweep blocks colored by condition, intervals. Durations labeled. Total time. Updates live as parameters change.

**Protocol management:** Load / Save / Save As. Stimulus preview runs on the animal's monitor.

Collapsed summary: `Drifting Bar — LR/RL/TB/BT × 10 reps — 12:30`

**What is NOT here:** Geometry (viewing distance, offsets, projection) — those are in Setup. The protocol defines the stimulus pattern; Setup defines the physical context it runs in.

### Acquire — run the experiment

**Pre-acquisition:**
- Readiness checklist with status for each prerequisite:
  - Display selected and validated
  - Geometry configured (viewing distance > 0, dimensions valid)
  - Camera connected and FPS validated
  - Anatomical captured
  - Protocol loaded
  - Save path set
- Each unmet prerequisite links to the section that resolves it
- Animal ID field (persisted in .oisi metadata)
- Experiment notes field (free text, persisted in .oisi metadata)
- Estimated duration (computed from protocol + geometry)
- Start button (enabled only when all prerequisites met, confirmation dialog on click)

**During acquisition:**
- Progress bar (percentage)
- Elapsed time / estimated remaining
- Current sweep (N of M) and condition label
- Camera FPS (live)
- Dropped frame counter
- Timing jitter
- Camera preview stays visible
- Stop button (prominent)

**After acquisition:**
- Save prompt: "Save acquisition? (N sweeps, M frames, T duration)" — Save / Save As / Discard
- On save, file appears in Library

Collapsed summary: `✓ Saved: 40 sweeps, 12000 frames, 12:34 — experiment_2024_03_15.oisi`

### Analyze (inline, within session)

Quick-look at results after an acquisition, without leaving the session. Runs analysis on the just-saved file.

- Phase maps (azimuth, altitude)
- Amplitude maps
- Visual field sign map
- Overlay on anatomical image
- For deeper analysis, user opens the file in the Analysis view via the icon bar

Collapsed summary: `✓ Maps computed` or `— Not yet analyzed`

## Analysis view (icon bar)

Activates when the user selects a file in Library and clicks "Open for Analysis." Loads a single `.oisi` file.

**Maps display:**
- Phase maps (azimuth, altitude) — colormapped
- Amplitude maps
- Visual field sign map
- Anatomical image (if present in file)
- Map overlay on anatomy with opacity slider
- Colorbar with angular scale

**Parameters (editable, re-run on change):**
- Smoothing sigma
- Rotation (k × 90°)
- Azimuth angular range (degrees) — pre-populated from acquisition metadata
- Altitude angular range (degrees) — pre-populated from acquisition metadata
- Azimuth offset (degrees)
- Altitude offset (degrees)
- Epsilon

**Export:**
- TIFF/PNG (individual maps)
- CSV (phase/magnitude arrays)
- MATLAB `.mat`
- Export includes metadata (stimulus parameters, geometry, timing)

## Values NOT in the workflow UI

System internals that stay in `app.toml` only. A scientist doesn't need to see these. Power users can edit the TOML file directly.

**Camera internals:**
- `frame_send_interval_ms` — UI preview FPS cap
- `poll_interval_ms` — frame poll frequency
- `first_frame_timeout_ms` — camera startup timeout
- `first_frame_poll_ms` — camera startup poll sleep

**Display internals:**
- `validation_sample_count` — vsync measurement samples
- `preview_width_px` — sidebar preview resolution
- `preview_interval_ms` — preview refresh rate
- `preview_cycle_sec` — preview sweep cycle duration
- `idle_sleep_ms` — CPU usage when idle

**Acquisition internals:**
- `drop_detection_warmup_frames` — frames to skip before drop detection
- `drop_detection_threshold` — dropped frame ratio threshold

**Performance internals:**
- `fps_window_frames` — rolling FPS calculation window

**Daemon (legacy, may be removed):**
- `startup_delay_ms`, `shm_name`, `shm_num_buffers`, `health_check_interval_ms`

**Window state (auto-persisted):**
- `maximized`, `position_x`, `position_y`, `width`, `height`

**UI preferences:**
- `show_debug_overlay`, `show_timing_info`

## Config file architecture

Given the reorganization, config files should reflect where values are used:

**`app.toml`** — system-level settings that don't change between sessions:
- Camera hardware defaults (exposure, gain, target FPS)
- Display defaults (target stimulus FPS, inverted)
- System internals (poll intervals, timeouts, thresholds)
- Window state, UI preferences, paths
- Analysis defaults (smoothing, rotation, ranges, offsets, epsilon)

**`session geometry`** — the physical rig arrangement, set in Setup:
- Viewing distance, horizontal/vertical offset, projection type, monitor rotation
- Monitor physical dimensions (override)
- Stored in session state at runtime; persisted to config so it's remembered across app restarts

**`protocol files`** (.protocol.json) — portable stimulus definitions:
- Stimulus type, carrier, envelope parameters
- Presentation (conditions, repetitions, structure, order)
- Timing (baselines, intervals)
- NO geometry — geometry is a session property, not a protocol property

**`.oisi files`** — acquisition output:
- Raw frames + timestamps
- Full snapshot of geometry, protocol, and hardware state at time of acquisition
- Analysis results (when computed)
- Session notes, animal ID

## State management

- **Library** state: data directory path (from config), file list (scanned on view), selected file
- **Session** state: hardware connections, validation results, geometry, anatomical capture, current protocol, acquisition progress — all in `AppState`
- **Analysis** state: loaded `.oisi` file path, computed maps, current parameters

Each view manages its own state. Navigation between views does not destroy state — switching from Session to Library and back preserves the session. Switching from Analysis to Library and back preserves the loaded analysis.

## Milestone evolution

### Alpha
- Icon bar with three icons, functional dark theme
- Library shows file list with basic metadata, New Session and Open for Analysis buttons
- Session workflow rail with collapsible sections, status indicators, all controls functional
- Geometry in Setup section (not Protocol)
- Analysis view shows maps, overlay, export
- Layout is correct from day one — no structural rework needed later
- Visually simple: functional HTML/CSS, no animations

### Beta
- Visual polish: consistent spacing, typography, transitions
- Timeline visualization in Protocol section
- Readiness checklist with linked prerequisites in Acquire section
- Histogram in Focus section
- Head ring overlay in Focus section
- File management in Library (sort, filter, delete)
- Confirmation dialogs, inline validation feedback
- Looks like professional scientific software

### v1.0.0+
- Multi-file analysis (load and compare multiple `.oisi` files)
- Keyboard shortcuts for power users
- Customizable data directory per project
- Detachable preview panel
