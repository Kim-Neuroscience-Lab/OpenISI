## Settings - Persistent configuration manager (autoload)
##
## Single source of truth for persistent settings (hardware, preferences, stimulus).
## Note: Runtime state (camera/display selection) is in Session autoload.
##
## JSON files hold VALUES. This class holds CONTRACTS (type, min, max, step, unit).
## NO hardcoded defaults - all values come from config files.
## If JSON is missing a value, that's a bug that must be fixed in the JSON.
extends Node


# -----------------------------------------------------------------------------
# Signals
# -----------------------------------------------------------------------------

signal hardware_changed(section: String, key: String, value: Variant)
signal preferences_changed(section: String, key: String, value: Variant)
signal stimulus_changed(section: String, key: String, value: Variant)

signal config_loaded()
signal config_saved(path: String)
signal snapshot_saved(name: String, path: String)
signal snapshot_loaded(name: String)
signal snapshot_deleted(name: String)


# -----------------------------------------------------------------------------
# File Paths
# -----------------------------------------------------------------------------

const RES_HARDWARE := "res://config/hardware.json"
const RES_PREFERENCES := "res://config/preferences.json"
const RES_STIMULUS := "res://config/stimulus.json"

const USER_HARDWARE := "user://hardware.json"
const USER_PREFERENCES := "user://preferences.json"
const USER_STIMULUS := "user://stimulus.json"
const USER_PROTOCOLS_DIR := "user://protocols/"


# -----------------------------------------------------------------------------
# Parameter Contracts (NO defaults - just validation rules)
# -----------------------------------------------------------------------------

const GEOMETRY_PARAMS := {
	"viewing_distance_cm": { "type": TYPE_FLOAT, "min": 1.0, "max": 500.0, "step": 1.0, "unit": "cm" },
	"horizontal_offset_deg": { "type": TYPE_FLOAT, "min": -180.0, "max": 180.0, "step": 1.0, "unit": "deg" },
	"vertical_offset_deg": { "type": TYPE_FLOAT, "min": -90.0, "max": 90.0, "step": 1.0, "unit": "deg" },
	"projection_type": { "type": TYPE_INT, "min": 0, "max": 2, "step": 1 },
}

const STIMULUS_PARAMS := {
	"type": { "type": TYPE_STRING },
	"carrier": { "type": TYPE_INT, "min": 0, "max": 1, "step": 1 },
	"envelope": { "type": TYPE_INT, "min": 0, "max": 3, "step": 1 },
	"strobe_enabled": { "type": TYPE_BOOL },
	"check_size_cm": { "type": TYPE_FLOAT, "min": 0.1, "max": 100.0, "step": 0.1, "unit": "cm" },
	"check_size_deg": { "type": TYPE_FLOAT, "min": 0.1, "max": 90.0, "step": 0.1, "unit": "deg" },
	"stimulus_width_cm": { "type": TYPE_FLOAT, "min": 0.1, "max": 200.0, "step": 0.1, "unit": "cm" },
	"stimulus_width_deg": { "type": TYPE_FLOAT, "min": 1.0, "max": 180.0, "step": 1.0, "unit": "deg" },
	"sweep_speed_deg_per_sec": { "type": TYPE_FLOAT, "min": 0.1, "max": 100.0, "step": 0.1, "unit": "deg/s" },
	"rotation_speed_deg_per_sec": { "type": TYPE_FLOAT, "min": 0.1, "max": 360.0, "step": 0.1, "unit": "deg/s" },
	"expansion_speed_deg_per_sec": { "type": TYPE_FLOAT, "min": 0.1, "max": 180.0, "step": 0.1, "unit": "deg/s" },
	"rotation_deg": { "type": TYPE_FLOAT, "min": 0.0, "max": 360.0, "step": 1.0, "unit": "deg" },
	"contrast": { "type": TYPE_FLOAT, "min": 0.0, "max": 1.0, "step": 0.01 },
	"mean_luminance": { "type": TYPE_FLOAT, "min": 0.0, "max": 1.0, "step": 0.01 },
	"luminance_min": { "type": TYPE_FLOAT, "min": 0.0, "max": 1.0, "step": 0.01 },
	"luminance_max": { "type": TYPE_FLOAT, "min": 0.0, "max": 1.0, "step": 0.01 },
	"background_luminance": { "type": TYPE_FLOAT, "min": 0.0, "max": 1.0, "step": 0.01 },
	"strobe_frequency_hz": { "type": TYPE_FLOAT, "min": 0.1, "max": 60.0, "step": 0.1, "unit": "Hz" },
}

