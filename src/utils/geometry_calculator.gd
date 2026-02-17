## GeometryCalculator - Pure geometry computations for visual field calculations
##
## Static functions for computing visual field angles, pixels-per-degree, and
## sweep durations. These take explicit parameters (no autoload dependencies)
## making them easy to test and understand.
class_name GeometryCalculator
extends RefCounted


## Compute visual field width in degrees (flat screen, Cartesian)
## Uses standard formula: 2 * atan(half_width / viewing_distance)
static func visual_field_width_deg(display_width_cm: float, viewing_distance_cm: float) -> float:
	if viewing_distance_cm <= 0.0:
		return 0.0
	var half_width := display_width_cm / 2.0
	return 2.0 * rad_to_deg(atan(half_width / viewing_distance_cm))


## Compute visual field height in degrees (flat screen, Cartesian)
## Uses standard formula: 2 * atan(half_height / viewing_distance)
static func visual_field_height_deg(display_height_cm: float, viewing_distance_cm: float) -> float:
	if viewing_distance_cm <= 0.0:
		return 0.0
	var half_height := display_height_cm / 2.0
	return 2.0 * rad_to_deg(atan(half_height / viewing_distance_cm))


## Compute pixels per degree (for converting between visual and pixel space)
static func pixels_per_degree(display_width_px: int, visual_field_width_deg_val: float) -> float:
	if visual_field_width_deg_val <= 0.0:
		return 0.0
	return float(display_width_px) / visual_field_width_deg_val


## Compute sweep duration based on envelope type and speed parameters
## Returns duration in seconds
static func sweep_duration_sec(
	envelope_type: int,
	visual_field_width_deg_val: float,
	sweep_speed_deg_per_sec: float,
	rotation_speed_deg_per_sec: float,
	expansion_speed_deg_per_sec: float,
	inter_stimulus_sec: float
) -> float:
	match envelope_type:
		0:  # NONE (full-field)
			return inter_stimulus_sec if inter_stimulus_sec > 0 else 1.0
		1:  # BAR
			if sweep_speed_deg_per_sec <= 0:
				return 0.0
			return visual_field_width_deg_val / sweep_speed_deg_per_sec
		2:  # WEDGE
			if rotation_speed_deg_per_sec <= 0:
				return 0.0
			return 360.0 / rotation_speed_deg_per_sec
		3:  # RING
			if expansion_speed_deg_per_sec <= 0:
				return 0.0
			var max_ecc := visual_field_width_deg_val / 2.0
			return max_ecc / expansion_speed_deg_per_sec
	return 0.0


## Compute total protocol duration based on timing parameters
## Returns duration in seconds
static func total_duration_sec(
	sweep_duration: float,
	conditions_count: int,
	repetitions: int,
	baseline_start_sec: float,
	baseline_end_sec: float,
	inter_direction_sec: float
) -> float:
	var num_dirs := conditions_count if conditions_count > 0 else 1
	var total_sweeps := num_dirs * repetitions

	# Inter-direction intervals (between different directions, not after last)
	var inter_dir_time := 0.0
	if num_dirs > 1:
		inter_dir_time = float(num_dirs - 1) * inter_direction_sec

	return baseline_start_sec + (float(total_sweeps) * sweep_duration) + inter_dir_time + baseline_end_sec
