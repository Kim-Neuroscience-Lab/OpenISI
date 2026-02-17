extends Node
## Session autoload: Screen navigation and session state management.
##
## This is the Single Source of Truth (SSoT) for:
## - Current screen (what UI is displayed)
## - Active session state (session data, anatomical, acquisitions)
##
## Navigation is unrestricted - screens adapt to available state rather than
## blocking access.

## Emitted when the current screen changes.
signal screen_changed(new_screen: Screen)

## Emitted when the selected camera changes.
signal camera_selected(camera: Dictionary)

## Emitted when the selected display changes.
signal display_selected(display: Dictionary)


## Application screens.
enum Screen {
	SETUP,           ## Hardware configuration (cameras, displays)
	FOCUS,           ## Camera preview, exposure, anatomical capture
	STIMULUS,        ## Protocol/stimulus design
	ACQUIRE,         ## Data acquisition
	RESULTS,         ## Session analysis and review
	SETTINGS,        ## App preferences (future)
	SESSION_BROWSER, ## Manage/select sessions (future)
}

## Screen display names (human-readable)
const SCREEN_DISPLAY_NAMES := {
	Screen.SETUP: "Setup",
	Screen.FOCUS: "Focus",
	Screen.STIMULUS: "Stimulus",
	Screen.ACQUIRE: "Acquire",
	Screen.RESULTS: "Results",
	Screen.SETTINGS: "Settings",
	Screen.SESSION_BROWSER: "Sessions",
}

## Screen scene paths
const SCREEN_SCENES := {
	Screen.SETUP: "res://src/ui/screens/setup/setup_screen.tscn",
	Screen.FOCUS: "res://src/ui/screens/focus/focus_screen.tscn",
	Screen.STIMULUS: "res://src/ui/screens/stimulus/stimulus_screen.tscn",
	Screen.ACQUIRE: "res://src/ui/screens/run/run_screen.tscn",
	Screen.RESULTS: "res://src/ui/screens/analyze/analyze_screen.tscn",
	# Future screens:
	# Screen.SETTINGS: "res://src/ui/screens/settings/settings_screen.tscn",
	# Screen.SESSION_BROWSER: "res://src/ui/screens/session_browser/session_browser_screen.tscn",
}

## Screen configuration (footer button text, etc.)
const SCREEN_CONFIG := {
	Screen.SETUP: {"primary_text": "Continue", "show_back": false},
	Screen.FOCUS: {"primary_text": "Continue", "show_back": true},
	Screen.STIMULUS: {"primary_text": "Continue", "show_back": true},
	Screen.ACQUIRE: {"primary_text": "Stop", "show_back": true},
	Screen.RESULTS: {"primary_text": "New Session", "show_back": true},
	Screen.SETTINGS: {"primary_text": "Done", "show_back": true},
	Screen.SESSION_BROWSER: {"primary_text": "Select", "show_back": true},
}

## Primary navigation screens (shown in header nav bar)
const PRIMARY_SCREENS: Array[Screen] = [
	Screen.SETUP,
	Screen.FOCUS,
	Screen.STIMULUS,
	Screen.ACQUIRE,
	Screen.RESULTS,
]

## Current screen being displayed.
var current_screen: Screen = Screen.SETUP

## Session state container.
var state: SessionState = SessionState.new()

## Whether acquisition is currently running (blocks navigation).
var acquisition_running: bool = false

## Convenience accessor for anatomical_texture from state
var anatomical_texture: ImageTexture:
	get: return state.anatomical_texture as ImageTexture
	set(value): state.anatomical_texture = value


func _ready() -> void:
	print("Session autoload initialized")
	print("  Starting screen: ", Screen.keys()[current_screen])


# -----------------------------------------------------------------------------
# Navigation
# -----------------------------------------------------------------------------

## Navigate to a screen. Always succeeds - screens adapt to state.
## Navigation is blocked during active acquisition to prevent data loss.
func navigate_to(screen: Screen) -> void:
	if current_screen == screen:
		return

	# Guard: Block navigation during acquisition
	if acquisition_running:
		push_warning("Navigation blocked: acquisition in progress")
		return

	var old_screen := current_screen
	current_screen = screen

	print("Screen: %s -> %s" % [Screen.keys()[old_screen], Screen.keys()[screen]])
	screen_changed.emit(screen)


## Get the display name for a screen.
func get_screen_name(screen: Screen) -> String:
	return SCREEN_DISPLAY_NAMES[screen]


## Get all primary screen names in order.
func get_primary_screen_names() -> Array[String]:
	var names: Array[String] = []
	for s in PRIMARY_SCREENS:
		names.append(SCREEN_DISPLAY_NAMES[s])
	return names


## Get the scene path for a screen.
func get_screen_scene_path(screen: Screen) -> String:
	return SCREEN_SCENES[screen]


## Get the configuration for a screen.
func get_screen_config(screen: Screen) -> Dictionary:
	return SCREEN_CONFIG[screen]


## Navigate to next primary screen (if any).
func navigate_next() -> void:
	var idx := PRIMARY_SCREENS.find(current_screen)
	if idx >= 0 and idx < PRIMARY_SCREENS.size() - 1:
		navigate_to(PRIMARY_SCREENS[idx + 1])


## Navigate to previous primary screen (if any).
func navigate_back() -> void:
	var idx := PRIMARY_SCREENS.find(current_screen)
	if idx > 0:
		navigate_to(PRIMARY_SCREENS[idx - 1])


# -----------------------------------------------------------------------------
# Session State Helpers
# -----------------------------------------------------------------------------

## Check if anatomical image has been captured.
func has_anatomical_image() -> bool:
	return state.has_anatomical


