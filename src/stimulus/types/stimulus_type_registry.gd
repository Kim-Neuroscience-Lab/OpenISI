## StimulusTypeRegistry - Factory and registry for stimulus types
##
## Provides discovery and instantiation of available stimulus types.
## New stimulus types should be registered here.
class_name StimulusTypeRegistry
extends RefCounted

## Singleton instance
static var _instance: StimulusTypeRegistry = null

## Registered stimulus types
var _types: Dictionary = {}


## Get singleton instance
static func get_instance() -> StimulusTypeRegistry:
	if _instance == null:
		_instance = StimulusTypeRegistry.new()
		_instance._register_builtin_types()
	return _instance


## Register built-in stimulus types
func _register_builtin_types() -> void:
	register_type(DriftingBarType.new())
	register_type(CheckerboardType.new())
	register_type(RotatingWedgeType.new())
	register_type(ExpandingRingType.new())


## Register a stimulus type
func register_type(stimulus_type: StimulusTypeBase) -> void:
	var type_id := stimulus_type.get_type_id()
	if _types.has(type_id):
		push_warning("StimulusTypeRegistry: Overwriting existing type: %s" % type_id)
	_types[type_id] = stimulus_type


## Get a stimulus type by ID
func get_type(type_id: String) -> StimulusTypeBase:
	return _types[type_id] as StimulusTypeBase


## Get all registered type IDs
func get_type_ids() -> Array[String]:
	var ids: Array[String] = []
	for key in _types.keys():
		ids.append(key)
	return ids


## Get all registered types
func get_all_types() -> Array[StimulusTypeBase]:
	var types: Array[StimulusTypeBase] = []
	for type in _types.values():
		types.append(type)
	return types


## Get types by category
func get_types_by_category() -> Dictionary:
	var by_category := {}
	for type in _types.values():
		var category: String = type.get_category()
		if not by_category.has(category):
			by_category[category] = []
		by_category[category].append(type)
	return by_category


## Get type display info for UI
func get_type_options() -> Array[Dictionary]:
	var options: Array[Dictionary] = []
	for type in _types.values():
		options.append({
			"id": type.get_type_id(),
			"name": type.get_type_name(),
			"category": type.get_category(),
			"description": type.get_description(),
		})
	# Sort by category then name
	options.sort_custom(func(a, b):
		if a["category"] != b["category"]:
			return a["category"] < b["category"]
		return a["name"] < b["name"]
	)
	return options


## Create default params for a type
func get_default_params(type_id: String) -> Dictionary:
	var type := get_type(type_id)
	if type:
		return type.get_default_params()
	return {}


## Validate params for a type
func validate_params(type_id: String, params: Dictionary) -> StimulusTypeBase.ValidationResult:
	var type := get_type(type_id)
	if type:
		return type.validate_params(params)
	var result := StimulusTypeBase.ValidationResult.new()
	result.add_error("Unknown stimulus type: %s" % type_id)
	return result


## Compute sweep duration for a type
func compute_sweep_duration(type_id: String, params: Dictionary, visual_field_width_deg: float) -> float:
	var type := get_type(type_id)
	assert(type != null, "Unknown stimulus type: %s" % type_id)
	return type.compute_sweep_duration(params, visual_field_width_deg)


## Check if type exists
func has_type(type_id: String) -> bool:
	return _types.has(type_id)
