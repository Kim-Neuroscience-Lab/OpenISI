extends BaseScreen
## Run screen: Active acquisition with stimulus presentation.
##
## UI-only layer that:
## - Builds and manages card layout
## - Connects coordinator signals to card update methods
## - Routes user actions (start/stop/continue) to coordinator
## - Updates camera preview from daemon
##
## All domain logic delegated to RunController.


# -----------------------------------------------------------------------------
# Card Components
# -----------------------------------------------------------------------------

var _status_card: AcquisitionStatusCard = null
var _metrics_card: MetricsCard = null
var _camera_card: CameraPreviewCard = null
var _stimulus_card: StimulusPreviewCard = null


# -----------------------------------------------------------------------------
# Domain Coordinator
# -----------------------------------------------------------------------------

var _coordinator: RunController = null


# -----------------------------------------------------------------------------
# Camera Preview State
# -----------------------------------------------------------------------------

var _last_frame_count: int = 0


# -----------------------------------------------------------------------------
# Lifecycle
# -----------------------------------------------------------------------------

func _ready() -> void:
	super._ready()


func _process(_delta: float) -> void:
	if _coordinator and _coordinator.is_running():
		_coordinator.update(_delta)
	_update_camera_preview()


func _exit_tree() -> void:
	if _coordinator:
		_coordinator.cleanup()


# -----------------------------------------------------------------------------
# BaseScreen Overrides
# -----------------------------------------------------------------------------

func _load_state() -> void:
	_load_config()


func _load_config() -> void:
	print("Run Screen: Initializing acquisition coordinator")
	_coordinator = RunController.new()
	_connect_coordinator_signals()


func _build_ui() -> void:
	# ScrollContainer for content that may exceed available space
	var scroll := SmoothScrollContainer.new()
	scroll.name = "ScrollContainer"
	scroll.set_anchors_preset(Control.PRESET_FULL_RECT)
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	scroll.vertical_scroll_mode = ScrollContainer.SCROLL_MODE_AUTO
	scroll.scrollbar_vertical_inset = AppTheme.SCROLL_FADE_HEIGHT
	add_child(scroll)

	# Inner margin for content positioning within scroll area
	var inner_margin := MarginContainer.new()
	inner_margin.name = "InnerMargin"
	inner_margin.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	inner_margin.size_flags_vertical = Control.SIZE_EXPAND_FILL
	inner_margin.theme_type_variation = "MarginScreenContent"
	scroll.add_child(inner_margin)

	# Main horizontal layout: Camera on left, other content on right
	var hbox := HBoxContainer.new()
	hbox.name = "MainLayout"
	hbox.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	hbox.size_flags_vertical = Control.SIZE_EXPAND_FILL
	hbox.theme_type_variation = "HBox2XL"
	inner_margin.add_child(hbox)

	# Left: Camera preview card
	_camera_card = CameraPreviewCard.new()
	hbox.add_child(_camera_card)

	# Right: Status on top, then Metrics and Stimulus side by side
	var right_column := VBoxContainer.new()
	right_column.name = "RightColumn"
	right_column.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	right_column.size_flags_vertical = Control.SIZE_EXPAND_FILL
	right_column.theme_type_variation = "VBox2XL"
	hbox.add_child(right_column)

	# Top: Status card
	_status_card = AcquisitionStatusCard.new()
	right_column.add_child(_status_card)

	# Bottom: Metrics and Stimulus side by side
	var bottom_row := HBoxContainer.new()
	bottom_row.name = "BottomRow"
	bottom_row.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	bottom_row.size_flags_vertical = Control.SIZE_EXPAND_FILL
	bottom_row.theme_type_variation = "HBox2XL"
	right_column.add_child(bottom_row)

	# Left: Metrics card
	_metrics_card = MetricsCard.new()
	bottom_row.add_child(_metrics_card)

	# Right: Stimulus preview card
	_stimulus_card = StimulusPreviewCard.new()
	_stimulus_card.set_aspect_ratio(_get_stimulus_aspect_ratio())
	bottom_row.add_child(_stimulus_card)


