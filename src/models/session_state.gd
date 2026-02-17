## SessionState - Typed state container for session data
##
## Replaces the Dictionary-based session_data with typed properties.
## Provides type safety and clear documentation of what data the session manages.
class_name SessionState
extends RefCounted

## Session identification
var session_name: String = ""
var session_dir: String = ""
var created_at: String = ""

## Anatomical image
var anatomical_path: String = ""
var anatomical_texture: Texture2D = null
var has_anatomical: bool:
	get: return anatomical_texture != null

## Hardware state (captured at session start)
class HardwareState:
	var camera_index: int = -1
	var camera_type: String = ""
	var monitor_index: int = -1
	var exposure_us: int = 0

var hardware: HardwareState = HardwareState.new()

## Active stimulus configuration snapshot (stored as Dictionary from Config)
var stimulus_snapshot: Dictionary = {}

## Acquisition results
class AcquisitionResult:
	var completed: bool = false
	var total_frames: int = 0
	var duration_ms: int = 0
	var dropped_frames: int = 0
	var output_path: String = ""

	func get_duration_sec() -> float:
		return duration_ms / 1000.0

	func get_avg_fps() -> float:
		if duration_ms <= 0:
			return 0.0
		return total_frames / (duration_ms / 1000.0)

var acquisition: AcquisitionResult = AcquisitionResult.new()

## Reset state for new session
func reset() -> void:
	session_name = ""
	session_dir = ""
	created_at = ""
	anatomical_path = ""
	anatomical_texture = null
	hardware = HardwareState.new()
	stimulus_snapshot = {}
	acquisition = AcquisitionResult.new()

## Create session directory and set paths
func initialize(name: String, base_dir: String) -> bool:
	session_name = name
	created_at = Time.get_datetime_string_from_system()
	session_dir = base_dir.path_join(name)
	# Create directory if needed
	if not DirAccess.dir_exists_absolute(session_dir):
		var err := DirAccess.make_dir_recursive_absolute(session_dir)
		if err != OK:
			push_error("SessionState: Failed to create directory: %s" % session_dir)
			return false
	return true

## Set anatomical image
func set_anatomical(texture: Texture2D, save_path: String) -> void:
	anatomical_texture = texture
	anatomical_path = save_path

## Clear anatomical capture state
func clear_anatomical() -> void:
	anatomical_path = ""
	anatomical_texture = null

## Record acquisition completion
func set_acquisition_complete(frames: int, duration_ms_value: int, dropped: int = 0) -> void:
	acquisition.completed = true
	acquisition.total_frames = frames
	acquisition.duration_ms = duration_ms_value
	acquisition.dropped_frames = dropped

## Capture current stimulus configuration from Config
func capture_stimulus_snapshot() -> void:
	stimulus_snapshot = Settings.get_stimulus_data()


## Capture current hardware state from Session
func capture_hardware_state() -> void:
	hardware.camera_index = Session.camera_device_index
	hardware.camera_type = Session.camera_type
	hardware.monitor_index = Session.display_index
	hardware.exposure_us = Settings.exposure_us


## Get stimulus snapshot value
func get_stimulus_value(section: String, key: String) -> Variant:
	return stimulus_snapshot[section][key]