const TIMING_PARAMS := {
	"paradigm": { "type": TYPE_STRING },
	"baseline_start_sec": { "type": TYPE_FLOAT, "min": 0.0, "max": 300.0, "step": 0.5, "unit": "sec" },
	"baseline_end_sec": { "type": TYPE_FLOAT, "min": 0.0, "max": 300.0, "step": 0.5, "unit": "sec" },
	"inter_stimulus_sec": { "type": TYPE_FLOAT, "min": 0.0, "max": 300.0, "step": 0.5, "unit": "sec" },
	"inter_direction_sec": { "type": TYPE_FLOAT, "min": 0.0, "max": 300.0, "step": 0.5, "unit": "sec" },
}

const PRESENTATION_PARAMS := {
	"conditions": { "type": TYPE_ARRAY },
	"repetitions": { "type": TYPE_INT, "min": 1, "max": 1000, "step": 1 },
	"structure": { "type": TYPE_STRING },
	"order": { "type": TYPE_STRING },
}

const HARDWARE_CAMERA_PARAMS := {
	"type": { "type": TYPE_STRING },
	"device_index": { "type": TYPE_INT, "min": 0, "max": 10, "step": 1 },
	"width_px": { "type": TYPE_INT, "min": 64, "max": 8192, "step": 1, "unit": "px" },
	"height_px": { "type": TYPE_INT, "min": 64, "max": 8192, "step": 1, "unit": "px" },
	"bits_per_pixel": { "type": TYPE_INT, "min": 8, "max": 64, "step": 8, "unit": "bit" },
	"exposure_us": { "type": TYPE_INT, "min": 1, "max": 1000000, "step": 100, "unit": "us" },
	"gain": { "type": TYPE_INT, "min": -1, "max": 100, "step": 1 },
	"use_hardware_timestamps": { "type": TYPE_BOOL },
}

const HARDWARE_DISPLAY_PARAMS := {
	"index": { "type": TYPE_INT, "min": 0, "max": 10, "step": 1 },
	"width_cm": { "type": TYPE_FLOAT, "min": 1.0, "max": 500.0, "step": 0.1, "unit": "cm" },
	"height_cm": { "type": TYPE_FLOAT, "min": 1.0, "max": 500.0, "step": 0.1, "unit": "cm" },
	"refresh_hz": { "type": TYPE_INT, "min": 1, "max": 480, "step": 1, "unit": "Hz" },
	"scale_factor": { "type": TYPE_FLOAT, "min": 0.1, "max": 10.0, "step": 0.1 },
	"fps_divisor": { "type": TYPE_INT, "min": 1, "max": 60, "step": 1 },
}

const HARDWARE_DAEMON_PARAMS := {
	"startup_delay_ms": { "type": TYPE_INT, "min": 0, "max": 10000, "step": 100, "unit": "ms" },
	"shm_name": { "type": TYPE_STRING },
	"shm_num_buffers": { "type": TYPE_INT, "min": 1, "max": 64, "step": 1 },
}

const PREFERENCES_WINDOW_PARAMS := {
	"maximized": { "type": TYPE_BOOL },
	"position_x": { "type": TYPE_INT, "min": -10000, "max": 10000, "step": 1, "unit": "px" },
	"position_y": { "type": TYPE_INT, "min": -10000, "max": 10000, "step": 1, "unit": "px" },
	"width": { "type": TYPE_INT, "min": 100, "max": 10000, "step": 1, "unit": "px" },
	"height": { "type": TYPE_INT, "min": 100, "max": 10000, "step": 1, "unit": "px" },
}

const PREFERENCES_UI_PARAMS := {
	"show_debug_overlay": { "type": TYPE_BOOL },
	"show_timing_info": { "type": TYPE_BOOL },
	"target_fps": { "type": TYPE_INT, "min": 0, "max": 240, "step": 1, "unit": "fps" },
}


# -----------------------------------------------------------------------------
# Parameter Contract Lookup
# -----------------------------------------------------------------------------

