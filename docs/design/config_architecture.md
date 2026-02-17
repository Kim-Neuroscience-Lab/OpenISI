# Unified Config Architecture

## Overview

A single `Config` autoload serves as the sole interface between JSON configuration files and the codebase. All code reads configuration values directly from Config - no caching, no copying, no passing config dictionaries around.

---

## Core Principles

### 1. Config is the Single Source of Truth

**All code reads from Config directly.** There is no other way.

```gdscript
# CORRECT: Read from Config when you need the value
var width := Config.stimulus_width_deg
var speed := Config.sweep_speed_deg_per_sec

# WRONG: Cache config values in instance variables
var _cached_width: float  # NO
var _config_data: Dictionary  # NO

# WRONG: Pass config dictionaries between functions
func initialize(config_data: Dictionary)  # NO
func apply_config(config_data: Dictionary)  # NO
```

### 2. No Fallbacks - Fail Fast and Hard

**There is ONE correct way to do things. If it doesn't work, the program fails immediately.**

```gdscript
# CORRECT: Direct access - crashes if key missing (reveals bugs)
var envelope: int = _stimulus["stimulus"]["envelope"]

# WRONG: Fallback defaults mask bugs
var envelope: int = config_data.get("envelope", Envelopes.Type.BAR)  # NO
```

Fallback implies failure. We do not fail silently. If JSON is missing a value, that's a bug in the JSON that must be fixed - not masked with a fallback.

### 3. No Legacy, No Backward Compatibility, No Exceptions

**There is no legacy code. There is no backward compatibility. There is no "old way" that still works.**

- No "legacy mode" fallbacks
- No "deprecated" functions that still work
- No code paths for "when config isn't set"
- No historical comments explaining what code "used to do"

If something is removed, it is removed completely. Code either does things the correct way or it doesn't exist.

### 4. JSON Holds Values, Config Holds Contracts

**JSON files are the authoritative source of configuration VALUES.**

**Config defines CONTRACTS** (type, min, max, step, unit) but **NO default values**.

```gdscript
# Config defines what a parameter IS (contract)
const STIMULUS_PARAMS := {
    "stimulus_width_deg": { "type": TYPE_FLOAT, "min": 1.0, "max": 180.0, "step": 1.0, "unit": "deg" },
}

# JSON defines what the value IS (value)
# config/stimulus.json: { "stimulus": { "params": { "stimulus_width_deg": 20.0 } } }

# Code reads the value from Config (which loads from JSON)
var width := Config.stimulus_width_deg
```

### 5. Running State vs. Recorded State

**Config is live and mutable** - it represents the current configuration that can change at any time.

**When an acquisition run starts, the dataset SNAPSHOTS the Config values.** This snapshot becomes part of the immutable scientific record for that run. The snapshot is stored in the run's output files, not in Config.

```gdscript
# Config is live - can change anytime
Config.stimulus_width_deg = 25.0

# When acquisition starts, dataset captures the current values
# These values are locked for that run's data analysis
func start_acquisition():
    _dataset.snapshot_config()  # Captures current Config values
    # From this point, the dataset has its own copy for the scientific record
    # Config can keep changing for future runs
```

### 6. The Running Stimulus is Nameless

The running stimulus has **no identity metadata** - no name, no description, no timestamps. It is simply the current state of configuration values.

- **Save = snapshot** - creates a named copy in `user://protocols/`
- **Load = replace** - overwrites running state from a snapshot
- The filename IS the protocol name

---

## File Structure

```
config/                              (Bundled - copied to user:// on first run)
├── hardware.json                    Physical rig setup
├── preferences.json                 App UI state
└── stimulus.json                    Experiment definition

user://                              (User data - read/write)
├── hardware.json                    THE hardware config
├── preferences.json                 THE preferences
├── stimulus.json                    THE running stimulus (nameless)
└── protocols/                       Saved stimulus snapshots
    └── {name}.json
```

---

## Data Flow

