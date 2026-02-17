extends BaseScreen
## Stimulus screen: Define and preview stimulus parameters.
##
## Coordinates stimulus configuration through focused card components:
## - CompositionCard: Carrier/envelope selection
## - ParametersCard: Dynamic parameter controls
## - SequenceCard: Condition selection/ordering
## - GeometryCard: Display geometry configuration
## - TimingCard: Timing configuration
## - PreviewController: Stimulus preview management

# Card components
var _composition_card: CompositionCard = null
var _parameters_card: ParametersCard = null
var _sequence_card: SequenceCard = null
var _geometry_card: GeometryCard = null
var _timing_card: TimingCard = null

# Preview
var _preview_controller: PreviewController = null
var _preview_container: Control = null
var _play_button: StyledButton = null

# Protocol toolbar
var _protocol_label: Label = null
var _save_button: StyledButton = null
var _load_button: StyledButton = null

# Summary
var _sweep_duration_row: InfoRow = null
var _total_sweeps_row: InfoRow = null
var _total_duration_row: InfoRow = null

# Current snapshot name (for save/load UI)
var _current_snapshot_name: String = ""


func _build_ui() -> void:
	# Main HBox fills entire screen area - scroll container extends into fade zone
	var hbox := HBoxContainer.new()
	hbox.name = "MainLayout"
	hbox.set_anchors_preset(Control.PRESET_FULL_RECT)
	hbox.theme_type_variation = "HBox2XL"
	add_child(hbox)

	_build_preview_section(hbox)
	_build_controls_section(hbox)


func _build_preview_section(parent: Control) -> void:
	# Preview section with margin wrapper for proper positioning (non-scrolling)
	var preview_margin := MarginContainer.new()
	preview_margin.name = "PreviewMargin"
	preview_margin.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	preview_margin.size_flags_vertical = Control.SIZE_EXPAND_FILL
	preview_margin.size_flags_stretch_ratio = 2.0
	preview_margin.theme_type_variation = "MarginScreenContentLeftOnly"
	parent.add_child(preview_margin)

	var preview_card := Card.new()
	preview_card.title = "Stimulus Preview"
	preview_card.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	preview_card.size_flags_vertical = Control.SIZE_EXPAND_FILL
	preview_margin.add_child(preview_card)

	var content := VBoxContainer.new()
	content.theme_type_variation = "VBoxLG"
	content.size_flags_vertical = Control.SIZE_EXPAND_FILL
	preview_card.get_content_slot().add_child(content)

	var preview_well := PanelContainer.new()
	preview_well.name = "PreviewWell"
	preview_well.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	preview_well.size_flags_vertical = Control.SIZE_EXPAND_FILL
	preview_well.custom_minimum_size = Vector2(0, AppTheme.PREVIEW_HEIGHT_MD)
	preview_well.theme_type_variation = "PanelWellFlush"
	content.add_child(preview_well)

	# AspectRatioContainer to maintain display aspect ratio
	var aspect_container := AspectRatioContainer.new()
	aspect_container.name = "AspectContainer"
	aspect_container.set_anchors_preset(Control.PRESET_FULL_RECT)
	aspect_container.ratio = Session.display_width_cm / Session.display_height_cm
	aspect_container.stretch_mode = AspectRatioContainer.STRETCH_FIT
	preview_well.add_child(aspect_container)

	_preview_container = Control.new()
	_preview_container.name = "StimulusContainer"
	_preview_container.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_preview_container.size_flags_vertical = Control.SIZE_EXPAND_FILL
	_preview_container.clip_contents = true
	aspect_container.add_child(_preview_container)

	_preview_controller = PreviewController.new()
	_preview_controller.initialize(_preview_container)

	var button_row := HBoxContainer.new()
	button_row.theme_type_variation = "HBoxMD"
	button_row.alignment = BoxContainer.ALIGNMENT_CENTER
	content.add_child(button_row)

	_play_button = StyledButton.new()
	_play_button.text = "Play Preview"
	_play_button.button_pressed = true
	button_row.add_child(_play_button)
	_preview_controller.set_play_button(_play_button)


