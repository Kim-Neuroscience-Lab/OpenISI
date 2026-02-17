class_name ParameterTraits
extends RefCounted
## Parameter trait definitions for stimulus types.
##
## Traits are reusable groups of related parameters. Stimulus types declare
## which traits they use, and settings persist per-trait (not per-type).
## This allows switching between related types while preserving shared settings.


## Trait identifiers
enum Trait {
	SWEEP_PARAMS,    ## Retinotopy sweep: width, speed, background (bar, wedge, ring)
	CHECKERBOARD,    ## Checkerboard: check size, strobe frequency
	GRATING,         ## Grating: spatial freq, orientation, phase
	LUMINANCE,       ## Luminance/contrast settings
	TIMING_EPISODIC, ## Explicit stimulus duration (for episodic types)
}


## Get the parameter names belonging to a trait
static func get_trait_params(t: Trait) -> Array[String]:
	match t:
		Trait.SWEEP_PARAMS:
			return ["stimulus_width_deg", "sweep_speed_deg_per_sec", "background_luminance"]
		Trait.CHECKERBOARD:
			return ["check_size_deg", "strobe_frequency_hz"]
		Trait.GRATING:
			return ["spatial_frequency_cpd", "orientation_deg", "num_orientations", "phase_randomize"]
		Trait.LUMINANCE:
			return ["contrast", "mean_luminance"]
		Trait.TIMING_EPISODIC:
			return ["stimulus_duration_sec"]
	return []


## Get display name for UI grouping
static func get_trait_display_name(t: Trait) -> String:
	match t:
		Trait.SWEEP_PARAMS:
			return "Sweep Settings"
		Trait.CHECKERBOARD:
			return "Checkerboard"
		Trait.GRATING:
			return "Grating"
		Trait.LUMINANCE:
			return "Luminance"
		Trait.TIMING_EPISODIC:
			return "Timing"
	return "Settings"


## Get trait name as string (for storage keys)
static func get_trait_key(t: Trait) -> String:
	return Trait.keys()[t]


## Get trait from string key (for loading from storage)
static func get_trait_from_key(key: String) -> Trait:
	var idx := Trait.keys().find(key)
	assert(idx >= 0, "Unknown trait key: %s" % key)
	return idx as Trait


## Check if a parameter name belongs to a specific trait
static func param_belongs_to_trait(param_name: String, t: Trait) -> bool:
	return param_name in get_trait_params(t)


## Find which trait a parameter belongs to (returns -1 if not found)
static func find_trait_for_param(param_name: String) -> int:
	for t in Trait.values():
		if param_name in get_trait_params(t):
			return t
	return -1


## Get all trait enum values
static func get_all_traits() -> Array[Trait]:
	var traits: Array[Trait] = []
	for t in Trait.values():
		traits.append(t)
	return traits
