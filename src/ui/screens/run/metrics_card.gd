## MetricsCard - Timing metrics and control display
##
## Shows elapsed time, storage, camera/stimulus metrics, and start/stop button.
class_name MetricsCard
extends MarginContainer


signal start_requested()
signal stop_requested()
signal continue_requested()


# UI references - Header metrics
var _card: Control = null
var _elapsed_row: InfoRow = null
var _storage_row: InfoRow = null

# UI references - Camera Metrics
var _cam_fps_row: InfoRow = null
var _cam_jitter_row: InfoRow = null
var _cam_inst_drift_row: InfoRow = null
var _cam_total_drift_row: InfoRow = null
var _cam_dropped_row: InfoRow = null

# UI references - Stimulus Metrics
var _stim_fps_row: InfoRow = null
var _stim_jitter_row: InfoRow = null
var _stim_inst_drift_row: InfoRow = null
var _stim_total_drift_row: InfoRow = null

# UI references - Control
var _action_button: StyledButton = null

# State
enum ButtonMode { START, STOP, CONTINUE }
var _button_mode := ButtonMode.START


func _ready() -> void:
	_build_ui()
	_connect_signals()


func _build_ui() -> void:
	size_flags_horizontal = Control.SIZE_EXPAND_FILL
	size_flags_vertical = Control.SIZE_EXPAND_FILL

	_card = Card.new()
	_card.title = "Metrics"
	_card.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_card.size_flags_vertical = Control.SIZE_EXPAND_FILL
	add_child(_card)

	# Main vertical container
	var content := VBoxContainer.new()
	content.theme_type_variation = "VBoxMD"
	_card.get_content_slot().add_child(content)

	# Elapsed + Storage row
	var top_row := HBoxContainer.new()
	top_row.theme_type_variation = "HBoxLG"
	content.add_child(top_row)

	_elapsed_row = InfoRow.new()
	_elapsed_row.label_text = "Elapsed"
	_elapsed_row.value_text = "0:00"
	_elapsed_row.mono_value = true
	top_row.add_child(_elapsed_row)

	_storage_row = InfoRow.new()
	_storage_row.label_text = "Storage"
	_storage_row.value_text = "0 MB"
	_storage_row.mono_value = true
	top_row.add_child(_storage_row)

	# Camera section label
	var cam_label := Label.new()
	cam_label.text = "Camera"
	cam_label.theme_type_variation = "LabelCaption"
	content.add_child(cam_label)

	# Camera metrics grid (2 columns)
	var cam_grid := GridContainer.new()
	cam_grid.columns = 2
	cam_grid.theme_type_variation = "GridMD"
	content.add_child(cam_grid)

	_cam_fps_row = InfoRow.new()
	_cam_fps_row.label_text = "FPS"
	_cam_fps_row.value_text = "—"
	_cam_fps_row.mono_value = true
	cam_grid.add_child(_cam_fps_row)

	_cam_jitter_row = InfoRow.new()
	_cam_jitter_row.label_text = "Jitter"
	_cam_jitter_row.value_text = "—"
	_cam_jitter_row.mono_value = true
	cam_grid.add_child(_cam_jitter_row)

	_cam_inst_drift_row = InfoRow.new()
	_cam_inst_drift_row.label_text = "Drift"
	_cam_inst_drift_row.value_text = "—"
	_cam_inst_drift_row.mono_value = true
	cam_grid.add_child(_cam_inst_drift_row)

	_cam_total_drift_row = InfoRow.new()
	_cam_total_drift_row.label_text = "Total"
	_cam_total_drift_row.value_text = "—"
	_cam_total_drift_row.mono_value = true
	cam_grid.add_child(_cam_total_drift_row)

	_cam_dropped_row = InfoRow.new()
	_cam_dropped_row.label_text = "Dropped"
	_cam_dropped_row.value_text = "0"
	_cam_dropped_row.mono_value = true
	_cam_dropped_row.status = "success"
	cam_grid.add_child(_cam_dropped_row)

	# Stimulus section label
	var stim_label := Label.new()
	stim_label.text = "Stimulus"
	stim_label.theme_type_variation = "LabelCaption"
	content.add_child(stim_label)

	# Stimulus metrics grid (2 columns)
	var stim_grid := GridContainer.new()
	stim_grid.columns = 2
	stim_grid.theme_type_variation = "GridMD"
	content.add_child(stim_grid)

	_stim_fps_row = InfoRow.new()
	_stim_fps_row.label_text = "FPS"
	_stim_fps_row.value_text = "—"
	_stim_fps_row.mono_value = true
	stim_grid.add_child(_stim_fps_row)

	_stim_jitter_row = InfoRow.new()
	_stim_jitter_row.label_text = "Jitter"
	_stim_jitter_row.value_text = "—"
	_stim_jitter_row.mono_value = true
	stim_grid.add_child(_stim_jitter_row)

	_stim_inst_drift_row = InfoRow.new()
	_stim_inst_drift_row.label_text = "Drift"
	_stim_inst_drift_row.value_text = "—"
	_stim_inst_drift_row.mono_value = true
	stim_grid.add_child(_stim_inst_drift_row)

	_stim_total_drift_row = InfoRow.new()
	_stim_total_drift_row.label_text = "Total"
	_stim_total_drift_row.value_text = "—"
	_stim_total_drift_row.mono_value = true
	stim_grid.add_child(_stim_total_drift_row)

	# Spacer to push button to bottom
	var spacer := Control.new()
	spacer.size_flags_vertical = Control.SIZE_EXPAND_FILL
	content.add_child(spacer)

	# Start/Stop button at bottom
	_action_button = StyledButton.new()
	_action_button.text = "Start Acquisition"
	_action_button.button_pressed = true  # Nightlight - required action to start
	content.add_child(_action_button)


