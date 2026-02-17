class_name StyledButton
extends Control
## A styled button using the unified button shader.
##
## All button styling is handled by a single shader for SSoT.
## The shader supports two modes:
##   - Mode 0 (Secondary): Default raised surface style
##   - Mode 1 (Nightlight): Amber gradient for toggle ON state
##
## Styling behavior:
##   - Non-toggleable buttons: Always secondary style
##   - Toggleable OFF: Secondary style
##   - Toggleable ON: Nightlight style
##   - DESTRUCTIVE variant: Secondary style with error-colored text

signal pressed
signal toggled(toggled_on: bool)

## Button style variants (only affects text color for DESTRUCTIVE).
enum Variant { DEFAULT, DESTRUCTIVE }

## The button's display text.
@export var text: String = "":
	set(value):
		text = value
		if _inner_button:
			_inner_button.text = value
			call_deferred("_update_minimum_size")

## Whether the button is disabled.
@export var disabled: bool = false:
	set(value):
		disabled = value
		if _inner_button:
			_inner_button.disabled = value
		_update_shader_mode()

## The button's style variant.
@export var variant: Variant = Variant.DEFAULT:
	set(value):
		variant = value
		_update_text_style()

## Whether the button is toggleable.
@export var toggle_mode: bool = false:
	set(value):
		toggle_mode = value
		if _inner_button:
			_inner_button.toggle_mode = value
		_update_shader_mode()

## The toggle state (only relevant when toggle_mode is true).
@export var button_pressed: bool = false:
	set(value):
		button_pressed = value
		if _inner_button:
			_inner_button.button_pressed = value
		_update_shader_mode()

# Internal components
var _inner_button: Button = null
var _shader_bg: ColorRect = null
var _shader_material: ShaderMaterial = null


func _ready() -> void:
	# Prevent button from stretching in containers - size based on content
	size_flags_horizontal = Control.SIZE_SHRINK_CENTER
	size_flags_vertical = Control.SIZE_SHRINK_CENTER

	# Setup shader background (always present)
	_setup_shader()

	# Create inner button
	_inner_button = Button.new()
	_inner_button.name = "InnerButton"
	_inner_button.text = text
	_inner_button.disabled = disabled
	_inner_button.toggle_mode = toggle_mode
	_inner_button.button_pressed = button_pressed

	# Transparent button background with padding via theme variation - shader handles visuals
	_inner_button.theme_type_variation = "ButtonStyledSecondary"

	# Connect signals
	_inner_button.pressed.connect(_on_inner_button_pressed)
	_inner_button.toggled.connect(_on_inner_button_toggled)
	_inner_button.mouse_entered.connect(_on_mouse_entered)
	_inner_button.mouse_exited.connect(_on_mouse_exited)
	_inner_button.button_down.connect(_on_button_down)
	_inner_button.button_up.connect(_on_button_up)

	# Add inner button - it will determine the control's size
	add_child(_inner_button)

	# Apply initial styling (this sets the font which affects size calculation)
	_update_shader_mode()
	_update_text_style()

	# Connect signals for size updates
	_inner_button.minimum_size_changed.connect(_on_inner_minimum_size_changed)

	# Defer minimum size calculation until inner button has calculated its size
	call_deferred("_update_minimum_size")

	# Update shader and inner button size when control resizes
	resized.connect(_update_shader_size)
	resized.connect(_sync_inner_button_size)

	# Initial sync after layout settles
	call_deferred("_sync_inner_button_size")
	call_deferred("_update_shader_size")


func _setup_shader() -> void:
	# Use factory method for standard setup
	_shader_material = AppTheme.create_button_material()

	# Create ColorRect for shader
	# Extend beyond button bounds to show neumorphic inset
	_shader_bg = ColorRect.new()
	_shader_bg.name = "ShaderBG"
	_shader_bg.color = Color.WHITE  # Shader will override
	_shader_bg.set_anchors_preset(Control.PRESET_FULL_RECT)
	# Extend by inset amount on all sides (1px ring + ~2.5px gradient)
	var inset_extend := float(AppTheme.BUTTON_SHADER_INSET)
	_shader_bg.offset_left = -inset_extend
	_shader_bg.offset_top = -inset_extend
	_shader_bg.offset_right = inset_extend
	_shader_bg.offset_bottom = inset_extend
	_shader_bg.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_shader_bg.material = _shader_material

	add_child(_shader_bg)


