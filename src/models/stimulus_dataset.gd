## StimulusDataset - Complete per-frame stimulus data for analysis
##
## Captures everything needed for downstream analysis:
## - Hardware timestamps for each frame
## - Sequence state (condition, sweep, progress)
## - Paradigm-specific state (texture composition, element positions, media frame)
## - Timing quality metrics (jitter, dropped frames)
##
## Supports two modes:
## - Real-time: Populated incrementally as frames render
## - Pre-generated: Computed ahead of time for deterministic playback
class_name StimulusDataset
extends RefCounted


## Signals
signal frame_recorded(frame_index: int)
signal recording_started()
signal recording_stopped()


## Operating mode
enum Mode {
	REALTIME,      ## Populated incrementally during acquisition
	PREGENERATED,  ## Computed ahead of time
}

var mode: Mode = Mode.REALTIME


## Session metadata (JSON-serializable)
var session_id: String = ""
var session_start_time: String = ""  # ISO 8601 format


## Stimulus definition (from protocol)
var paradigm: String = ""  # "texture", "element", "media"
var stimulus_type: String = ""  # e.g., "drifting_bar", "random_dot", etc.
var stimulus_params: Dictionary = {}  # All stimulus parameters


## Sequence definition
var conditions: Array[String] = []
var repetitions: int = 0
var sequence_structure: Dictionary = {}  # type, shuffle, pairs, etc.
var sweep_sequence: Array[String] = []  # Precomputed order of conditions


## Timing configuration
var baseline_start_sec: float = 0.0
var baseline_end_sec: float = 0.0
var inter_trial_sec: float = 0.0
var sweep_duration_sec: float = 0.0


## Display configuration
var display_width_px: int = 0
var display_height_px: int = 0
var display_width_cm: float = 0.0
var display_height_cm: float = 0.0
var display_physical_source: String = ""  # "edid", "user_override", "none"
var visual_field_width_deg: float = 0.0
var visual_field_height_deg: float = 0.0
var viewing_distance_cm: float = 0.0
var center_azimuth_deg: float = 0.0
var center_elevation_deg: float = 0.0
var projection_type: String = ""  # "cartesian", "spherical", "cylindrical"


## Refresh rate (validated at display selection time, stored in Config)
## _reported_refresh_hz: What the OS/display advertises
## _measured_refresh_hz: What was measured before acquisition (stored in Session.display_measured_refresh_hz)
## _refresh_rate_validated: Always true if validated (required to start acquisition)
var _reported_refresh_hz: float = -1.0
var _measured_refresh_hz: float = -1.0
var _refresh_rate_validated: bool = false

## Timestamp source tracking
## _hardware_timestamps: True if timestamps are from VK_GOOGLE_display_timing (true hardware vsync)
## _timestamps_finalized: True if software timestamps have been mapped to hardware vsync
var _hardware_timestamps: bool = false
var _timestamps_finalized: bool = false


## Per-frame data arrays (efficient packed arrays for HDF5 export)
var frame_count: int = 0
var timestamps_us: PackedInt64Array = PackedInt64Array()
var conditions_per_frame: PackedStringArray = PackedStringArray()
var sweep_indices: PackedInt32Array = PackedInt32Array()
var frame_indices: PackedInt32Array = PackedInt32Array()  # Frame within sweep
var progress: PackedFloat32Array = PackedFloat32Array()  # 0-1 within sweep
var states: PackedStringArray = PackedStringArray()  # "baseline_start", "stimulus", "inter_trial", etc.

## Sequence-agnostic metadata (for analysis regardless of ordering strategy)
var condition_occurrences: PackedInt32Array = PackedInt32Array()  # Nth time this condition shown (1-indexed)
var is_baseline: PackedByteArray = PackedByteArray()  # 1=baseline/inter-trial, 0=stimulus


## Timing quality metrics
var frame_deltas_us: PackedInt64Array = PackedInt64Array()
var dropped_frame_indices: PackedInt32Array = PackedInt32Array()
var _last_timestamp_us: int = 0
var _expected_delta_us: int = -1  # -1 = not yet set (requires probe_refresh_rate())


