## MonitorCard - Stimulus display selection and validation component
##
## Handles monitor enumeration, physical dimension input, and refresh rate validation.
## Interacts directly with Session (SSoT) and DisplayValidator for operations.
## Emits signals when selection or validation state changes.
class_name MonitorCard
extends MarginContainer


signal display_selected(monitor: Dictionary)
signal validation_changed(is_valid: bool)
signal dimensions_changed(width_cm: float, height_cm: float)


# UI Controls
var _card: Control = null
var _monitor_selector: StyledOptionButton = null
var _monitor_resolution_row: InfoRow = null
var _monitor_refresh_row: InfoRow = null
var _monitor_width_input: StyledSpinBox = null
var _monitor_height_input: StyledSpinBox = null
var _monitor_warning_label: Label = null
var _monitor_dim_hint_label: Label = null
var _monitor_refresh_warning: Label = null

# Local state for validation
var _is_validating: bool = false


func _ready() -> void:
	_build_ui()
	_connect_signals()
	_load_state()


func _build_ui() -> void:
	size_flags_horizontal = Control.SIZE_EXPAND_FILL

	_card = Card.new()
	_card.title = "Stimulus Display"
	_card.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	add_child(_card)

	var content := VBoxContainer.new()
	content.theme_type_variation = "VBoxMD"
	_card.get_content_slot().add_child(content)

	# Monitor selector row
	var selector_row := HBoxContainer.new()
	selector_row.theme_type_variation = "HBoxSM"
	content.add_child(selector_row)

	var selector_label := Label.new()
	selector_label.text = "Monitor"
	selector_label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_SM
	selector_row.add_child(selector_label)

	_monitor_selector = StyledOptionButton.new()
	_monitor_selector.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_monitor_selector.custom_minimum_size.y = AppTheme.INPUT_HEIGHT
	selector_row.add_child(_monitor_selector)

	# Warning label for single monitor (shown under selector when only one monitor)
	_monitor_warning_label = Label.new()
	_monitor_warning_label.text = "Single monitor - testing mode only"
	_monitor_warning_label.theme_type_variation = "LabelSmallError"
	_monitor_warning_label.visible = false
	content.add_child(_monitor_warning_label)

	# Width and Height on same row - 2-cell grid layout
	var size_row := HBoxContainer.new()
	size_row.theme_type_variation = "HBoxLG"
	content.add_child(size_row)

	# Width cell (takes 50% of row)
	var width_cell := HBoxContainer.new()
	width_cell.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	width_cell.theme_type_variation = "HBoxSM"
	size_row.add_child(width_cell)

	var width_label := Label.new()
	width_label.text = "W"
	width_label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_SM
	width_cell.add_child(width_label)

	_monitor_width_input = StyledSpinBox.new()
	_monitor_width_input.min_value = 10
	_monitor_width_input.max_value = 200
	_monitor_width_input.step = 0.5
	_monitor_width_input.suffix = " cm"
	_monitor_width_input.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_monitor_width_input.custom_minimum_size.y = AppTheme.INPUT_HEIGHT
	width_cell.add_child(_monitor_width_input)

	# Height cell (takes 50% of row)
	var height_cell := HBoxContainer.new()
	height_cell.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	height_cell.theme_type_variation = "HBoxSM"
	size_row.add_child(height_cell)

	var height_label := Label.new()
	height_label.text = "H"
	height_label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_SM
	height_cell.add_child(height_label)

	_monitor_height_input = StyledSpinBox.new()
	_monitor_height_input.min_value = 5
	_monitor_height_input.max_value = 150
	_monitor_height_input.step = 0.5
	_monitor_height_input.suffix = " cm"
	_monitor_height_input.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_monitor_height_input.custom_minimum_size.y = AppTheme.INPUT_HEIGHT
	height_cell.add_child(_monitor_height_input)

	# Hint about auto-detection
	_monitor_dim_hint_label = Label.new()
	_monitor_dim_hint_label.text = "Measure if precision required."
	_monitor_dim_hint_label.theme_type_variation = "LabelSmallDim"
	content.add_child(_monitor_dim_hint_label)

	# Divider
	var divider := Divider.new()
	divider.margin = 4
	content.add_child(divider)

	# Specs
	_monitor_resolution_row = InfoRow.new()
	_monitor_resolution_row.label_text = "Resolution"
	_monitor_resolution_row.value_text = "--"
	_monitor_resolution_row.mono_value = true
	content.add_child(_monitor_resolution_row)

	_monitor_refresh_row = InfoRow.new()
	_monitor_refresh_row.label_text = "Refresh Rate"
	_monitor_refresh_row.value_text = "--"
	_monitor_refresh_row.status = "warning"
	_monitor_refresh_row.mono_value = true
	content.add_child(_monitor_refresh_row)

	# Hint label for refresh rate info (mismatch warnings, validation status)
	_monitor_refresh_warning = Label.new()
	_monitor_refresh_warning.theme_type_variation = "LabelSmallDim"
	_monitor_refresh_warning.visible = false
	content.add_child(_monitor_refresh_warning)


