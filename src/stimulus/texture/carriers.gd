class_name Carriers
extends RefCounted
## Carrier patterns for texture-based stimuli.
##
## Carriers define the base pattern that fills the envelope aperture.


enum Type {
	CHECKERBOARD,  ## 2D checkerboard pattern (primary for retinotopy)
	SOLID,         ## Uniform luminance (simple bar/wedge/ring)
}


## Display names for UI
const DISPLAY_NAMES := {
	Type.CHECKERBOARD: "Checkerboard",
	Type.SOLID: "Solid",
}


## Valid check sizes that divide evenly into 360 degrees.
## Used for wedge/ring stimuli to ensure seamless wrapping at 0/360.
## UI spinbox step should be quantized to these values for polar stimuli.
const VALID_POLAR_CHECK_SIZES := [
	1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 9.0, 10.0, 12.0, 15.0, 18.0, 20.0,
	24.0, 30.0, 36.0, 40.0, 45.0, 60.0, 72.0, 90.0, 120.0, 180.0
]


## Parameter definitions for each carrier type
## Only specifies which params apply - min/max/step/unit come from Settings.STIMULUS_PARAMS (SSoT)
## Note: For polar stimuli (wedge/ring), the UI should quantize check_size_deg
## to values in VALID_POLAR_CHECK_SIZES for seamless wrapping.
## Note: CHECKERBOARD uses check_size_cm in Cartesian space, check_size_deg otherwise
const PARAMS := {
	Type.CHECKERBOARD: [
		{ "name": "check_size", "display": "Check Size", "cartesian_suffix": "_cm", "angular_suffix": "_deg" },
	],
	Type.SOLID: [],
}


## Find the nearest valid polar check size
static func get_nearest_polar_check_size(value: float) -> float:
	var nearest := VALID_POLAR_CHECK_SIZES[0]
	var min_diff := absf(value - nearest)
	for size: float in VALID_POLAR_CHECK_SIZES:
		var diff := absf(value - size)
		if diff < min_diff:
			min_diff = diff
			nearest = size
	return nearest


## Get display name for a carrier type
static func get_display_name(type: Type) -> String:
	return DISPLAY_NAMES[type]


## Get all carrier types
static func get_all_types() -> Array[Type]:
	return [Type.CHECKERBOARD, Type.SOLID]


## Get parameter definitions for a carrier type (uses angular units by default)
static func get_params(type: Type) -> Array:
	return get_params_for_space(type, DisplayGeometry.ProjectionType.SPHERICAL)


## Get parameter definitions for a carrier type based on projection/coordinate space
## For CARTESIAN: uses check_size_cm (physical measurement in centimeters)
## For SPHERICAL/CYLINDRICAL: uses check_size_deg (visual angle in degrees)
static func get_params_for_space(type: Type, projection_type: int) -> Array:
	var use_cartesian := projection_type == DisplayGeometry.ProjectionType.CARTESIAN
	var params: Array = PARAMS[type]
	var result: Array = []

	for param in params:
		if param.has("cartesian_suffix"):
			# Parameter needs unit substitution based on coordinate space
			var suffix: String = param["cartesian_suffix"] if use_cartesian else param["angular_suffix"]
			result.append({ "name": param["name"] + suffix, "display": param["display"] })
		else:
			result.append(param)

	return result
