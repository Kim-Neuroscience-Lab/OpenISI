class_name SequenceCard
extends MarginContainer
## SequenceCard - Condition selection and ordering UI
##
## Handles condition selection, ordering, repetitions, and sequence structure.
## Provides a dual-list interface for selecting and ordering conditions.

## Condition order enum
enum ConditionOrder {
	SEQUENTIAL,
	INTERLEAVED,
	RANDOMIZED,
}

signal sequence_changed(conditions: Array, repetitions: int, order: ConditionOrder)

var _card: Control = null
var _available_conditions_container: VBoxContainer = null
var _selected_conditions_container: VBoxContainer = null
var _selected_conditions_label: Label = null
var _selected_conditions: Array[String] = []
var _available_conditions: Array[String] = []
var _repetitions_input: StyledSpinBox = null
var _structure_group: ButtonGroup = null
var _structure_buttons: Dictionary = {}  # structure_name -> Button
var _sequence_preview_label: Label = null
var _total_label: Label = null
var _current_structure: String = "blocked"


func _ready() -> void:
	_build_ui()
	_connect_signals()


func _build_ui() -> void:
	_card = Card.new()
	_card.title = "Sequence"
	add_child(_card)

	var content := VBoxContainer.new()
	content.theme_type_variation = "VBoxMD"
	_card.get_content_slot().add_child(content)

	# Structure section header
	var struct_header := SectionHeader.new()
	struct_header.title = "STRUCTURE"
	content.add_child(struct_header)

	# Structure radio buttons
	_structure_group = ButtonGroup.new()

	var structures := [
		["interleaved", "Interleaved", "cycle through all conditions"],
		["blocked", "Blocked", "all reps of each condition"],
		["shuffled", "Shuffled", "fully random order"],
		["shuffled_blocks", "Shuffled Blocks", "blocks in random order"],
	]

	for struct_info in structures:
		var struct_row := HBoxContainer.new()
		struct_row.theme_type_variation = "HBoxSM"
		content.add_child(struct_row)

		var radio := CheckBox.new()
		radio.text = struct_info[1]
		radio.button_group = _structure_group
		radio.button_pressed = struct_info[0] == "blocked"  # Default to blocked
		radio.custom_minimum_size.x = AppTheme.RADIO_BUTTON_WIDTH
		struct_row.add_child(radio)
		_structure_buttons[struct_info[0]] = radio

		var hint := Label.new()
		hint.text = struct_info[2]
		hint.theme_type_variation = "LabelSmallDim"
		struct_row.add_child(hint)

	# Divider
	var divider1 := Divider.new()
	divider1.margin = 4
	content.add_child(divider1)

	# Conditions section header
	var cond_header := SectionHeader.new()
	cond_header.title = "CONDITIONS"
	content.add_child(cond_header)

	# Conditions layout: Available | Selected
	var conditions_layout := HBoxContainer.new()
	conditions_layout.theme_type_variation = "HBoxLG"
	content.add_child(conditions_layout)

	# Available conditions column
	var available_col := VBoxContainer.new()
	available_col.theme_type_variation = "VBoxXS"
	conditions_layout.add_child(available_col)

	var available_label := Label.new()
	available_label.text = "Available:"
	available_label.theme_type_variation = "LabelSmall"
	available_col.add_child(available_label)

	_available_conditions_container = VBoxContainer.new()
	_available_conditions_container.theme_type_variation = "VBoxXS"
	available_col.add_child(_available_conditions_container)

	# Selected conditions column
	var selected_col := VBoxContainer.new()
	selected_col.theme_type_variation = "VBoxXS"
	selected_col.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	conditions_layout.add_child(selected_col)

	_selected_conditions_label = Label.new()
	_selected_conditions_label.text = "Selected (in order):"
	_selected_conditions_label.theme_type_variation = "LabelSmall"
	selected_col.add_child(_selected_conditions_label)

	_selected_conditions_container = VBoxContainer.new()
	_selected_conditions_container.theme_type_variation = "VBoxXS"
	selected_col.add_child(_selected_conditions_container)

	# Repetitions row
	var reps_row := HBoxContainer.new()
	reps_row.theme_type_variation = "HBoxSM"
	content.add_child(reps_row)

	var reps_label := Label.new()
	reps_label.text = "Repetitions"
	reps_label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_MD
	reps_row.add_child(reps_label)

	var reps_contract := Settings.lookup_param_contract("repetitions")
	_repetitions_input = StyledSpinBox.new()
	_repetitions_input.min_value = reps_contract["min"]
	_repetitions_input.max_value = reps_contract["max"]
	_repetitions_input.step = reps_contract["step"]
	_repetitions_input.value = Settings.repetitions
	_repetitions_input.custom_minimum_size = Vector2(AppTheme.INPUT_SPINBOX_WIDTH, AppTheme.INPUT_HEIGHT)
	reps_row.add_child(_repetitions_input)

	var reps_hint := Label.new()
	reps_hint.text = "per condition"
	reps_hint.theme_type_variation = "LabelSmallDim"
	reps_row.add_child(reps_hint)

	# Divider
	var divider2 := Divider.new()
	divider2.margin = 4
	content.add_child(divider2)

	# Preview section
	var preview_header := SectionHeader.new()
	preview_header.title = "PREVIEW"
	content.add_child(preview_header)

	_sequence_preview_label = Label.new()
	_sequence_preview_label.text = "TB x 10 -> BT x 10 -> LR x 10 -> RL x 10"
	_sequence_preview_label.theme_type_variation = "LabelMono"
	_sequence_preview_label.autowrap_mode = TextServer.AUTOWRAP_WORD
	content.add_child(_sequence_preview_label)

	_total_label = Label.new()
	_total_label.text = "Total: 40 sweeps"
	_total_label.theme_type_variation = "LabelSmallDim"
	content.add_child(_total_label)


