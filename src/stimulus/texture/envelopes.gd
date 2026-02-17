class_name Envelopes
extends RefCounted
## Envelope (aperture) shapes for texture-based stimuli.
##
## Envelopes define the spatial windowing - where the carrier pattern is visible.
## Each envelope type has specific available directions for retinotopic mapping:
##   NONE  = Full-field stimulus (no directions)
##   BAR   = Cartesian sweeps (LR, RL, TB, BT)
##   WEDGE = Polar rotation (CW, CCW)
##   RING  = Radial expansion (EXP, CON)


enum Type {
	NONE,   ## Full-field, no envelope (static pattern)
	BAR,    ## Rectangular bar aperture (sweeps LR, RL, TB, BT)
	WEDGE,  ## Pie-wedge aperture (rotates CW, CCW)
	RING,   ## Annular ring aperture (expands/contracts EXP, CON)
}


## Display names for UI
const DISPLAY_NAMES := {
	Type.NONE: "None (Full Field)",
	Type.BAR: "Bar",
	Type.WEDGE: "Wedge",
	Type.RING: "Ring",
}


## Common rotation parameter (applies to all envelope types except NONE)
const ROTATION_PARAM := { "name": "rotation_deg", "display": "Rotation" }


## Parameter definitions for each envelope type
## Only specifies name and display - contracts (min/max/step/unit) come from Config (SSoT)
## Note: Rotation parameter is added to all types except NONE
## Note: BAR envelope uses stimulus_width_cm in Cartesian space, stimulus_width_deg otherwise
const PARAMS := {
	Type.NONE: [],  # No envelope params for full-field
	Type.BAR: [
		{ "name": "stimulus_width", "display": "Envelope Width", "cartesian_suffix": "_cm", "angular_suffix": "_deg" },
		{ "name": "sweep_speed_deg_per_sec", "display": "Sweep Speed" },
		ROTATION_PARAM,
	],
	Type.WEDGE: [
		{ "name": "stimulus_width_deg", "display": "Envelope Width" },
		{ "name": "rotation_speed_deg_per_sec", "display": "Rotation Speed" },
		ROTATION_PARAM,
	],
	Type.RING: [
		{ "name": "stimulus_width_deg", "display": "Envelope Width" },
		{ "name": "expansion_speed_deg_per_sec", "display": "Expansion Speed" },
		ROTATION_PARAM,
	],
}


## Check if an envelope type uses polar/angular coordinates (needs angular check size)
static func uses_polar_coordinates(type: Type) -> bool:
	return type == Type.WEDGE or type == Type.RING


## Get display name for an envelope type
static func get_display_name(type: Type) -> String:
	return DISPLAY_NAMES[type]


## Get all envelope types
static func get_all_types() -> Array[Type]:
	return [Type.NONE, Type.BAR, Type.WEDGE, Type.RING]


## Get parameter definitions for an envelope type (uses angular units by default)
static func get_params(type: Type) -> Array:
	return get_params_for_space(type, DisplayGeometry.ProjectionType.SPHERICAL)


## Get parameter definitions for an envelope type based on projection/coordinate space
## For CARTESIAN: uses stimulus_width_cm for bar envelope
## For SPHERICAL/CYLINDRICAL: uses stimulus_width_deg
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
