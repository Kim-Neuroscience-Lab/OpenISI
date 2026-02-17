## DatasetExporter - Export stimulus dataset to JSON + binary format
##
## Exports:
## - metadata.json: Session, protocol, display config (human-readable)
## - stimulus_frames.bin: Per-frame data in binary format (efficient)
## - schema.json: Describes the binary file structure for HDF5 conversion
##
## A Python script can convert the binary to HDF5 using the schema.
class_name DatasetExporter
extends RefCounted


## Export a dataset to the specified directory
static func export_dataset(dataset: StimulusDataset, output_dir: String) -> Error:
	# Ensure output directory exists
	var dir := DirAccess.open(output_dir.get_base_dir())
	if dir == null:
		dir = DirAccess.open("res://")
	if not DirAccess.dir_exists_absolute(output_dir):
		var err: Error = DirAccess.make_dir_recursive_absolute(output_dir)
		if err != OK:
			push_error("Failed to create output directory: %s" % output_dir)
			return err

	# Export metadata
	var metadata_err: Error = _export_metadata(dataset, output_dir)
	if metadata_err != OK:
		return metadata_err

	# Export frame data
	var frames_err: Error = _export_frames(dataset, output_dir)
	if frames_err != OK:
		return frames_err

	# Export schema for HDF5 conversion
	var schema_err: Error = _export_schema(dataset, output_dir)
	if schema_err != OK:
		return schema_err

	print("Dataset exported to: %s" % output_dir)
	return OK


static func _export_metadata(dataset: StimulusDataset, output_dir: String) -> Error:
	var metadata := dataset.export_metadata()
	var json_string := JSON.stringify(metadata, "\t")

	var file := FileAccess.open(output_dir.path_join("metadata.json"), FileAccess.WRITE)
	if file == null:
		push_error("Failed to create metadata.json")
		return FileAccess.get_open_error()

	file.store_string(json_string)
	file.close()
	return OK


static func _export_frames(dataset: StimulusDataset, output_dir: String) -> Error:
	var file := FileAccess.open(output_dir.path_join("stimulus_frames.bin"), FileAccess.WRITE)
	if file == null:
		push_error("Failed to create stimulus_frames.bin")
		return FileAccess.get_open_error()

	# Write header
	file.store_string("STIM")  # Magic bytes
	file.store_32(1)  # Version
	file.store_32(dataset.frame_count)
	file.store_string(dataset.paradigm.pad_zeros(16).substr(0, 16))  # Paradigm (16 bytes, null-padded)

	# Write timing arrays
	_write_packed_int64_array(file, dataset.timestamps_us)
	_write_packed_int64_array(file, dataset.frame_deltas_us)
	_write_packed_int32_array(file, dataset.dropped_frame_indices)

	# Write sequence arrays
	_write_string_array(file, dataset.conditions_per_frame)
	_write_packed_int32_array(file, dataset.sweep_indices)
	_write_packed_int32_array(file, dataset.frame_indices)
	_write_packed_float32_array(file, dataset.progress)
	_write_string_array(file, dataset.states)

	# Write sequence-agnostic metadata
	_write_packed_int32_array(file, dataset.condition_occurrences)
	_write_packed_byte_array(file, dataset.is_baseline)

	# Write paradigm-specific data
	match dataset.paradigm:
		"texture":
			_export_texture_data(file, dataset.texture_data)
		"element":
			_export_element_data(file, dataset.element_data)
		"media":
			_export_media_data(file, dataset.media_data)

	file.close()
	return OK


static func _export_texture_data(file: FileAccess, data: TextureFrameData) -> void:
	if data == null:
		return

	_write_packed_float32_array(file, data.envelope_x)
	_write_packed_float32_array(file, data.envelope_y)
	_write_packed_float32_array(file, data.carrier_phase)
	_write_packed_float32_array(file, data.strobe_phase)
	_write_packed_float32_array(file, data.rotate_angle)
	_write_packed_float32_array(file, data.expand_eccentricity)
	_write_packed_float32_array(file, data.direction_value)


static func _export_element_data(file: FileAccess, data: ElementFrameData) -> void:
	if data == null:
		return

	file.store_32(data.max_objects)
	file.store_32(data.positions.size())

	# Write summary arrays
	_write_packed_float32_array(file, data.coherence_actual)
	_write_packed_int32_array(file, data.n_visible)

	# Write per-frame object data
	for i: int in range(data.positions.size()):
		_write_packed_vector2_array(file, data.positions[i])
		_write_packed_vector2_array(file, data.velocities[i])
		_write_packed_int32_array(file, data.lifetimes[i])
		_write_packed_byte_array(file, data.visible[i])
		_write_packed_float32_array(file, data.values[i])


static func _export_media_data(file: FileAccess, data: MediaFrameData) -> void:
	if data == null:
		return

	_write_string(file, data.media_file_path)
	file.store_32(data.media_total_frames)
	file.store_float(data.media_duration_sec)
	file.store_float(data.media_fps)

	_write_packed_int32_array(file, data.media_frame_indices)
	_write_packed_float32_array(file, data.media_timestamps)
	_write_packed_float32_array(file, data.playback_position)
	_write_packed_int32_array(file, data.loop_count)