func _build_controls_section(parent: Control) -> void:
	# Scroll container extends full height so content can scroll into fade zone
	var scroll := SmoothScrollContainer.new()
	scroll.name = "ControlsScroll"
	scroll.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	scroll.scrollbar_vertical_inset = AppTheme.SCROLL_FADE_HEIGHT  # Inset scrollbar from fade zone
	parent.add_child(scroll)

	# Inner margin for scroll content - padding keeps content below fade initially
	var scroll_margin := MarginContainer.new()
	scroll_margin.name = "ScrollContentMargin"
	scroll_margin.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	scroll_margin.theme_type_variation = "MarginScreenContentRightOnly"
	scroll.add_child(scroll_margin)

	var vbox := VBoxContainer.new()
	vbox.name = "ControlsContent"
	vbox.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	vbox.theme_type_variation = "VBox2XL"
	scroll_margin.add_child(vbox)

	# Protocol toolbar (save/load)
	_build_protocol_toolbar(vbox)

	_composition_card = CompositionCard.new()
	_composition_card.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	vbox.add_child(_composition_card)

	_parameters_card = ParametersCard.new()
	_parameters_card.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	vbox.add_child(_parameters_card)

	_sequence_card = SequenceCard.new()
	_sequence_card.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	vbox.add_child(_sequence_card)

	_geometry_card = GeometryCard.new()
	_geometry_card.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	vbox.add_child(_geometry_card)

	_timing_card = TimingCard.new()
	_timing_card.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	vbox.add_child(_timing_card)

	_build_summary_card(vbox)


func _build_protocol_toolbar(parent: Control) -> void:
	var toolbar := HBoxContainer.new()
	toolbar.name = "ProtocolToolbar"
	toolbar.theme_type_variation = "HBoxMD"
	parent.add_child(toolbar)

	_protocol_label = Label.new()
	_protocol_label.text = "Protocol: Default"
	_protocol_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_protocol_label.theme_type_variation = "LabelSmallDim"
	toolbar.add_child(_protocol_label)

	_save_button = StyledButton.new()
	_save_button.text = "Save"
	toolbar.add_child(_save_button)

	_load_button = StyledButton.new()
	_load_button.text = "Load..."
	toolbar.add_child(_load_button)


func _build_summary_card(parent: Control) -> void:
	var card := Card.new()
	card.title = "Summary"
	parent.add_child(card)

	var content := VBoxContainer.new()
	content.theme_type_variation = "VBoxSM"
	card.get_content_slot().add_child(content)

	_sweep_duration_row = InfoRow.new()
	_sweep_duration_row.label_text = "Sweep Duration"
	_sweep_duration_row.value_text = "--"
	_sweep_duration_row.mono_value = true
	content.add_child(_sweep_duration_row)

	_total_sweeps_row = InfoRow.new()
	_total_sweeps_row.label_text = "Total Sweeps"
	_total_sweeps_row.value_text = "--"
	content.add_child(_total_sweeps_row)

	_total_duration_row = InfoRow.new()
	_total_duration_row.label_text = "Total Duration"
	_total_duration_row.value_text = "--"
	_total_duration_row.important_value = true
	content.add_child(_total_duration_row)


func _connect_signals() -> void:
	_composition_card.composition_changed.connect(_on_composition_changed)
	_parameters_card.parameter_changed.connect(_on_parameter_changed)
	_sequence_card.sequence_changed.connect(_on_sequence_changed)
	_geometry_card.geometry_changed.connect(_on_geometry_changed)
	_timing_card.timing_changed.connect(_on_timing_changed)
	_play_button.pressed.connect(_on_play_pressed)
	_save_button.pressed.connect(_on_save_pressed)
	_load_button.pressed.connect(_on_load_pressed)


func _load_state() -> void:
	# Load current Config state into UI
	_apply_config_to_ui()
	_update_protocol_label("")


