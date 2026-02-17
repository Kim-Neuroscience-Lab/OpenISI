# OpenISI Architecture

## Overview

OpenISI is scientific software for **Intrinsic Signal Imaging (ISI)**, a neuroscience technique that maps the functional organization of visual cortex. The software coordinates three real-time systems:

1. **Stimulus presentation** — VSync-locked rendering on a secondary monitor
2. **Camera acquisition** — Continuous capture via Python daemon
3. **UI/Control** — Responsive interface on the primary monitor

**Godot version**: 4.6 stable

---

## Why This Architecture

### The Problem with Python

Python's Global Interpreter Lock (GIL) prevents true parallelism. For OpenISI's requirements:

- VSync-locked stimulus rendering at 60/120Hz
- Continuous camera buffer drain
- Responsive UI event handling
- **All simultaneously with timing guarantees**

The GIL causes unpredictable stalls. A "VSync'd" stimulus will stutter when other threads get their time slice. `multiprocessing` sidesteps the GIL but introduces IPC complexity and still lacks a proper render loop.

### Why Godot

Game engines are purpose-built for this problem:

| Concern | Python | Godot |
|---------|--------|-------|
| VSync-locked rendering | Fighting the GIL | Native, deterministic |
| Concurrent buffer + UI + render | Practically impossible | Engine handles it |
| Multi-window coordination | Complex, fragile | Built-in |
| Time-sensitive operations | GIL stalls unpredictably | C++ core, predictable |

The Python subprocess handles only camera I/O — which is I/O-bound and works fine with Python's threading model.

---

## Language Responsibilities

OpenISI uses three languages, each doing what it's best at:

| Language | Responsibility |
|----------|----------------|
| **Python** | Camera drivers only — hardware access requiring Python bindings (PyObjC for AVFoundation, camera SDK wrappers). Streams frames + hardware timestamps via shared memory. |
| **Rust** | Performance-critical code via GDExtension — shared memory reading, timing analysis algorithms (future), schema definitions (future). |
| **Godot/GDScript** | Orchestration — UI, stimulus presentation, acquisition logging, test scenarios. Owns the complete acquisition log since it has both camera timestamps (from shm) and stimulus timing (its own). |

**Design principle:** Each language does ONE thing well. No overlap, no redundancy.

---

## Two-Process Architecture

```
┌─────────────────────┐                        ┌─────────────────────┐
│       GODOT         │                        │       PYTHON        │
│  (UI + Stimulus)    │     Shared Memory      │  (Camera Daemon)    │
│                     │◄──────────────────────►│                     │
│  GDExtension        │   Frame ring buffer    │  Camera interface   │
│  shm wrapper        │   + control struct     │  (hardware agnostic)│
└─────────────────────┘                        └─────────────────────┘
```

### Communication: Shared Memory

**Why not TCP/JSON:**
- No network stack overhead
- Fast enough for live frame preview (~30fps, 512×512 frames)
- Control + status can live in same shared region
- Cross-platform: Python `multiprocessing.shared_memory`, Rust GDExtension abstracts OS differences

### Shared Memory Layout

```
┌─────────────────────────────────────────────────────────────────┐
│ Control Region (64 bytes)                                       │
│ ┌─────────────────────────────────────────────────────────────┐ │
│ │ Offset  0: write_index (u32)                                │ │
│ │ Offset  4: read_index (u32)                                 │ │
│ │ Offset  8: frame_width (u32)                                │ │
│ │ Offset 12: frame_height (u32)                               │ │
│ │ Offset 16: frame_count (u32)                                │ │
│ │ Offset 20: num_buffers (u32)                                │ │
│ │ Offset 24: status (u8)                                      │ │
│ │ Offset 25: command (u8)                                     │ │
│ │ Offset 26: latest_timestamp_us (u64) ← HARDWARE TIMESTAMP   │ │
│ │ Offset 34: reserved (30 bytes)                              │ │
│ └─────────────────────────────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────────────┤
│ Frame Ring Buffer (N frames)                                    │
│ ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐           │
│ │ Frame 0  │ │ Frame 1  │ │ Frame 2  │ │ Frame 3  │  ...      │
│ └──────────┘ └──────────┘ └──────────┘ └──────────┘           │
└─────────────────────────────────────────────────────────────────┘
```

