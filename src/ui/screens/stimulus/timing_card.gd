class_name TimingCard
extends MarginContainer
## TimingCard - Timing configuration UI
##
## Handles timing configuration including baseline periods,
## inter-stimulus intervals, and inter-direction intervals.

signal timing_changed(timing_dict: Dictionary)

var _card: Control = null
var _baseline_start_input: StyledSpinBox = null
var _baseline_end_input: StyledSpinBox = null
var _inter_stimulus_input: StyledSpinBox = null
var _inter_direction_input: StyledSpinBox = null


func _ready() -> void:
	_build_ui()
	_connect_signals()


func _build_ui() -> void:
	_card = Card.new()
	_card.title = "Timing"
	add_child(_card)

	var content := VBoxContainer.new()
	content.theme_type_variation = "VBoxMD"
	_card.get_content_slot().add_child(content)

	# Baseline start row
	var baseline_start_row := HBoxContainer.new()
	baseline_start_row.theme_type_variation = "HBoxSM"
	content.add_child(baseline_start_row)

	var baseline_start_label := Label.new()
	baseline_start_label.text = "Baseline Start"
	baseline_start_label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_MD
	baseline_start_row.add_child(baseline_start_label)

	var bs_contract := Settings.lookup_param_contract("baseline_start_sec")
	_baseline_start_input = StyledSpinBox.new()
	_baseline_start_input.min_value = bs_contract["min"]
	_baseline_start_input.max_value = bs_contract["max"]
	_baseline_start_input.step = bs_contract["step"]
	_baseline_start_input.value = Settings.baseline_start_sec
	_baseline_start_input.suffix = " s"
	_baseline_start_input.custom_minimum_size = Vector2(AppTheme.INPUT_SPINBOX_WIDTH, AppTheme.INPUT_HEIGHT)
	baseline_start_row.add_child(_baseline_start_input)

	# Baseline end row
	var baseline_end_row := HBoxContainer.new()
	baseline_end_row.theme_type_variation = "HBoxSM"
	content.add_child(baseline_end_row)

	var baseline_end_label := Label.new()
	baseline_end_label.text = "Baseline End"
	baseline_end_label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_MD
	baseline_end_row.add_child(baseline_end_label)

	var be_contract := Settings.lookup_param_contract("baseline_end_sec")
	_baseline_end_input = StyledSpinBox.new()
	_baseline_end_input.min_value = be_contract["min"]
	_baseline_end_input.max_value = be_contract["max"]
	_baseline_end_input.step = be_contract["step"]
	_baseline_end_input.value = Settings.baseline_end_sec
	_baseline_end_input.suffix = " s"
	_baseline_end_input.custom_minimum_size = Vector2(AppTheme.INPUT_SPINBOX_WIDTH, AppTheme.INPUT_HEIGHT)
	baseline_end_row.add_child(_baseline_end_input)

	# Divider
	var divider := Divider.new()
	divider.margin = 4
	content.add_child(divider)

	# Inter-stimulus interval row
	var isi_row := HBoxContainer.new()
	isi_row.theme_type_variation = "HBoxSM"
	content.add_child(isi_row)

	var isi_label := Label.new()
	isi_label.text = "Inter-Stimulus"
	isi_label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_MD
	isi_row.add_child(isi_label)

	var is_contract := Settings.lookup_param_contract("inter_stimulus_sec")
	_inter_stimulus_input = StyledSpinBox.new()
	_inter_stimulus_input.min_value = is_contract["min"]
	_inter_stimulus_input.max_value = is_contract["max"]
	_inter_stimulus_input.step = is_contract["step"]
	_inter_stimulus_input.value = Settings.inter_stimulus_sec
	_inter_stimulus_input.suffix = " s"
	_inter_stimulus_input.custom_minimum_size = Vector2(AppTheme.INPUT_SPINBOX_WIDTH, AppTheme.INPUT_HEIGHT)
	isi_row.add_child(_inter_stimulus_input)

	var isi_hint := Label.new()
	isi_hint.text = "between sweeps"
	isi_hint.theme_type_variation = "LabelSmallDim"
	isi_row.add_child(isi_hint)

	# Inter-direction interval row
	var idi_row := HBoxContainer.new()
	idi_row.theme_type_variation = "HBoxSM"
	content.add_child(idi_row)

	var idi_label := Label.new()
	idi_label.text = "Inter-Direction"
	idi_label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_MD
	idi_row.add_child(idi_label)

	var id_contract := Settings.lookup_param_contract("inter_direction_sec")
	_inter_direction_input = StyledSpinBox.new()
	_inter_direction_input.min_value = id_contract["min"]
	_inter_direction_input.max_value = id_contract["max"]
	_inter_direction_input.step = id_contract["step"]
	_inter_direction_input.value = Settings.inter_direction_sec
	_inter_direction_input.suffix = " s"
	_inter_direction_input.custom_minimum_size = Vector2(AppTheme.INPUT_SPINBOX_WIDTH, AppTheme.INPUT_HEIGHT)
	idi_row.add_child(_inter_direction_input)

	var idi_hint := Label.new()
	idi_hint.text = "between directions"
	idi_hint.theme_type_variation = "LabelSmallDim"
	idi_row.add_child(idi_hint)


func _connect_signals() -> void:
	_baseline_start_input.value_changed.connect(_on_timing_value_changed)
	_baseline_end_input.value_changed.connect(_on_timing_value_changed)
	_inter_stimulus_input.value_changed.connect(_on_timing_value_changed)
	_inter_direction_input.value_changed.connect(_on_timing_value_changed)


func _on_timing_value_changed(_value: float) -> void:
	timing_changed.emit(get_timing())


## Get the current timing configuration as a Dictionary
func get_timing() -> Dictionary:
	return {
		"paradigm": Settings.paradigm,
		"baseline_start_sec": _baseline_start_input.value,
		"baseline_end_sec": _baseline_end_input.value,
		"inter_stimulus_sec": _inter_stimulus_input.value,
		"inter_direction_sec": _inter_direction_input.value,
	}


## Set timing configuration from a Dictionary
func set_timing(timing: Dictionary) -> void:
	# Temporarily disconnect signals to avoid triggering change events
	_baseline_start_input.value_changed.disconnect(_on_timing_value_changed)
	_baseline_end_input.value_changed.disconnect(_on_timing_value_changed)
	_inter_stimulus_input.value_changed.disconnect(_on_timing_value_changed)
	_inter_direction_input.value_changed.disconnect(_on_timing_value_changed)

	_baseline_start_input.value = timing["baseline_start_sec"]
	_baseline_end_input.value = timing["baseline_end_sec"]
	_inter_stimulus_input.value = timing["inter_stimulus_sec"]
	_inter_direction_input.value = timing["inter_direction_sec"]

	# Reconnect signals
	_baseline_start_input.value_changed.connect(_on_timing_value_changed)
	_baseline_end_input.value_changed.connect(_on_timing_value_changed)
	_inter_stimulus_input.value_changed.connect(_on_timing_value_changed)
	_inter_direction_input.value_changed.connect(_on_timing_value_changed)


## Get baseline start value
func get_baseline_start() -> float:
	return _baseline_start_input.value


## Get baseline end value
func get_baseline_end() -> float:
	return _baseline_end_input.value


## Get inter-stimulus interval
func get_inter_stimulus() -> float:
	return _inter_stimulus_input.value


## Get inter-direction interval
func get_inter_direction() -> float:
	return _inter_direction_input.value