```
JSON Files (VALUES)              Config (CONTRACTS + ACCESS)           Codebase
─────────────────               ──────────────────────────            ─────────
stimulus.json      ◄──────────►  Typed accessors                      ALL code
hardware.json      ◄──────────►  (reads/writes JSON)                  reads from
preferences.json   ◄──────────►  Computed properties                  Config directly
                                 Persistence logic
```

**There is no intermediate layer.** Code does not receive config dictionaries. Code does not cache config values. Code reads from Config.

---

## Configuration Categories

### 1. Hardware (`user://hardware.json`)

**Purpose:** Physical rig setup. Rig-specific. NOT shareable. No save/load UI.

| Section | Parameter | Type | Description |
|---------|-----------|------|-------------|
| **camera** | type | string | Actual hardware name from system |
| | device_index | int | Camera device selection |
| | width_px | int | Frame width in pixels |
| | height_px | int | Frame height in pixels |
| | bit_depth | int | Bits per pixel |
| | exposure_us | int | Exposure time in microseconds |
| | gain | int | Camera gain (-1 = auto) |
| | use_hardware_timestamps | bool | Use hardware frame timestamps |
| **display** | index | int | Display output index |
| | width_cm | float | Physical display width |
| | height_cm | float | Physical display height |
| | physical_source | string | "edid", "user_override", or "none" |
| | edid_width_cm | float | Original EDID width (if overridden) |
| | edid_height_cm | float | Original EDID height (if overridden) |
| | refresh_hz | int | Display refresh rate |
| | measured_refresh_hz | float | Validated measured refresh rate |
| | refresh_validated | bool | Whether refresh rate has been validated |
| | scale_factor | float | UI scaling factor |
| | fps_divisor | int | Frame divisor for camera sync |
| **daemon** | startup_delay_ms | int | Daemon initialization delay |
| | shm_name | string | Shared memory buffer name |
| | shm_num_buffers | int | Ringbuffer slot count |

**Physical Dimension Tracking:**

Display physical dimensions come from EDID when available. If users override dimensions:
- Original EDID values preserved in `edid_width_cm` / `edid_height_cm`
- `physical_source` set to "user_override"
- This allows tracking provenance and reverting to EDID values

**Note:** DPI-based fallback is NOT used — OS returns logical DPI (UI scaling factor) not physical pixel density, causing ~25% errors in visual angle calculations.

### 2. Preferences (`user://preferences.json`)

**Purpose:** App UI state. Machine-specific. NOT shareable. No save/load UI.

| Section | Parameter | Type | Description |
|---------|-----------|------|-------------|
| (root) | last_save_directory | string | Last data save directory |
| | last_session_name | string | Last session name |
| **window_state** | maximized | bool | Window maximized state |
| | position_x | int | Window X position |
| | position_y | int | Window Y position |
| | width | int | Window width |
| | height | int | Window height |
| **ui** | show_debug_overlay | bool | Show debug info |
| | show_timing_info | bool | Show timing info |

### 3. Stimulus (`user://stimulus.json`)

**Purpose:** Experiment definition. SHAREABLE. Can save/load snapshots.

**The running stimulus has NO metadata** - no name, no timestamps. It's just values.
When saved as a snapshot, the filename IS the name.

#### Geometry (part of stimulus for reproducibility)
| Parameter | Type | Unit | Description |
|-----------|------|------|-------------|
| viewing_distance_cm | float | cm | Eye to display distance |
| horizontal_offset_deg | float | deg | Display horizontal offset |
| vertical_offset_deg | float | deg | Display vertical offset |
| projection_type | int | - | 0=CARTESIAN, 1=SPHERICAL, 2=CYLINDRICAL |

#### Stimulus Composition
| Parameter | Type | Values | Description |
|-----------|------|--------|-------------|
| type | string | "drifting_bar", "rotating_wedge", "expanding_ring", "full_field" | Renderer type |
| carrier | int | 0=CHECKERBOARD, 1=SOLID | Carrier pattern |
| envelope | int | 0=NONE, 1=BAR, 2=WEDGE, 3=RING | Spatial envelope |
| strobe_enabled | bool | true/false | Enable contrast reversal |

