## TextureFrameData - Per-frame data for texture paradigm stimuli
##
## Captures the complete state of Carrier × Envelope × Modulation composition:
## - Envelope position (bar position, wedge angle, ring eccentricity)
## - Carrier phase (checkerboard phase from strobe)
## - Modulation state (strobe phase)
class_name TextureFrameData
extends RefCounted


## Envelope state - meaning depends on envelope type:
## - bar: x/y position in degrees from center
## - wedge: angle in degrees (0-360)
## - ring: eccentricity in degrees from center
## - gaussian: x/y position in degrees
## - full_field: always (0, 0)
var envelope_x: PackedFloat32Array = PackedFloat32Array()
var envelope_y: PackedFloat32Array = PackedFloat32Array()

## Carrier state
## Phase of the carrier pattern (0-360 degrees or 0-1 normalized)
var carrier_phase: PackedFloat32Array = PackedFloat32Array()

## Modulation states
## Strobe: current phase of contrast reversal (0-1)
var strobe_phase: PackedFloat32Array = PackedFloat32Array()
## Rotate: current rotation angle for rotating stimuli (degrees)
var rotate_angle: PackedFloat32Array = PackedFloat32Array()
## Expand: current eccentricity for expanding/contracting (degrees)
var expand_eccentricity: PackedFloat32Array = PackedFloat32Array()

## Direction/orientation
## For sweep: position along sweep axis (0-1 normalized)
## For orientation stimuli: current orientation in degrees
var direction_value: PackedFloat32Array = PackedFloat32Array()


## Record a frame's texture state
func record_frame(state: Dictionary) -> void:
	# Envelope position
	envelope_x.append(float(state["envelope_x"]))
	envelope_y.append(float(state["envelope_y"]))

	# Carrier phase
	carrier_phase.append(float(state["carrier_phase"]))

	# Modulation states
	strobe_phase.append(float(state["strobe_phase"]))
	rotate_angle.append(float(state["rotate_angle"]))
	expand_eccentricity.append(float(state["expand_eccentricity"]))

	# Direction/orientation value
	direction_value.append(float(state["direction_value"]))


## Get current frame data for UI display
func get_current_frame_data() -> Dictionary:
	if envelope_x.size() == 0:
		return {}

	var idx := envelope_x.size() - 1
	return {
		"envelope_x": envelope_x[idx],
		"envelope_y": envelope_y[idx],
		"carrier_phase": carrier_phase[idx],
		"strobe_phase": strobe_phase[idx],
		"rotate_angle": rotate_angle[idx],
		"expand_eccentricity": expand_eccentricity[idx],
		"direction_value": direction_value[idx],
	}


## Get arrays for HDF5 export
func get_export_arrays() -> Dictionary:
	return {
		"envelope_x": envelope_x,
		"envelope_y": envelope_y,
		"carrier_phase": carrier_phase,
		"strobe_phase": strobe_phase,
		"rotate_angle": rotate_angle,
		"expand_eccentricity": expand_eccentricity,
		"direction_value": direction_value,
	}


## Clear all data
func clear() -> void:
	envelope_x.clear()
	envelope_y.clear()
	carrier_phase.clear()
	strobe_phase.clear()
	rotate_angle.clear()
	expand_eccentricity.clear()
	direction_value.clear()
