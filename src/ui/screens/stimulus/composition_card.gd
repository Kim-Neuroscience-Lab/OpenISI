class_name CompositionCard
extends MarginContainer
## CompositionCard - Carrier/envelope/space selection UI
##
## Handles stimulus composition selection including carrier patterns,
## envelope shapes, coordinate space (Cartesian/Spherical/Cylindrical),
## and optional strobe (counterphase reversal).


signal composition_changed(carrier: Carriers.Type, envelope: Envelopes.Type, space: int, strobe_enabled: bool)

var _paradigm_selector: OptionButton = null
var _carrier_selector: OptionButton = null
var _envelope_selector: OptionButton = null
var _space_selector: OptionButton = null
var _strobe_checkbox: CheckboxTile = null

var _current_carrier: Carriers.Type = Carriers.Type.CHECKERBOARD
var _current_envelope: Envelopes.Type = Envelopes.Type.BAR
var _current_space: int = DisplayGeometry.ProjectionType.CARTESIAN
var _strobe_enabled: bool = false

# Flag to suppress change events during programmatic updates
var _updating: bool = false


func _ready() -> void:
	_build_ui()
	_connect_signals()


func _build_ui() -> void:
	var card := Card.new()
	card.title = "Stimulus"
	add_child(card)

	var content := VBoxContainer.new()
	content.theme_type_variation = "VBoxMD"
	card.get_content_slot().add_child(content)

	# Paradigm selector row
	var paradigm_row := HBoxContainer.new()
	paradigm_row.theme_type_variation = "HBoxSM"
	content.add_child(paradigm_row)

	var paradigm_label := Label.new()
	paradigm_label.text = "Paradigm"
	paradigm_label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_MD
	paradigm_row.add_child(paradigm_label)

	_paradigm_selector = OptionButton.new()
	_paradigm_selector.add_item("Texture", 0)
	_paradigm_selector.add_item("Element (coming soon)", 1)
	_paradigm_selector.add_item("Media (coming soon)", 2)
	_paradigm_selector.set_item_disabled(1, true)
	_paradigm_selector.set_item_disabled(2, true)
	_paradigm_selector.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_paradigm_selector.custom_minimum_size.y = AppTheme.INPUT_HEIGHT
	paradigm_row.add_child(_paradigm_selector)

	# Divider
	var divider1 := Divider.new()
	divider1.margin = 4
	content.add_child(divider1)

	# Composition section header
	var comp_header := SectionHeader.new()
	comp_header.title = "COMPOSITION"
	content.add_child(comp_header)

	# Carrier selector row
	var carrier_row := HBoxContainer.new()
	carrier_row.theme_type_variation = "HBoxSM"
	content.add_child(carrier_row)

	var carrier_label := Label.new()
	carrier_label.text = "Carrier"
	carrier_label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_MD
	carrier_row.add_child(carrier_label)

	_carrier_selector = OptionButton.new()
	for carrier_type in Carriers.get_all_types():
		_carrier_selector.add_item(Carriers.get_display_name(carrier_type), carrier_type)
	_carrier_selector.select(Carriers.Type.CHECKERBOARD)
	_carrier_selector.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_carrier_selector.custom_minimum_size.y = AppTheme.INPUT_HEIGHT
	carrier_row.add_child(_carrier_selector)

	# Envelope selector row
	var envelope_row := HBoxContainer.new()
	envelope_row.theme_type_variation = "HBoxSM"
	content.add_child(envelope_row)

	var envelope_label := Label.new()
	envelope_label.text = "Envelope"
	envelope_label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_MD
	envelope_row.add_child(envelope_label)

	_envelope_selector = OptionButton.new()
	for envelope_type in Envelopes.get_all_types():
		_envelope_selector.add_item(Envelopes.get_display_name(envelope_type), envelope_type)
	_envelope_selector.select(Envelopes.Type.BAR)
	_envelope_selector.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_envelope_selector.custom_minimum_size.y = AppTheme.INPUT_HEIGHT
	envelope_row.add_child(_envelope_selector)

	# Space selector row (determines coordinate system and check_size units)
	var space_row := HBoxContainer.new()
	space_row.theme_type_variation = "HBoxSM"
	content.add_child(space_row)

	var space_label := Label.new()
	space_label.text = "Space"
	space_label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_MD
	space_row.add_child(space_label)

	_space_selector = OptionButton.new()
	_space_selector.add_item("Cartesian", DisplayGeometry.ProjectionType.CARTESIAN)
	_space_selector.add_item("Spherical", DisplayGeometry.ProjectionType.SPHERICAL)
	_space_selector.add_item("Cylindrical", DisplayGeometry.ProjectionType.CYLINDRICAL)
	_space_selector.select(0)  # Default to Cartesian
	_space_selector.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_space_selector.custom_minimum_size.y = AppTheme.INPUT_HEIGHT
	space_row.add_child(_space_selector)

	# Strobe checkbox row
	var strobe_row := HBoxContainer.new()
	strobe_row.theme_type_variation = "HBoxSM"
	content.add_child(strobe_row)

	var strobe_label := Label.new()
	strobe_label.text = "Strobe"
	strobe_label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_MD
	strobe_row.add_child(strobe_label)

	_strobe_checkbox = CheckboxTile.new()
	_strobe_checkbox.text = "Counterphase reversal"
	_strobe_checkbox.button_pressed = false
	strobe_row.add_child(_strobe_checkbox)


func _connect_signals() -> void:
	_carrier_selector.item_selected.connect(_on_selection_changed)
	_envelope_selector.item_selected.connect(_on_selection_changed)
	_space_selector.item_selected.connect(_on_selection_changed)
	_strobe_checkbox.toggled.connect(_on_selection_changed)


func _on_selection_changed(_value: Variant = null) -> void:
	if _updating:
		return

	_current_carrier = _carrier_selector.get_selected_id() as Carriers.Type
	_current_envelope = _envelope_selector.get_selected_id() as Envelopes.Type
	_current_space = _space_selector.get_selected_id()
	_strobe_enabled = _strobe_checkbox.button_pressed

	composition_changed.emit(_current_carrier, _current_envelope, _current_space, _strobe_enabled)


## Get the currently selected carrier type
func get_carrier() -> Carriers.Type:
	return _current_carrier


## Get the currently selected envelope type
func get_envelope() -> Envelopes.Type:
	return _current_envelope


## Get the currently selected coordinate space (projection type)
func get_space() -> int:
	return _current_space


## Check if strobe (counterphase reversal) is enabled
func is_strobe_enabled() -> bool:
	return _strobe_enabled


## Set all composition values programmatically
func set_composition(carrier: Carriers.Type, envelope: Envelopes.Type, space: int, strobe: bool) -> void:
	_current_carrier = carrier
	_current_envelope = envelope
	_current_space = space
	_strobe_enabled = strobe

	# Update UI without triggering change events
	_updating = true

	# Find correct index for carrier (use ID matching for robustness)
	for i: int in range(_carrier_selector.item_count):
		if _carrier_selector.get_item_id(i) == carrier:
			_carrier_selector.select(i)
			break

	# Find correct index for envelope (enum values may not match indices after NONE was added)
	for i: int in range(_envelope_selector.item_count):
		if _envelope_selector.get_item_id(i) == envelope:
			_envelope_selector.select(i)
			break

	# Find correct index for space
	for i: int in range(_space_selector.item_count):
		if _space_selector.get_item_id(i) == space:
			_space_selector.select(i)
			break

	_strobe_checkbox.button_pressed = strobe
	_updating = false