func _connect_signals() -> void:
	# HardwareManager signals
	HardwareManager.monitors_enumerated.connect(_on_monitors_enumerated)

	# DisplayValidator signals (direct connection to SSoT)
	DisplayValidator.validation_started.connect(_on_validation_started)
	DisplayValidator.validation_completed.connect(_on_validation_completed)
	DisplayValidator.validation_failed.connect(_on_validation_failed)

	# UI signals
	if _monitor_selector:
		_monitor_selector.item_selected.connect(_on_monitor_selected)

	if _monitor_width_input:
		_monitor_width_input.value_changed.connect(_on_monitor_width_changed)

	if _monitor_height_input:
		_monitor_height_input.value_changed.connect(_on_monitor_height_changed)


func _load_state() -> void:
	# Load physical dimensions if a display has been selected
	if Session.has_selected_display():
		if _monitor_width_input:
			_monitor_width_input.value = Session.display_width_cm
		if _monitor_height_input:
			_monitor_height_input.value = Session.display_height_cm

	# Update monitor info
	if HardwareManager.has_enumerated_monitors():
		var monitors := HardwareManager.get_detected_monitors()
		_populate_monitor_selector(monitors)
		_update_selected_monitor_info()
	else:
		_enumerate_monitors()


# -----------------------------------------------------------------------------
# Public API
# -----------------------------------------------------------------------------

## Get the currently selected monitor dictionary
func get_selected_monitor() -> Dictionary:
	if not _monitor_selector or _monitor_selector.item_count == 0:
		return {}

	var selected_idx := _monitor_selector.get_selected_id()
	return HardwareManager.get_monitor_at_index(selected_idx)


## Trigger display refresh rate validation for current selection
func trigger_validation() -> void:
	if not _monitor_selector or _monitor_selector.item_count == 0:
		return

	var selected_idx := _monitor_selector.get_selected_id()
	_trigger_display_validation(selected_idx)


## Check if display is validated
func is_validated() -> bool:
	return Session.display_refresh_validated


## Check if validation is in progress
func is_validating() -> bool:
	return _is_validating or DisplayValidator.is_validating()


## Refresh monitor list
func refresh_monitors() -> void:
	_enumerate_monitors()


# -----------------------------------------------------------------------------
# Internal Methods
# -----------------------------------------------------------------------------

func _enumerate_monitors() -> void:
	var monitors := HardwareManager.enumerate_monitors()
	_populate_monitor_selector(monitors)
	_update_selected_monitor_info()


func _populate_monitor_selector(monitors: Array[Dictionary]) -> void:
	if not _monitor_selector:
		return

	_monitor_selector.clear()

	for monitor in monitors:
		var label := HardwareManager.get_monitor_display_label(monitor)
		var idx: int = int(monitor["index"])
		_monitor_selector.add_item(label, idx)

	# Show warning if only one monitor
	if _monitor_warning_label:
		_monitor_warning_label.visible = HardwareManager.is_single_monitor()

	# Select the configured monitor (or first non-primary, or primary if only one)
	var target_idx := HardwareManager.get_stimulus_monitor_index()
	if Session.has_selected_display():
		var configured_idx := Session.display_index
		if configured_idx < monitors.size():
			target_idx = configured_idx

	# Find and select the item with matching index
	for i in range(_monitor_selector.item_count):
		if _monitor_selector.get_item_id(i) == target_idx:
			_monitor_selector.select(i)
			# Only trigger validation if not already validated for this display
			if not Session.display_refresh_validated or Session.display_index != target_idx:
				_trigger_display_validation(target_idx)
			else:
				# Restore validated state in UI
				_update_validation_ui_from_session()
			break


func _update_selected_monitor_info() -> void:
	if not _monitor_selector or _monitor_selector.item_count == 0:
		if _monitor_resolution_row:
			_monitor_resolution_row.set_value("No monitors detected")
		if _monitor_refresh_row:
			_monitor_refresh_row.set_value("--")
		return

	var selected_idx := _monitor_selector.get_selected_id()
	var monitor := HardwareManager.get_monitor_at_index(selected_idx)

	if monitor.is_empty():
		return

	# Update Session (SSoT for runtime state)
	Session.set_selected_display(monitor)

	var resolution: Vector2i = monitor["size"]
	var refresh: float = float(monitor["refresh"])
	var dpi: int = int(monitor["dpi"])
	var width_cm: float = float(monitor["width_cm"])
	var height_cm: float = float(monitor["height_cm"])

	if _monitor_resolution_row:
		var res_text := "%d x %d px" % [resolution.x, resolution.y]
		if dpi > 0:
			res_text += " @ %d DPI" % dpi
		_monitor_resolution_row.set_value(res_text)

	if _monitor_refresh_row:
		_monitor_refresh_row.set_value("%.0f Hz" % refresh)

	# Auto-populate physical dimensions if detected
	var physical_source: String = str(monitor["physical_source"])
	if width_cm > 0.0 and height_cm > 0.0:
		if _monitor_width_input:
			_monitor_width_input.value = snapped(width_cm, 0.5)
		if _monitor_height_input:
			_monitor_height_input.value = snapped(height_cm, 0.5)

		# Update Session dimensions
		Session.display_width_cm = snapped(width_cm, 0.5)
		Session.display_height_cm = snapped(height_cm, 0.5)

	# Update hint text based on detection source - use theme variations
	if _monitor_dim_hint_label:
		match physical_source:
			"edid":
				_monitor_dim_hint_label.text = "From EDID. Verify if precision required."
				_monitor_dim_hint_label.theme_type_variation = "LabelSmallDim"
			_:
				_monitor_dim_hint_label.text = "EDID unavailable. Measure and enter dimensions."
				_monitor_dim_hint_label.theme_type_variation = "LabelSmallError"

	# Emit selection signal
	display_selected.emit(monitor)


