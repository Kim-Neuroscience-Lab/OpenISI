class_name CheckboxTile
extends PanelContainer
## A tile-style checkbox for options like direction selection.
##
## Features a raised surface with raised/inset states based on selection.
## Includes a custom checkbox indicator with lavender glow when selected.

## Emitted when the tile is toggled.
signal toggled(pressed: bool)

## The tile label text.
@export var text: String = "Option":
	set(value):
		text = value
		_update_display()

## Whether the tile is currently selected.
@export var button_pressed: bool = false:
	set(value):
		if button_pressed == value:
			return  # No change, don't emit signal
		button_pressed = value
		_apply_style()
		toggled.emit(value)

## Whether the tile is disabled.
@export var disabled: bool = false:
	set(value):
		disabled = value
		_apply_style()

var _indicator_container: PanelContainer = null
var _label: Label = null
var _checkmark: Label = null
var _shader_material: ShaderMaterial = null


func _ready() -> void:
	_build_ui()
	_setup_shader()
	_apply_style()
	_update_display()

	# Enable mouse input, disable focus (no outline)
	mouse_filter = Control.MOUSE_FILTER_STOP
	focus_mode = Control.FOCUS_NONE
	gui_input.connect(_on_gui_input)
	resized.connect(_on_resized)


func _on_resized() -> void:
	if _shader_material:
		_shader_material.set_shader_parameter("rect_size", size)


func _setup_shader() -> void:
	_shader_material = AppTheme.create_raised_surface_material(
		AppTheme.SURFACE_COLOR_CHECKBOX_OFF,
		float(AppTheme.RADIUS_MD)
	)
	_shader_material.set_shader_parameter("rect_size", size)
	material = _shader_material


func _build_ui() -> void:
	# Create inner HBox
	var hbox := HBoxContainer.new()
	hbox.name = "Content"
	hbox.theme_type_variation = "HBoxMD"
	hbox.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(hbox)

	# Create indicator box (PanelContainer styled directly, no ColorRect)
	_indicator_container = PanelContainer.new()
	_indicator_container.name = "IndicatorContainer"
	_indicator_container.custom_minimum_size = Vector2(AppTheme.CHECKBOX_INDICATOR_SIZE, AppTheme.CHECKBOX_INDICATOR_SIZE)
	_indicator_container.mouse_filter = Control.MOUSE_FILTER_IGNORE
	hbox.add_child(_indicator_container)

	# Create checkmark (centered in indicator)
	_checkmark = Label.new()
	_checkmark.name = "Checkmark"
	_checkmark.text = "✓"  # Unicode checkmark
	_checkmark.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_checkmark.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	_checkmark.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_checkmark.set_anchors_preset(Control.PRESET_FULL_RECT)
	_indicator_container.add_child(_checkmark)

	# Create label
	_label = Label.new()
	_label.name = "Label"
	_label.mouse_filter = Control.MOUSE_FILTER_IGNORE
	hbox.add_child(_label)


func _apply_style() -> void:
	if not is_inside_tree():
		return  # Will be called again from _ready()

	# Use theme variation for padding - shader handles all visuals
	theme_type_variation = "PanelCheckboxTile"

	# Update shader base color based on state
	if _shader_material:
		if button_pressed:
			_shader_material.set_shader_parameter("base_color", AppTheme.SURFACE_COLOR_CHECKBOX_ON)
			_shader_material.set_shader_parameter("rim_top_color", AppTheme.with_alpha(AppTheme.CREAM, AppTheme.RIM_LIGHT_STRONG_ALPHA))
		else:
			_shader_material.set_shader_parameter("base_color", AppTheme.SURFACE_COLOR_CHECKBOX_OFF)
			_shader_material.set_shader_parameter("rim_top_color", AppTheme.with_alpha(AppTheme.CREAM, AppTheme.RIM_LIGHT_ALPHA))

	# Style indicator container - use theme variation based on state
	if _indicator_container:
		if button_pressed:
			_indicator_container.theme_type_variation = "PanelCheckboxIndicatorOn"
		else:
			_indicator_container.theme_type_variation = "PanelCheckboxIndicatorOff"

	# Style checkmark - visible only when selected
	if _checkmark:
		_checkmark.visible = button_pressed
		_checkmark.theme_type_variation = "LabelCheckmark"

	# Style label - use theme variation based on state
	if _label:
		if disabled:
			_label.theme_type_variation = "LabelCheckboxDisabled"
		elif button_pressed:
			_label.theme_type_variation = ""  # Default Label (FONT_BODY + CREAM)
		else:
			_label.theme_type_variation = "LabelCheckboxOff"


func _update_display() -> void:
	if _label:
		_label.text = text


func _on_gui_input(event: InputEvent) -> void:
	if disabled:
		return

	if event is InputEventMouseButton:
		if event.button_index == MOUSE_BUTTON_LEFT and event.pressed:
			button_pressed = not button_pressed
			accept_event()


## Toggle the tile state.
func toggle() -> void:
	if not disabled:
		button_pressed = not button_pressed


## Set the pressed state.
func set_pressed(pressed: bool) -> void:
	button_pressed = pressed
