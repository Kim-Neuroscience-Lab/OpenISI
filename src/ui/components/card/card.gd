@tool
class_name Card
extends BaseCard
## A card component with optional title and content slot.

signal title_changed(new_title: String)

enum Style { RAISED, INSET, FLAT }

@export var title: String = "":
	set(value):
		title = value
		_update_title()
		title_changed.emit(value)

@export var style: Style = Style.RAISED:
	set(value):
		style = value
		_apply_style()

@export var show_title: bool = true:
	set(value):
		show_title = value
		_update_title()

var _vbox: VBoxContainer = null
var _title_container: HBoxContainer = null
var _title_label: Label = null
var _header_slot: Control = null
var _content_slot: MarginContainer = null


func _ready() -> void:
	_corner_radius = float(AppTheme.RADIUS_2XL)
	_build_ui()
	super._ready()
	_apply_style()
	_update_title()


func _build_ui() -> void:
	_vbox = VBoxContainer.new()
	_vbox.name = "VBoxContainer"
	_vbox.theme_type_variation = "VBoxMD"
	add_child(_vbox)

	# Title container (horizontal: title + header slot)
	_title_container = HBoxContainer.new()
	_title_container.name = "TitleContainer"
	_vbox.add_child(_title_container)

	# Title label
	_title_label = Label.new()
	_title_label.name = "TitleLabel"
	_title_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_title_label.text = "Card Title"
	_title_container.add_child(_title_label)

	# Header slot (for optional widgets next to title)
	_header_slot = Control.new()
	_header_slot.name = "HeaderSlot"
	_header_slot.size_flags_horizontal = Control.SIZE_SHRINK_BEGIN
	_title_container.add_child(_header_slot)

	# Content slot
	_content_slot = MarginContainer.new()
	_content_slot.name = "ContentSlot"
	_content_slot.size_flags_vertical = Control.SIZE_EXPAND_FILL
	_vbox.add_child(_content_slot)


func _apply_style() -> void:
	if not is_inside_tree():
		return

	match style:
		Style.RAISED:
			theme_type_variation = ""
			_draw_gradient = true
			_draw_rim_highlight = true
		Style.INSET:
			theme_type_variation = "PanelWell"
			_draw_gradient = false
			_draw_rim_highlight = false
		Style.FLAT:
			theme_type_variation = "PanelTransparent"
			_draw_gradient = false
			_draw_rim_highlight = false

	# Update shader with new style settings
	_update_shader()

	if _title_label:
		_title_label.theme_type_variation = "LabelHeading"  # 15px, Medium, creamMuted per mockup


func _update_title() -> void:
	if not is_inside_tree():
		return

	if _title_container:
		_title_container.visible = show_title and not title.is_empty()
	if _title_label:
		_title_label.text = title


func get_content_slot() -> MarginContainer:
	return _content_slot


func get_title_label() -> Label:
	return _title_label


func get_header_slot() -> Control:
	return _header_slot