func _update_validation_ui_from_session() -> void:
	## Restore validation UI state from Session without re-running validation.
	if _monitor_refresh_row:
		_monitor_refresh_row.set_value("%.1f Hz" % Session.display_measured_refresh_hz)
		_monitor_refresh_row.status = "success"
	if _monitor_refresh_warning:
		_monitor_refresh_warning.visible = false
	validation_changed.emit(true)


func _trigger_display_validation(screen_idx: int) -> void:
	# Update UI to show validating state
	if _monitor_refresh_row:
		_monitor_refresh_row.status = "warning"
	if _monitor_refresh_warning:
		_monitor_refresh_warning.text = "Validating..."
		_monitor_refresh_warning.theme_type_variation = "LabelSmallDim"
		_monitor_refresh_warning.visible = true

	# Clear previous validation in Session
	Session.clear_display_validation()

	# Emit validation state change
	validation_changed.emit(false)

	# Start validation via DisplayValidator (SSoT for validation)
	_is_validating = true
	DisplayValidator.validate_display(screen_idx)


# -----------------------------------------------------------------------------
# Signal Handlers - HardwareManager
# -----------------------------------------------------------------------------

func _on_monitors_enumerated(_monitors: Array[Dictionary]) -> void:
	# Monitor enumeration is synchronous, so this is called immediately
	pass


# -----------------------------------------------------------------------------
# Signal Handlers - UI
# -----------------------------------------------------------------------------

func _on_monitor_selected(index: int) -> void:
	var monitor_idx := _monitor_selector.get_item_id(index)
	var monitor := HardwareManager.get_monitor_at_index(monitor_idx)
	if not monitor.is_empty():
		Session.set_selected_display(monitor)
	_update_selected_monitor_info()
	# Trigger validation on manual selection
	_trigger_display_validation(monitor_idx)


func _on_monitor_width_changed(value: float) -> void:
	Session.display_width_cm = value
	dimensions_changed.emit(value, Session.display_height_cm)


func _on_monitor_height_changed(value: float) -> void:
	Session.display_height_cm = value
	dimensions_changed.emit(Session.display_width_cm, value)


# -----------------------------------------------------------------------------
# Signal Handlers - DisplayValidator
# -----------------------------------------------------------------------------

func _on_validation_started(_screen_idx: int) -> void:
	_is_validating = true
	if _monitor_refresh_row:
		_monitor_refresh_row.status = "warning"
	if _monitor_refresh_warning:
		_monitor_refresh_warning.text = "Validating..."
		_monitor_refresh_warning.theme_type_variation = "LabelSmallDim"
		_monitor_refresh_warning.visible = true
	validation_changed.emit(false)


func _on_validation_completed(measured_hz: float, reported_hz: float, mismatch: bool) -> void:
	_is_validating = false

	# Update Session (SSoT) with validation result
	Session.set_display_validation(measured_hz)

	# Update UI - show measured rate as the actual refresh rate
	if _monitor_refresh_row:
		_monitor_refresh_row.set_value("%.1f Hz" % measured_hz)
		_monitor_refresh_row.status = "success"

	# Show hint if mismatch between reported and measured
	if _monitor_refresh_warning:
		if mismatch:
			_monitor_refresh_warning.text = "Reported %.0f Hz, measured %.1f Hz" % [reported_hz, measured_hz]
			_monitor_refresh_warning.theme_type_variation = "LabelSmallError"
			_monitor_refresh_warning.visible = true
		else:
			_monitor_refresh_warning.visible = false

	validation_changed.emit(true)


func _on_validation_failed(reason: String) -> void:
	_is_validating = false

	# Clear validation in Session (SSoT)
	Session.clear_display_validation()

	# Update UI
	if _monitor_refresh_row:
		_monitor_refresh_row.status = "error"
	if _monitor_refresh_warning:
		_monitor_refresh_warning.text = reason
		_monitor_refresh_warning.theme_type_variation = "LabelSmallError"
		_monitor_refresh_warning.visible = true

	validation_changed.emit(false)