## Get the contract (min/max/step/unit) for a parameter by name.
## Searches all param dictionaries. Returns empty dict if not found.
func lookup_param_contract(param_name: String) -> Dictionary:
	# Search all param dictionaries in order
	for params in [STIMULUS_PARAMS, GEOMETRY_PARAMS, TIMING_PARAMS, PRESENTATION_PARAMS,
				   HARDWARE_CAMERA_PARAMS, HARDWARE_DISPLAY_PARAMS, HARDWARE_DAEMON_PARAMS]:
		if params.has(param_name):
			return params[param_name]
	return {}


# -----------------------------------------------------------------------------
# State
# -----------------------------------------------------------------------------

var _hardware: Dictionary = {}
var _preferences: Dictionary = {}
var _stimulus: Dictionary = {}



# -----------------------------------------------------------------------------
# Lifecycle
# -----------------------------------------------------------------------------

func _ready() -> void:
	_ensure_user_files()
	_load_all()
	config_loaded.emit()


func _ensure_user_files() -> void:
	_ensure_dir(USER_PROTOCOLS_DIR)
	_ensure_user_file(RES_HARDWARE, USER_HARDWARE)
	_ensure_user_file(RES_PREFERENCES, USER_PREFERENCES)
	_ensure_user_file(RES_STIMULUS, USER_STIMULUS)


func _ensure_dir(path: String) -> void:
	if not DirAccess.dir_exists_absolute(path):
		DirAccess.make_dir_recursive_absolute(path)


func _ensure_user_file(res_path: String, user_path: String) -> void:
	var bundled := FileUtils.load_json(res_path)
	if bundled.is_empty():
		return

	if not FileAccess.file_exists(user_path):
		# No user file - copy bundled
		FileUtils.save_json(user_path, bundled)
	else:
		# User file exists - merge missing keys from bundled
		var user := FileUtils.load_json(user_path)
		if _merge_missing_keys(user, bundled):
			FileUtils.save_json(user_path, user)


## Recursively merge missing keys from source into target.
## Returns true if any keys were added.
func _merge_missing_keys(target: Dictionary, source: Dictionary) -> bool:
	var modified := false
	for key in source:
		if not target.has(key):
			target[key] = source[key]
			modified = true
		elif target[key] is Dictionary and source[key] is Dictionary:
			if _merge_missing_keys(target[key], source[key]):
				modified = true
	return modified


func _load_all() -> void:
	_hardware = FileUtils.load_json(USER_HARDWARE)
	_preferences = FileUtils.load_json(USER_PREFERENCES)
	_stimulus = FileUtils.load_json(USER_STIMULUS)


# -----------------------------------------------------------------------------
# Accessors: Geometry (in stimulus.json)
# -----------------------------------------------------------------------------

var viewing_distance_cm: float:
	get: return float(_stimulus["geometry"]["viewing_distance_cm"])
	set(v): _set_stimulus("geometry", "viewing_distance_cm", v)

var horizontal_offset_deg: float:
	get: return float(_stimulus["geometry"]["horizontal_offset_deg"])
	set(v): _set_stimulus("geometry", "horizontal_offset_deg", v)

var vertical_offset_deg: float:
	get: return float(_stimulus["geometry"]["vertical_offset_deg"])
	set(v): _set_stimulus("geometry", "vertical_offset_deg", v)

var projection_type: int:
	get: return int(_stimulus["geometry"]["projection_type"])
	set(v): _set_stimulus("geometry", "projection_type", v)


# -----------------------------------------------------------------------------
# Accessors: Stimulus Type/Composition
# -----------------------------------------------------------------------------

var stimulus_type: String:
	get: return str(_stimulus["stimulus"]["type"])
	set(v): _set_stimulus("stimulus", "type", v)

var carrier: int:
	get: return int(_stimulus["stimulus"]["carrier"])
	set(v): _set_stimulus("stimulus", "carrier", v)

var envelope: int:
	get: return int(_stimulus["stimulus"]["envelope"])
	set(v): _set_stimulus("stimulus", "envelope", v)

var strobe_enabled: bool:
	get: return bool(_stimulus["stimulus"]["strobe_enabled"])
	set(v): _set_stimulus("stimulus", "strobe_enabled", v)


# -----------------------------------------------------------------------------
# Accessors: Stimulus Params
# -----------------------------------------------------------------------------