func _apply_config_to_ui() -> void:
	## Apply current Config values to all UI cards
	var carrier: int = Settings.carrier
	var envelope: int = Settings.envelope
	var space: int = Settings.projection_type
	var strobe: bool = Settings.strobe_enabled

	# Apply to composition card
	_composition_card.set_composition(carrier, envelope, space, strobe)

	# Apply to parameters card (set values first so controls can read them during creation)
	_parameters_card.set_parameters(Settings.get_stimulus_params())
	_parameters_card.set_component_types(carrier, envelope, space, strobe)

	# Apply to sequence card
	var available_conditions := _get_available_conditions_for_envelope(envelope)
	_sequence_card.set_available_conditions(available_conditions)

	# Convert Config conditions to typed array
	var selected_conditions: Array[String] = []
	for cond in Settings.conditions:
		selected_conditions.append(str(cond))

	if not selected_conditions.is_empty():
		var order := _config_order_to_card_order(Settings.order)
		_sequence_card.set_sequence(
			selected_conditions,
			Settings.repetitions,
			order
		)

	# Apply to geometry card
	var geom := DisplayGeometry.from_config()
	_geometry_card.set_geometry(geom)

	# Apply to timing card
	_timing_card.set_timing(Settings.get_timing())

	# Update preview and summary
	_update_summary()
	_update_preview()
	_validate()


func _config_order_to_card_order(order_str: String) -> SequenceCard.ConditionOrder:
	match order_str:
		"interleaved":
			return SequenceCard.ConditionOrder.INTERLEAVED
		"randomized":
			return SequenceCard.ConditionOrder.RANDOMIZED
		_:
			return SequenceCard.ConditionOrder.SEQUENTIAL


func _card_order_to_config_order(order: SequenceCard.ConditionOrder) -> String:
	match order:
		SequenceCard.ConditionOrder.INTERLEAVED:
			return "interleaved"
		SequenceCard.ConditionOrder.RANDOMIZED:
			return "randomized"
		_:
			return "sequential"


func _update_protocol_label(path: String) -> void:
	if _protocol_label == null:
		return

	if _current_snapshot_name.is_empty():
		if path.is_empty():
			_protocol_label.text = "Protocol: Default"
		else:
			_protocol_label.text = "Protocol: Unsaved"
	else:
		_protocol_label.text = "Protocol: %s" % _current_snapshot_name


func _get_available_conditions_for_envelope(envelope: int) -> Array[String]:
	# Use envelope to determine direction system
	var system := DirectionSystem.get_system_for_envelope(envelope)
	return DirectionSystem.get_directions(system)


# -----------------------------------------------------------------------------
# Signal Handlers
# -----------------------------------------------------------------------------

func _on_composition_changed(carrier: Carriers.Type, envelope: Envelopes.Type, space: int, strobe: bool) -> void:
	# Update Settings (SSoT)
	Settings.carrier = carrier
	Settings.envelope = envelope
	Settings.projection_type = space
	Settings.strobe_enabled = strobe
	Settings.stimulus_type = _get_renderer_type_id(envelope)

	# Update dependent UI cards based on composition
	_parameters_card.set_component_types(carrier, envelope, space, strobe)

	var conditions := _get_available_conditions_for_envelope(envelope)
	_sequence_card.set_available_conditions(conditions)

	_sync_ui_to_config()
	_update_summary()
	_update_preview()
	_validate()


func _on_parameter_changed(key: String, value: Variant) -> void:
	Settings.set_stimulus_param(key, value)
	_update_summary()
	_update_preview()


func _on_sequence_changed(conditions: Array, reps: int, order: SequenceCard.ConditionOrder) -> void:
	Settings.conditions = conditions
	Settings.repetitions = reps
	Settings.order = _card_order_to_config_order(order)
	_update_summary()
	_validate()


func _on_geometry_changed(geometry: DisplayGeometry) -> void:
	if geometry:
		Settings.viewing_distance_cm = geometry.viewing_distance_cm
		Settings.horizontal_offset_deg = geometry.center_azimuth_deg
		Settings.vertical_offset_deg = geometry.center_elevation_deg
	_update_summary()
	_update_preview()


