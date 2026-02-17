# OpenISI Architecture Guide

This guide explains how the OpenISI codebase is organized. It's written for neuroscientists who want to understand, modify, or contribute to the code.

## Quick Orientation

```
src/
├── autoload/       → Global services (Settings, Session, CameraClient, etc.)
├── controllers/    → Screen logic separated from UI
├── models/         → Data structures (datasets, session state, frame data)
├── core/           → Shared memory protocol definitions (daemon interface)
├── stimulus/       → Visual stimulus rendering system
├── ui/
│   ├── screens/    → Full-page workflows (Setup, Focus, Stimulus, Run, Results)
│   └── components/ → Reusable UI pieces (buttons, cards, inputs)
└── utils/          → Helper functions (file I/O, formatting, geometry)
```

---

## Core Concepts

### 1. Single Source of Truth (SSoT)

All configuration and state lives in two places:

| Autoload | What it holds | Persisted? |
|----------|---------------|------------|
| **Settings** | Stimulus parameters, hardware config, preferences | Yes (JSON files) |
| **Session** | Current screen, selected camera/display, session data | No (runtime only) |

**Rule:** When you need a value, read it from Settings or Session. When you change a value, write it to Settings or Session. Don't store copies elsewhere.

```gdscript
# Good - reading from SSoT
var distance = Settings.viewing_distance_cm

# Good - writing to SSoT
Settings.viewing_distance_cm = 25.0

# Bad - storing a copy that can get out of sync
var _cached_distance = Settings.viewing_distance_cm  # Don't do this
```

### 2. Screen + Controller Pattern

Every screen follows the same structure:

```
┌─────────────────────────────────────────┐
│              Screen (.gd)                │
│  - Builds UI (buttons, cards, labels)   │
│  - Connects signals                      │
│  - Displays data from controller         │
│  - Routes user actions to controller     │
└─────────────────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────┐
│            Controller (.gd)              │
│  - Contains all logic                    │
│  - Reads/writes Settings and Session     │
│  - Emits signals when state changes      │
│  - No UI code                            │
└─────────────────────────────────────────┘
```

**Why this pattern?**
- You can understand the UI by reading just the screen file
- You can understand the logic by reading just the controller file
- You can test the controller without needing the UI

### 3. Signals for Communication

Components communicate through signals, not direct method calls:

```gdscript
# In controller - emit when something changes
signal exposure_changed(value_us: int)

func set_exposure(us: int) -> void:
    _exposure_us = us
    Settings.camera_exposure_us = us
    exposure_changed.emit(us)

# In screen - connect and respond
func _connect_signals() -> void:
    _controller.exposure_changed.connect(_on_exposure_changed)

func _on_exposure_changed(us: int) -> void:
    _exposure_label.text = "%d µs" % us
```

---

## The Screens

OpenISI has 5 main screens that form the workflow:

| Screen | Complexity | Has Controller? | Why |
|--------|------------|-----------------|-----|
| Setup | Low | No | Just validates card states |
| Focus | High | Yes | Camera control, exposure, image capture |
| Stimulus | Medium | No | Direct Settings mutations (SSoT pattern) |
| Run | High | Yes | Real-time acquisition orchestration |
| Results | Low | No | Display-only, reads from Session |

**When to use a controller:**
- Real-time operations (camera frames, acquisition timing)
- Complex state machines
- Multiple coordinated subsystems

**When direct SSoT access is fine:**
- Simple validation (checking if cards are ready)
- Direct parameter editing (user changes a value → write to Settings)
- Display-only screens (reading from Session/Settings)

---

### Setup Screen
**Purpose:** Configure hardware (camera, display) before experiments

**Pattern:** UI-only (aggregates validation from self-contained cards)

**Files:**
- `ui/screens/setup/setup_screen.gd` - Validates cards are ready
- `ui/screens/setup/camera_card.gd` - Camera selection (uses CameraClient directly)
- `ui/screens/setup/monitor_card.gd` - Display selection (uses Session directly)
- `ui/screens/setup/session_config_card.gd` - Session name/path

### Focus Screen
**Purpose:** Preview camera, adjust exposure, capture anatomical image

**Pattern:** Screen + Controller (real-time camera operations)

**Files:**
- `ui/screens/focus/focus_screen.gd` - UI layout
- `controllers/focus_controller.gd` - Frame polling, exposure, anatomical capture

### Stimulus Screen
**Purpose:** Design the visual stimulus protocol

**Pattern:** UI-only with preview helper (card changes write directly to Settings)

**Files:**
- `ui/screens/stimulus/stimulus_screen.gd` - Coordinates cards, updates Settings
- `ui/screens/stimulus/*_card.gd` - Parameter input cards (Composition, Geometry, Timing, etc.)
- `controllers/preview_controller.gd` - Manages stimulus preview play/stop

### Run Screen (Acquire)
**Purpose:** Execute acquisition with live monitoring

**Pattern:** Screen + Controller (real-time acquisition)

**Files:**
- `ui/screens/run/run_screen.gd` - UI layout, displays metrics
- `controllers/run_controller.gd` - Orchestrates sequencer, camera, stimulus window
- `controllers/sequencer_controller.gd` - Manages timing state machine
- `ui/screens/run/*_card.gd` - Status, metrics, and preview cards

### Results Screen (Analyze)
**Purpose:** Review completed acquisition

**Pattern:** UI-only (display-only, reads from Session)

**Files:**
- `ui/screens/analyze/analyze_screen.gd` - Displays session results