func get_stimulus_param(key: String) -> Variant:
	return _stimulus["stimulus"]["params"][key]


func set_stimulus_param(key: String, value: Variant) -> void:
	if not _stimulus.has("stimulus"):
		_stimulus["stimulus"] = {}
	if not _stimulus["stimulus"].has("params"):
		_stimulus["stimulus"]["params"] = {}
	_stimulus["stimulus"]["params"][key] = value
	stimulus_changed.emit("stimulus.params", key, value)
	save_stimulus()


## Get all stimulus params as dictionary
func get_stimulus_params() -> Dictionary:
	return _stimulus["stimulus"]["params"].duplicate(true)


# Convenience accessors for common stimulus params
var check_size_cm: float:
	get: return float(get_stimulus_param("check_size_cm"))
	set(v): set_stimulus_param("check_size_cm", v)

var check_size_deg: float:
	get: return float(get_stimulus_param("check_size_deg"))
	set(v): set_stimulus_param("check_size_deg", v)

var stimulus_width_cm: float:
	get: return float(get_stimulus_param("stimulus_width_cm"))
	set(v): set_stimulus_param("stimulus_width_cm", v)

var stimulus_width_deg: float:
	get: return float(get_stimulus_param("stimulus_width_deg"))
	set(v): set_stimulus_param("stimulus_width_deg", v)

var sweep_speed_deg_per_sec: float:
	get: return float(get_stimulus_param("sweep_speed_deg_per_sec"))
	set(v): set_stimulus_param("sweep_speed_deg_per_sec", v)

var rotation_speed_deg_per_sec: float:
	get: return float(get_stimulus_param("rotation_speed_deg_per_sec"))
	set(v): set_stimulus_param("rotation_speed_deg_per_sec", v)

var expansion_speed_deg_per_sec: float:
	get: return float(get_stimulus_param("expansion_speed_deg_per_sec"))
	set(v): set_stimulus_param("expansion_speed_deg_per_sec", v)

var rotation_deg: float:
	get: return float(get_stimulus_param("rotation_deg"))
	set(v): set_stimulus_param("rotation_deg", v)

var contrast: float:
	get: return float(get_stimulus_param("contrast"))
	set(v): set_stimulus_param("contrast", v)

var mean_luminance: float:
	get: return float(get_stimulus_param("mean_luminance"))
	set(v): set_stimulus_param("mean_luminance", v)

var luminance_min: float:
	get: return float(get_stimulus_param("luminance_min"))
	set(v): set_stimulus_param("luminance_min", v)

var luminance_max: float:
	get: return float(get_stimulus_param("luminance_max"))
	set(v): set_stimulus_param("luminance_max", v)

var background_luminance: float:
	get: return float(get_stimulus_param("background_luminance"))
	set(v): set_stimulus_param("background_luminance", v)

var strobe_frequency_hz: float:
	get: return float(get_stimulus_param("strobe_frequency_hz"))
	set(v): set_stimulus_param("strobe_frequency_hz", v)


# -----------------------------------------------------------------------------
# Accessors: Timing
# -----------------------------------------------------------------------------

var paradigm: String:
	get: return str(_stimulus["timing"]["paradigm"])
	set(v): _set_stimulus("timing", "paradigm", v)

var baseline_start_sec: float:
	get: return float(_stimulus["timing"]["baseline_start_sec"])
	set(v): _set_stimulus("timing", "baseline_start_sec", v)

var baseline_end_sec: float:
	get: return float(_stimulus["timing"]["baseline_end_sec"])
	set(v): _set_stimulus("timing", "baseline_end_sec", v)

var inter_stimulus_sec: float:
	get: return float(_stimulus["timing"]["inter_stimulus_sec"])
	set(v): _set_stimulus("timing", "inter_stimulus_sec", v)

var inter_direction_sec: float:
	get: return float(_stimulus["timing"]["inter_direction_sec"])
	set(v): _set_stimulus("timing", "inter_direction_sec", v)

## Get full timing section as dictionary
func get_timing() -> Dictionary:
	return _stimulus["timing"].duplicate(true)


# -----------------------------------------------------------------------------
# Accessors: Presentation
# -----------------------------------------------------------------------------

var conditions: Array:
	get: return _stimulus["presentation"]["conditions"]
	set(v): _set_stimulus("presentation", "conditions", v)