func _connect_signals() -> void:
	if _action_button:
		_action_button.pressed.connect(_on_button_pressed)


# -----------------------------------------------------------------------------
# Public API
# -----------------------------------------------------------------------------

## Update elapsed time display
func update_elapsed(elapsed_string: String) -> void:
	if _elapsed_row:
		_elapsed_row.set_value(elapsed_string)


## Update storage display
func update_storage(storage_string: String) -> void:
	if _storage_row:
		_storage_row.set_value(storage_string)


## Update camera metrics from CameraDataset data
func update_camera_metrics(stats: Dictionary) -> void:
	# FPS
	if _cam_fps_row:
		if stats.has("fps"):
			_cam_fps_row.set_value("%.1f" % float(stats["fps"]))
		else:
			_cam_fps_row.set_value("—")

	# Jitter
	if _cam_jitter_row:
		if stats.has("jitter_ms"):
			var jitter_ms: float = float(stats["jitter_ms"])
			_cam_jitter_row.set_value("%.2f ms" % jitter_ms)
			_cam_jitter_row.status = "warning" if jitter_ms > 2.0 else "default"
		else:
			_cam_jitter_row.set_value("—")
			_cam_jitter_row.status = "default"

	# Instantaneous drift
	if _cam_inst_drift_row:
		if stats.has("drift_pct"):
			var drift_pct: float = float(stats["drift_pct"])
			_cam_inst_drift_row.set_value("%+.1f%%" % drift_pct)
			_cam_inst_drift_row.status = "warning" if absf(drift_pct) > 5.0 else "default"
		else:
			_cam_inst_drift_row.set_value("—")
			_cam_inst_drift_row.status = "default"

	# Total drift
	if _cam_total_drift_row:
		if stats.has("total_drift_frames"):
			var total_drift: float = float(stats["total_drift_frames"])
			_cam_total_drift_row.set_value("%+.0f frames" % total_drift)
			_cam_total_drift_row.status = "warning" if absf(total_drift) > 10.0 else "default"
		else:
			_cam_total_drift_row.set_value("—")
			_cam_total_drift_row.status = "default"

	# Dropped frames
	if _cam_dropped_row:
		var dropped: int = int(stats.get("dropped", 0))
		_cam_dropped_row.set_value(str(dropped))
		_cam_dropped_row.status = "warning" if dropped > 0 else "success"


## Update stimulus metrics
func update_stimulus_metrics(stats: Dictionary) -> void:
	# FPS
	if _stim_fps_row:
		if stats.has("fps"):
			_stim_fps_row.set_value("%.1f" % float(stats["fps"]))
		else:
			_stim_fps_row.set_value("—")

	# Jitter
	if _stim_jitter_row:
		if stats.has("jitter_ms"):
			var jitter_ms: float = float(stats["jitter_ms"])
			_stim_jitter_row.set_value("%.2f ms" % jitter_ms)
			_stim_jitter_row.status = "warning" if jitter_ms > 2.0 else "default"
		else:
			_stim_jitter_row.set_value("—")
			_stim_jitter_row.status = "default"

	# Instantaneous drift
	if _stim_inst_drift_row:
		if stats.has("drift_pct"):
			var drift_pct: float = float(stats["drift_pct"])
			_stim_inst_drift_row.set_value("%+.1f%%" % drift_pct)
			_stim_inst_drift_row.status = "warning" if absf(drift_pct) > 5.0 else "default"
		else:
			_stim_inst_drift_row.set_value("—")
			_stim_inst_drift_row.status = "default"

	# Total drift
	if _stim_total_drift_row:
		if stats.has("total_drift_frames"):
			var total_drift: float = float(stats["total_drift_frames"])
			_stim_total_drift_row.set_value("%+.0f frames" % total_drift)
			_stim_total_drift_row.status = "warning" if absf(total_drift) > 10.0 else "default"
		else:
			_stim_total_drift_row.set_value("—")
			_stim_total_drift_row.status = "default"


## Set button to running state (shows Stop)
func set_running(is_running: bool) -> void:
	if not _action_button:
		return

	if is_running:
		_button_mode = ButtonMode.STOP
		_action_button.text = "Stop"
		_action_button.button_pressed = false
	else:
		_button_mode = ButtonMode.START
		_action_button.text = "Start Acquisition"
		_action_button.button_pressed = true


## Set button to complete state (shows Continue)
func set_complete() -> void:
	if not _action_button:
		return

	_button_mode = ButtonMode.CONTINUE
	_action_button.text = "Continue"
	_action_button.button_pressed = true


## Set button to stopped state (shows Continue)
func set_stopped() -> void:
	if not _action_button:
		return

	_button_mode = ButtonMode.CONTINUE
	_action_button.text = "Continue"
	_action_button.button_pressed = true


## Reset all metrics to initial state
func reset() -> void:
	update_elapsed("0:00")
	update_storage("0 MB")
	update_camera_metrics({})
	update_stimulus_metrics({})
	set_running(false)


# -----------------------------------------------------------------------------
# Signal Handlers
# -----------------------------------------------------------------------------

func _on_button_pressed() -> void:
	match _button_mode:
		ButtonMode.START:
			start_requested.emit()
		ButtonMode.STOP:
			stop_requested.emit()
		ButtonMode.CONTINUE:
			continue_requested.emit()
