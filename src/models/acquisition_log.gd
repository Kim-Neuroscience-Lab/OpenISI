## AcquisitionLog - Unified timing data from camera and stimulus
##
## Combines camera and stimulus timing data into a unified log for analysis.
## Provides methods to export complete acquisition data and access raw timestamps.
##
## Usage:
##   var log = AcquisitionLog.new()
##   log.camera = camera_dataset
##   log.stimulus = stimulus_dataset
##   log.finalize()  # Compute summary stats
##   log.save_json("/path/to/output.json")
class_name AcquisitionLog
extends RefCounted


## Camera timing dataset (hardware timestamps)
var camera: CameraDataset = null

## Stimulus timing dataset
var stimulus: StimulusDataset = null

## Session metadata
var session_info: Dictionary = {
	"id": "",
	"start_time": "",
	"end_time": "",
	"duration_sec": 0.0,
	"subject": "",
	"experiment": "",
}

## Hardware configuration
var hardware_info: Dictionary = {
	"camera_model": "",
	"camera_fps": 0.0,
	"display_model": "",
	"display_reported_refresh_hz": 0.0,
	"display_measured_refresh_hz": 0.0,
	"sync_mode": "",  # "POST_HOC" or "TRIGGERED"
}

## Computed timing quality summary
var quality_summary: Dictionary = {}


## Finalize the log by computing summary statistics
func finalize() -> void:
	# Compute session duration
	if camera and camera.frame_count > 0 and camera.timestamps_us.size() >= 2:
		var first_ts := camera.timestamps_us[0]
		var last_ts := camera.timestamps_us[camera.timestamps_us.size() - 1]
		session_info["duration_sec"] = float(last_ts - first_ts) / 1000000.0

	# Session times
	session_info["end_time"] = Time.get_datetime_string_from_system(true)
	if session_info["id"].is_empty():
		session_info["id"] = "acq_%s" % Time.get_unix_time_from_system()

	# Compute quality summary
	_compute_quality_summary()


## Compute timing quality summary
func _compute_quality_summary() -> void:
	quality_summary = {}

	# Camera metrics
	if camera and camera.frame_count > 0:
		var cam_metrics := camera.get_current_metrics()
		var cam_summary := {
			"frame_count": camera.frame_count,
			"actual_fps": camera.get_current_fps(),
			"expected_fps": camera.expected_fps,
			"dropped_count": camera.dropped_frame_indices.size(),
		}
		if cam_metrics.has("jitter_us"):
			cam_summary["jitter_us"] = float(cam_metrics["jitter_us"])
		quality_summary["camera"] = cam_summary

	# Stimulus metrics (if available and validated)
	if stimulus and stimulus._refresh_rate_validated:
		quality_summary["stimulus"] = {
			"frame_count": stimulus.frame_count,
			"actual_fps": stimulus.get_current_fps(),
			"expected_fps": stimulus.get_display_refresh_hz(),
			"dropped_count": stimulus.dropped_frame_indices.size(),
		}


## Get camera timestamps as PackedInt64Array (for Rust analysis)
func get_camera_timestamps() -> PackedInt64Array:
	if camera:
		return camera.timestamps_us.duplicate()
	return PackedInt64Array()


## Get stimulus timestamps as PackedInt64Array (for Rust analysis)
func get_stimulus_timestamps() -> PackedInt64Array:
	if stimulus:
		return stimulus.timestamps_us.duplicate()
	return PackedInt64Array()


## Export complete log to JSON file
func save_json(path: String) -> Error:
	var data := export_data()
	var json_string := JSON.stringify(data, "  ")

	# Ensure parent directory exists
	var dir_path := path.get_base_dir()
	if not DirAccess.dir_exists_absolute(dir_path):
		var err := DirAccess.make_dir_recursive_absolute(dir_path)
		if err != OK:
			push_error("AcquisitionLog: Failed to create directory: %s" % dir_path)
			return err

	var file := FileAccess.open(path, FileAccess.WRITE)
	if file == null:
		push_error("AcquisitionLog: Failed to open file for writing: %s" % path)
		return FileAccess.get_open_error()

	file.store_string(json_string)
	file.close()
	return OK


## Export all data as a dictionary
func export_data() -> Dictionary:
	var data := {
		"version": "1.0",
		"session": session_info.duplicate(),
		"hardware": hardware_info.duplicate(),
		"quality": quality_summary.duplicate(),
	}

	# Include camera timing data
	if camera:
		data["camera_timing"] = camera.export_data()

	# Include stimulus timing metadata (not raw arrays to save space)
	if stimulus:
		data["stimulus_metadata"] = stimulus.export_metadata()
		# Optionally include raw stimulus timestamps for correlation
		data["stimulus_timestamps_us"] = Array(stimulus.timestamps_us)

	return data


## Create from existing datasets
static func from_datasets(
	cam_dataset: CameraDataset,
	stim_dataset: StimulusDataset,
	session: Dictionary = {},
	hardware: Dictionary = {}
) -> AcquisitionLog:
	var log := AcquisitionLog.new()
	log.camera = cam_dataset
	log.stimulus = stim_dataset

	# Merge provided session info
	for key in session:
		log.session_info[key] = session[key]

	# Merge provided hardware info
	for key in hardware:
		log.hardware_info[key] = hardware[key]

	log.finalize()
	return log
