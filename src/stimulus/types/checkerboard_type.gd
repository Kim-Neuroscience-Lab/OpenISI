class_name CheckerboardType
extends StimulusTypeBase
## Stimulus type definition for contrast-reversing checkerboard.
##
## Full-field checkerboard pattern with configurable check size and
## contrast reversal frequency. Commonly used for VEP studies, retinotopic
## mapping activation, and as a control stimulus.


func get_type_id() -> String:
	return "checkerboard"


func get_type_name() -> String:
	return "Checkerboard"


func get_description() -> String:
	return "Full-field contrast-reversing checkerboard pattern"


func get_category() -> String:
	return "Control/Simple"


func supports_directions() -> bool:
	# Checkerboard doesn't use cardinal directions
	return false


func get_traits() -> Array[ParameterTraits.Trait]:
	return [
		ParameterTraits.Trait.CHECKERBOARD,
		ParameterTraits.Trait.LUMINANCE,
		ParameterTraits.Trait.TIMING_EPISODIC,
	]


func get_configurable_params() -> Array[ParamDefinition]:
	return [
		ParamDefinition.new(
			"check_size_deg", "Check Size", "float", 2.0, "deg",
			0.1, 20.0, 0.1, [],
			"Size of each check square in visual degrees"
		),
		ParamDefinition.new(
			"strobe_frequency_hz", "Strobe Frequency", "float", 4.0, "Hz",
			0.0, 30.0, 0.5, [],
			"Contrast reversal frequency (0 = static)"
		),
		ParamDefinition.new(
			"contrast", "Contrast", "float", 1.0, "",
			0.0, 1.0, 0.05, [],
			"Michelson contrast of the checkerboard"
		),
		ParamDefinition.new(
			"mean_luminance", "Mean Luminance", "float", 0.5, "",
			0.0, 1.0, 0.05, [],
			"Mean luminance level (0.5 = mid-gray)"
		),
		ParamDefinition.new(
			"stimulus_duration_sec", "Stimulus Duration", "float", 16.0, "s",
			0.5, 60.0, 0.5, [],
			"Duration of each stimulus presentation"
		),
	]


func get_default_params() -> Dictionary:
	return {
		"check_size_deg": 2.0,
		"strobe_frequency_hz": 4.0,
		"contrast": 1.0,
		"mean_luminance": 0.5,
		"stimulus_duration_sec": 16.0,
	}


func validate_params(params: Dictionary) -> ValidationResult:
	var result := ValidationResult.new()

	# Basic validation from parent
	result = super.validate_params(params)

	# Type-specific validation - params must exist
	if not params.has("check_size_deg"):
		result.add_error("Missing required param: check_size_deg")
		return result
	if not params.has("strobe_frequency_hz"):
		result.add_error("Missing required param: strobe_frequency_hz")
		return result

	var check_size: float = float(params["check_size_deg"])
	var strobe_freq: float = float(params["strobe_frequency_hz"])

	# Warn if check size is very small (may cause aliasing)
	if check_size < 0.5:
		result.add_warning("Very small check size may cause aliasing artifacts")

	# Warn if strobe frequency is very high
	if strobe_freq > 15.0:
		result.add_warning("High strobe frequency (>15 Hz) may not be perceived accurately")

	return result


func compute_sweep_duration(params: Dictionary, _visual_field_width_deg: float) -> float:
	# Checkerboard uses explicit stimulus duration
	return float(params["stimulus_duration_sec"])


func get_default_timing_paradigm() -> String:
	return "episodic"
