class_name BaseCard
extends PanelContainer
## Base class for all card components with raised surface gradient and rim highlights.
## Uses a shader for consistent rendering of gradient and rim effects.

## Corner radius - default matches RADIUS_2XL, subclasses override in _ready()
var _corner_radius := float(AppTheme.RADIUS_2XL)

## Whether to draw the raised gradient
var _draw_gradient := true

## Whether to draw the rim highlights
var _draw_rim_highlight := true

## Base color for shader. Subclasses override this with explicit AppTheme constant.
var _base_color: Color = AppTheme.SURFACE_COLOR_CARD

## Shader material for raised surface effect
var _shader_material: ShaderMaterial = null


func _ready() -> void:
	resized.connect(_on_resized)
	_setup_shader()


func _on_resized() -> void:
	_update_shader_size()


func _setup_shader() -> void:
	if not _draw_gradient and not _draw_rim_highlight:
		return

	# Use factory method for standard setup
	_shader_material = AppTheme.create_raised_surface_material(_base_color, _corner_radius)
	_shader_material.set_shader_parameter("rect_size", size)

	# Override if gradient or rim disabled
	if not _draw_gradient:
		_shader_material.set_shader_parameter("gradient_intensity", 0.0)
	if not _draw_rim_highlight:
		_shader_material.set_shader_parameter("rim_top_color", Color.TRANSPARENT)
		_shader_material.set_shader_parameter("rim_bottom_color", Color.TRANSPARENT)

	material = _shader_material


func _update_shader_size() -> void:
	if _shader_material:
		_shader_material.set_shader_parameter("rect_size", size)


## Update shader parameters after style changes.
## Call this after modifying _draw_gradient, _draw_rim_highlight, or _base_color.
func _update_shader() -> void:
	if _shader_material == null:
		return

	_shader_material.set_shader_parameter("base_color", _base_color)
	_shader_material.set_shader_parameter("corner_radius", _corner_radius)
	_shader_material.set_shader_parameter("gradient_intensity", AppTheme.RAISED_GRADIENT_INTENSITY if _draw_gradient else 0.0)
	var rim_alpha := AppTheme.RIM_LIGHT_ALPHA if _draw_rim_highlight else 0.0
	_shader_material.set_shader_parameter("rim_top_color", AppTheme.with_alpha(AppTheme.CREAM, rim_alpha))
	_shader_material.set_shader_parameter("rim_bottom_color", AppTheme.with_alpha(Color.BLACK, AppTheme.RIM_DARK_ALPHA if _draw_rim_highlight else 0.0))
