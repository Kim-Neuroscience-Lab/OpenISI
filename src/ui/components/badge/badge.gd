@tool
class_name StatusBadge
extends PanelContainer
## A status badge component.
##
## Usage:
##   var badge = StatusBadge.new()
##   badge.text = "Connected"
##   badge.status = StatusBadge.Status.SUCCESS

## Status types that determine the badge color.
enum Status { SUCCESS, WARNING, ERROR, INFO, NEUTRAL }

## The badge text.
@export var text: String = "Status":
	set(value):
		text = value
		if _label:
			_label.text = value

## The badge status (determines color).
@export var status: Status = Status.NEUTRAL:
	set(value):
		status = value
		_apply_style()

## Optional icon character (emoji or icon font).
@export var icon: String = "":
	set(value):
		icon = value
		if _icon_label:
			_icon_label.text = value
			_icon_label.visible = not value.is_empty()

var _hbox: HBoxContainer = null
var _label: Label = null
var _icon_label: Label = null


func _ready() -> void:
	_build_ui()
	_apply_style()
	_update_content()


func _build_ui() -> void:
	_hbox = HBoxContainer.new()
	_hbox.name = "HBoxContainer"
	add_child(_hbox)

	_icon_label = Label.new()
	_icon_label.name = "IconLabel"
	_icon_label.visible = false
	_hbox.add_child(_icon_label)

	_label = Label.new()
	_label.name = "Label"
	_label.text = "Status"
	_hbox.add_child(_label)


func _apply_style() -> void:
	if not is_inside_tree():
		return

	# Set container separation from theme variation
	if _hbox:
		_hbox.theme_type_variation = "HBoxXS"

	var status_str := _status_to_string(status)

	# Use theme variation for panel styling based on status
	theme_type_variation = _get_panel_variation(status)

	# Use theme variations for status-colored labels
	var label_variation := AppTheme.get_status_label_variation(status_str, true)  # small=true
	if _label:
		_label.theme_type_variation = label_variation
	if _icon_label:
		_icon_label.theme_type_variation = label_variation


func _update_content() -> void:
	if _label:
		_label.text = text
	if _icon_label:
		_icon_label.text = icon
		_icon_label.visible = not icon.is_empty()


func _status_to_string(s: Status) -> String:
	match s:
		Status.SUCCESS:
			return "success"
		Status.WARNING:
			return "warning"
		Status.ERROR:
			return "error"
		Status.INFO:
			return "info"
		_:
			return "neutral"


func _get_panel_variation(s: Status) -> String:
	match s:
		Status.SUCCESS:
			return "PanelBadgeSuccess"
		Status.WARNING:
			return "PanelBadgeError"
		Status.ERROR:
			return "PanelBadgeError"
		Status.INFO:
			return "PanelBadgeInfo"
		_:
			return "PanelBadgeNeutral"


## Sets the status from a string value.
func set_status_from_string(status_str: String) -> void:
	match status_str.to_lower():
		"success", "connected", "ready", "pass", "complete":
			status = Status.SUCCESS
		"warning", "attention", "pending":
			status = Status.WARNING
		"error", "fail", "disconnected", "failed":
			status = Status.ERROR
		"info", "active", "running":
			status = Status.INFO
		_:
			status = Status.NEUTRAL
