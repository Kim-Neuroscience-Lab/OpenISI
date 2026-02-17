## DriftingBarType - Drifting bar stimulus for retinotopic mapping
##
## Standard Kalatsky & Stryker retinotopic mapping stimulus.
## A bar (solid or patterned) drifts across the visual field.
class_name DriftingBarType
extends StimulusTypeBase


func get_type_id() -> String:
	return "drifting_bar"


func get_type_name() -> String:
	return "Drifting Bar"


func get_description() -> String:
	return "A bar that drifts across the visual field for retinotopic mapping. " + \
		   "Standard Kalatsky & Stryker method for continuous periodic stimulation."


func get_category() -> String:
	return "Retinotopy"


func get_traits() -> Array[ParameterTraits.Trait]:
	return [ParameterTraits.Trait.SWEEP_PARAMS]


func get_configurable_params() -> Array[ParamDefinition]:
	return [
		ParamDefinition.new(
			"stimulus_width_deg", "Bar Width", "float", 20.0, "deg",
			1.0, 180.0, 1.0, [],
			"Width of the drifting bar in degrees of visual angle"
		),
		ParamDefinition.new(
			"sweep_speed_deg_per_sec", "Drift Speed", "float", 9.0, "deg/s",
			0.1, 180.0, 0.1, [],
			"Speed at which the bar drifts across the visual field"
		),
		ParamDefinition.new(
			"pattern", "Bar Pattern", "enum", "solid", "",
			null, null, null, ["solid", "checkerboard", "grating"],
			"Pattern displayed within the bar"
		),
		ParamDefinition.new(
			"pattern_spatial_freq_cpd", "Pattern Spatial Freq", "float", 0.05, "cpd",
			0.01, 1.0, 0.01, [],
			"Spatial frequency of pattern (for checkerboard/grating)"
		),
		ParamDefinition.new(
			"strobe_frequency_hz", "Strobe Frequency", "float", 2.0, "Hz",
			0.5, 30.0, 0.5, [],
			"Frequency of pattern contrast reversal"
		),
		ParamDefinition.new(
			"luminance_min", "Min Luminance", "float", 0.0, "",
			0.0, 1.0, 0.05, [],
			"Minimum luminance (black level)"
		),
		ParamDefinition.new(
			"luminance_max", "Max Luminance", "float", 1.0, "",
			0.0, 1.0, 0.05, [],
			"Maximum luminance (white level)"
		),
		ParamDefinition.new(
			"background_luminance", "Background", "float", 0.0, "",
			0.0, 1.0, 0.05, [],
			"Background luminance outside the bar"
		),
	]


func validate_params(params: Dictionary) -> ValidationResult:
	var result := super.validate_params(params)

	# Check required params exist
	for key in ["luminance_min", "luminance_max", "stimulus_width_deg", "sweep_speed_deg_per_sec"]:
		if not params.has(key):
			result.add_error("Missing required param: %s" % key)
			return result

	# Additional validation
	var lum_min: float = float(params["luminance_min"])
	var lum_max: float = float(params["luminance_max"])
	if lum_min >= lum_max:
		result.add_error("Min luminance must be less than max luminance")

	var bar_width: float = float(params["stimulus_width_deg"])
	var drift_speed: float = float(params["sweep_speed_deg_per_sec"])
	if drift_speed > 0 and bar_width / drift_speed > 60:
		result.add_warning("Very long sweep duration (>60s). Consider increasing drift speed.")

	return result


func compute_sweep_duration(params: Dictionary, visual_field_width_deg: float) -> float:
	var drift_speed: float = float(params["sweep_speed_deg_per_sec"])
	if drift_speed > 0:
		return visual_field_width_deg / drift_speed
	return 0.0


func is_periodic() -> bool:
	return true


func get_usage_notes() -> String:
	return """Retinotopic Mapping (Kalatsky & Stryker Method):
- Typically use 0.04-0.05 Hz temporal frequency (20-25s per sweep)
- 4 cardinal directions recommended: LR, RL, TB, BT
- 20-60 repetitions for intrinsic signal imaging
- Use contrast-reversing checkerboard pattern for enhanced response
- Checkerboard spatial frequency: ~0.05 cpd
- Checkerboard temporal frequency: 2 Hz"""
