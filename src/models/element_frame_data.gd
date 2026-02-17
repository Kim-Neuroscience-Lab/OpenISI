## ElementFrameData - Per-frame data for element paradigm stimuli
##
## Captures discrete object states for:
## - Random dot kinematograms (RDK)
## - Sparse noise
## - Other object-based stimuli
##
## Each frame stores the state of all objects (dots, checks, etc.)
class_name ElementFrameData
extends RefCounted


## Maximum number of objects to track per frame
## This is set based on stimulus parameters
var max_objects: int = 0

## Per-frame object positions (in degrees from center)
## Each element is a PackedVector2Array of object positions for that frame
var positions: Array[PackedVector2Array] = []

## Per-frame object velocities (in degrees/second)
var velocities: Array[PackedVector2Array] = []

## Per-frame object lifetimes (frames remaining before respawn)
var lifetimes: Array[PackedInt32Array] = []

## Per-frame object visibility (for sparse noise: which checks are active)
var visible: Array[PackedByteArray] = []

## Per-frame object values (for sparse noise: polarity +1/-1, for RDK: coherent/incoherent)
var values: Array[PackedFloat32Array] = []

## Summary statistics per frame (for efficient analysis)
var coherence_actual: PackedFloat32Array = PackedFloat32Array()  # Actual coherence achieved
var n_visible: PackedInt32Array = PackedInt32Array()  # Number of visible objects


## Initialize with maximum object count
func initialize(object_count: int) -> void:
	max_objects = object_count


## Record a frame's element state
func record_frame(state: Dictionary) -> void:
	# Object positions
	var pos_array: PackedVector2Array = state["positions"]
	positions.append(pos_array)

	# Object velocities
	var vel_array: PackedVector2Array = state["velocities"]
	velocities.append(vel_array)

	# Object lifetimes
	var life_array: PackedInt32Array = state["lifetimes"]
	lifetimes.append(life_array)

	# Object visibility
	var vis_array: PackedByteArray = state["visible"]
	visible.append(vis_array)

	# Object values
	var val_array: PackedFloat32Array = state["values"]
	values.append(val_array)

	# Summary stats
	coherence_actual.append(float(state["coherence_actual"]))
	n_visible.append(int(state["n_visible"]))


## Get current frame data for UI display
func get_current_frame_data() -> Dictionary:
	if positions.size() == 0:
		return {}

	var idx := positions.size() - 1
	assert(n_visible.size() > idx, "ElementFrameData: n_visible array out of sync")
	assert(coherence_actual.size() > idx, "ElementFrameData: coherence_actual array out of sync")
	return {
		"n_objects": positions[idx].size(),
		"n_visible": n_visible[idx],
		"coherence_actual": coherence_actual[idx],
	}


## Get arrays for HDF5 export
## Note: These need special handling due to variable object counts per frame
func get_export_arrays() -> Dictionary:
	return {
		"positions": positions,
		"velocities": velocities,
		"lifetimes": lifetimes,
		"visible": visible,
		"values": values,
		"coherence_actual": coherence_actual,
		"n_visible": n_visible,
		"max_objects": max_objects,
	}


## Clear all data
func clear() -> void:
	positions.clear()
	velocities.clear()
	lifetimes.clear()
	visible.clear()
	values.clear()
	coherence_actual.clear()
	n_visible.clear()
