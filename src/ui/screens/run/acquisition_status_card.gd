## AcquisitionStatusCard - Acquisition status and progress display
##
## Shows status pill, sweep info, direction, frame count, and progress bar.
class_name AcquisitionStatusCard
extends MarginContainer


# UI references
var _card: Control = null
var _status_pill: StatusPill = null
var _progress_bar: ProgressBar = null
var _frame_count_label: Label = null
var _current_sweep_label: Label = null
var _direction_label: Label = null


func _ready() -> void:
	_build_ui()


func _build_ui() -> void:
	size_flags_horizontal = Control.SIZE_EXPAND_FILL

	_card = Card.new()
	_card.title = "Acquisition Status"
	add_child(_card)

	var content := VBoxContainer.new()
	content.theme_type_variation = "VBoxMD"
	_card.get_content_slot().add_child(content)

	# Top row: Status pill, sweep info, direction, frame count
	var status_row := HBoxContainer.new()
	status_row.theme_type_variation = "HBoxLG"
	status_row.alignment = BoxContainer.ALIGNMENT_CENTER
	content.add_child(status_row)

	# Status pill
	_status_pill = StatusPill.new()
	_status_pill.status = "info"
	_status_pill.text = "Ready"
	_status_pill.pulsing = false
	status_row.add_child(_status_pill)

	# Sweep count
	_current_sweep_label = Label.new()
	_current_sweep_label.text = "Sweep 0 / 0"
	_current_sweep_label.theme_type_variation = "LabelMono"
	status_row.add_child(_current_sweep_label)

	# Direction
	_direction_label = Label.new()
	_direction_label.text = ""
	_direction_label.theme_type_variation = "LabelMono"
	status_row.add_child(_direction_label)

	# Spacer
	var spacer := Control.new()
	spacer.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	status_row.add_child(spacer)

	# Frame count
	_frame_count_label = Label.new()
	_frame_count_label.text = "0 frames"
	_frame_count_label.theme_type_variation = "LabelHeading"
	status_row.add_child(_frame_count_label)

	# Progress bar
	_progress_bar = ProgressBar.new()
	_progress_bar.name = "ProgressBar"
	_progress_bar.custom_minimum_size = Vector2(0, AppTheme.PROGRESS_BAR_HEIGHT)
	_progress_bar.value = 0
	_progress_bar.show_percentage = false
	content.add_child(_progress_bar)


# -----------------------------------------------------------------------------
# Public API
# -----------------------------------------------------------------------------

## Update progress percentage (0-100)
func update_progress(percent: float) -> void:
	if _progress_bar:
		_progress_bar.value = percent


## Update sweep info
func update_sweep(current: int, total: int, direction: String) -> void:
	if _current_sweep_label:
		_current_sweep_label.text = "Sweep %d / %d" % [current, total]

	if _direction_label:
		var dir_name := DirectionSystem.get_display_name(direction) if direction else ""
		_direction_label.text = dir_name if dir_name != "N/A" else ""


## Set status pill state
## @param status_text Display text for the pill
## @param level Status level: "info", "success", "warning", "error"
## @param pulsing Whether the pill should pulse
func set_status(status_text: String, level: String = "info", pulsing: bool = false) -> void:
	if _status_pill:
		_status_pill.text = status_text
		_status_pill.status = level
		_status_pill.pulsing = pulsing


## Set frame count
func set_frame_count(count: int) -> void:
	if _frame_count_label:
		_frame_count_label.text = "%s frames" % FormatUtils.format_number(count)


## Reset to initial state
func reset() -> void:
	set_status("Ready", "info", false)
	update_sweep(0, 0, "")
	set_frame_count(0)
	update_progress(0.0)
