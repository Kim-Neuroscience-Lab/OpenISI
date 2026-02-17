## MediaFrameData - Per-frame data for media paradigm stimuli
##
## Captures playback state for:
## - Static images
## - Video files
##
## Tracks which media frame is displayed at each stimulus frame
class_name MediaFrameData
extends RefCounted


## Media file information
var media_file_path: String = ""
var media_total_frames: int = 0
var media_duration_sec: float = 0.0
var media_fps: float = 0.0

## Per-frame media state
## Which frame of the media file is displayed
var media_frame_indices: PackedInt32Array = PackedInt32Array()

## Timestamp within media file (seconds from start)
var media_timestamps: PackedFloat32Array = PackedFloat32Array()

## For video: playback position (0-1 normalized)
var playback_position: PackedFloat32Array = PackedFloat32Array()

## For looping video: which loop iteration
var loop_count: PackedInt32Array = PackedInt32Array()


## Initialize with media file info
func initialize(file_path: String, total_frames: int, duration_sec: float, fps: float) -> void:
	media_file_path = file_path
	media_total_frames = total_frames
	media_duration_sec = duration_sec
	media_fps = fps


## Record a frame's media state
func record_frame(state: Dictionary) -> void:
	media_frame_indices.append(int(state["media_frame"]))
	media_timestamps.append(float(state["media_timestamp"]))
	playback_position.append(float(state["playback_position"]))
	loop_count.append(int(state["loop_count"]))


## Get current frame data for UI display
func get_current_frame_data() -> Dictionary:
	if media_frame_indices.size() == 0:
		return {}

	var idx := media_frame_indices.size() - 1
	return {
		"media_frame": media_frame_indices[idx],
		"media_timestamp": media_timestamps[idx],
		"playback_position": playback_position[idx],
		"loop_count": loop_count[idx],
		"media_total_frames": media_total_frames,
	}


## Get arrays for HDF5 export
func get_export_arrays() -> Dictionary:
	return {
		"media_file_path": media_file_path,
		"media_total_frames": media_total_frames,
		"media_duration_sec": media_duration_sec,
		"media_fps": media_fps,
		"media_frame_indices": media_frame_indices,
		"media_timestamps": media_timestamps,
		"playback_position": playback_position,
		"loop_count": loop_count,
	}


## Clear all data
func clear() -> void:
	media_frame_indices.clear()
	media_timestamps.clear()
	playback_position.clear()
	loop_count.clear()