var repetitions: int:
	get: return int(_stimulus["presentation"]["repetitions"])
	set(v): _set_stimulus("presentation", "repetitions", v)

var structure: String:
	get: return str(_stimulus["presentation"]["structure"])
	set(v): _set_stimulus("presentation", "structure", v)

var order: String:
	get: return str(_stimulus["presentation"]["order"])
	set(v): _set_stimulus("presentation", "order", v)

## Get full presentation section as dictionary
func get_presentation() -> Dictionary:
	return _stimulus["presentation"].duplicate(true)


# -----------------------------------------------------------------------------
# Accessors: Hardware - Camera (Persistent Settings Only)
# -----------------------------------------------------------------------------
# Note: Runtime camera selection state is in Session autoload, not here.
# These are user-configurable camera settings that persist across sessions.

var camera_exposure_us: int:
	get: return int(_hardware["camera"]["exposure_us"])
	set(v): _set_hardware("camera", "exposure_us", v)

var camera_gain: int:
	get: return int(_hardware["camera"]["gain"])
	set(v): _set_hardware("camera", "gain", v)


# -----------------------------------------------------------------------------
# Accessors: Hardware - Display (Persistent Settings Only)
# -----------------------------------------------------------------------------
# Note: Runtime display selection state is in Session autoload, not here.
# These are user-configurable display settings that persist across sessions.

var display_fps_divisor: int:
	get: return int(_hardware["display"]["fps_divisor"])
	set(v): _set_hardware("display", "fps_divisor", v)


# -----------------------------------------------------------------------------
# Accessors: Hardware - Daemon
# -----------------------------------------------------------------------------

var daemon_startup_delay_ms: int:
	get: return int(_hardware["daemon"]["startup_delay_ms"])
	set(v): _set_hardware("daemon", "startup_delay_ms", v)

var daemon_shm_name: String:
	get: return str(_hardware["daemon"]["shm_name"])
	set(v): _set_hardware("daemon", "shm_name", v)

var daemon_shm_num_buffers: int:
	get: return int(_hardware["daemon"]["shm_num_buffers"])
	set(v): _set_hardware("daemon", "shm_num_buffers", v)


# -----------------------------------------------------------------------------
# Accessors: Preferences - Session
# -----------------------------------------------------------------------------

var last_save_directory: String:
	get: return str(_preferences["last_save_directory"])
	set(v): _set_preferences_root("last_save_directory", v)

var last_session_name: String:
	get: return str(_preferences["last_session_name"])
	set(v): _set_preferences_root("last_session_name", v)


# -----------------------------------------------------------------------------
# Accessors: Preferences - Window State
# -----------------------------------------------------------------------------

var window_maximized: bool:
	get: return bool(_preferences["window_state"]["maximized"])
	set(v): _set_preferences("window_state", "maximized", v)

var window_position_x: int:
	get: return int(_preferences["window_state"]["position_x"])
	set(v): _set_preferences("window_state", "position_x", v)

var window_position_y: int:
	get: return int(_preferences["window_state"]["position_y"])
	set(v): _set_preferences("window_state", "position_y", v)

var window_width: int:
	get: return int(_preferences["window_state"]["width"])
	set(v): _set_preferences("window_state", "width", v)

var window_height: int:
	get: return int(_preferences["window_state"]["height"])
	set(v): _set_preferences("window_state", "height", v)


# -----------------------------------------------------------------------------
# Accessors: Preferences - UI
# -----------------------------------------------------------------------------

var show_debug_overlay: bool:
	get: return bool(_preferences["ui"]["show_debug_overlay"])
	set(v): _set_preferences("ui", "show_debug_overlay", v)

var show_timing_info: bool:
	get: return bool(_preferences["ui"]["show_timing_info"])
	set(v): _set_preferences("ui", "show_timing_info", v)

var target_fps: int:
	get: return int(_preferences["ui"]["target_fps"])
	set(v): _set_preferences("ui", "target_fps", v)


# -----------------------------------------------------------------------------
# Internal Setters
# -----------------------------------------------------------------------------

func _set_hardware(section: String, key: String, value: Variant) -> void:
	if not _hardware.has(section):
		_hardware[section] = {}
	_hardware[section][key] = value
	hardware_changed.emit(section, key, value)
	save_hardware()