static func _export_schema(dataset: StimulusDataset, output_dir: String) -> Error:
	## Generate schema describing the binary format for HDF5 conversion

	var schema := {
		"version": 1,
		"paradigm": dataset.paradigm,
		"frame_count": dataset.frame_count,
		"arrays": {
			"timing": {
				"timestamps_us": {"dtype": "int64", "shape": [dataset.frame_count]},
				"frame_deltas_us": {"dtype": "int64", "shape": [dataset.frame_deltas_us.size()]},
				"dropped_frame_indices": {"dtype": "int32", "shape": [dataset.dropped_frame_indices.size()]},
			},
			"sequence": {
				"conditions": {"dtype": "string", "shape": [dataset.frame_count]},
				"sweep_indices": {"dtype": "int32", "shape": [dataset.frame_count]},
				"frame_indices": {"dtype": "int32", "shape": [dataset.frame_count]},
				"progress": {"dtype": "float32", "shape": [dataset.frame_count]},
				"states": {"dtype": "string", "shape": [dataset.frame_count]},
				"condition_occurrences": {"dtype": "int32", "shape": [dataset.frame_count]},
				"is_baseline": {"dtype": "uint8", "shape": [dataset.frame_count]},
			},
		},
	}

	# Add paradigm-specific schema
	match dataset.paradigm:
		"texture":
			schema["arrays"]["texture"] = {
				"envelope_x": {"dtype": "float32", "shape": [dataset.frame_count]},
				"envelope_y": {"dtype": "float32", "shape": [dataset.frame_count]},
				"carrier_phase": {"dtype": "float32", "shape": [dataset.frame_count]},
				"strobe_phase": {"dtype": "float32", "shape": [dataset.frame_count]},
				"rotate_angle": {"dtype": "float32", "shape": [dataset.frame_count]},
				"expand_eccentricity": {"dtype": "float32", "shape": [dataset.frame_count]},
				"direction_value": {"dtype": "float32", "shape": [dataset.frame_count]},
			}
		"element":
			if dataset.element_data:
				schema["arrays"]["element"] = {
					"max_objects": dataset.element_data.max_objects,
					"coherence_actual": {"dtype": "float32", "shape": [dataset.frame_count]},
					"n_visible": {"dtype": "int32", "shape": [dataset.frame_count]},
					"positions": {"dtype": "float32", "shape": [dataset.frame_count, "variable", 2]},
					"velocities": {"dtype": "float32", "shape": [dataset.frame_count, "variable", 2]},
					"lifetimes": {"dtype": "int32", "shape": [dataset.frame_count, "variable"]},
					"visible": {"dtype": "uint8", "shape": [dataset.frame_count, "variable"]},
					"values": {"dtype": "float32", "shape": [dataset.frame_count, "variable"]},
				}
		"media":
			if dataset.media_data:
				schema["arrays"]["media"] = {
					"media_file_path": dataset.media_data.media_file_path,
					"media_total_frames": dataset.media_data.media_total_frames,
					"media_duration_sec": dataset.media_data.media_duration_sec,
					"media_fps": dataset.media_data.media_fps,
					"media_frame_indices": {"dtype": "int32", "shape": [dataset.frame_count]},
					"media_timestamps": {"dtype": "float32", "shape": [dataset.frame_count]},
					"playback_position": {"dtype": "float32", "shape": [dataset.frame_count]},
					"loop_count": {"dtype": "int32", "shape": [dataset.frame_count]},
				}

	var json_string := JSON.stringify(schema, "\t")
	var file := FileAccess.open(output_dir.path_join("schema.json"), FileAccess.WRITE)
	if file == null:
		push_error("Failed to create schema.json")
		return FileAccess.get_open_error()

	file.store_string(json_string)
	file.close()
	return OK


# Helper functions for writing packed arrays

static func _write_packed_int64_array(file: FileAccess, arr: PackedInt64Array) -> void:
	file.store_32(arr.size())
	for value in arr:
		file.store_64(value)


static func _write_packed_int32_array(file: FileAccess, arr: PackedInt32Array) -> void:
	file.store_32(arr.size())
	for value in arr:
		file.store_32(value)


static func _write_packed_float32_array(file: FileAccess, arr: PackedFloat32Array) -> void:
	file.store_32(arr.size())
	for value in arr:
		file.store_float(value)


static func _write_packed_byte_array(file: FileAccess, arr: PackedByteArray) -> void:
	file.store_32(arr.size())
	file.store_buffer(arr)


static func _write_packed_vector2_array(file: FileAccess, arr: PackedVector2Array) -> void:
	file.store_32(arr.size())
	for vec in arr:
		file.store_float(vec.x)
		file.store_float(vec.y)


static func _write_string_array(file: FileAccess, arr: PackedStringArray) -> void:
	file.store_32(arr.size())
	for s in arr:
		_write_string(file, s)


static func _write_string(file: FileAccess, s: String) -> void:
	var bytes := s.to_utf8_buffer()
	file.store_32(bytes.size())
	file.store_buffer(bytes)
