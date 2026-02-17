class_name SectionHeader
extends HBoxContainer
## A section header component with uppercase lavender text.
##
## Used to create visual rhythm and organize dense control panels.
## Features a subtle glow effect per the Sleep Punk design system.

## The section title text.
@export var title: String = "SECTION":
	set(value):
		title = value
		_update_display()

## Whether to show an optional underline.
@export var show_underline: bool = false:
	set(value):
		show_underline = value
		_update_display()

var _label: Label = null
var _underline: ColorRect = null


func _ready() -> void:
	_build_ui()
	_apply_style()
	_update_display()


func _build_ui() -> void:
	# Create label
	_label = Label.new()
	_label.name = "TitleLabel"
	_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	add_child(_label)

	# Create underline (hidden by default)
	_underline = ColorRect.new()
	_underline.name = "Underline"
	_underline.custom_minimum_size = Vector2(0, AppTheme.DIVIDER_LINE_HEIGHT)
	_underline.visible = false
	add_child(_underline)


func _apply_style() -> void:
	if _label:
		_label.theme_type_variation = "LabelSection"
		_label.uppercase = true
		# Add glow effect on top of theme styling
		var glow_mat = AppTheme.create_text_glow_material(AppTheme.LAVENDER, AppTheme.GLOW_INTENSITY)
		if glow_mat:
			_label.material = glow_mat

	if _underline:
		_underline.color = AppTheme.with_alpha(AppTheme.LAVENDER_DEEP, AppTheme.SHADOW_ALPHA_SUBTLE)


func _update_display() -> void:
	if _label:
		_label.text = title

	if _underline:
		_underline.visible = show_underline


## Set the section title.
func set_title(new_title: String) -> void:
	title = new_title
