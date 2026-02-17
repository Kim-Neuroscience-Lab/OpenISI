class_name Divider
extends Control
## A divider line with ridge styling.
##
## Supports two styles: ridge (physical feel) and glow (subtle line).

## Divider style variant.
enum Style { RIDGE, GLOW }

## The divider style.
@export var style: Style = Style.RIDGE:
	set(value):
		style = value
		_apply_style()

## Vertical margin above and below the divider.
@export var margin: int = 16:
	set(value):
		margin = value
		custom_minimum_size.y = margin * 2 + AppTheme.DIVIDER_LINE_HEIGHT * 2

var _line: ColorRect = null
var _line_bottom: ColorRect = null


func _ready() -> void:
	_build_ui()
	_apply_style()


func _build_ui() -> void:
	# Set minimum size (margin above + margin below + two line heights)
	custom_minimum_size = Vector2(0, margin * 2 + AppTheme.DIVIDER_LINE_HEIGHT * 2)

	# Create top line (dark shadow)
	_line = ColorRect.new()
	_line.name = "LineDark"
	_line.custom_minimum_size = Vector2(0, AppTheme.DIVIDER_LINE_HEIGHT)
	_line.anchor_left = 0
	_line.anchor_right = 1
	_line.anchor_top = 0.5
	_line.anchor_bottom = 0.5
	_line.offset_left = 0
	_line.offset_right = 0
	_line.offset_top = -AppTheme.DIVIDER_LINE_HEIGHT
	_line.offset_bottom = 0
	add_child(_line)

	# Create bottom line (rim light)
	_line_bottom = ColorRect.new()
	_line_bottom.name = "LineLight"
	_line_bottom.custom_minimum_size = Vector2(0, AppTheme.DIVIDER_LINE_HEIGHT)
	_line_bottom.anchor_left = 0
	_line_bottom.anchor_right = 1
	_line_bottom.anchor_top = 0.5
	_line_bottom.anchor_bottom = 0.5
	_line_bottom.offset_left = 0
	_line_bottom.offset_right = 0
	_line_bottom.offset_top = 0
	_line_bottom.offset_bottom = AppTheme.DIVIDER_LINE_HEIGHT
	add_child(_line_bottom)


func _apply_style() -> void:
	if not _line:
		return

	match style:
		Style.RIDGE:
			_line.color = AppTheme.with_alpha(Color.BLACK, AppTheme.DIVIDER_DARK_ALPHA)
			_line.custom_minimum_size.y = AppTheme.DIVIDER_LINE_HEIGHT
			if _line_bottom:
				_line_bottom.visible = true
				_line_bottom.color = AppTheme.with_alpha(AppTheme.CREAM, AppTheme.DIVIDER_LIGHT_ALPHA)
				_line_bottom.custom_minimum_size.y = AppTheme.DIVIDER_LINE_HEIGHT

		Style.GLOW:
			_line.color = AppTheme.with_alpha(AppTheme.LAVENDER_DEEP, AppTheme.DIVIDER_DARK_ALPHA)
			_line.custom_minimum_size.y = AppTheme.DIVIDER_LINE_HEIGHT
			if _line_bottom:
				_line_bottom.visible = false