func _connect_signals() -> void:
	_repetitions_input.value_changed.connect(_on_repetitions_changed)
	for struct_name in _structure_buttons:
		_structure_buttons[struct_name].pressed.connect(_on_structure_changed)


func _on_structure_changed() -> void:
	for struct_name in _structure_buttons:
		if _structure_buttons[struct_name].button_pressed:
			_current_structure = struct_name
			break

	var order_matters := _current_structure in ["interleaved", "blocked"]

	# Update label based on whether order matters
	if _selected_conditions_label:
		_selected_conditions_label.text = "Selected (in order):" if order_matters else "Selected:"

	# Rebuild selected conditions to show/hide reorder controls
	_rebuild_selected_conditions()
	_update_sequence_preview()
	_emit_change()


func _on_repetitions_changed(_value: float) -> void:
	_update_sequence_preview()
	_emit_change()


func _on_add_condition(condition: String) -> void:
	if condition not in _selected_conditions:
		_selected_conditions.append(condition)
		_rebuild_available_conditions()
		_rebuild_selected_conditions()
		_update_sequence_preview()
		_emit_change()


func _on_remove_condition(index: int) -> void:
	if index >= 0 and index < _selected_conditions.size():
		_selected_conditions.remove_at(index)
		_rebuild_available_conditions()
		_rebuild_selected_conditions()
		_update_sequence_preview()
		_emit_change()


func _on_move_condition(index: int, delta: int) -> void:
	var new_index := index + delta
	if new_index >= 0 and new_index < _selected_conditions.size():
		var temp: String = _selected_conditions[index]
		_selected_conditions[index] = _selected_conditions[new_index]
		_selected_conditions[new_index] = temp
		_rebuild_selected_conditions()
		_update_sequence_preview()
		_emit_change()


func _emit_change() -> void:
	sequence_changed.emit(_selected_conditions.duplicate(), get_repetitions(), get_condition_order())


