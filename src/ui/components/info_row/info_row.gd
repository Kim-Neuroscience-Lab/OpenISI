class_name InfoRow
extends HBoxContainer
## A label-value pair row for displaying information.
##
## Used in info cards for detected hardware, computed values, etc.
## Label on the left, value on the right with proper typography.

## The label text (left side, muted).
@export var label_text: String = "Label":
	set(value):
		label_text = value
		_update_display()

## The value text (right side, prominent).
@export var value_text: String = "Value":
	set(value):
		value_text = value
		_update_display()

## Whether the value should use monospace styling.
@export var mono_value: bool = false:
	set(value):
		mono_value = value
		_apply_style()

## Whether the value should use amber (important) color.
@export var important_value: bool = false:
	set(value):
		important_value = value
		_apply_style()

## Status color for the value (overrides other styling if set).
@export_enum("none", "success", "warning", "error") var status: String = "none":
	set(value):
		status = value
		_apply_style()

var _label: Label = null
var _value: Label = null


func _ready() -> void:
	_build_ui()
	_apply_style()
	_update_display()


func _build_ui() -> void:
	# Label on left
	_label = Label.new()
	_label.name = "Label"
	_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	add_child(_label)

	# Value on right
	_value = Label.new()
	_value.name = "Value"
	_value.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	add_child(_value)


func _apply_style() -> void:
	if not is_inside_tree():
		return  # Will be called again from _ready()

	# Style label (per mockup: 12px, CREAM_DIM)
	if _label:
		_label.theme_type_variation = "LabelSmallDim"

	# Style value based on settings - use theme variations for all color cases
	if _value:
		if status != "none":
			# Use status-specific theme variation
			_value.theme_type_variation = AppTheme.get_status_label_variation(status, true)
		elif important_value:
			# Use amber for important values - need to add this variation
			_value.theme_type_variation = "LabelSmallAmber"
		else:
			# Default cream color
			_value.theme_type_variation = "LabelSmall"


func _update_display() -> void:
	if _label:
		_label.text = label_text
	if _value:
		_value.text = value_text


## Set both label and value.
func set_content(label: String, value: String) -> void:
	label_text = label
	value_text = value


## Set just the value.
func set_value(value: String) -> void:
	value_text = value


## Set the status color for the value.
func set_status(new_status: String) -> void:
	status = new_status
