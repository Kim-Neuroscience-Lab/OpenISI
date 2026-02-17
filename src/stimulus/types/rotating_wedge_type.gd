## RotatingWedgeType - Rotating wedge stimulus for polar angle retinotopic mapping
##
## A wedge that rotates around the fixation point to map polar angle.
## Standard stimulus for retinotopic mapping alongside drifting bar and
## expanding ring stimuli.
##
## Uses unified sweep parameters for consistency:
## - stimulus_width_deg = wedge angle in degrees
## - sweep_speed_deg_per_sec = rotation speed in degrees per second
class_name RotatingWedgeType
extends StimulusTypeBase


func get_type_id() -> String:
	return "rotating_wedge"


func get_type_name() -> String:
	return "Rotating Wedge"


func get_description() -> String:
	return "A wedge that rotates around the fixation point for polar angle mapping. " + \
		   "Standard stimulus for retinotopic mapping of angular visual field position."


func get_category() -> String:
	return "Retinotopy"


func get_traits() -> Array[ParameterTraits.Trait]:
	return [
		ParameterTraits.Trait.SWEEP_PARAMS,
		ParameterTraits.Trait.CHECKERBOARD,
		ParameterTraits.Trait.LUMINANCE,
	]


func get_configurable_params() -> Array[ParamDefinition]:
	return [
		ParamDefinition.new(
			"stimulus_width_deg", "Wedge Angle", "float", 45.0, "deg",
			1.0, 359.0, 1.0, [],
			"Angular width of the wedge in degrees"
		),
		ParamDefinition.new(
			"sweep_speed_deg_per_sec", "Rotation Speed", "float", 15.0, "deg/s",
			0.1, 360.0, 0.1, [],
			"Angular rotation speed (360 deg / rotation_period)"
		),
		ParamDefinition.new(
			"pattern", "Wedge Pattern", "enum", "checkerboard", "",
			null, null, null, ["solid", "checkerboard"],
			"Pattern displayed within the wedge"
		),
		ParamDefinition.new(
			"check_size_deg", "Check Size", "float", 5.0, "deg",
			0.5, 20.0, 0.5, [],
			"Size of each checkerboard square in degrees"
		),
		ParamDefinition.new(
			"strobe_frequency_hz", "Strobe Frequency", "float", 4.0, "Hz",
			0.5, 30.0, 0.5, [],
			"Frequency of contrast reversal"
		),
		ParamDefinition.new(
			"contrast", "Contrast", "float", 1.0, "",
			0.0, 1.0, 0.05, [],
			"Michelson contrast of the pattern"
		),
		ParamDefinition.new(
			"mean_luminance", "Mean Luminance", "float", 0.5, "",
			0.0, 1.0, 0.05, [],
			"Mean luminance of the pattern"
		),
		ParamDefinition.new(
			"background_luminance", "Background", "float", 0.0, "",
			0.0, 1.0, 0.05, [],
			"Background luminance outside the wedge"
		),
	]


func supports_directions() -> bool:
	return true


func supports_direction(direction: String) -> bool:
	return direction in ["CW", "CCW"]


func get_recommended_directions() -> Array[String]:
	return ["CW", "CCW"]


func validate_params(params: Dictionary) -> ValidationResult:
	var result := super.validate_params(params)

	# Check required params exist
	for key in ["stimulus_width_deg", "sweep_speed_deg_per_sec"]:
		if not params.has(key):
			result.add_error("Missing required param: %s" % key)
			return result

	var wedge_angle: float = float(params["stimulus_width_deg"])
	var rotation_speed: float = float(params["sweep_speed_deg_per_sec"])

	if rotation_speed > 0:
		var rotation_period: float = 360.0 / rotation_speed
		if rotation_period < 8:
			result.add_warning("Short rotation period (<8s) may not allow hemodynamic response to develop")

	if wedge_angle > 90:
		result.add_warning("Large wedge angle (>90 deg) may reduce spatial resolution of mapping")

	return result


func compute_sweep_duration(params: Dictionary, _visual_field_width_deg: float) -> float:
	# For rotating wedge, sweep duration = 360 / rotation_speed
	var rotation_speed: float = float(params["sweep_speed_deg_per_sec"])
	if rotation_speed > 0:
		return 360.0 / rotation_speed
	return 0.0


func is_periodic() -> bool:
	return true


func get_usage_notes() -> String:
	return """Polar Angle Retinotopic Mapping:
- Typical rotation period: 24-36 seconds (rotation speed: 10-15 deg/s)
- Wedge angle: 30-90 degrees (narrower = higher resolution)
- Use both CW and CCW directions for phase-encoded mapping
- Checkerboard pattern at 4-8 Hz provides strong visual drive
- 10-20 rotations typically used for intrinsic signal imaging
- Combine with expanding ring for complete retinotopic map"""