## Rebuild the available conditions UI
func _rebuild_available_conditions() -> void:
	# Clear existing
	for child in _available_conditions_container.get_children():
		child.queue_free()

	# Create add buttons for conditions not yet selected
	for condition in _available_conditions:
		if condition not in _selected_conditions:
			var btn := Button.new()
			btn.text = "+ " + DirectionSystem.get_short_name(condition)
			btn.custom_minimum_size = Vector2(AppTheme.SEQUENCE_BTN_WIDTH, AppTheme.INPUT_HEIGHT)
			btn.pressed.connect(_on_add_condition.bind(condition))
			_available_conditions_container.add_child(btn)


## Rebuild the selected conditions UI
func _rebuild_selected_conditions() -> void:
	# Clear existing
	for child in _selected_conditions_container.get_children():
		child.queue_free()

	var order_matters := _current_structure in ["interleaved", "blocked"]

	# Create rows for each selected condition
	for i: int in range(_selected_conditions.size()):
		var condition: String = _selected_conditions[i]
		var row := HBoxContainer.new()
		row.theme_type_variation = "HBoxXS"
		_selected_conditions_container.add_child(row)

		# Index label (only if order matters)
		if order_matters:
			var idx_label := Label.new()
			idx_label.text = "%d." % (i + 1)
			idx_label.custom_minimum_size.x = AppTheme.SEQUENCE_INDEX_WIDTH
			idx_label.theme_type_variation = "LabelSmallDim"
			row.add_child(idx_label)

		# Condition name
		var name_label := Label.new()
		name_label.text = DirectionSystem.get_short_name(condition)
		name_label.custom_minimum_size.x = AppTheme.SEQUENCE_NAME_WIDTH
		row.add_child(name_label)

		# Reorder buttons (only if order matters)
		if order_matters:
			# Move up button
			var up_btn := Button.new()
			up_btn.text = "\u2191"
			up_btn.custom_minimum_size = Vector2(AppTheme.ICON_BUTTON_SIZE, AppTheme.ICON_BUTTON_SIZE)
			up_btn.disabled = i == 0
			up_btn.pressed.connect(_on_move_condition.bind(i, -1))
			row.add_child(up_btn)

			# Move down button
			var down_btn := Button.new()
			down_btn.text = "\u2193"
			down_btn.custom_minimum_size = Vector2(AppTheme.ICON_BUTTON_SIZE, AppTheme.ICON_BUTTON_SIZE)
			down_btn.disabled = i == _selected_conditions.size() - 1
			down_btn.pressed.connect(_on_move_condition.bind(i, 1))
			row.add_child(down_btn)

		# Remove button
		var remove_btn := Button.new()
		remove_btn.text = "\u00d7"
		remove_btn.custom_minimum_size = Vector2(AppTheme.ICON_BUTTON_SIZE, AppTheme.ICON_BUTTON_SIZE)
		remove_btn.pressed.connect(_on_remove_condition.bind(i))
		row.add_child(remove_btn)


