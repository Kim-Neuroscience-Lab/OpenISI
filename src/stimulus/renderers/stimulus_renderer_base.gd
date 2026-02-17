class_name StimulusRendererBase
extends RefCounted
## Abstract base class for stimulus renderers.
##
## Each stimulus type (drifting bar, grating, sparse noise, etc.) has its own
## renderer that knows how to draw it. Renderers can use either immediate-mode
## drawing (draw_rect, draw_polygon) or shader-based rendering.
##
## Subclasses must implement all methods marked with "Override in subclass".


## Rendering state passed from sequencer/display
class RenderState:
	## Current direction: "LR", "RL", "TB", "BT", or "" for non-directional
	var direction: String = ""
	## Current orientation in degrees (for gratings)
	var orientation_deg: float = 0.0
	## Progress within current sweep/presentation (0.0 to 1.0)
	var progress: float = 0.0
	## Whether currently in baseline period (show gray)
	var is_baseline: bool = false
	## Elapsed time since stimulus start (for temporal modulation)
	var elapsed_sec: float = 0.0
	## Current sweep/trial index
	var sweep_index: int = 0
	## Total sweeps in protocol
	var total_sweeps: int = 1


# -----------------------------------------------------------------------------
# Properties
# -----------------------------------------------------------------------------

## Display size in pixels
var display_size: Vector2 = Vector2.ZERO

## Display geometry (SSoT for visual field and projection transformation)
var geometry: DisplayGeometry = null

## Current render state
var state: RenderState = RenderState.new()

## Stimulus parameters (type-specific)
var _params: Dictionary = {}


# -----------------------------------------------------------------------------
# Abstract Methods - Override in subclass
# -----------------------------------------------------------------------------

## Returns the stimulus type ID this renderer handles (e.g., "drifting_bar")
func get_type_id() -> String:
	push_error("StimulusRendererBase.get_type_id() must be overridden")
	return ""


## Returns true if this renderer uses a shader material
func requires_shader() -> bool:
	return false


## Initialize the renderer with parameters and display configuration.
## Called once when the stimulus is set up.
## [param params] Type-specific stimulus parameters
## [param size] Display size in pixels
## [param geom] Display geometry (required - SSoT for visual field and projection)
func initialize(params: Dictionary, size: Vector2, geom: DisplayGeometry) -> void:
	assert(geom != null, "StimulusRendererBase: geometry is required")
	_params = params.duplicate()
	display_size = size
	geometry = geom
	_on_initialize()


## Set display geometry (can be called after initialization)
func set_geometry(geom: DisplayGeometry) -> void:
	geometry = geom
	# Update shader if applicable
	if geometry and requires_shader():
		var mat := get_shader_material()
		if mat:
			geometry.apply_to_shader(mat)


## Override to perform type-specific initialization
func _on_initialize() -> void:
	pass


## Update renderer state. Called every frame during stimulus presentation.
## [param delta] Time since last frame in seconds
## [param render_state] Current render state from sequencer
func update(delta: float, render_state: RenderState) -> void:
	state = render_state
	_on_update(delta)


## Override to perform type-specific per-frame updates
func _on_update(_delta: float) -> void:
	pass


## Render the stimulus. Called from CanvasItem._draw().
## [param canvas] The CanvasItem to draw on (use canvas.draw_* methods)
func render(canvas: CanvasItem) -> void:
	if state.is_baseline:
		_render_baseline(canvas)
	else:
		_on_render(canvas)


## Override to perform type-specific rendering
func _on_render(_canvas: CanvasItem) -> void:
	push_error("StimulusRendererBase._on_render() must be overridden")


## Render baseline (gray screen). Can be overridden for custom baseline.
func _render_baseline(canvas: CanvasItem) -> void:
	var bg_lum: float
	if _params.has("background_luminance"):
		bg_lum = float(_params["background_luminance"])
	else:
		bg_lum = Settings.background_luminance
	var bg_color := Color(bg_lum, bg_lum, bg_lum)
	canvas.draw_rect(Rect2(Vector2.ZERO, display_size), bg_color)


## Get the shader material if this renderer uses one.
## Returns null for renderers that use immediate-mode drawing.
func get_shader_material() -> ShaderMaterial:
	return null


## Clean up resources. Called when renderer is no longer needed.
func cleanup() -> void:
	_on_cleanup()


## Override to perform type-specific cleanup
func _on_cleanup() -> void:
	pass


# -----------------------------------------------------------------------------
# Utility Methods
# -----------------------------------------------------------------------------

## Convert degrees to pixels (delegates to geometry)
func deg_to_px(deg: float, horizontal: bool = true) -> float:
	assert(geometry != null, "StimulusRendererBase: geometry required for deg_to_px")
	return geometry.deg_to_px(deg, horizontal)


## Convert pixels to degrees (delegates to geometry)
func px_to_deg(px: float, horizontal: bool = true) -> float:
	assert(geometry != null, "StimulusRendererBase: geometry required for px_to_deg")
	return geometry.px_to_deg(px, horizontal)


## Get visual field from geometry (SSoT)
func get_visual_field_deg() -> Vector2:
	assert(geometry != null, "StimulusRendererBase: geometry required for visual field")
	return Vector2(geometry.visual_field_width_deg, geometry.visual_field_height_deg)


## Get parameter value.
## Get a parameter value - fails if missing (SSoT).
func get_param(key: String) -> Variant:
	return _params[key]


## Check if parameter exists
func has_param(key: String) -> bool:
	return _params.has(key)


## Check if current direction is horizontal (LR or RL)
func is_horizontal() -> bool:
	return state.direction == "LR" or state.direction == "RL"


## Check if current direction is reversed (RL or BT)
func is_reversed() -> bool:
	return state.direction == "RL" or state.direction == "BT"


## Get check size in degrees, handling unit conversion based on projection type.
## For Cartesian mode: converts check_size_cm to degrees using viewing distance
## For Spherical/Cylindrical: returns check_size_deg directly
func get_check_size_deg() -> float:
	var projection_type: int
	if has_param("projection_type"):
		projection_type = get_param("projection_type")
	elif geometry:
		projection_type = geometry.projection_type
	else:
		projection_type = DisplayGeometry.ProjectionType.CARTESIAN

	if projection_type == DisplayGeometry.ProjectionType.CARTESIAN:
		# Cartesian mode: UI provides check_size_cm, convert to degrees
		var check_size_cm: float = float(get_param("check_size_cm"))
		assert(geometry != null, "Geometry required for Cartesian projection")
		var viewing_distance: float = geometry.viewing_distance_cm
		# Convert cm to degrees: deg = atan(cm / viewing_distance)
		return rad_to_deg(atan(check_size_cm / viewing_distance))
	else:
		# Spherical/Cylindrical: UI provides check_size_deg directly
		return float(get_param("check_size_deg"))


## Get sweep dimension in pixels (width for horizontal, height for vertical)
func get_sweep_dimension() -> float:
	return display_size.x if is_horizontal() else display_size.y


## Get paradigm-specific state for dataset recording.
## Override in subclasses to return stimulus-specific data.
## For texture paradigm: envelope position, carrier phase, modulation states
## For element paradigm: object positions, velocities, visibility
## For media paradigm: media frame index, playback position
func get_paradigm_state() -> Dictionary:
	return {}