## Mark anatomical image as captured.
func set_anatomical_captured(path: String, texture: ImageTexture) -> void:
	state.set_anatomical(texture, path)


## Clear anatomical capture state.
func clear_anatomical() -> void:
	state.clear_anatomical()


## Mark acquisition as complete.
func set_acquisition_complete(total_frames: int, duration_ms: int) -> void:
	state.set_acquisition_complete(total_frames, duration_ms)


## Get total frames acquired.
func get_total_frames() -> int:
	return state.acquisition.total_frames


## Get acquisition duration in milliseconds.
func get_acquisition_duration() -> int:
	return state.acquisition.duration_ms


## Reset session state for a new session.
func reset_session() -> void:
	state.reset()
	# Keep hardware selection - user doesn't need to re-select camera/display
	navigate_to(Screen.SETUP)


# -----------------------------------------------------------------------------
# Hardware Selection (Runtime - NOT Persisted)
# -----------------------------------------------------------------------------

## Runtime state for selected camera (populated from HardwareManager detection)
var _selected_camera: Dictionary = {}

## Runtime state for selected display (populated from HardwareManager detection)
var _selected_display: Dictionary = {}


# --- Camera Selection ---

## Set the selected camera from HardwareManager enumeration result
func set_selected_camera(device: Dictionary) -> void:
	_selected_camera = device
	camera_selected.emit(device)


## Set camera with format merged in (for AVFoundation cameras with multiple formats)
func set_selected_camera_with_format(device: Dictionary, format: Dictionary) -> void:
	if device.is_empty():
		return

	# Merge format values into device
	var device_with_format: Dictionary = device.duplicate()
	device_with_format["width"] = int(format.get("width", 0))
	device_with_format["height"] = int(format.get("height", 0))
	device_with_format["fps"] = float(format.get("max_fps", 30.0))
	device_with_format["min_fps"] = float(format.get("min_fps", 1.0))
	device_with_format["max_fps"] = float(format.get("max_fps", 30.0))

	if format.has("bits_per_pixel"):
		device_with_format["bits_per_pixel"] = int(format["bits_per_pixel"])
	if format.has("bits_per_component"):
		device_with_format["bits_per_component"] = int(format["bits_per_component"])

	set_selected_camera(device_with_format)


## Get the selected camera device info (returns copy)
func get_selected_camera() -> Dictionary:
	return _selected_camera.duplicate(true)


## Check if a camera has been selected
func has_selected_camera() -> bool:
	return not _selected_camera.is_empty()


## Computed camera properties (convenience accessors)
var camera_type: String:
	get: return str(_selected_camera.get("type", ""))

var camera_device_index: int:
	get: return int(_selected_camera.get("index", 0))

var camera_width_px: int:
	get: return int(_selected_camera.get("width", 0))
	set(v): _selected_camera["width"] = v

var camera_height_px: int:
	get: return int(_selected_camera.get("height", 0))
	set(v): _selected_camera["height"] = v

var camera_bits_per_pixel: int:
	get: return int(_selected_camera.get("bits_per_pixel", 8))
	set(v): _selected_camera["bits_per_pixel"] = v

var camera_fps: float:
	get: return float(_selected_camera.get("fps", 30.0))


# --- Display Selection ---

## Set the selected display from HardwareManager enumeration result
func set_selected_display(monitor: Dictionary) -> void:
	_selected_display = monitor
	display_selected.emit(monitor)


## Get the selected display info (returns copy)
func get_selected_display() -> Dictionary:
	return _selected_display.duplicate(true)


## Check if a display has been selected
func has_selected_display() -> bool:
	return not _selected_display.is_empty()


## Computed display properties (convenience accessors)
var display_index: int:
	get: return int(_selected_display.get("index", 0))

var display_width_cm: float:
	get: return float(_selected_display.get("width_cm", 0.0))
	set(v):
		# Track if user is overriding EDID value
		if _selected_display.get("physical_source", "") != "user_override":
			_selected_display["edid_width_cm"] = _selected_display.get("width_cm", 0.0)
			_selected_display["edid_height_cm"] = _selected_display.get("height_cm", 0.0)
			_selected_display["edid_source"] = _selected_display.get("physical_source", "none")
			_selected_display["physical_source"] = "user_override"
		_selected_display["width_cm"] = v

var display_height_cm: float:
	get: return float(_selected_display.get("height_cm", 0.0))
	set(v):
		# Track if user is overriding EDID value
		if _selected_display.get("physical_source", "") != "user_override":
			_selected_display["edid_width_cm"] = _selected_display.get("width_cm", 0.0)
			_selected_display["edid_height_cm"] = _selected_display.get("height_cm", 0.0)
			_selected_display["edid_source"] = _selected_display.get("physical_source", "none")
			_selected_display["physical_source"] = "user_override"
		_selected_display["height_cm"] = v

var display_physical_source: String:
	get: return str(_selected_display.get("physical_source", "none"))

var display_refresh_hz: int:
	get: return int(_selected_display.get("refresh", 60))

var display_refresh_validated: bool:
	get: return bool(_selected_display.get("refresh_validated", false))

var display_measured_refresh_hz: float:
	get: return float(_selected_display.get("measured_refresh_hz", -1.0))


## Set display validation results (called by SetupScreen when validation completes)
func set_display_validation(measured_hz: float) -> void:
	_selected_display["measured_refresh_hz"] = measured_hz
	_selected_display["refresh_validated"] = true


## Clear display validation (called when validation fails or display changes)
func clear_display_validation() -> void:
	_selected_display["refresh_validated"] = false
	_selected_display.erase("measured_refresh_hz")