## Update the sequence preview text
func _update_sequence_preview() -> void:
	if _sequence_preview_label == null:
		return

	var reps := int(_repetitions_input.value)

	# Handle NONE envelope (full-field, no directions)
	if _available_conditions.is_empty():
		_sequence_preview_label.text = "Full-field x %d" % reps
		if _total_label:
			_total_label.text = "Total: %d presentations" % reps
		return

	if _selected_conditions.is_empty():
		_sequence_preview_label.text = "(no conditions selected)"
		if _total_label:
			_total_label.text = "Total: 0 sweeps"
		return
	var preview_parts: Array[String] = []

	match _current_structure:
		"interleaved":
			# Show one cycle, then indicate repetition
			var cycle_parts: Array[String] = []
			for cond in _selected_conditions:
				cycle_parts.append(DirectionSystem.get_short_name(cond))
			var cycle_str := ", ".join(cycle_parts)
			preview_parts.append("(%s) x %d" % [cycle_str, reps])
		"blocked":
			# Show each condition with its repetitions
			var block_parts: Array[String] = []
			for cond in _selected_conditions:
				block_parts.append("%s x %d" % [DirectionSystem.get_short_name(cond), reps])
			var blocks_str := ", ".join(block_parts)
			preview_parts.append("(%s)" % blocks_str)
		"shuffled":
			var cond_parts: Array[String] = []
			for cond in _selected_conditions:
				cond_parts.append(DirectionSystem.get_short_name(cond))
			var cond_str := ", ".join(cond_parts)
			preview_parts.append("{%s} x %d" % [cond_str, reps])
		"shuffled_blocks":
			var block_parts: Array[String] = []
			for cond in _selected_conditions:
				block_parts.append("%s x %d" % [DirectionSystem.get_short_name(cond), reps])
			var blocks_str := ", ".join(block_parts)
			preview_parts.append("{%s}" % blocks_str)

	_sequence_preview_label.text = " -> ".join(preview_parts)

	# Update total sweeps
	var total := _selected_conditions.size() * reps
	if _total_label:
		_total_label.text = "Total: %d sweeps" % total


## Set the available conditions based on envelope type
func set_available_conditions(conditions: Array[String]) -> void:
	_available_conditions = conditions.duplicate()

	# For NONE envelope (empty conditions), clear selected
	if _available_conditions.is_empty():
		_selected_conditions = []
		_rebuild_available_conditions()
		_rebuild_selected_conditions()
		_update_sequence_preview()
		return

	# Filter out any selected conditions that are no longer available
	var valid_selected: Array[String] = []
	for cond in _selected_conditions:
		if cond in _available_conditions:
			valid_selected.append(cond)

	# If no valid conditions remain, select all available
	if valid_selected.is_empty() and not _available_conditions.is_empty():
		_selected_conditions = _available_conditions.duplicate()
	else:
		_selected_conditions = valid_selected

	_rebuild_available_conditions()
	_rebuild_selected_conditions()
	_update_sequence_preview()


## Get the currently selected conditions
func get_conditions() -> Array[String]:
	return _selected_conditions.duplicate()


## Get the number of repetitions
func get_repetitions() -> int:
	return int(_repetitions_input.value) if _repetitions_input else 10


## Get the condition order based on current structure
func get_condition_order() -> ConditionOrder:
	match _current_structure:
		"interleaved":
			return ConditionOrder.INTERLEAVED
		"blocked":
			return ConditionOrder.SEQUENTIAL
		"shuffled", "shuffled_blocks":
			return ConditionOrder.RANDOMIZED
	return ConditionOrder.SEQUENTIAL


## Get current structure string
func get_structure() -> String:
	return _current_structure


## Set the sequence configuration
func set_sequence(conditions: Array[String], reps: int, order: ConditionOrder) -> void:
	_selected_conditions = conditions.duplicate()

	# Set repetitions without triggering change
	_repetitions_input.value_changed.disconnect(_on_repetitions_changed)
	_repetitions_input.value = reps
	_repetitions_input.value_changed.connect(_on_repetitions_changed)

	# Set structure based on order
	match order:
		ConditionOrder.INTERLEAVED:
			_current_structure = "interleaved"
		ConditionOrder.SEQUENTIAL:
			_current_structure = "blocked"
		ConditionOrder.RANDOMIZED:
			_current_structure = "shuffled"

	# Update structure buttons
	for struct_name in _structure_buttons:
		_structure_buttons[struct_name].button_pressed = struct_name == _current_structure

	_rebuild_available_conditions()
	_rebuild_selected_conditions()
	_update_sequence_preview()


## Check if selection is valid (at least one condition, or NONE envelope with no conditions)
func has_valid_selection() -> bool:
	# For NONE envelope (empty available conditions), always valid
	if _available_conditions.is_empty():
		return true
	return _selected_conditions.size() > 0