func _on_timing_changed(timing: Dictionary) -> void:
	Settings.baseline_start_sec = timing["baseline_start_sec"]
	Settings.baseline_end_sec = timing["baseline_end_sec"]
	Settings.inter_stimulus_sec = timing["inter_stimulus_sec"]
	Settings.inter_direction_sec = timing["inter_direction_sec"]
	_update_summary()


func _on_play_pressed() -> void:
	_preview_controller.toggle()


func _on_save_pressed() -> void:
	_sync_ui_to_config()

	if not _current_snapshot_name.is_empty():
		# Save to existing snapshot
		var path := Settings.save_snapshot(_current_snapshot_name)
		if not path.is_empty():
			_update_protocol_label(path)
	else:
		# Need to save as new
		_show_save_dialog()


func _on_load_pressed() -> void:
	_show_load_dialog()


func _show_save_dialog() -> void:
	# Create a simple name input dialog
	var dialog := AcceptDialog.new()
	dialog.title = "Save Protocol"
	dialog.dialog_text = "Enter protocol name:"
	dialog.ok_button_text = "Save"

	var input := LineEdit.new()
	input.placeholder_text = "Protocol Name"
	input.text = _current_snapshot_name
	input.custom_minimum_size = Vector2(AppTheme.DIALOG_INPUT_MIN_WIDTH, 0)
	dialog.add_child(input)

	dialog.confirmed.connect(func():
		var protocol_name := input.text.strip_edges()
		if protocol_name.is_empty():
			protocol_name = "Untitled"
		_sync_ui_to_config()
		var saved_path := Settings.save_snapshot(protocol_name)
		if not saved_path.is_empty():
			_current_snapshot_name = protocol_name
			_update_protocol_label(saved_path)
		dialog.queue_free()
	)

	dialog.canceled.connect(func():
		dialog.queue_free()
	)

	add_child(dialog)
	dialog.popup_centered()
	input.grab_focus()


func _show_load_dialog() -> void:
	# Create a protocol selection dialog
	var dialog := AcceptDialog.new()
	dialog.title = "Load Protocol"
	dialog.ok_button_text = "Load"
	dialog.size = Vector2(AppTheme.DIALOG_WIDTH_SM, AppTheme.DIALOG_HEIGHT_SM)

	var vbox := VBoxContainer.new()
	vbox.theme_type_variation = "VBoxSM"
	dialog.add_child(vbox)

	var label := Label.new()
	label.text = "Select a protocol:"
	vbox.add_child(label)

	var protocol_list := ItemList.new()
	protocol_list.custom_minimum_size = Vector2(AppTheme.DIALOG_LIST_MIN_WIDTH, AppTheme.DIALOG_LIST_MIN_HEIGHT)
	protocol_list.select_mode = ItemList.SELECT_SINGLE
	vbox.add_child(protocol_list)

	# Populate list with available snapshots
	var snapshots := Settings.list_snapshots()
	for snap_info in snapshots:
		var display_name: String = snap_info["name"]
		protocol_list.add_item(display_name)
		protocol_list.set_item_metadata(protocol_list.item_count - 1, snap_info["name"])

	# Select first item by default
	if protocol_list.item_count > 0:
		protocol_list.select(0)

	dialog.confirmed.connect(func():
		var selected := protocol_list.get_selected_items()
		if selected.size() > 0:
			var snap_name: String = protocol_list.get_item_metadata(selected[0])
			if Settings.load_snapshot(snap_name):
				_current_snapshot_name = snap_name
				_apply_config_to_ui()
				_update_protocol_label(snap_name)
		dialog.queue_free()
	)

	dialog.canceled.connect(func():
		dialog.queue_free()
	)

	# Handle double-click to load
	protocol_list.item_activated.connect(func(index: int):
		var snap_name: String = protocol_list.get_item_metadata(index)
		if Settings.load_snapshot(snap_name):
			_current_snapshot_name = snap_name
			_apply_config_to_ui()
			_update_protocol_label(snap_name)
		dialog.queue_free()
	)

	add_child(dialog)
	dialog.popup_centered()