func _connect_signals() -> void:
	# Connect user action signals from metrics card
	if _metrics_card:
		_metrics_card.start_requested.connect(_on_start_requested)
		_metrics_card.stop_requested.connect(_on_stop_requested)
		_metrics_card.continue_requested.connect(_on_continue_requested)


# -----------------------------------------------------------------------------
# Coordinator Signal Connections
# -----------------------------------------------------------------------------

func _connect_coordinator_signals() -> void:
	if not _coordinator:
		return

	_coordinator.status_changed.connect(_on_status_changed)
	_coordinator.progress_updated.connect(_on_progress_updated)
	_coordinator.camera_metrics_updated.connect(_on_camera_metrics_updated)
	_coordinator.stimulus_metrics_updated.connect(_on_stimulus_metrics_updated)
	_coordinator.stimulus_preview_updated.connect(_on_stimulus_preview_updated)
	_coordinator.elapsed_updated.connect(_on_elapsed_updated)
	_coordinator.acquisition_complete.connect(_on_acquisition_complete)
	_coordinator.acquisition_stopped.connect(_on_acquisition_stopped)


# -----------------------------------------------------------------------------
# Coordinator Signal Handlers
# -----------------------------------------------------------------------------

func _on_status_changed(text: String, level: String, pulsing: bool) -> void:
	if _status_card:
		_status_card.set_status(text, level, pulsing)


func _on_progress_updated(percent: float, sweep_current: int, sweep_total: int, direction: String, frame_count: int) -> void:
	if _status_card:
		_status_card.update_progress(percent)
		_status_card.update_sweep(sweep_current, sweep_total, direction)
		_status_card.set_frame_count(frame_count)


func _on_camera_metrics_updated(stats: Dictionary) -> void:
	if _metrics_card:
		_metrics_card.update_camera_metrics(stats)


func _on_stimulus_metrics_updated(stats: Dictionary) -> void:
	if _metrics_card:
		_metrics_card.update_stimulus_metrics(stats)


func _on_stimulus_preview_updated(condition: String, sweep_current: int, sweep_total: int, state: String, progress: float) -> void:
	if _stimulus_card:
		_stimulus_card.update_metrics(condition, sweep_current, sweep_total, state, progress)


func _on_elapsed_updated(elapsed_string: String, storage_string: String) -> void:
	if _metrics_card:
		_metrics_card.update_elapsed(elapsed_string)
		_metrics_card.update_storage(storage_string)


func _on_acquisition_complete() -> void:
	if _status_card:
		_status_card.update_progress(100.0)
	if _metrics_card:
		_metrics_card.set_complete()


func _on_acquisition_stopped() -> void:
	if _metrics_card:
		_metrics_card.set_stopped()


# -----------------------------------------------------------------------------
# User Action Handlers
# -----------------------------------------------------------------------------

func _on_start_requested() -> void:
	if _coordinator:
		_coordinator.start()
	if _metrics_card:
		_metrics_card.set_running(true)


func _on_stop_requested() -> void:
	if _coordinator:
		_coordinator.stop()


func _on_continue_requested() -> void:
	request_next_screen.emit()


# -----------------------------------------------------------------------------
# Camera Preview
# -----------------------------------------------------------------------------

func _update_camera_preview() -> void:
	if not CameraClient.is_daemon_connected():
		return

	var frame_count := CameraClient.get_frame_count()
	if frame_count == _last_frame_count:
		return

	_last_frame_count = frame_count

	var frame_data := CameraClient.get_frame()
	if frame_data.is_empty():
		return

	var width: int = Session.camera_width_px
	var height: int = Session.camera_height_px

	if _camera_card:
		_camera_card.update_frame(frame_data, width, height)


# -----------------------------------------------------------------------------
# Utilities
# -----------------------------------------------------------------------------

func _get_stimulus_aspect_ratio() -> float:
	assert(Session.has_selected_display(), "RunScreen: No display selected")
	var width_cm := Session.display_width_cm
	var height_cm := Session.display_height_cm
	assert(height_cm > 0, "RunScreen: display_height_cm must be > 0")
	return width_cm / height_cm


## Get the camera dataset (for testing/external access)
func get_camera_dataset() -> CameraDataset:
	if _coordinator:
		return _coordinator.get_camera_dataset()
	return null