## Paradigm-specific data (only one will be populated based on paradigm)
var texture_data: TextureFrameData = null
var element_data: ElementFrameData = null
var media_data: MediaFrameData = null


## Recording state
var _is_recording: bool = false


## Initialize dataset from current Config values (snapshots config at call time)
func initialize_from_config(display_geometry: DisplayGeometry) -> void:
	# Determine stimulus type from envelope
	var envelope: int = Settings.envelope
	match envelope:
		Envelopes.Type.NONE:
			stimulus_type = "full_field"
		Envelopes.Type.BAR:
			stimulus_type = "drifting_bar"
		Envelopes.Type.WEDGE:
			stimulus_type = "rotating_wedge"
		Envelopes.Type.RING:
			stimulus_type = "expanding_ring"

	# Snapshot all stimulus params from Config
	stimulus_params = Settings.get_stimulus_params()

	# Determine paradigm from stimulus type
	paradigm = _determine_paradigm(stimulus_type)

	# Sequence info from Config
	var conds: Array = Settings.conditions
	conditions.assign(conds)
	repetitions = Settings.repetitions
	sequence_structure = {
		"order": Settings.order,
	}
	sweep_sequence = _generate_sweep_sequence()

	# Timing info from Config
	baseline_start_sec = Settings.baseline_start_sec
	baseline_end_sec = Settings.baseline_end_sec
	inter_trial_sec = Settings.inter_stimulus_sec

	# Compute sweep duration
	sweep_duration_sec = _compute_sweep_duration(display_geometry.visual_field_width_deg)

	# Display info
	display_width_px = display_geometry.display_width_px
	display_height_px = display_geometry.display_height_px
	display_width_cm = display_geometry.display_width_cm
	display_height_cm = display_geometry.display_height_cm
	display_physical_source = Session.display_physical_source
	visual_field_width_deg = display_geometry.visual_field_width_deg
	visual_field_height_deg = display_geometry.visual_field_height_deg
	viewing_distance_cm = display_geometry.viewing_distance_cm
	center_azimuth_deg = display_geometry.center_azimuth_deg
	center_elevation_deg = display_geometry.center_elevation_deg
	projection_type = DisplayGeometry.ProjectionType.keys()[display_geometry.projection_type].to_lower()

	# Get validated refresh rate from Config (validation happened at display selection time)
	assert(Session.display_refresh_validated,
		"Display not validated - validation must succeed before acquisition")
	_measured_refresh_hz = Session.display_measured_refresh_hz
	_reported_refresh_hz = float(Session.display_refresh_hz)
	_refresh_rate_validated = true
	_expected_delta_us = int(1000000.0 / _measured_refresh_hz)

	# Initialize paradigm-specific data
	_initialize_paradigm_data()


## Compute sweep duration from Config
func _compute_sweep_duration(vf_width_deg: float) -> float:
	var width_deg: float = Settings.stimulus_width_deg
	var speed: float = Settings.sweep_speed_deg_per_sec
	if speed <= 0:
		return 0.0
	return (vf_width_deg + width_deg) / speed


## Generate the sweep sequence from Config
func _generate_sweep_sequence() -> Array[String]:
	var conds: Array = Settings.conditions
	var reps: int = Settings.repetitions
	var order: String = Settings.order

	var sequence: Array[String] = []

	match order:
		"sequential":  # Blocked
			for cond in conds:
				for _i in range(reps):
					sequence.append(cond)
		"interleaved":
			for _i in range(reps):
				for cond in conds:
					sequence.append(cond)
		"randomized":
			for cond in conds:
				for _i in range(reps):
					sequence.append(cond)
			sequence.shuffle()

	return sequence


func _determine_paradigm(type_id: String) -> String:
	# Map stimulus types to paradigms
	match type_id:
		"drifting_bar", "checkerboard", "rotating_wedge", "expanding_ring":
			return "texture"
		"random_dot", "sparse_noise":
			return "element"
		"image", "video":
			return "media"
		_:
			assert(false, "Unknown stimulus type: %s" % type_id)
			return ""  # Unreachable