func _set_preferences(section: String, key: String, value: Variant) -> void:
	if not _preferences.has(section):
		_preferences[section] = {}
	_preferences[section][key] = value
	preferences_changed.emit(section, key, value)
	save_preferences()


func _set_preferences_root(key: String, value: Variant) -> void:
	_preferences[key] = value
	preferences_changed.emit("", key, value)
	save_preferences()


func _set_stimulus(section: String, key: String, value: Variant) -> void:
	if not _stimulus.has(section):
		_stimulus[section] = {}
	_stimulus[section][key] = value
	stimulus_changed.emit(section, key, value)
	save_stimulus()


# -----------------------------------------------------------------------------
# Computed Properties
# -----------------------------------------------------------------------------
# Note: These use GeometryCalculator for pure computations.
# Session values (display_width_cm, display_height_cm) are read here as
# the single point where runtime state is combined with persistent settings.

## Sweep duration based on envelope type
## Uses GeometryCalculator for visual field calculation
var sweep_duration_sec: float:
	get:
		# Compute visual field width using GeometryCalculator (pure function)
		var vf_width := GeometryCalculator.visual_field_width_deg(
			Session.display_width_cm,
			viewing_distance_cm
		)
		return GeometryCalculator.sweep_duration_sec(
			envelope,
			vf_width,
			sweep_speed_deg_per_sec,
			rotation_speed_deg_per_sec,
			expansion_speed_deg_per_sec,
			inter_stimulus_sec
		)

## Total number of sweeps (conditions x repetitions)
var total_sweeps: int:
	get:
		var conds := conditions
		if conds.is_empty():
			return repetitions
		return conds.size() * repetitions

## Total protocol duration in seconds
## Uses GeometryCalculator for duration calculation
var total_duration_sec: float:
	get:
		var conds := conditions
		return GeometryCalculator.total_duration_sec(
			sweep_duration_sec,
			conds.size(),
			repetitions,
			baseline_start_sec,
			baseline_end_sec,
			inter_direction_sec
		)


# -----------------------------------------------------------------------------
# Persistence
# -----------------------------------------------------------------------------

func save_hardware() -> void:
	if FileUtils.save_json(USER_HARDWARE, _hardware):
		config_saved.emit(USER_HARDWARE)


func save_preferences() -> void:
	if FileUtils.save_json(USER_PREFERENCES, _preferences):
		config_saved.emit(USER_PREFERENCES)


func save_stimulus() -> void:
	if FileUtils.save_json(USER_STIMULUS, _stimulus):
		config_saved.emit(USER_STIMULUS)


func save_all() -> void:
	save_hardware()
	save_preferences()
	save_stimulus()


# -----------------------------------------------------------------------------
# Snapshots (Stimulus Only)
# -----------------------------------------------------------------------------

func save_snapshot(snap_name: String) -> String:
	if snap_name.is_empty():
		ErrorHandler.report_config_error(
			"Snapshot name cannot be empty",
			"",
			ErrorHandler.Code.CONFIG_INVALID_VALUE,
			ErrorHandler.Severity.WARNING
		)
		return ""

	# Sanitize name for filename
	var safe_name := snap_name.to_lower().replace(" ", "_")
	safe_name = safe_name.replace("/", "_").replace("\\", "_")

	var path := USER_PROTOCOLS_DIR.path_join(safe_name + ".json")

	# Create snapshot with metadata (snapshots have identity)
	var snapshot := _stimulus.duplicate(true)
	snapshot["_meta"] = {
		"name": snap_name,
		"created_at": Time.get_datetime_string_from_system(),
	}

	if FileUtils.save_json(path, snapshot):
		snapshot_saved.emit(snap_name, path)
		return path

	return ""


func load_snapshot(snap_name: String) -> bool:
	var safe_name := snap_name.to_lower().replace(" ", "_")
	safe_name = safe_name.replace("/", "_").replace("\\", "_")

	var path := USER_PROTOCOLS_DIR.path_join(safe_name + ".json")

	if not FileAccess.file_exists(path):
		ErrorHandler.report_config_error(
			"Snapshot not found",
			"Path: %s" % path,
			ErrorHandler.Code.CONFIG_LOAD_FAILED,
			ErrorHandler.Severity.WARNING
		)
		return false

	var data := FileUtils.load_json(path)
	if data.is_empty():
		return false

	# Remove metadata before applying (running stimulus is nameless)
	data.erase("_meta")

	set_stimulus_data(data)  # Bulk update with signal and persistence
	snapshot_loaded.emit(snap_name)
	return true


