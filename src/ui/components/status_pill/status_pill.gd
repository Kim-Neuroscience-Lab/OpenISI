class_name StatusPill
extends PanelContainer
## A status pill showing a glowing dot and status text.
##
## Supports status types: success, warning, error, info, pending.
## The dot pulses gently for active states.

## Status type affecting color.
@export_enum("success", "warning", "error", "info", "pending") var status: String = "info":
	set(value):
		status = value
		_apply_style()

## The status label text.
@export var text: String = "Status":
	set(value):
		text = value
		_update_display()

## Whether the dot should pulse (for active states).
@export var pulsing: bool = false:
	set(value):
		pulsing = value
		_update_pulse()

var _dot: ColorRect = null
var _label: Label = null
var _pulse_tween: Tween = null
var _shader_material: ShaderMaterial = null


func _ready() -> void:
	_build_ui()
	_setup_shader()
	_apply_style()
	_update_display()


func _setup_shader() -> void:
	_shader_material = AppTheme.create_raised_surface_material(
		AppTheme.SURFACE_COLOR_STATUS,
		float(AppTheme.RADIUS_MD)
	)
	_shader_material.set_shader_parameter("rect_size", size)
	material = _shader_material

	resized.connect(_on_resized)


func _on_resized() -> void:
	if _shader_material:
		_shader_material.set_shader_parameter("rect_size", size)


func _build_ui() -> void:
	# Create inner HBox
	var hbox := HBoxContainer.new()
	hbox.name = "Content"
	hbox.theme_type_variation = "HBoxSM"
	hbox.alignment = BoxContainer.ALIGNMENT_CENTER
	add_child(hbox)

	# Create status dot
	_dot = ColorRect.new()
	_dot.name = "StatusDot"
	_dot.custom_minimum_size = Vector2(AppTheme.STATUS_DOT_SIZE, AppTheme.STATUS_DOT_SIZE)
	hbox.add_child(_dot)

	# Create label
	_label = Label.new()
	_label.name = "StatusLabel"
	hbox.add_child(_label)


func _apply_style() -> void:
	if not is_inside_tree():
		return  # Will be called again from _ready()

	var status_color: Color = AppTheme.get_status_color(status)

	# Use theme variation for padding - shader handles all visuals
	theme_type_variation = "PanelPill"

	# Update shader with status-colored accent border
	if _shader_material:
		var border_color := AppTheme.with_alpha(status_color, AppTheme.BORDER_ACCENT_ALPHA)
		_shader_material.set_shader_parameter("accent_border_color", border_color)
		_shader_material.set_shader_parameter("accent_border_width", float(AppTheme.BORDER_WIDTH_ACCENT))

	# Style the dot
	if _dot:
		_dot.color = status_color

	# Style the label
	if _label:
		_label.theme_type_variation = "LabelCaption"


func _update_display() -> void:
	if _label:
		_label.text = text


func _update_pulse() -> void:
	# Stop existing pulse
	if _pulse_tween and _pulse_tween.is_running():
		_pulse_tween.kill()
		if _dot:
			_dot.modulate.a = 1.0

	if not pulsing or not _dot:
		return

	# Start new pulse animation using theme constant (SSoT)
	var pulse_duration := AppTheme.ANIM_PULSE

	_pulse_tween = create_tween()
	_pulse_tween.set_loops()
	_pulse_tween.set_ease(Tween.EASE_IN_OUT)
	_pulse_tween.set_trans(Tween.TRANS_SINE)
	_pulse_tween.tween_property(_dot, "modulate:a", 0.5, pulse_duration / 2.0)
	_pulse_tween.tween_property(_dot, "modulate:a", 1.0, pulse_duration / 2.0)


## Set the status type and text.
func set_status(new_status: String, new_text: String = "") -> void:
	status = new_status
	if not new_text.is_empty():
		text = new_text


## Stop any pulse animation.
func stop_pulse() -> void:
	pulsing = false
