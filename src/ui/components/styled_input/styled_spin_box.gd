class_name StyledSpinBox
extends Control
## A styled spin box with recessed/inset shader effect.
##
## Creates an input field that looks recessed into the surface,
## with dark top edge (shadow) and light bottom edge (highlight).

signal value_changed(new_value: float)

## The current value.
@export var value: float = 0.0:
	set(v):
		value = v
		if _inner_spin:
			_inner_spin.value = v

## Minimum value.
@export var min_value: float = 0.0:
	set(v):
		min_value = v
		if _inner_spin:
			_inner_spin.min_value = v

## Maximum value.
@export var max_value: float = 100.0:
	set(v):
		max_value = v
		if _inner_spin:
			_inner_spin.max_value = v

## Step value.
@export var step: float = 1.0:
	set(v):
		step = v
		if _inner_spin:
			_inner_spin.step = v

## Suffix shown after the value.
@export var suffix: String = "":
	set(v):
		suffix = v
		if _inner_spin:
			_inner_spin.suffix = v

## Prefix shown before the value.
@export var prefix: String = "":
	set(v):
		prefix = v
		if _inner_spin:
			_inner_spin.prefix = v

## Whether the input is editable.
@export var editable: bool = true:
	set(v):
		editable = v
		if _inner_spin:
			_inner_spin.editable = v
			var line_edit := _inner_spin.get_line_edit()
			if line_edit:
				line_edit.editable = v

# Internal components
var _inner_spin: SpinBox = null
var _shader_bg: ColorRect = null
var _shader_material: ShaderMaterial = null


func _ready() -> void:
	# Setup shader background
	_setup_shader()

	# Create inner SpinBox
	_inner_spin = SpinBox.new()
	_inner_spin.name = "InnerSpinBox"
	_inner_spin.set_anchors_preset(Control.PRESET_FULL_RECT)

	# IMPORTANT: Set min/max/step BEFORE value to avoid clamping issues
	_inner_spin.min_value = min_value
	_inner_spin.max_value = max_value
	_inner_spin.step = step
	_inner_spin.value = value
	_inner_spin.suffix = suffix
	_inner_spin.prefix = prefix
	_inner_spin.editable = editable

	# Allow direct text editing
	_inner_spin.update_on_text_changed = true

	# Get the internal LineEdit and style it
	var line_edit := _inner_spin.get_line_edit()
	if line_edit:
		# Transparent background via theme variation - shader handles visuals
		line_edit.theme_type_variation = "LineEditTransparentCompact"

		# Ensure the line edit is editable and can receive focus
		line_edit.editable = editable
		line_edit.selecting_enabled = true

	# Connect signals
	_inner_spin.value_changed.connect(_on_value_changed)

	add_child(_inner_spin)

	# Update shader size when control resizes
	resized.connect(_update_shader_size)
	call_deferred("_update_shader_size")


func _setup_shader() -> void:
	var result := AppTheme.create_inset_shader_background(float(AppTheme.RADIUS_SM))
	_shader_bg = result[0]
	_shader_material = result[1]
	add_child(_shader_bg)


func _update_shader_size() -> void:
	if _shader_material and size.x > 0 and size.y > 0:
		_shader_material.set_shader_parameter("rect_size", size)


func _on_value_changed(new_value: float) -> void:
	value = new_value
	value_changed.emit(new_value)


## Get the inner SpinBox for advanced configuration.
func get_spin_box() -> SpinBox:
	return _inner_spin