func _update_shader_size() -> void:
	if _shader_material and size.x > 0 and size.y > 0:
		# The shader ColorRect is expanded by inset_extend on all sides
		# Pass the expanded size so the shader knows its full render area
		var inset_extend := float(AppTheme.BUTTON_SHADER_INSET)
		var shader_size := size + Vector2(inset_extend * 2, inset_extend * 2)
		_shader_material.set_shader_parameter("rect_size", shader_size)
		# Tell shader how much inset to use for drawing the button within the expanded area
		_shader_material.set_shader_parameter("button_inset", inset_extend)


func _update_shader_mode() -> void:
	if not _shader_material:
		return

	# Mode 1 (nightlight) when button_pressed is true (highlighted state)
	# Works for both toggle buttons and programmatically controlled highlight
	var use_nightlight: bool = button_pressed and not disabled
	_shader_material.set_shader_parameter("mode", 1 if use_nightlight else 0)

	# Update button state for disabled
	if disabled:
		_set_shader_button_state(3)  # Disabled state
	else:
		_set_shader_button_state(0)  # Reset to normal when re-enabled

	# Update text style when mode changes
	_update_text_style()


func _update_text_style() -> void:
	if not _inner_button:
		return

	var use_nightlight: bool = button_pressed and not disabled

	# Use theme variations instead of manual overrides
	if use_nightlight:
		# Nightlight: dark text on amber (semibold font set in theme)
		_inner_button.theme_type_variation = "ButtonStyledPrimary"
	else:
		# Secondary: cream text on dark surface
		match variant:
			Variant.DEFAULT:
				_inner_button.theme_type_variation = "ButtonStyledSecondary"
			Variant.DESTRUCTIVE:
				_inner_button.theme_type_variation = "ButtonStyledDestructive"


func _set_shader_button_state(state: int) -> void:
	if _shader_material:
		# If disabled, force state to 3 regardless of input
		var actual_state := 3 if disabled else state
		_shader_material.set_shader_parameter("button_state", actual_state)
		# Pressed state (2) uses dark-only rim
		_shader_material.set_shader_parameter("rim_mode", 1 if actual_state == 2 else 0)
		# Glow intensity for amber mode: subtle for normal, pronounced for hover, pulled in for pressed, none for disabled
		var glow := AppTheme.BUTTON_GLOW_NORMAL
		if actual_state == 1:
			glow = AppTheme.BUTTON_GLOW_HOVER
		elif actual_state == 2:
			glow = AppTheme.BUTTON_GLOW_PRESSED
		elif actual_state == 3:
			glow = AppTheme.BUTTON_GLOW_DISABLED
		_shader_material.set_shader_parameter("glow_intensity", glow)


func _on_inner_button_pressed() -> void:
	pressed.emit()


func _on_inner_button_toggled(toggled_on: bool) -> void:
	button_pressed = toggled_on
	toggled.emit(toggled_on)


func _on_mouse_entered() -> void:
	if not disabled:
		_set_shader_button_state(1)


func _on_mouse_exited() -> void:
	_set_shader_button_state(0)


func _on_button_down() -> void:
	if not disabled:
		_set_shader_button_state(2)


func _on_button_up() -> void:
	if _inner_button and _inner_button.get_global_rect().has_point(_inner_button.get_global_mouse_position()):
		_set_shader_button_state(1)
	else:
		_set_shader_button_state(0)


## Programmatically set the toggle state without emitting toggled signal.
func set_pressed_no_signal(is_pressed: bool) -> void:
	if _inner_button:
		_inner_button.set_pressed_no_signal(is_pressed)
	button_pressed = is_pressed


## Override to provide content-based minimum size
func _get_minimum_size() -> Vector2:
	if _inner_button:
		var content_size := _inner_button.get_combined_minimum_size()
		# Return max of content size and custom_minimum_size
		return Vector2(
			max(content_size.x, custom_minimum_size.x),
			max(content_size.y, custom_minimum_size.y)
		)
	return custom_minimum_size


func _update_minimum_size() -> void:
	# Notify the layout system that our minimum size may have changed
	update_minimum_size()
	# Also request parent to re-sort its children
	var parent := get_parent()
	if parent and parent is Container:
		parent.queue_sort()


func _on_inner_minimum_size_changed() -> void:
	call_deferred("_update_minimum_size")


func _sync_inner_button_size() -> void:
	if _inner_button:
		# Make inner button fill the outer control
		_inner_button.position = Vector2.ZERO
		_inner_button.size = size