func _initialize_paradigm_data() -> void:
	match paradigm:
		"texture":
			texture_data = TextureFrameData.new()
		"element":
			element_data = ElementFrameData.new()
		"media":
			media_data = MediaFrameData.new()


## NOTE: Refresh rate validation now happens at display selection time.
## This method is kept for backwards compatibility but should not be called.
## The validated rate is set in initialize_from_config() from Settings.
func probe_refresh_rate() -> void:
	assert(_refresh_rate_validated,
		"Refresh rate should already be validated at display selection time")


## Get the validated display refresh rate.
## Asserts if probe_refresh_rate() has not been called.
func get_display_refresh_hz() -> float:
	assert(_refresh_rate_validated,
		"Refresh rate not validated - call probe_refresh_rate() after warmup frames")
	return _measured_refresh_hz


## Start recording frames
func start_recording() -> void:
	if _is_recording:
		return

	# Validation must have happened at display selection time
	assert(_refresh_rate_validated,
		"Refresh rate not validated - call initialize_from_config() first")

	_is_recording = true
	session_start_time = Time.get_datetime_string_from_system(true)
	session_id = "session_%s" % Time.get_unix_time_from_system()
	_last_timestamp_us = 0

	recording_started.emit()


## Stop recording frames
func stop_recording() -> void:
	if not _is_recording:
		return

	_is_recording = false
	recording_stopped.emit()


## Record a frame with all its data
func record_frame(
	timestamp_us: int,
	condition: String,
	sweep_index: int,
	frame_in_sweep: int,
	sweep_progress: float,
	state: String,
	condition_occurrence: int,
	baseline: bool,
	paradigm_state: Dictionary
) -> void:
	if not _is_recording:
		return

	# Core timing data
	timestamps_us.append(timestamp_us)
	conditions_per_frame.append(condition)
	sweep_indices.append(sweep_index)
	frame_indices.append(frame_in_sweep)
	progress.append(sweep_progress)
	states.append(state)

	# Sequence-agnostic metadata
	condition_occurrences.append(condition_occurrence)
	is_baseline.append(1 if baseline else 0)

	# Timing quality - compute frame delta
	if _last_timestamp_us > 0:
		var delta_us := timestamp_us - _last_timestamp_us
		frame_deltas_us.append(delta_us)

		# Detect dropped frames (delta > 1.5x expected)
		# Only check after refresh rate has been validated (probe_refresh_rate() called)
		if _refresh_rate_validated and delta_us > _expected_delta_us * 1.5:
			dropped_frame_indices.append(frame_count)

	_last_timestamp_us = timestamp_us

	# Paradigm-specific data
	match paradigm:
		"texture":
			if texture_data:
				texture_data.record_frame(paradigm_state)
		"element":
			if element_data:
				element_data.record_frame(paradigm_state)
		"media":
			if media_data:
				media_data.record_frame(paradigm_state)

	frame_count += 1
	frame_recorded.emit(frame_count - 1)


## Get current frame data for UI display
func get_current_frame_data() -> Dictionary:
	if frame_count == 0:
		return {}

	var idx := frame_count - 1
	var data := {
		"frame_index": idx,
		"timestamp_us": timestamps_us[idx],
		"condition": conditions_per_frame[idx],
		"sweep_index": sweep_indices[idx],
		"frame_in_sweep": frame_indices[idx],
		"progress": progress[idx],
		"state": states[idx],
		"total_sweeps": sweep_sequence.size(),
		"condition_occurrence": condition_occurrences[idx],
		"is_baseline": is_baseline[idx] == 1,
	}

	# Add timing quality
	if frame_deltas_us.size() > 0:
		data["frame_delta_us"] = frame_deltas_us[frame_deltas_us.size() - 1]
		data["dropped_frames"] = dropped_frame_indices.size()

		# Compute jitter (std dev of recent frame deltas)
		var recent_count := mini(60, frame_deltas_us.size())
		if recent_count > 1:
			var sum: int = 0
			var sum_sq: int = 0
			for i: int in range(frame_deltas_us.size() - recent_count, frame_deltas_us.size()):
				var delta: int = frame_deltas_us[i]
				sum += delta
				sum_sq += delta * delta
			var mean: float = float(sum) / recent_count
			var variance: float = (float(sum_sq) / recent_count) - (mean * mean)
			data["jitter_us"] = sqrt(maxf(0.0, variance))

	# Add paradigm-specific data
	match paradigm:
		"texture":
			if texture_data:
				data.merge(texture_data.get_current_frame_data())
		"element":
			if element_data:
				data.merge(element_data.get_current_frame_data())
		"media":
			if media_data:
				data.merge(media_data.get_current_frame_data())

	return data