#### Stimulus Parameters
| Parameter | Type | Unit | Description |
|-----------|------|------|-------------|
| check_size_cm | float | cm | Check size for Cartesian space |
| check_size_deg | float | deg | Check size for angular space |
| stimulus_width_deg | float | deg | Width of bar/wedge/ring |
| sweep_speed_deg_per_sec | float | deg/s | Bar sweep speed |
| rotation_speed_deg_per_sec | float | deg/s | Wedge rotation speed |
| expansion_speed_deg_per_sec | float | deg/s | Ring expansion speed |
| rotation_deg | float | deg | Pattern rotation offset |
| contrast | float | 0-1 | Michelson contrast |
| mean_luminance | float | 0-1 | Mean brightness |
| luminance_min | float | 0-1 | Black level |
| luminance_max | float | 0-1 | White level |
| background_luminance | float | 0-1 | Background outside stimulus |
| strobe_frequency_hz | float | Hz | Contrast reversal frequency |

#### Timing
| Parameter | Type | Unit | Description |
|-----------|------|------|-------------|
| paradigm | string | - | "periodic", "episodic", "event_related" |
| baseline_start_sec | float | sec | Pre-stimulus baseline |
| baseline_end_sec | float | sec | Post-stimulus baseline |
| inter_stimulus_sec | float | sec | Inter-trial interval |
| inter_direction_sec | float | sec | Inter-direction interval |

#### Presentation
| Parameter | Type | Description |
|-----------|------|-------------|
| conditions | array | Conditions to present (e.g., ["LR","RL","TB","BT"]) |
| structure | string | "blocked" or "interleaved" |
| order | string | "sequential" or "randomized" |
| repetitions | int | Reps per condition |

---

## Config API

### Typed Accessors

Direct property access for all values. Code reads these properties - it does not cache them.

```gdscript
# Geometry
Config.viewing_distance_cm
Config.projection_type

# Stimulus
Config.stimulus_type
Config.carrier
Config.envelope
Config.strobe_enabled
Config.stimulus_width_deg
Config.sweep_speed_deg_per_sec
Config.check_size_deg

# Timing
Config.paradigm
Config.baseline_start_sec

# Presentation
Config.conditions
Config.repetitions

# Hardware
Config.camera_width_px
Config.display_refresh_hz

# Preferences
Config.last_save_directory
Config.window_maximized
```

### Computed Properties

```gdscript
Config.visual_field_width_deg   # Computed from display geometry
Config.visual_field_height_deg
Config.camera_fps               # From selected camera hardware
Config.bytes_per_frame          # width * height * (bit_depth / 8)
Config.sweep_duration_sec       # Based on envelope type and speed
Config.total_sweeps             # conditions.size() * repetitions
Config.total_duration_sec       # Full protocol duration
```

### Persistence

```gdscript
Config.save_hardware()          # Auto-save to user://hardware.json
Config.save_preferences()       # Auto-save to user://preferences.json
Config.save_stimulus()          # Auto-save to user://stimulus.json
```

### Snapshots (Stimulus Only)

```gdscript
Config.save_snapshot(name: String) -> String    # Returns path
Config.load_snapshot(name: String) -> bool      # Returns success
Config.list_snapshots() -> Array[Dictionary]    # [{name, path, created_at}, ...]
Config.delete_snapshot(name: String) -> bool    # Returns success
```

### Signals

```gdscript
signal hardware_changed(section: String, key: String, value: Variant)
signal preferences_changed(section: String, key: String, value: Variant)
signal stimulus_changed(section: String, key: String, value: Variant)
signal config_loaded()
```

---

## Behavioral Notes

### Wedge Envelope
- **gradual_seam** is not a toggle - wedge ALWAYS uses gradual seam (hardcoded behavior)
- Wedge/Ring envelopes always use SPHERICAL coordinate space for check_size

### Dataset Recording
- When acquisition starts, the dataset snapshots current Config values
- This snapshot is part of the scientific record and does not change
- Config can continue to change for future runs

### Refresh Pattern
- When Config changes, affected components call `refresh()` to update from Config
- Components do not store config copies - they read from Config each time they need a value