**Key:** Hardware timestamps from the camera are embedded in the control region, allowing Godot to build complete acquisition logs with scientifically valid timing.

### Hardware Agnosticism

Godot never touches camera SDKs. The shared memory protocol is the abstraction boundary:

```
Godot ←──[shared memory]──→ Python Daemon ←──[camera interface]──→ pco.panda
                                          ←──[camera interface]──→ Mock
                                          ←──[camera interface]──→ Future cameras
```

---

## Project Structure

Feature-based organization with clean separation of concerns:

```
/
├── project.godot
├── src/
│   ├── main.tscn
│   ├── main.gd
│   │
│   ├── autoload/                   # Singletons
│   │   ├── config.gd               # SSoT for configuration
│   │   ├── theme.gd                # Visual design system (AppTheme)
│   │   ├── session.gd              # Navigation and session state
│   │   ├── camera_client.gd        # Shared memory interface
│   │   └── hardware_manager.gd     # Hardware enumeration
│   │
│   ├── core/                       # Domain logic
│   │   ├── protocol.gd             # Shared memory struct definitions
│   │   └── timing_statistics.gd    # Uniform timing metric computation
│   │
│   ├── camera/                     # Camera subsystem
│   │   └── camera_dataset.gd       # Per-frame camera timing data
│   │
│   ├── domain/                     # Domain layer
│   │   ├── acquisition_controller.gd
│   │   └── session_state.gd
│   │
│   ├── stimulus/                   # Stimulus rendering (secondary display)
│   │   ├── stimulus_display.gd     # Orchestrator
│   │   ├── stimulus_sequencer.gd   # Timing state machine
│   │   ├── stimulus_window.gd      # Window management
│   │   ├── display_geometry.gd     # Coordinate transforms
│   │   ├── direction_system.gd     # Condition/direction logic
│   │   ├── dataset/                # Recording and export
│   │   │   ├── stimulus_dataset.gd
│   │   │   ├── dataset_exporter.gd
│   │   │   └── *_frame_data.gd     # Paradigm-specific data
│   │   ├── renderers/              # Stimulus renderers
│   │   ├── types/                  # Stimulus type definitions
│   │   ├── texture/                # Carriers, envelopes, modulations
│   │   └── shaders/                # GLSL stimulus shaders
│   │
│   ├── ui/                         # Control UI (primary display)
│   │   ├── theme/                  # UI shaders (ceramic, button, etc.)
│   │   ├── components/             # 18 reusable component directories
│   │   │   ├── card/               # Panels and sections
│   │   │   ├── button/             # Actions with ceramic styling
│   │   │   ├── badge/              # Status indicators
│   │   │   ├── input/              # Form inputs
│   │   │   └── ...                 # InfoRow, StatusPill, etc.
│   │   ├── screens/                # Screen implementations
│   │   │   ├── setup/              # Hardware and session config
│   │   │   ├── focus/              # Camera preview and exposure
│   │   │   ├── stimulus/           # Protocol design
│   │   │   ├── run/                # Acquisition monitoring
│   │   │   └── analyze/            # Results summary
│   │   └── tools/                  # Diagnostic tools
│   │       └── timing_diagnostics.gd
│   │
│   └── util/
│
├── extension/                      # Rust GDExtension for shared memory
│
├── daemon/                         # Python camera daemon (MINIMAL)
│   ├── main.py                     # Entry point - streams frames to shm
│   ├── protocol.py                 # Shared memory protocol (matches Rust)
│   ├── shm.py                      # Shared memory writer
│   ├── config.py                   # Configuration dataclasses
│   └── camera/                     # Camera drivers (hardware access)
│       ├── interface.py            # Base Camera, SyncMode, Capabilities
│       ├── avfoundation.py         # macOS (PyObjC) - hardware timestamps
│       ├── opencv.py               # Cross-platform (software timestamps)
│       ├── pco.py                  # Scientific camera SDK
│       ├── mock.py                 # Testing
│       └── enumerate.py            # Camera discovery
│
├── test/
├── assets/
│   ├── fonts/
│   └── icons/
└── docs/
```

