## CameraDataset - Per-frame camera timing data for analysis
##
## Captures hardware timestamps and frame indices from the camera daemon.
## Computes timing metrics: jitter, dropped frames, actual FPS.
##
## Usage:
##   var dataset = CameraDataset.new()
##   dataset.initialize(30.0)  # Expected FPS
##   dataset.start_recording()
##   # In _process loop:
##   dataset.record_frame(frame_index, timestamp_us)
##   dataset.stop_recording()
class_name CameraDataset
extends RefCounted


## Signals
signal frame_recorded(frame_index: int)
signal recording_started()
signal recording_stopped()


## Configuration - must be set before start_recording()
var expected_fps: float = 0.0
var _expected_delta_us: int = 0  # Computed from expected_fps


## Per-frame data arrays (efficient packed arrays for analysis)
var frame_count: int = 0
var timestamps_us: PackedInt64Array = PackedInt64Array()     # Hardware timestamps
var frame_indices: PackedInt32Array = PackedInt32Array()      # Daemon frame counter
var frame_deltas_us: PackedInt64Array = PackedInt64Array()    # Computed intervals


## Timing quality metrics
var dropped_frame_indices: PackedInt32Array = PackedInt32Array()
var _last_timestamp_us: int = 0
var _last_frame_index: int = 0


## Session metadata
var session_start_time: String = ""
var session_id: String = ""


## Recording state
var _is_recording: bool = false


## Initialize dataset with expected camera FPS
func initialize(fps: float) -> void:
	expected_fps = fps
	if fps > 0:
		_expected_delta_us = int(1000000.0 / fps)
	else:
		_expected_delta_us = 0


## Start recording frames
func start_recording() -> void:
	if _is_recording:
		return

	# Ensure expected_fps is set before recording
	assert(expected_fps > 0, "CameraDataset: expected_fps must be set before start_recording()")
	_expected_delta_us = int(1000000.0 / expected_fps)

	# Clear previous data
	frame_count = 0
	timestamps_us.clear()
	frame_indices.clear()
	frame_deltas_us.clear()
	dropped_frame_indices.clear()
	_last_timestamp_us = 0
	_last_frame_index = 0

	_is_recording = true
	session_start_time = Time.get_datetime_string_from_system(true)
	session_id = "camera_%s" % Time.get_unix_time_from_system()

	recording_started.emit()


## Stop recording frames
func stop_recording() -> void:
	if not _is_recording:
		return

	_is_recording = false
	recording_stopped.emit()


## Record a frame with its hardware timestamp and daemon frame index
func record_frame(daemon_frame_index: int, timestamp_us: int) -> void:
	if not _is_recording:
		return

	# Store raw data
	timestamps_us.append(timestamp_us)
	frame_indices.append(daemon_frame_index)

	# Compute frame delta if we have a previous frame
	if _last_timestamp_us > 0 and timestamp_us > _last_timestamp_us:
		var delta_us := timestamp_us - _last_timestamp_us
		frame_deltas_us.append(delta_us)

		# Detect dropped frames (delta > 1.5x expected)
		if _expected_delta_us > 0 and delta_us > _expected_delta_us * 1.5:
			dropped_frame_indices.append(frame_count)

	# Also detect dropped frames via frame index jumps
	if _last_frame_index > 0:
		var index_jump := daemon_frame_index - _last_frame_index
		if index_jump > 1:
			# Multiple daemon frames were missed
			for i in range(index_jump - 1):
				if not dropped_frame_indices.has(frame_count):
					dropped_frame_indices.append(frame_count)

	_last_timestamp_us = timestamp_us
	_last_frame_index = daemon_frame_index

	frame_count += 1
	frame_recorded.emit(frame_count - 1)


## Get current timing metrics for UI display
func get_current_metrics() -> Dictionary:
	if frame_count == 0:
		return {
			"frame_count": 0,
			"dropped_frames": 0,
		}

	# If we have frames, we must have timestamps
	assert(timestamps_us.size() > 0, "CameraDataset: frame_count > 0 but no timestamps")

	var metrics := {
		"frame_count": frame_count,
		"dropped_frames": dropped_frame_indices.size(),
		"latest_timestamp_us": timestamps_us[timestamps_us.size() - 1],
	}

	# Delta/jitter metrics require at least 2 frames
	if frame_deltas_us.size() > 0:
		metrics["latest_delta_us"] = frame_deltas_us[frame_deltas_us.size() - 1]

	# Compute jitter (std dev of recent frame deltas) - requires multiple deltas
	var recent_count := mini(60, frame_deltas_us.size())
	if recent_count > 1:
		var sum: int = 0
		var sum_sq: int = 0
		for i in range(frame_deltas_us.size() - recent_count, frame_deltas_us.size()):
			var delta: int = frame_deltas_us[i]
			sum += delta
			sum_sq += delta * delta
		var mean: float = float(sum) / recent_count
		var variance: float = (float(sum_sq) / recent_count) - (mean * mean)
		metrics["jitter_us"] = sqrt(maxf(0.0, variance))
		metrics["mean_delta_us"] = mean

	return metrics


## Check if we have valid timing data to compute FPS
func has_valid_timing_data() -> bool:
	if frame_deltas_us.size() < 2:
		return false
	# Check that deltas aren't all zeros (which would indicate missing timestamps)
	for delta in frame_deltas_us:
		if delta > 0:
			return true
	return false


## Get actual FPS from recent frame deltas
## Caller must check has_valid_timing_data() first
func get_current_fps() -> float:
	assert(frame_deltas_us.size() >= 2, "CameraDataset: Not enough frame deltas to compute FPS")

	# Average of last 10 frame deltas
	var count := mini(10, frame_deltas_us.size())
	var sum: int = 0
	for i in range(frame_deltas_us.size() - count, frame_deltas_us.size()):
		sum += frame_deltas_us[i]
	var avg_us := float(sum) / count

	assert(avg_us > 0, "CameraDataset: Invalid timestamps (all zeros) - camera not providing hardware timestamps")
	return 1000000.0 / avg_us


## Export timing data to dictionary (for JSON serialization)
func export_data() -> Dictionary:
	return {
		"session": {
			"id": session_id,
			"start_time": session_start_time,
		},
		"config": {
			"expected_fps": expected_fps,
			"expected_delta_us": _expected_delta_us,
		},
		"summary": {
			"frame_count": frame_count,
			"dropped_count": dropped_frame_indices.size(),
			"actual_fps": get_current_fps(),
		},
		"timestamps_us": Array(timestamps_us),
		"frame_indices": Array(frame_indices),
		"frame_deltas_us": Array(frame_deltas_us),
		"dropped_frame_indices": Array(dropped_frame_indices),
	}


## Check if recording
func is_recording() -> bool:
	return _is_recording


## Get timestamp array for analysis (returns copy)
func get_timestamps() -> PackedInt64Array:
	return timestamps_us.duplicate()


## Get frame deltas for analysis (returns copy)
func get_deltas() -> PackedInt64Array:
	return frame_deltas_us.duplicate()


## Get comprehensive timing statistics
## Returns dictionary with all timing metrics (see TimingStatistics class)
func get_full_statistics() -> Dictionary:
	if timestamps_us.size() == 0:
		return {"frame_count": 0}

	return TimingStatistics.compute(
		frame_deltas_us,
		_expected_delta_us,
		dropped_frame_indices,
		timestamps_us[0],
		timestamps_us[timestamps_us.size() - 1]
	)
