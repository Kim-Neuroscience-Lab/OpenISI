## StimulusTypeBase - Abstract base class for stimulus types
##
## Defines the interface that all stimulus types must implement.
## Each stimulus type provides parameter definitions, validation,
## and rendering capabilities.
class_name StimulusTypeBase
extends RefCounted

## Parameter definition for UI generation
class ParamDefinition:
	var name: String          ## Internal parameter name
	var display_name: String  ## UI display label
	var type: String          ## "float", "int", "bool", "string", "enum"
	var default_value: Variant
	var min_value: Variant    ## For numeric types
	var max_value: Variant    ## For numeric types
	var step: Variant         ## For numeric types
	var unit: String          ## Display unit (e.g., "deg", "Hz", "cpd")
	var options: Array        ## For enum type
	var description: String   ## Tooltip/help text

	func _init(
		p_name: String,
		p_display_name: String,
		p_type: String,
		p_default: Variant,
		p_unit: String = "",
		p_min: Variant = null,
		p_max: Variant = null,
		p_step: Variant = null,
		p_options: Array = [],
		p_description: String = ""
	) -> void:
		name = p_name
		display_name = p_display_name
		type = p_type
		default_value = p_default
		unit = p_unit
		min_value = p_min
		max_value = p_max
		step = p_step
		options = p_options
		description = p_description


## Validation result
class ValidationResult:
	var valid: bool = true
	var errors: Array[String] = []
	var warnings: Array[String] = []

	func add_error(message: String) -> void:
		valid = false
		errors.append(message)

	func add_warning(message: String) -> void:
		warnings.append(message)


## Get the unique type identifier
func get_type_id() -> String:
	push_error("StimulusTypeBase.get_type_id() must be overridden")
	return ""


## Get the display name for UI
func get_type_name() -> String:
	push_error("StimulusTypeBase.get_type_name() must be overridden")
	return ""


## Get a description of this stimulus type
func get_description() -> String:
	return ""


## Get the category for organizing in UI (e.g., "Retinotopy", "Tuning", "Motion")
func get_category() -> String:
	return "General"


## Get the list of configurable parameters
func get_configurable_params() -> Array[ParamDefinition]:
	push_error("StimulusTypeBase.get_configurable_params() must be overridden")
	return []


## Get default parameter values as dictionary
func get_default_params() -> Dictionary:
	var defaults := {}
	for param in get_configurable_params():
		defaults[param.name] = param.default_value
	return defaults


## Validate parameter values
func validate_params(params: Dictionary) -> ValidationResult:
	var result := ValidationResult.new()

	for param_def in get_configurable_params():
		if not params.has(param_def.name):
			result.add_error("Missing required param: %s" % param_def.name)
			continue
		var value: Variant = params[param_def.name]

		# Type checking
		match param_def.type:
			"float":
				if not (value is float or value is int):
					result.add_error("%s must be a number" % param_def.display_name)
					continue
				if param_def.min_value != null and value < param_def.min_value:
					result.add_error("%s must be at least %s" % [param_def.display_name, param_def.min_value])
				if param_def.max_value != null and value > param_def.max_value:
					result.add_error("%s must be at most %s" % [param_def.display_name, param_def.max_value])

			"int":
				if not value is int:
					result.add_error("%s must be an integer" % param_def.display_name)
					continue
				if param_def.min_value != null and value < param_def.min_value:
					result.add_error("%s must be at least %s" % [param_def.display_name, param_def.min_value])
				if param_def.max_value != null and value > param_def.max_value:
					result.add_error("%s must be at most %s" % [param_def.display_name, param_def.max_value])

			"bool":
				if not value is bool:
					result.add_error("%s must be true or false" % param_def.display_name)

			"enum":
				if value not in param_def.options:
					result.add_error("%s must be one of: %s" % [param_def.display_name, ", ".join(param_def.options)])

	return result


## Compute sweep duration based on parameters and visual field
func compute_sweep_duration(_params: Dictionary, _visual_field_width_deg: float) -> float:
	assert(false, "StimulusTypeBase.compute_sweep_duration() must be overridden")
	return 0.0  # Unreachable


## Check if this stimulus type uses cardinal directions (LR, RL, TB, BT)
## Override to return false for types that don't use directions (e.g., checkerboard)
func supports_directions() -> bool:
	return true


## Check if this stimulus type supports the given direction
func supports_direction(direction: String) -> bool:
	return direction in ["LR", "RL", "TB", "BT"]


## Get recommended directions for this stimulus type
func get_recommended_directions() -> Array[String]:
	return ["LR", "RL", "TB", "BT"]


## Check if this stimulus type uses periodic paradigm (continuous sweeps)
func is_periodic() -> bool:
	return true


## Get any additional notes or usage hints
func get_usage_notes() -> String:
	return ""


# -----------------------------------------------------------------------------
# Parameter Traits System
# -----------------------------------------------------------------------------

## Get the parameter traits this type uses.
## Override in subclasses to declare trait composition.
## Traits allow settings to persist when switching between related types.
func get_traits() -> Array[ParameterTraits.Trait]:
	return []


## Get parameters unique to this type (not from any trait).
## Override in subclasses to define type-specific parameters.
func get_unique_params() -> Array[ParamDefinition]:
	return []


## Get parameter definitions for a specific trait.
## Returns ParamDefinition objects for params belonging to the trait.
func get_params_for_trait(t: ParameterTraits.Trait) -> Array[ParamDefinition]:
	var trait_param_names := ParameterTraits.get_trait_params(t)
	var result: Array[ParamDefinition] = []

	for param_def in get_configurable_params():
		if param_def.name in trait_param_names:
			result.append(param_def)

	return result


## Get parameter definitions organized by trait (for UI grouping).
## Returns Dictionary: trait -> Array[ParamDefinition]
## Also includes "unique" key for type-specific params.
func get_params_by_trait() -> Dictionary:
	var result := {}
	var all_params := get_configurable_params()
	var assigned_params: Array[String] = []

	# Group params by trait
	for t in get_traits():
		var trait_param_names := ParameterTraits.get_trait_params(t)
		var trait_params: Array[ParamDefinition] = []

		for param_def in all_params:
			if param_def.name in trait_param_names:
				trait_params.append(param_def)
				assigned_params.append(param_def.name)

		if not trait_params.is_empty():
			result[t] = trait_params

	# Collect unassigned params as "unique"
	var unique_params: Array[ParamDefinition] = []
	for param_def in all_params:
		if param_def.name not in assigned_params:
			unique_params.append(param_def)

	if not unique_params.is_empty():
		result["unique"] = unique_params

	return result


## Get the names of parameters that are unique to this type (not from traits).
func get_unique_param_names() -> Array[String]:
	var trait_params: Array[String] = []
	for t in get_traits():
		trait_params.append_array(ParameterTraits.get_trait_params(t))

	var unique_names: Array[String] = []
	for param_def in get_configurable_params():
		if param_def.name not in trait_params:
			unique_names.append(param_def.name)

	return unique_names
