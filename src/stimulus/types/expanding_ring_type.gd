## ExpandingRingType - Expanding ring stimulus for eccentricity retinotopic mapping
##
## A ring that expands outward (or contracts inward) from the fixation point
## to map visual eccentricity. Standard stimulus for retinotopic mapping
## alongside drifting bar and rotating wedge stimuli.
##
## Uses unified sweep parameters for consistency:
## - stimulus_width_deg = ring width in degrees
## - sweep_speed_deg_per_sec = expansion speed in degrees per second
class_name ExpandingRingType
extends StimulusTypeBase


func get_type_id() -> String:
	return "expanding_ring"


func get_type_name() -> String:
	return "Expanding Ring"


func get_description() -> String:
	return "A ring that expands outward (or contracts inward) for eccentricity mapping. " + \
		   "Standard stimulus for retinotopic mapping of visual field eccentricity."


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
			"stimulus_width_deg", "Ring Width", "float", 15.0, "deg",
			1.0, 180.0, 1.0, [],
			"Width of the ring in degrees of visual angle"
		),
		ParamDefinition.new(
			"sweep_speed_deg_per_sec", "Expansion Speed", "float", 9.0, "deg/s",
			0.1, 180.0, 0.1, [],
			"Radial expansion speed in degrees per second"
		),
		ParamDefinition.new(
			"max_eccentricity_deg", "Max Eccentricity", "float", 60.0, "deg",
			5.0, 180.0, 1.0, [],
			"Maximum eccentricity the ring travels to"
		),
		ParamDefinition.new(
			"pattern", "Ring Pattern", "enum", "checkerboard", "",
			null, null, null, ["solid", "checkerboard"],
			"Pattern displayed within the ring"
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
			"Background luminance outside the ring"
		),
	]


func supports_directions() -> bool:
	return true


func supports_direction(direction: String) -> bool:
	return direction in ["EXP", "CON"]


func get_recommended_directions() -> Array[String]:
	return ["EXP", "CON"]


func validate_params(params: Dictionary) -> ValidationResult:
	var result := super.validate_params(params)

	# Check required params exist
	for key in ["stimulus_width_deg", "max_eccentricity_deg", "sweep_speed_deg_per_sec"]:
		if not params.has(key):
			result.add_error("Missing required param: %s" % key)
			return result

	var ring_width: float = float(params["stimulus_width_deg"])
	var max_ecc: float = float(params["max_eccentricity_deg"])
	var expansion_speed: float = float(params["sweep_speed_deg_per_sec"])

	if ring_width > max_ecc / 2:
		result.add_warning("Ring width is more than half the max eccentricity. Consider reducing ring width.")

	if expansion_speed > 0:
		var expansion_period: float = max_ecc / expansion_speed
		if expansion_period < 8:
			result.add_warning("Short expansion period (<8s) may not allow hemodynamic response to develop")

	return result


func compute_sweep_duration(params: Dictionary, _visual_field_width_deg: float) -> float:
	# For expanding ring, sweep duration = max_eccentricity / expansion_speed
	var max_ecc: float = float(params["max_eccentricity_deg"])
	var expansion_speed: float = float(params["sweep_speed_deg_per_sec"])
	if expansion_speed > 0:
		return max_ecc / expansion_speed
	return 0.0


func is_periodic() -> bool:
	return true


func get_usage_notes() -> String:
	return """Eccentricity Retinotopic Mapping:
- Typical expansion period: 24-36 seconds
- Ring width: 10-20 degrees (narrower = higher resolution)
- Use both EXP (expand) and CON (contract) directions for phase-encoded mapping
- Checkerboard pattern at 4-8 Hz provides strong visual drive
- 10-20 cycles typically used for intrinsic signal imaging
- Combine with rotating wedge for complete retinotopic map
- Log-scaled expansion can improve foveal resolution (not yet implemented)"""
