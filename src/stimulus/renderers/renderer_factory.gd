class_name RendererFactory
extends RefCounted
## Factory for creating stimulus renderers.
##
## Maps stimulus type IDs to renderer classes and creates instances on demand.
## New renderer types can be registered at runtime.


## Singleton instance
static var _instance: RendererFactory = null


## Registry of type_id -> renderer script path
var _renderer_scripts: Dictionary = {}

## Cached loaded scripts
var _loaded_scripts: Dictionary = {}


func _init() -> void:
	_register_builtin_renderers()


## Get the singleton instance
static func get_instance() -> RendererFactory:
	if _instance == null:
		_instance = RendererFactory.new()
	return _instance


## Register built-in renderer types
func _register_builtin_renderers() -> void:
	# All texture paradigm stimuli use the unified TextureRenderer
	# The renderer selects the appropriate shader based on envelope type
	var texture_renderer := "res://src/stimulus/renderers/texture_renderer.gd"

	# NONE envelope (full-field)
	register_renderer("full_field", texture_renderer)
	register_renderer("checkerboard", texture_renderer)

	# BAR envelope (drifting bar)
	register_renderer("drifting_bar", texture_renderer)

	# WEDGE envelope (rotating wedge)
	register_renderer("rotating_wedge", texture_renderer)

	# RING envelope (expanding ring)
	register_renderer("expanding_ring", texture_renderer)


## Register a renderer for a stimulus type
## [param type_id] The stimulus type ID (e.g., "drifting_bar")
## [param script_path] Path to the renderer script
func register_renderer(type_id: String, script_path: String) -> void:
	_renderer_scripts[type_id] = script_path


## Check if a renderer is registered for a type
func has_renderer(type_id: String) -> bool:
	return type_id in _renderer_scripts


## Get list of registered renderer type IDs
func get_registered_types() -> Array[String]:
	var types: Array[String] = []
	for type_id in _renderer_scripts.keys():
		types.append(type_id)
	return types


## Create a renderer instance for the given stimulus type
## [param type_id] The stimulus type ID
## [returns] A new renderer instance, or null if type not registered
func create_renderer(type_id: String) -> StimulusRendererBase:
	if not has_renderer(type_id):
		push_error("RendererFactory: No renderer registered for type '%s'" % type_id)
		return null

	var script_path: String = _renderer_scripts[type_id]

	# Load script if not cached
	if script_path not in _loaded_scripts:
		var loaded_script: GDScript = load(script_path) as GDScript
		if loaded_script == null:
			push_error("RendererFactory: Failed to load renderer script: %s" % script_path)
			return null
		_loaded_scripts[script_path] = loaded_script

	# Create instance
	var script: GDScript = _loaded_scripts[script_path]
	var renderer: StimulusRendererBase = script.new() as StimulusRendererBase

	if renderer == null:
		push_error("RendererFactory: Script did not create a StimulusRendererBase: %s" % script_path)
		return null

	return renderer


## Convenience function to create a renderer without needing the singleton
static func create(type_id: String) -> StimulusRendererBase:
	return get_instance().create_renderer(type_id)