## Get FPS from recent frame deltas.
## Asserts if not enough frames have been recorded.
func get_current_fps() -> float:
	assert(frame_deltas_us.size() >= 2,
		"Need at least 2 frame deltas to compute FPS, have %d" % frame_deltas_us.size())

	# Average of last 10 frame deltas
	var count := mini(10, frame_deltas_us.size())
	var sum: int = 0
	for i: int in range(frame_deltas_us.size() - count, frame_deltas_us.size()):
		sum += frame_deltas_us[i]
	var avg_us := float(sum) / count

	assert(avg_us > 0, "Average frame delta is zero - invalid timestamp data")
	return 1000000.0 / avg_us


## Export metadata to JSON-compatible dictionary
func export_metadata() -> Dictionary:
	return {
		"session": {
			"id": session_id,
			"start_time": session_start_time,
		},
		"stimulus": {
			"paradigm": paradigm,
			"type": stimulus_type,
			"params": stimulus_params,
		},
		"sequence": {
			"conditions": conditions,
			"repetitions": repetitions,
			"structure": sequence_structure,
			"sweep_sequence": sweep_sequence,
		},
		"timing": {
			"baseline_start_sec": baseline_start_sec,
			"baseline_end_sec": baseline_end_sec,
			"inter_trial_sec": inter_trial_sec,
			"sweep_duration_sec": sweep_duration_sec,
		},
		"display": {
			"width_px": display_width_px,
			"height_px": display_height_px,
			"width_cm": display_width_cm,
			"height_cm": display_height_cm,
			"physical_source": display_physical_source,
			"reported_refresh_hz": _reported_refresh_hz,
			"measured_refresh_hz": _measured_refresh_hz,
			"refresh_rate_validated": _refresh_rate_validated,
			"visual_field_width_deg": visual_field_width_deg,
			"visual_field_height_deg": visual_field_height_deg,
			"viewing_distance_cm": viewing_distance_cm,
			"center_azimuth_deg": center_azimuth_deg,
			"center_elevation_deg": center_elevation_deg,
			"projection": projection_type,
		},
		"recording": {
			"frame_count": frame_count,
			"dropped_frames": dropped_frame_indices.size(),
			"hardware_timestamps": _hardware_timestamps,
			"timestamps_finalized": _timestamps_finalized,
			"timestamp_source": "VK_GOOGLE_display_timing" if _hardware_timestamps else "software",
		},
	}


## Check if recording
func is_recording() -> bool:
	return _is_recording


## Mark timestamps as hardware vsync timestamps (from VK_GOOGLE_display_timing)
func set_hardware_timestamps(enabled: bool) -> void:
	_hardware_timestamps = enabled


## Mark timestamps as finalized (software timestamps mapped to hardware vsync)
func set_timestamps_finalized(finalized: bool) -> void:
	_timestamps_finalized = finalized


## Check if using hardware timestamps
func has_hardware_timestamps() -> bool:
	return _hardware_timestamps


## Get comprehensive timing statistics.
## Requires probe_refresh_rate() to have been called.
## Returns dictionary with all timing metrics (see TimingStatistics class).
func get_full_statistics() -> Dictionary:
	assert(timestamps_us.size() > 0, "No frames recorded")
	assert(_refresh_rate_validated,
		"Refresh rate not validated - call probe_refresh_rate() after warmup frames")

	return TimingStatistics.compute(
		frame_deltas_us,
		_expected_delta_us,
		dropped_frame_indices,
		timestamps_us[0],
		timestamps_us[timestamps_us.size() - 1]
	)