---

## The Stimulus System

The stimulus system renders visual patterns on a secondary display.

### Composition Model

Stimuli are built from three components:

```
Stimulus = Carrier × Envelope × Modulation

Carrier:   The pattern (solid color, checkerboard)
Envelope:  The shape (bar, wedge, ring, full-field)
Modulation: The motion (sweep, rotate, expand)
```

### Adding a New Stimulus Type

1. **Create the type class** in `stimulus/types/`:
```gdscript
# my_stimulus_type.gd
class_name MyStimulus
extends StimulusTypeBase

func get_type_id() -> String:
    return "my_stimulus"

func get_display_name() -> String:
    return "My Custom Stimulus"

func get_parameters() -> Array[Dictionary]:
    return [
        {"name": "speed", "type": "float", "default": 10.0, "min": 1.0, "max": 100.0}
    ]
```

2. **Register it** in `stimulus/types/stimulus_type_registry.gd`

3. **Create shader** in `stimulus/shaders/` if needed

4. **Add to UI** by updating the composition card options

---

## Settings (Configuration)

Settings manages all persistent configuration through JSON files:

| File | Contents |
|------|----------|
| `hardware.json` | Camera exposure, display settings, daemon config |
| `stimulus.json` | All stimulus parameters |
| `preferences.json` | Window position, UI state |

### Reading Settings

```gdscript
# Properties have getters
var speed = Settings.bar_speed_deg_sec
var conditions = Settings.conditions
```

### Writing Settings

```gdscript
# Properties have setters that auto-save and emit signals
Settings.bar_speed_deg_sec = 15.0
Settings.conditions = ["LR", "RL"]
```

### Adding a New Parameter

1. Add to the JSON file with a default value
2. Add a property accessor in `settings.gd`:

```gdscript
var my_param: float:
    get: return float(_stimulus["my_section"]["my_param"])
    set(v): _set_stimulus("my_section", "my_param", v)
```

---

## Session (Runtime State)

Session holds state that exists only while the app is running:

```gdscript
# Navigation
Session.navigate_to(Session.Screen.FOCUS)
Session.current_screen

# Hardware selection (from enumeration, not persisted)
Session.set_selected_camera(device_dict)
Session.set_selected_display(monitor_dict)

# Session data
Session.set_anatomical_captured(path, texture)
Session.set_acquisition_complete(frames, duration)
```

---

## Adding a New Screen

1. **Create the screen file** in `ui/screens/myscreen/`:
```gdscript
extends BaseScreen

func _build_ui() -> void:
    # Create your UI here
    pass

func _connect_signals() -> void:
    # Connect to controller signals
    pass

func _load_state() -> void:
    # Initialize from Settings/Session
    pass
```

2. **Create the controller** in `controllers/`:
```gdscript
class_name MyScreenController
extends RefCounted

signal something_changed(value)

func do_something() -> void:
    # Logic here
    something_changed.emit(result)
```

3. **Register the screen** in `session.gd`:
```gdscript
enum Screen {
    # ...existing...
    MY_SCREEN,
}
```

4. **Add navigation** in the header or where appropriate

---

## Common Tasks

### "Where do I find...?"

| Looking for... | Location |
|----------------|----------|
| Camera connection code | `autoload/camera_client.gd` |
| Stimulus timing logic | `stimulus/stimulus_sequencer.gd` |
| Visual field calculations | `utils/geometry_calculator.gd` |
| Timing statistics | `utils/timing_statistics.gd` |
| Dataset export | `utils/dataset_exporter.gd` |
| UI color definitions | `autoload/theme.gd` |
| Parameter validation | `autoload/settings.gd` property setters |

### "How do I...?"

**Change a stimulus parameter's range:**
Edit the property in `settings.gd` and update the corresponding card's SpinBox min/max.

**Add a new condition/direction:**
Edit `stimulus/direction_system.gd` to add the mapping.

**Change the acquisition flow:**
Edit `controllers/run_controller.gd` - the screen just displays what the controller tells it.

**Add a new export format:**
Add a method to `utils/dataset_exporter.gd`.

---

## Code Style

### Naming Conventions

```gdscript
# Classes: PascalCase
class_name FocusController

# Functions and variables: snake_case
func update_preview() -> void:
    var frame_count = 0

# Private members: prefix with underscore
var _internal_state: int = 0
func _helper_method() -> void:
    pass

# Signals: past tense describing what happened
signal exposure_changed(us: int)
signal acquisition_completed()
```

### File Organization

```gdscript
extends BaseClass
## Brief description of what this file does.
##
## Longer explanation if needed.

# Signals first
signal something_happened()

# Constants
const MAX_VALUE := 100

# Public variables (exported or API)
var public_thing: int = 0

# Private variables
var _private_thing: int = 0

# Lifecycle methods
func _ready() -> void:
    pass

# Public API methods
func do_public_thing() -> void:
    pass

# Private helper methods
func _do_private_thing() -> void:
    pass

# Signal handlers (at bottom)
func _on_button_pressed() -> void:
    pass
```

---

## Testing Your Changes

1. **Run the app** and test the UI flow manually
2. **Check the console** for errors (F10 opens output in Godot)
3. **Test edge cases:** empty inputs, maximum values, rapid clicking

---

## Getting Help

- **Issues:** Check `docs/ISSUES.md` for known problems
- **Design docs:** See `docs/design/` for detailed specifications
- **GitHub:** Open an issue at [repository URL]
