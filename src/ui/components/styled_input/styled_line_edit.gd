class_name StyledLineEdit
extends Control
## A styled line edit with recessed/inset shader effect.
##
## Creates an input field that looks recessed into the surface,
## with dark top edge (shadow) and light bottom edge (highlight).

signal text_changed(new_text: String)
signal text_submitted(new_text: String)

## The input text.
@export var text: String = "":
	set(value):
		text = value
		if _inner_edit:
			_inner_edit.text = value

## Placeholder text shown when empty.
@export var placeholder_text: String = "":
	set(value):
		placeholder_text = value
		if _inner_edit:
			_inner_edit.placeholder_text = value

## Whether the input is editable.
@export var editable: bool = true:
	set(value):
		editable = value
		if _inner_edit:
			_inner_edit.editable = value

# Internal components
var _inner_edit: LineEdit = null
var _shader_bg: ColorRect = null
var _shader_material: ShaderMaterial = null


func _ready() -> void:
	# Setup shader background
	_setup_shader()

	# Create inner LineEdit
	_inner_edit = LineEdit.new()
	_inner_edit.name = "InnerLineEdit"
	_inner_edit.set_anchors_preset(Control.PRESET_FULL_RECT)
	_inner_edit.text = text
	_inner_edit.placeholder_text = placeholder_text
	_inner_edit.editable = editable

	# Transparent background via theme variation - shader handles visuals
	_inner_edit.theme_type_variation = "LineEditTransparent"

	# Text colors inherited from theme (LineEdit styles in theme.gd)

	# Connect signals
	_inner_edit.text_changed.connect(_on_text_changed)
	_inner_edit.text_submitted.connect(_on_text_submitted)

	add_child(_inner_edit)

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


func _on_text_changed(new_text: String) -> void:
	text = new_text
	text_changed.emit(new_text)


func _on_text_submitted(new_text: String) -> void:
	text_submitted.emit(new_text)


## Get the inner LineEdit for advanced configuration.
func get_line_edit() -> LineEdit:
	return _inner_edit