### Layer Responsibilities

| Layer | Knows about | Doesn't know about |
|-------|-------------|-------------------|
| `core/` | Math, validation, domain rules | Godot nodes, UI, hardware |
| `stimulus/` | Rendering patterns, timing | Camera, UI details |
| `ui/screens/` | Presenting state, user input | Hardware, domain math |
| `autoload/` | Coordinating between layers | Implementation details |
| `daemon/` | Camera SDKs, hardware timestamps | Godot, UI, stimulus, acquisition logs |
| `extension/` | Shared memory, timing analysis | UI, stimulus patterns |

---

## State Management: Workspace Navigation

The session uses a flexible workspace model with screen navigation:

```gdscript
# autoload/session.gd
enum Screen { SETUP, FOCUS, STIMULUS, ACQUIRE, RESULTS }

var current_screen: Screen = Screen.SETUP

func navigate_to(screen: Screen) -> void:
    var prev := current_screen
    current_screen = screen
    screen_changed.emit(screen)

func navigate_next() -> void:
    var next_idx := clampi(current_screen + 1, 0, Screen.size() - 1)
    navigate_to(next_idx as Screen)

func navigate_back() -> void:
    var prev_idx := clampi(current_screen - 1, 0, Screen.size() - 1)
    navigate_to(prev_idx as Screen)
```

**Design principles:**
- Free navigation between screens (no validation gating)
- Screens are workspaces, not sequential steps
- State is inspectable and debuggable
- Scenes are "views" of session state

---

## Autoloads

Five autoloads, each with focused responsibility:

| Autoload | Lines | Responsibility |
|----------|-------|----------------|
| `AppTheme` | ~1,100 | Visual design system, colors, typography, StyleBox factories |
| `Config` | ~850 | SSoT for all configuration, typed accessors, persistence |
| `CameraClient` | ~550 | Shared memory interface to daemon, frame retrieval |
| `HardwareManager` | ~360 | Hardware enumeration (cameras via Python, monitors via DisplayServer/EDID) |
| `Session` | ~175 | Navigation, session state container |

**Dependency direction:** UI → Session → CameraClient / Config / HardwareManager. No circular dependencies. `AppTheme` is standalone.

---

## UI Component Architecture

### Design System: "Sleep Punk Night + Ceramic"

