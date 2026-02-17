class_name StyledOptionButton
extends Control
## A styled option button (dropdown) with recessed/inset shader effect.
##
## Creates an input field that looks recessed into the surface,
## with dark top edge (shadow) and light bottom edge (highlight).

signal item_selected(index: int)

## Whether the control is disabled.
@export var disabled: bool = false:
	set(value):
		disabled = value
		if _inner_option:
			_inner_option.disabled = value

# Internal components
var _inner_option: OptionButton = null
var _shader_bg: ColorRect = null
var _shader_material: ShaderMaterial = null


func _ready() -> void:
	# Setup shader background
	_setup_shader()

	# Create inner OptionButton
	_inner_option = OptionButton.new()
	_inner_option.name = "InnerOptionButton"
	_inner_option.set_anchors_preset(Control.PRESET_FULL_RECT)
	_inner_option.disabled = disabled

	# Transparent background via theme variation - shader handles visuals
	_inner_option.theme_type_variation = "OptionButtonTransparent"

	# Connect signals
	_inner_option.item_selected.connect(_on_item_selected)

	add_child(_inner_option)

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


func _on_item_selected(index: int) -> void:
	item_selected.emit(index)


## Get the inner OptionButton for advanced configuration.
func get_option_button() -> OptionButton:
	return _inner_option


# Proxy methods for common OptionButton operations

func add_item(label: String, id: int = -1) -> void:
	if _inner_option:
		_inner_option.add_item(label, id)


func clear() -> void:
	if _inner_option:
		_inner_option.clear()


func select(index: int) -> void:
	if _inner_option:
		_inner_option.select(index)


func get_selected() -> int:
	if _inner_option:
		return _inner_option.get_selected()
	return -1


func get_selected_id() -> int:
	if _inner_option:
		return _inner_option.get_selected_id()
	return -1


func get_item_id(index: int) -> int:
	if _inner_option:
		return _inner_option.get_item_id(index)
	return -1


func get_item_count() -> int:
	if _inner_option:
		return _inner_option.item_count
	return 0


var item_count: int:
	get:
		if _inner_option:
			return _inner_option.item_count
		return 0


func set_item_metadata(index: int, metadata: Variant) -> void:
	if _inner_option:
		_inner_option.set_item_metadata(index, metadata)


func get_item_metadata(index: int) -> Variant:
	if _inner_option:
		return _inner_option.get_item_metadata(index)
	return null