func list_snapshots() -> Array[Dictionary]:
	var snapshots: Array[Dictionary] = []

	var dir := DirAccess.open(USER_PROTOCOLS_DIR)
	if dir == null:
		return snapshots

	dir.list_dir_begin()
	var file_name := dir.get_next()

	while file_name != "":
		if not dir.current_is_dir() and file_name.ends_with(".json"):
			var path := USER_PROTOCOLS_DIR.path_join(file_name)
			var data := FileUtils.load_json(path)
			if not data.is_empty() and data.has("_meta"):
				var meta: Dictionary = data["_meta"]
				snapshots.append({
					"name": str(meta["name"]),
					"path": path,
					"created_at": str(meta["created_at"]),
				})
		file_name = dir.get_next()

	dir.list_dir_end()
	return snapshots


func delete_snapshot(snap_name: String) -> bool:
	var safe_name := snap_name.to_lower().replace(" ", "_")
	safe_name = safe_name.replace("/", "_").replace("\\", "_")

	var path := USER_PROTOCOLS_DIR.path_join(safe_name + ".json")

	if not FileAccess.file_exists(path):
		return false

	var dir := DirAccess.open(USER_PROTOCOLS_DIR)
	if dir:
		var err := dir.remove(safe_name + ".json")
		if err == OK:
			snapshot_deleted.emit(snap_name)
			return true

	return false


# -----------------------------------------------------------------------------
# Window State Helpers
# -----------------------------------------------------------------------------

func update_window_state(window: Window) -> void:
	if window:
		window_maximized = window.mode == Window.MODE_MAXIMIZED
		if not window_maximized:
			window_position_x = int(window.position.x)
			window_position_y = int(window.position.y)
			window_width = int(window.size.x)
			window_height = int(window.size.y)


func apply_window_state(window: Window) -> void:
	if window:
		if window_maximized:
			window.mode = Window.MODE_MAXIMIZED
		else:
			window.position = Vector2i(window_position_x, window_position_y)
			window.size = Vector2i(window_width, window_height)


# -----------------------------------------------------------------------------
# Parameter Contract Access (for UI validation)
# -----------------------------------------------------------------------------

func get_param_contract(category: String, key: String) -> Dictionary:
	match category:
		"geometry": return GEOMETRY_PARAMS[key]
		"stimulus": return STIMULUS_PARAMS[key]
		"timing": return TIMING_PARAMS[key]
		"presentation": return PRESENTATION_PARAMS[key]
		"camera": return HARDWARE_CAMERA_PARAMS[key]
		"display": return HARDWARE_DISPLAY_PARAMS[key]
		"daemon": return HARDWARE_DAEMON_PARAMS[key]
		"window": return PREFERENCES_WINDOW_PARAMS[key]
		"ui": return PREFERENCES_UI_PARAMS[key]
	ErrorHandler.report_config_error(
		"Unknown parameter category: %s" % category,
		"",
		ErrorHandler.Code.CONFIG_INVALID_VALUE,
		ErrorHandler.Severity.WARNING
	)
	return {}


func validate_value(category: String, key: String, value: Variant) -> bool:
	var contract := get_param_contract(category, key)
	var expected_type: int = int(contract["type"])
	if typeof(value) != expected_type:
		return false

	if contract.has("min") and value < contract["min"]:
		return false
	if contract.has("max") and value > contract["max"]:
		return false

	return true


# -----------------------------------------------------------------------------
# Raw Data Access (for advanced use cases)
# -----------------------------------------------------------------------------

## Get full stimulus data as dictionary
func get_stimulus_data() -> Dictionary:
	return _stimulus.duplicate(true)


## Set full stimulus data (bulk update with signal and persistence)
func set_stimulus_data(data: Dictionary) -> void:
	_stimulus = data.duplicate(true)
	save_stimulus()
	stimulus_changed.emit("", "", null)


## Get full hardware data as dictionary
func get_hardware_data() -> Dictionary:
	return _hardware.duplicate(true)


## Get full preferences data as dictionary
func get_preferences_data() -> Dictionary:
	return _preferences.duplicate(true)