# -----------------------------------------------------------------------------
# Config Sync
# -----------------------------------------------------------------------------

func _sync_ui_to_config() -> void:
	## Sync all UI values to Settings (SSoT)
	var carrier := _composition_card.get_carrier()
	var envelope := _composition_card.get_envelope()
	var space := _composition_card.get_space()
	var strobe := _composition_card.is_strobe_enabled()

	Settings.carrier = carrier
	Settings.envelope = envelope
	Settings.projection_type = space
	Settings.strobe_enabled = strobe
	Settings.stimulus_type = _get_renderer_type_id(envelope)

	# Merge in parameter card values
	var params := _parameters_card.get_parameters()
	for param_name in params:
		Settings.set_stimulus_param(param_name, params[param_name])

	# Map composition to renderer params
	_map_params_to_renderer(carrier, strobe)

	# Timing
	var timing := _timing_card.get_timing()
	Settings.baseline_start_sec = timing["baseline_start_sec"]
	Settings.baseline_end_sec = timing["baseline_end_sec"]
	Settings.inter_stimulus_sec = timing["inter_stimulus_sec"]
	Settings.inter_direction_sec = timing["inter_direction_sec"]

	# Presentation
	var selected_conditions := _sequence_card.get_conditions()
	Settings.conditions = selected_conditions
	Settings.repetitions = _sequence_card.get_repetitions()
	Settings.order = _card_order_to_config_order(_sequence_card.get_condition_order())
	Settings.structure = _sequence_card.get_structure()

	# Geometry
	var geom := _geometry_card.get_geometry()
	if geom:
		Settings.viewing_distance_cm = geom.viewing_distance_cm
		Settings.horizontal_offset_deg = geom.center_azimuth_deg
		Settings.vertical_offset_deg = geom.center_elevation_deg
		Settings.projection_type = space


func _update_preview() -> void:
	## Refresh the preview from current Config values
	_preview_controller.refresh()


func _get_renderer_type_id(envelope: int) -> String:
	## Get renderer type ID from envelope
	match envelope:
		Envelopes.Type.NONE:
			return "full_field"
		Envelopes.Type.BAR:
			return "drifting_bar"
		Envelopes.Type.WEDGE:
			return "rotating_wedge"
		Envelopes.Type.RING:
			return "expanding_ring"
	return "drifting_bar"


func _map_params_to_renderer(carrier: int, strobe: bool) -> void:
	## Map composition state to renderer-specific params
	# Pattern parameter for bar renderer
	match carrier:
		Carriers.Type.SOLID:
			Settings.set_stimulus_param("pattern", "solid")
		Carriers.Type.CHECKERBOARD:
			Settings.set_stimulus_param("pattern", "checkerboard")

	Settings.set_stimulus_param("strobe_enabled", strobe)


# -----------------------------------------------------------------------------
# Summary
# -----------------------------------------------------------------------------

func _update_summary() -> void:
	var sweep_duration := Settings.sweep_duration_sec

	var num_conditions := Settings.conditions.size()
	if num_conditions == 0:
		num_conditions = 1  # Full-field has implicit single condition
	var reps := Settings.repetitions
	var sweeps := num_conditions * reps

	var total_sec := Settings.total_duration_sec

	if _sweep_duration_row:
		_sweep_duration_row.set_value("%.1fs" % sweep_duration)

	if _total_sweeps_row:
		_total_sweeps_row.set_value(str(sweeps))

	if _total_duration_row:
		var minutes := int(total_sec / 60)
		var seconds := int(total_sec) % 60
		_total_duration_row.set_value("%d:%02d" % [minutes, seconds])


func _validate() -> void:
	var valid := _sequence_card.has_valid_selection()
	validation_changed.emit(valid)