OpenISI uses a **ceramic** design language optimized for dark room use:
- Deep night backgrounds (#13111a base)
- Cream/lavender accents for text and highlights
- Amber highlights for primary actions
- Ceramic shader styling with rim highlights and soft edges
- Consistent 8px spacing grid

### Styling Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                     AppTheme Autoload                                │
│  (src/autoload/theme.gd)                                            │
│  - Color palette (semantic names)                                    │
│  - Typography specs (14+ styles: LabelTitle, LabelHeading, etc.)    │
│  - Spacing constants (8px grid)                                      │
│  - Ceramic shader preloads                                           │
│  - StyleBox factory methods                                          │
└──────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│                     UI Components                                    │
│  (src/ui/components/)                                               │
│  - 18 component directories                                          │
│  - Programmatic UI construction                                      │
│  - Ceramic shader styling with rim highlights                        │
└──────────────────────────────────────────────────────────────────────┘
```

### Core Components

| Component | Purpose |
|-----------|---------|
| `Card` / `BaseCard` | Panels, sections with ceramic styling |
| `Button` / `StyledButton` | Actions with ceramic shader effects |
| `StyledInput` / `StyledLineEdit` | Form inputs with proper styling |
| `Badge` / `StatusPill` | Status indicators (SUCCESS, WARNING, ERROR, INFO) |
| `InfoRow` / `InfoCard` | Key-value display |

### Layout Components

| Component | Purpose |
|-----------|---------|
| `layout/` | VBox/HBox wrappers with consistent spacing |
| `SmoothScrollContainer` | Scrolling with smooth animation |
| `Divider` | Visual separation |

### Pattern: Programmatic UI Construction

Components build their UI in code rather than relying on .tscn files:

```gdscript
func _build_ui() -> void:
    var card := BaseCard.create_section("Settings")
    var content := VBoxContainer.new()
    content.theme_type_variation = "VBoxSM"
    # ... build UI tree
    card.set_content(content)
    add_child(card)
```

### AppTheme Responsibilities

| Concern | Implementation |
|---------|---------------|
| Colors | Semantic constants (CREAM, NIGHT_DEEP, AMBER, etc.) |
| Typography | Theme type variations (LabelTitle, LabelHeading, LabelCaption) |
| Spacing | Grid constants (SPACING_XS through SPACING_XL) |
| Shadows | Ceramic rim highlight colors and intensities |
| Status | `get_status_color()` for semantic status colors |

---

## Multi-Window

```
PRIMARY MONITOR          SECONDARY MONITOR
┌─────────────────┐      ┌─────────────────┐
│  Main Window    │      │ Stimulus Window │
│  (UI/Control)   │      │ (VSync-locked)  │
└─────────────────┘      └─────────────────┘
```

- Godot 4.x native multi-window support
- `Session` autoload coordinates both windows via signals
- Stimulus window: `Window.MODE_FULLSCREEN`, `current_screen = 1`, VSync ON
- Both windows are "views" of session state

---

## Signals

**When to use signals:**
- One-to-many (multiple listeners)
- Decoupling (emitter doesn't know listeners)
- Events ("something happened")

**When to use direct calls:**
- One-to-one, known target
- Parent calling child
- Commands ("do this now")

**Flow:** UI → Session (autoload) → other systems. No global event bus needed.

---

## Error Handling

| Category | Example | Handling |
|----------|---------|----------|
| Validation | Invalid input | Inline UI feedback |
| Recoverable | Camera disconnected | Banner with retry |
| Fatal | Shared memory failed | Modal, exit gracefully |

**Pattern:** Return dictionaries with `error` key, or emit error signals. UI decides display. Always provide recovery path.

---

## Validation

**Start simple:**
```gdscript
# core/validation.gd
static func validate_distance(value: float) -> String:
    if value <= 0:
        return "Must be positive"
    return ""  # empty = valid
```

**Layer responsibilities:**
- `core/` — domain rules (pure functions)
- `ui/components/` — call validators, show inline feedback
- `ui/screens/` — aggregate validity for Continue button

**Future:** If warning levels needed, expand to Dictionary. Don't build until needed.

---

## Input Handling

**Approach:** Hybrid
- **Global shortcuts** (Ctrl+Q, F11) → `main.gd`
- **Screen-specific** (Space for capture) → screen scenes via `_unhandled_input`
- **Use InputMap** → define actions in Project Settings, not hardcoded keys

---

## Logging

- **Dev logs:** `print()`, `push_warning()`, `push_error()`
- **Session logs:** In-memory array in Session autoload, optionally saved
- Start simple, add structure if needed

---

## Data & Persistence

### Configuration

**User settings** (`user://config.json`):
- Default directory, last profile, window positions, recent sessions

**Profiles** (`user://profiles/*.json`):
- Reusable experiment configurations

### Session Output

```
/data/sessions/mouse_2026-01-17_001/
├── metadata.json            # Session, protocol, display config
├── stimulus_frames.bin      # Per-frame stimulus data (binary)
├── schema.json              # Binary format description for HDF5 conversion
├── anatomical.png           # Anatomical reference image
└── frames/
    └── frames.h5            # Raw camera data
```

**Note:** Godot owns the complete acquisition log because it orchestrates both camera and stimulus. The daemon only streams frames + timestamps via shared memory — it does not write any output files.

### Nameless Protocol Architecture

The running stimulus configuration has **no identity metadata** — no name, no description, no timestamps. It is simply the current state of configuration values.

- **Protocols are only named when explicitly saved** (to share with others)
- Day-to-day on a single rig: a running, living, nameless protocol
- The filename IS the protocol name when saved to `user://protocols/`

When acquisition starts, the dataset **snapshots** the current Config values. This snapshot becomes part of the immutable scientific record and is stored with the output files.

### Display Geometry Capture

The dataset captures comprehensive display geometry for reproducibility:

| Field | Description |
|-------|-------------|
| `display_width_px`, `display_height_px` | Resolution in pixels |
| `display_width_cm`, `display_height_cm` | Physical dimensions |
| `display_physical_source` | "edid", "user_override", or "none" |
| `visual_field_width_deg`, `visual_field_height_deg` | Calculated visual angles |
| `viewing_distance_cm` | Eye-to-display distance |
| `center_azimuth_deg`, `center_elevation_deg` | Display center position |
| `projection_type` | "cartesian", "spherical", or "cylindrical" |

Physical dimensions are obtained from EDID when available. If EDID fails, users must enter dimensions manually. **DPI-based estimation is not used** — OS returns logical DPI (UI scaling factor) not physical pixel density, leading to ~25% errors in visual angle calculations.

### Sequence-Agnostic Metadata

For analysis independent of sequence ordering strategy, each frame records:

| Field | Type | Description |
|-------|------|-------------|
| `condition_occurrence` | int | Nth time this condition is shown (1-indexed) |
| `is_baseline` | bool | True if baseline/inter-trial, false if active stimulus |

This allows analysis across blocked, interleaved, and randomized presentations without knowing the sequence structure.

### Resource Management

- **Preload** screen scenes and components
- Camera frames handled by daemon, not in Godot memory
- Preview: one frame at a time via shared memory

---

## Code Quality

### Naming Conventions

**GDScript:**

| Element | Convention | Example |
|---------|------------|---------|
| Files | snake_case | `session_manager.gd` |
| Classes | PascalCase | `class_name SessionManager` |
| Functions | snake_case | `func start_acquisition():` |
| Variables | snake_case | `var current_screen` |
| Constants | SCREAMING_SNAKE | `const MAX_FRAMES = 1000` |
| Signals | snake_case, past tense | `signal screen_changed` |
| Private | underscore prefix | `var _internal` |

**Python:** PEP 8

### Documentation

```gdscript
## A card component with slots for header and content injection.
class_name Card extends PanelContainer

## Emitted when the card title changes.
signal title_changed(new_title: String)
```

**Document:** Public API, non-obvious behavior, complex logic.
**Don't document:** Obvious code, private details.
**Inline comments:** Explain WHY, not WHAT.

---

## Testing

| Layer | Framework | Focus |
|-------|-----------|-------|
| `core/` | GUT | Pure logic, geometry, validation |
| `ui/` | GUT | Component behavior, state transitions |
| `daemon/` | pytest | Camera interface, shm protocol |
| **Timing** | Rust + Godot | Timing analysis algorithms (Rust), test scenarios (Godot) |
| Integration | Godot test mode | End-to-end acquisition with metrics |

**Testing architecture:** Godot runs acquisition test scenarios and calls Rust timing analysis functions via GDExtension. This ensures tests exercise the complete system with both camera and stimulus timing.

---

## GDExtension: Rust

Using `gdext` (godot-rust) + `shared_memory` crate:
- Memory safety for shared memory code
- Cross-platform abstraction
- Mature Godot 4.x bindings

**Current capabilities (~1,100 lines):**

| Class | Purpose |
|-------|---------|
| `SharedMemoryReader` | Background thread for reading camera frames + hardware timestamps from daemon |
| `MonitorInfo` | Cross-platform physical monitor dimension detection (EDID/CoreGraphics/WinAPI) |
| `TimingAnalyzer` | Cross-stream sync analysis (~550 lines) |

**TimingAnalyzer methods:**
- `compute_nearest_neighbor_offsets()` — finds nearest stimulus frame for each camera frame
- `compute_cross_correlation()` — optimal alignment lag and correlation strength
- `compute_relative_drift()` — relative clock drift in ppm
- `analyze_sync()` — combined analysis with quality assessment
- `check_sync_quality()` — threshold-based quality flags

---

## Distribution

OpenISI ships as a single self-contained download per platform. Three language runtimes are bundled:

- **Godot** — Application binary + `.pck` resource pack
- **Rust** — GDExtension native library (auto-extracted by Godot during export)
- **Python** — Camera daemon with embedded CPython runtime (via PyInstaller)

The daemon cannot go inside Godot's `.pck` file — it must exist on the real filesystem for `OS.create_process()`. A post-export packaging script assembles the final bundle.

See [design/distribution.md](../design/distribution.md) for the full distribution architecture.
See [RELEASING.md](../RELEASING.md) for the release process.
