@tool
class_name LabeledInput
extends VBoxContainer
## A styled input field with label and optional suffix.
##
## Usage:
##   var input = LabeledInput.new()
##   input.label = "Distance"
##   input.suffix = "cm"
##   input.placeholder = "Enter value..."

## Emitted when the text changes.
signal text_changed(new_text: String)

## Emitted when Enter is pressed.
signal text_submitted(text: String)

## The input label text.
@export var label: String = "Label":
	set(value):
		label = value
		if _label:
			_label.text = value
			_label.visible = not value.is_empty()

## The input suffix/unit text.
@export var suffix: String = "":
	set(value):
		suffix = value
		if _suffix:
			_suffix.text = value
			_suffix.visible = not value.is_empty()

## The placeholder text.
@export var placeholder: String = "":
	set(value):
		placeholder = value
		if _input:
			_input.placeholder_text = value

## The current input text.
@export var text: String = "":
	set(value):
		text = value
		if _input:
			_input.text = value
	get:
		if _input:
			return _input.text
		return text

## Whether the input is editable.
@export var editable: bool = true:
	set(value):
		editable = value
		if _input:
			_input.editable = value

var _label: Label = null
var _input_container: HBoxContainer = null
var _input: LineEdit = null
var _suffix: Label = null


func _ready() -> void:
	_build_ui()
	_apply_style()
	_update_properties()

	if _input:
		_input.text_changed.connect(_on_input_text_changed)
		_input.text_submitted.connect(_on_input_text_submitted)


func _build_ui() -> void:
	# Label (top)
	_label = Label.new()
	_label.name = "Label"
	_label.text = "Label"
	add_child(_label)

	# Input container (horizontal: LineEdit + optional suffix)
	_input_container = HBoxContainer.new()
	_input_container.name = "InputContainer"
	add_child(_input_container)

	# LineEdit
	_input = LineEdit.new()
	_input.name = "Input"
	_input.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_input.placeholder_text = "Enter value..."
	_input_container.add_child(_input)

	# Suffix label (hidden by default)
	_suffix = Label.new()
	_suffix.name = "Suffix"
	_suffix.visible = false
	_suffix.size_flags_vertical = Control.SIZE_SHRINK_BEGIN
	_suffix.text = "unit"
	_suffix.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	_input_container.add_child(_suffix)


func _apply_style() -> void:
	if not is_inside_tree():
		return

	# Set container separations via theme variations
	theme_type_variation = "VBoxSM"
	if _input_container:
		_input_container.theme_type_variation = "HBoxSM"

	# LineEdit inherits styles from theme automatically - no overrides needed

	if _label:
		_label.theme_type_variation = "LabelSmall"

	if _suffix:
		_suffix.theme_type_variation = "LabelSmall"


func _update_properties() -> void:
	if _label:
		_label.text = label
		_label.visible = not label.is_empty()
	if _input:
		_input.placeholder_text = placeholder
		_input.text = text
		_input.editable = editable
	if _suffix:
		_suffix.text = suffix
		_suffix.visible = not suffix.is_empty()


func _on_input_text_changed(new_text: String) -> void:
	text = new_text
	text_changed.emit(new_text)


func _on_input_text_submitted(submitted_text: String) -> void:
	text_submitted.emit(submitted_text)


## Gets the LineEdit node for advanced customization.
func get_line_edit() -> LineEdit:
	return _input


## Sets focus to the input field.
func grab_input_focus() -> void:
	if _input:
		_input.grab_focus()
