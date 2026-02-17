## TimingDiagnostics - Developer tools panel for timing validation
##
## Displays real-time timing metrics during acquisition and allows
## running quick diagnostic tests to validate camera-stimulus sync.
##
## Access via: Debug menu or keyboard shortcut (Ctrl+Shift+T)
class_name TimingDiagnostics
extends Window


const StimulusWindowScene := preload("res://src/stimulus/stimulus_window.tscn")


## Status states
enum Status { IDLE, RUNNING, COMPLETE }

## Current status
var _status: Status = Status.IDLE
var _start_time: int = 0

## UI references
var _status_label: Label = null
var _duration_label: Label = null

# Camera metrics labels
var _cam_fps_label: Label = null
var _cam_fps_status: ColorRect = null
var _cam_jitter_label: Label = null
var _cam_jitter_status: ColorRect = null
var _cam_drops_label: Label = null
var _cam_drops_status: ColorRect = null
var _cam_delta_min_label: Label = null
var _cam_delta_max_label: Label = null
var _cam_delta_mean_label: Label = null
var _cam_drift_total_label: Label = null
var _cam_drift_rate_label: Label = null

# Stimulus metrics labels
var _stim_fps_label: Label = null
var _stim_fps_status: ColorRect = null
var _stim_jitter_label: Label = null
var _stim_jitter_status: ColorRect = null
var _stim_drops_label: Label = null
var _stim_drops_status: ColorRect = null
var _stim_delta_min_label: Label = null
var _stim_delta_max_label: Label = null
var _stim_delta_mean_label: Label = null
var _stim_drift_total_label: Label = null
var _stim_drift_rate_label: Label = null

# Sync metrics (from Rust analyzer)
var _sync_offset_mean_label: Label = null
var _sync_offset_max_label: Label = null
var _sync_offset_sd_label: Label = null
var _sync_offset_status: ColorRect = null
var _sync_lag_label: Label = null
var _sync_corr_label: Label = null
var _sync_align_status: ColorRect = null
var _sync_drift_label: Label = null
var _sync_drift_status: ColorRect = null

# Buttons
var _quick_test_button: Button = null
var _export_button: Button = null
var _clear_button: Button = null

# Data references
var _camera_dataset: CameraDataset = null
var _stimulus_dataset: StimulusDataset = null
var _last_report: Dictionary = {}

# Timing analyzer (Rust)
var _timing_analyzer = null

# Quick test stimulus window
var _test_stimulus_window: Window = null


func _ready() -> void:
	title = "Timing Diagnostics"
	size = AppTheme.DIAG_WINDOW_SIZE
	min_size = AppTheme.DIAG_WINDOW_MIN_SIZE

	# Try to get TimingAnalyzer from Rust extension
	if ClassDB.class_exists(&"TimingAnalyzer"):
		_timing_analyzer = ClassDB.instantiate(&"TimingAnalyzer")

	_build_ui()
	_update_status_display()


func _notification(what: int) -> void:
	if what == NOTIFICATION_WM_CLOSE_REQUEST:
		# Clean up stimulus window if test is running
		if _test_stimulus_window:
			_test_stimulus_window.queue_free()
			_test_stimulus_window = null


func _process(_delta: float) -> void:
	if _status == Status.RUNNING:
		_update_duration()
		_update_live_metrics()


func _build_ui() -> void:
	var margin := MarginContainer.new()
	margin.theme_type_variation = "MarginPanel"
	add_child(margin)

	var root := VBoxContainer.new()
	root.theme_type_variation = "VBoxMD"
	margin.add_child(root)

	# Status row
	var status_row := HBoxContainer.new()
	status_row.theme_type_variation = "HBoxLG"
	root.add_child(status_row)

	var status_box := HBoxContainer.new()
	status_box.theme_type_variation = "HBoxSM"
	status_row.add_child(status_box)

	var status_indicator := ColorRect.new()
	status_indicator.custom_minimum_size = Vector2(AppTheme.STATUS_INDICATOR_SIZE, AppTheme.STATUS_INDICATOR_SIZE)
	status_indicator.color = AppTheme.DIAG_STATUS_IDLE
	status_box.add_child(status_indicator)

	_status_label = Label.new()
	_status_label.text = "Idle"
	status_box.add_child(_status_label)

	var spacer := Control.new()
	spacer.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	status_row.add_child(spacer)

	_duration_label = Label.new()
	_duration_label.text = "Duration: --:--"
	status_row.add_child(_duration_label)

	# Separator
	root.add_child(HSeparator.new())

	# Metrics columns
	var metrics_row := HBoxContainer.new()
	metrics_row.theme_type_variation = "HBox2XL"
	root.add_child(metrics_row)

	# Camera column
	var cam_col := _create_metrics_column("CAMERA")
	metrics_row.add_child(cam_col)
	_cam_fps_label = cam_col.get_node("FPS/Value")
	_cam_fps_status = cam_col.get_node("FPS/Status")
	_cam_jitter_label = cam_col.get_node("Jitter/Value")
	_cam_jitter_status = cam_col.get_node("Jitter/Status")
	_cam_drops_label = cam_col.get_node("Drops/Value")
	_cam_drops_status = cam_col.get_node("Drops/Status")
	_cam_delta_min_label = cam_col.get_node("min/Value")
	_cam_delta_max_label = cam_col.get_node("max/Value")
	_cam_delta_mean_label = cam_col.get_node("mean/Value")
	_cam_drift_total_label = cam_col.get_node("total/Value")
	_cam_drift_rate_label = cam_col.get_node("rate/Value")

	# Separator
	metrics_row.add_child(VSeparator.new())

	# Stimulus column
	var stim_col := _create_metrics_column("STIMULUS")
	metrics_row.add_child(stim_col)
	_stim_fps_label = stim_col.get_node("FPS/Value")
	_stim_fps_status = stim_col.get_node("FPS/Status")
	_stim_jitter_label = stim_col.get_node("Jitter/Value")
	_stim_jitter_status = stim_col.get_node("Jitter/Status")
	_stim_drops_label = stim_col.get_node("Drops/Value")
	_stim_drops_status = stim_col.get_node("Drops/Status")
	_stim_delta_min_label = stim_col.get_node("min/Value")
	_stim_delta_max_label = stim_col.get_node("max/Value")
	_stim_delta_mean_label = stim_col.get_node("mean/Value")
	_stim_drift_total_label = stim_col.get_node("total/Value")
	_stim_drift_rate_label = stim_col.get_node("rate/Value")

	# Separator
	root.add_child(HSeparator.new())

	# Sync section (from Rust analyzer - framerate-agnostic sync analysis)
	var sync_header := Label.new()
	sync_header.text = "SYNC (Camera ↔ Stimulus)"
	# Uses default Label theme styling (FONT_BODY)
	root.add_child(sync_header)

	var sync_container := VBoxContainer.new()
	sync_container.theme_type_variation = "VBoxXS"
	root.add_child(sync_container)

	# Offset row: mean, max, SD
	var offset_row := HBoxContainer.new()
	offset_row.theme_type_variation = "HBoxSM"
	sync_container.add_child(offset_row)

	var offset_label := Label.new()
	offset_label.text = "Offset:"
	offset_label.custom_minimum_size.x = AppTheme.DIAG_LABEL_WIDTH_SM
	offset_row.add_child(offset_label)

	var offset_mean_box := HBoxContainer.new()
	offset_mean_box.theme_type_variation = "HBoxXS"
	offset_row.add_child(offset_mean_box)
	var offset_mean_prefix := Label.new()
	offset_mean_prefix.text = "mean"
	offset_mean_prefix.theme_type_variation = "LabelDim"
	offset_mean_box.add_child(offset_mean_prefix)
	_sync_offset_mean_label = Label.new()
	_sync_offset_mean_label.text = "—"
	_sync_offset_mean_label.custom_minimum_size.x = AppTheme.DIAG_VALUE_WIDTH_MD
	offset_mean_box.add_child(_sync_offset_mean_label)

	var offset_max_box := HBoxContainer.new()
	offset_max_box.theme_type_variation = "HBoxXS"
	offset_row.add_child(offset_max_box)
	var offset_max_prefix := Label.new()
	offset_max_prefix.text = "max"
	offset_max_prefix.theme_type_variation = "LabelDim"
	offset_max_box.add_child(offset_max_prefix)
	_sync_offset_max_label = Label.new()
	_sync_offset_max_label.text = "—"
	_sync_offset_max_label.custom_minimum_size.x = AppTheme.DIAG_VALUE_WIDTH_MD
	offset_max_box.add_child(_sync_offset_max_label)

	var offset_sd_box := HBoxContainer.new()
	offset_sd_box.theme_type_variation = "HBoxXS"
	offset_row.add_child(offset_sd_box)
	var offset_sd_prefix := Label.new()
	offset_sd_prefix.text = "SD"
	offset_sd_prefix.theme_type_variation = "LabelDim"
	offset_sd_box.add_child(offset_sd_prefix)
	_sync_offset_sd_label = Label.new()
	_sync_offset_sd_label.text = "—"
	_sync_offset_sd_label.custom_minimum_size.x = AppTheme.DIAG_VALUE_WIDTH_SM
	offset_sd_box.add_child(_sync_offset_sd_label)

	_sync_offset_status = ColorRect.new()
	_sync_offset_status.custom_minimum_size = Vector2(AppTheme.STATUS_INDICATOR_SIZE, AppTheme.STATUS_INDICATOR_SIZE)
	_sync_offset_status.color = AppTheme.DIAG_STATUS_IDLE
	offset_row.add_child(_sync_offset_status)

	# Alignment row: lag, correlation
	var align_row := HBoxContainer.new()
	align_row.theme_type_variation = "HBoxSM"
	sync_container.add_child(align_row)

	var align_label := Label.new()
	align_label.text = "Align:"
	align_label.custom_minimum_size.x = AppTheme.DIAG_LABEL_WIDTH_SM
	align_row.add_child(align_label)

	var lag_box := HBoxContainer.new()
	lag_box.theme_type_variation = "HBoxXS"
	align_row.add_child(lag_box)
	var lag_prefix := Label.new()
	lag_prefix.text = "lag"
	lag_prefix.theme_type_variation = "LabelDim"
	lag_box.add_child(lag_prefix)
	_sync_lag_label = Label.new()
	_sync_lag_label.text = "—"
	_sync_lag_label.custom_minimum_size.x = AppTheme.DIAG_VALUE_WIDTH_MD
	lag_box.add_child(_sync_lag_label)

	var corr_box := HBoxContainer.new()
	corr_box.theme_type_variation = "HBoxXS"
	align_row.add_child(corr_box)
	var corr_prefix := Label.new()
	corr_prefix.text = "corr"
	corr_prefix.theme_type_variation = "LabelDim"
	corr_box.add_child(corr_prefix)
	_sync_corr_label = Label.new()
	_sync_corr_label.text = "—"
	_sync_corr_label.custom_minimum_size.x = AppTheme.DIAG_VALUE_WIDTH_SM
	corr_box.add_child(_sync_corr_label)

	var align_spacer := Control.new()
	align_spacer.custom_minimum_size.x = AppTheme.DIAG_VALUE_WIDTH_MD
	align_row.add_child(align_spacer)

	_sync_align_status = ColorRect.new()
	_sync_align_status.custom_minimum_size = Vector2(AppTheme.STATUS_INDICATOR_SIZE, AppTheme.STATUS_INDICATOR_SIZE)
	_sync_align_status.color = AppTheme.DIAG_STATUS_IDLE
	align_row.add_child(_sync_align_status)

	# Drift row
	var drift_row := HBoxContainer.new()
	drift_row.theme_type_variation = "HBoxSM"
	sync_container.add_child(drift_row)

	var drift_label := Label.new()
	drift_label.text = "Drift:"
	drift_label.custom_minimum_size.x = AppTheme.DIAG_LABEL_WIDTH_SM
	drift_row.add_child(drift_label)

	_sync_drift_label = Label.new()
	_sync_drift_label.text = "—"
	_sync_drift_label.custom_minimum_size.x = AppTheme.DIAG_VALUE_WIDTH_LG
	drift_row.add_child(_sync_drift_label)

	var drift_spacer := Control.new()
	drift_spacer.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	drift_row.add_child(drift_spacer)

	_sync_drift_status = ColorRect.new()
	_sync_drift_status.custom_minimum_size = Vector2(AppTheme.STATUS_INDICATOR_SIZE, AppTheme.STATUS_INDICATOR_SIZE)
	_sync_drift_status.color = AppTheme.DIAG_STATUS_IDLE
	drift_row.add_child(_sync_drift_status)

	# Separator
	root.add_child(HSeparator.new())

	# Buttons row
	var button_row := HBoxContainer.new()
	button_row.theme_type_variation = "HBoxSM"
	root.add_child(button_row)

	_quick_test_button = Button.new()
	_quick_test_button.text = "Quick Test 5s"
	_quick_test_button.pressed.connect(_on_quick_test_pressed)
	button_row.add_child(_quick_test_button)

	_export_button = Button.new()
	_export_button.text = "Export Report"
	_export_button.pressed.connect(_on_export_pressed)
	button_row.add_child(_export_button)

	var btn_spacer := Control.new()
	btn_spacer.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	button_row.add_child(btn_spacer)

	_clear_button = Button.new()
	_clear_button.text = "Clear"
	_clear_button.pressed.connect(_on_clear_pressed)
	button_row.add_child(_clear_button)


func _create_metrics_column(column_title: String) -> VBoxContainer:
	var col := VBoxContainer.new()
	col.theme_type_variation = "VBoxXS"
	col.size_flags_horizontal = Control.SIZE_EXPAND_FILL

	var header := Label.new()
	header.text = column_title
	# Uses default Label theme styling (FONT_BODY)
	col.add_child(header)

	# Primary metrics with status indicators
	col.add_child(_create_metric_row("FPS", true))
	col.add_child(_create_metric_row("Jitter", true))
	col.add_child(_create_metric_row("Drops", true))

	# Delta statistics (no status indicators)
	var delta_header := Label.new()
	delta_header.text = "Delta"
	delta_header.theme_type_variation = "LabelSmallDim"
	col.add_child(delta_header)

	col.add_child(_create_metric_row("  min", false))
	col.add_child(_create_metric_row("  max", false))
	col.add_child(_create_metric_row("  mean", false))

	# Drift statistics (no status indicators)
	var drift_header := Label.new()
	drift_header.text = "Drift"
	drift_header.theme_type_variation = "LabelSmallDim"
	col.add_child(drift_header)

	col.add_child(_create_metric_row("  total", false))
	col.add_child(_create_metric_row("  rate", false))

	return col


func _create_metric_row(label_text: String, with_status: bool = true) -> HBoxContainer:
	var row := HBoxContainer.new()
	row.name = label_text.strip_edges().replace(" ", "")
	row.theme_type_variation = "HBoxSM"

	var label := Label.new()
	label.text = label_text + ":"
	label.custom_minimum_size.x = AppTheme.DIAG_LABEL_WIDTH_MD
	row.add_child(label)

	var value := Label.new()
	value.name = "Value"
	value.text = "—"
	value.custom_minimum_size.x = AppTheme.DIAG_VALUE_WIDTH_LG
	row.add_child(value)

	if with_status:
		var status := ColorRect.new()
		status.name = "Status"
		status.custom_minimum_size = Vector2(AppTheme.STATUS_INDICATOR_SIZE, AppTheme.STATUS_INDICATOR_SIZE)
		status.color = AppTheme.DIAG_STATUS_IDLE
		row.add_child(status)

	return row


func _update_status_display() -> void:
	match _status:
		Status.IDLE:
			_status_label.text = "Idle"
			_status_label.get_parent().get_child(0).color = AppTheme.DIAG_STATUS_IDLE
		Status.RUNNING:
			_status_label.text = "Running"
			_status_label.get_parent().get_child(0).color = AppTheme.DIAG_STATUS_RUNNING
		Status.COMPLETE:
			_status_label.text = "Complete"
			_status_label.get_parent().get_child(0).color = AppTheme.DIAG_STATUS_COMPLETE


func _update_duration() -> void:
	var elapsed_sec := (Time.get_ticks_msec() - _start_time) / 1000.0
	var total_secs := int(elapsed_sec)
	var mins := floori(total_secs / 60.0)
	var secs := total_secs % 60
	_duration_label.text = "Duration: %02d:%02d" % [mins, secs]


func _update_live_metrics() -> void:
	# Record camera frames during test
	if _camera_dataset and _camera_dataset.is_recording():
		if not CameraClient.is_daemon_connected():
			print("TimingDiagnostics: WARNING - daemon not connected during recording")
			return

		var frame_count := CameraClient.get_frame_count()
		var timestamp_us := CameraClient.get_latest_timestamp_us()

		# Only record if we have new frames
		if frame_count > _camera_dataset.frame_count:
			_camera_dataset.record_frame(frame_count, timestamp_us)

	# Update camera metrics using full statistics
	if _camera_dataset and _camera_dataset.frame_count > 0:
		var stats := _camera_dataset.get_full_statistics()
		_update_column_from_stats(stats, _camera_dataset.expected_fps,
			_cam_fps_label, _cam_fps_status,
			_cam_jitter_label, _cam_jitter_status,
			_cam_drops_label, _cam_drops_status,
			_cam_delta_min_label, _cam_delta_max_label, _cam_delta_mean_label,
			_cam_drift_total_label, _cam_drift_rate_label)

	# Update stimulus metrics using full statistics (requires validated refresh rate and frames)
	if _stimulus_dataset and _stimulus_dataset._refresh_rate_validated and _stimulus_dataset.timestamps_us.size() > 0:
		var stats := _stimulus_dataset.get_full_statistics()
		var expected_fps := _stimulus_dataset.get_display_refresh_hz()
		_update_column_from_stats(stats, expected_fps,
			_stim_fps_label, _stim_fps_status,
			_stim_jitter_label, _stim_jitter_status,
			_stim_drops_label, _stim_drops_status,
			_stim_delta_min_label, _stim_delta_max_label, _stim_delta_mean_label,
			_stim_drift_total_label, _stim_drift_rate_label)


## Update a column's labels from statistics dictionary
func _update_column_from_stats(
	stats: Dictionary,
	expected_fps: float,
	fps_label: Label, fps_status: ColorRect,
	jitter_label: Label, jitter_status: ColorRect,
	drops_label: Label, drops_status: ColorRect,
	delta_min_label: Label, delta_max_label: Label, delta_mean_label: Label,
	drift_total_label: Label, drift_rate_label: Label
) -> void:
	if stats.get("frame_count", 0) == 0:
		return

	# FPS
	if stats.has("actual_fps"):
		var actual_fps: float = float(stats["actual_fps"])
		fps_label.text = "%.1f / %.1f" % [actual_fps, expected_fps]
		_set_status_color(fps_status, _check_fps_ok(actual_fps, expected_fps, 1.0))
	else:
		fps_label.text = "—"
		_set_status_color(fps_status, false)

	# Jitter
	if stats.has("jitter_us"):
		var jitter_us: float = float(stats["jitter_us"])
		jitter_label.text = "%.0f µs" % jitter_us
		_set_status_color(jitter_status, jitter_us < 2000.0)
	else:
		jitter_label.text = "—"

	# Drops with rate
	var drop_count: int = int(stats.get("drop_count", 0))
	var drop_rate: float = float(stats.get("drop_rate_per_min", 0.0))
	drops_label.text = "%d (%.1f/min)" % [drop_count, drop_rate]
	_set_status_color(drops_status, drop_count == 0)

	# Delta statistics
	if stats.has("min_delta_us"):
		delta_min_label.text = "%d µs" % int(stats["min_delta_us"])
	if stats.has("max_delta_us"):
		delta_max_label.text = "%d µs" % int(stats["max_delta_us"])
	if stats.has("mean_delta_us"):
		delta_mean_label.text = "%.0f µs" % float(stats["mean_delta_us"])

	# Drift statistics
	if stats.has("total_drift_us"):
		var total_drift: int = int(stats["total_drift_us"])
		drift_total_label.text = "%+d µs" % total_drift
	if stats.has("drift_rate_ppm"):
		var drift_ppm: float = float(stats["drift_rate_ppm"])
		drift_rate_label.text = "%+.0f ppm" % drift_ppm


func _check_fps_ok(actual: float, expected: float, tolerance: float) -> bool:
	return absf(actual - expected) <= tolerance


func _set_status_color(rect: ColorRect, ok: bool) -> void:
	if rect:
		rect.color = AppTheme.DIAG_STATUS_OK if ok else AppTheme.DIAG_STATUS_FAIL


## Set the camera dataset to monitor
func set_camera_dataset(dataset: CameraDataset) -> void:
	_camera_dataset = dataset


## Set the stimulus dataset to monitor
func set_stimulus_dataset(dataset: StimulusDataset) -> void:
	_stimulus_dataset = dataset


## Called when acquisition starts
func on_acquisition_started() -> void:
	_status = Status.RUNNING
	_start_time = Time.get_ticks_msec()
	_update_status_display()


## Called when acquisition completes
func on_acquisition_completed() -> void:
	_status = Status.COMPLETE
	_update_status_display()
	_run_full_analysis()


func _run_full_analysis() -> void:
	if not _timing_analyzer:
		print("TimingDiagnostics: TimingAnalyzer not available")
		return

	if not _camera_dataset or not _stimulus_dataset:
		print("TimingDiagnostics: Missing datasets for analysis")
		return

	# Get timestamps
	var camera_ts := _camera_dataset.get_timestamps()
	var stimulus_ts := _stimulus_dataset.timestamps_us.duplicate()

	# Run Rust analysis (requires validated refresh rate)
	assert(_stimulus_dataset._refresh_rate_validated,
		"Stimulus refresh rate not validated - cannot run sync analysis")
	_last_report = _timing_analyzer.analyze(
		camera_ts,
		stimulus_ts,
		_camera_dataset.expected_fps,
		_stimulus_dataset.get_display_refresh_hz()
	)

	# Update sync metrics from report
	if _last_report.has("sync"):
		var sync: Dictionary = _last_report["sync"]

		# Offset metrics (nearest-neighbor analysis)
		if sync.has("offset_mean_us"):
			var offset_mean: float = float(sync["offset_mean_us"])
			_sync_offset_mean_label.text = "%+.0f µs" % offset_mean
		if sync.has("offset_max_us"):
			var offset_max: int = int(sync["offset_max_us"])
			_sync_offset_max_label.text = "%d µs" % offset_max
			_set_status_color(_sync_offset_status, offset_max < 50000)  # 50ms threshold
		if sync.has("offset_sd_us"):
			var offset_sd: float = float(sync["offset_sd_us"])
			_sync_offset_sd_label.text = "%.0f µs" % offset_sd

		# Alignment metrics (cross-correlation analysis)
		if sync.has("optimal_lag_us"):
			var lag: int = int(sync["optimal_lag_us"])
			_sync_lag_label.text = "%+d µs" % lag
		if sync.has("correlation"):
			var corr: float = float(sync["correlation"])
			_sync_corr_label.text = "%.3f" % corr
			_set_status_color(_sync_align_status, corr > 0.5)

		# Drift metric
		if sync.has("relative_drift_ppm"):
			var drift_ppm: float = float(sync["relative_drift_ppm"])
			_sync_drift_label.text = "%+.0f ppm" % drift_ppm
			_set_status_color(_sync_drift_status, absf(drift_ppm) < 1000.0)

	# Print quality summary
	if _last_report.has("quality"):
		var quality: Dictionary = _last_report["quality"]
		var overall_ok: bool = bool(quality["overall_ok"])
		print("TimingDiagnostics: Analysis complete - %s" % ("PASS" if overall_ok else "FAIL"))
		if quality.has("issues"):
			var issues: String = str(quality["issues"])
			if not issues.is_empty():
				print("  Issues: %s" % issues)


func _on_quick_test_pressed() -> void:
	if _status == Status.RUNNING:
		# Stop current test
		_stop_quick_test()
		return

	# Check prerequisites
	if not Session.has_selected_camera():
		print("TimingDiagnostics: No camera selected - select a camera in Setup first")
		return

	if not Session.has_selected_display():
		print("TimingDiagnostics: No display selected - select a display in Setup first")
		return

	# Verify display is validated (like camera format validation)
	if not Session.display_refresh_validated:
		print("TimingDiagnostics: Display not validated - validating...")
		DisplayValidator.validation_completed.connect(_on_display_validated_for_test, CONNECT_ONE_SHOT)
		DisplayValidator.validation_failed.connect(_on_display_validation_failed_for_test, CONNECT_ONE_SHOT)
		DisplayValidator.validate_display(Session.display_index)
		_quick_test_button.text = "Validating..."
		_quick_test_button.disabled = true
		return

	# Verify camera is connected
	if not CameraClient.is_daemon_connected():
		print("TimingDiagnostics: Camera not connected - connecting...")
		CameraClient.daemon_connected.connect(_on_daemon_connected_for_test, CONNECT_ONE_SHOT)
		CameraClient.daemon_failed.connect(_on_daemon_failed_for_test, CONNECT_ONE_SHOT)
		CameraClient.start_daemon_async()
		_quick_test_button.text = "Connecting..."
		_quick_test_button.disabled = true
		return

	_start_quick_test()


func _on_display_validated_for_test(measured_hz: float, reported_hz: float, mismatch: bool) -> void:
	# Disconnect failure handler if still connected
	if DisplayValidator.validation_failed.is_connected(_on_display_validation_failed_for_test):
		DisplayValidator.validation_failed.disconnect(_on_display_validation_failed_for_test)
	# Update Config (controller pattern - same as SetupScreen)
	Session.set_display_validation(measured_hz)
	if mismatch:
		print("TimingDiagnostics: Display mismatch - reports %.0f Hz, actually %.1f Hz (using measured)" % [reported_hz, measured_hz])
	_quick_test_button.disabled = false
	_quick_test_button.text = "Quick Test 5s"
	# Continue with test setup (check camera next)
	_on_quick_test_pressed()


func _on_display_validation_failed_for_test(reason: String) -> void:
	# Disconnect success handler if still connected
	if DisplayValidator.validation_completed.is_connected(_on_display_validated_for_test):
		DisplayValidator.validation_completed.disconnect(_on_display_validated_for_test)
	Session.clear_display_validation()
	_quick_test_button.text = "Quick Test 5s"
	_quick_test_button.disabled = false
	print("TimingDiagnostics: Display validation failed - %s" % reason)


func _on_daemon_connected_for_test() -> void:
	# Disconnect failure handler if still connected
	if CameraClient.daemon_failed.is_connected(_on_daemon_failed_for_test):
		CameraClient.daemon_failed.disconnect(_on_daemon_failed_for_test)
	_quick_test_button.disabled = false
	_start_quick_test()


func _on_daemon_failed_for_test() -> void:
	# Disconnect success handler if still connected
	if CameraClient.daemon_connected.is_connected(_on_daemon_connected_for_test):
		CameraClient.daemon_connected.disconnect(_on_daemon_connected_for_test)
	_quick_test_button.text = "Quick Test 5s"
	_quick_test_button.disabled = false
	print("TimingDiagnostics: Failed to connect camera daemon")


func _start_quick_test() -> void:
	print("TimingDiagnostics: Starting 5-second timing test...")
	print("  Daemon connected: %s" % CameraClient.is_daemon_connected())

	# Get expected camera FPS from detected hardware (NOT display settings)
	var expected_fps := Session.camera_fps
	assert(expected_fps > 0, "TimingDiagnostics: camera_fps is 0 - camera not properly enumerated")

	print("  Camera native FPS: %.2f" % expected_fps)

	# Create camera dataset
	_camera_dataset = CameraDataset.new()
	_camera_dataset.expected_fps = expected_fps
	_camera_dataset.start_recording()

	# Create stimulus window on secondary display
	_create_test_stimulus_window()

	# Start status
	_status = Status.RUNNING
	_start_time = Time.get_ticks_msec()
	_quick_test_button.text = "Stop Test"
	_update_status_display()

	# Schedule auto-stop after 5 seconds
	get_tree().create_timer(5.0).timeout.connect(_stop_quick_test)


func _create_test_stimulus_window() -> void:
	if _test_stimulus_window != null:
		_test_stimulus_window.queue_free()
		_test_stimulus_window = null

	_test_stimulus_window = StimulusWindowScene.instantiate() as Window
	if _test_stimulus_window == null:
		push_error("TimingDiagnostics: Failed to create stimulus window")
		return

	# Configure window for selected display - no fallback
	assert(Session.has_selected_display(), "TimingDiagnostics: No display selected - configure display in Setup first")
	var target_screen := Session.display_index

	var screen_size := DisplayServer.screen_get_size(target_screen)
	var screen_pos := DisplayServer.screen_get_position(target_screen)

	_test_stimulus_window.position = screen_pos
	_test_stimulus_window.size = screen_size
	_test_stimulus_window.current_screen = target_screen
	_test_stimulus_window.mode = Window.MODE_EXCLUSIVE_FULLSCREEN

	# Add window to scene tree FIRST so _process() runs
	get_tree().root.add_child(_test_stimulus_window)
	_test_stimulus_window.show()

	# Get stimulus display and start it AFTER it's in the scene tree
	var stimulus_display = _test_stimulus_window.get_display()
	if stimulus_display == null:
		push_error("TimingDiagnostics: Failed to get stimulus display from window")
		_abort_quick_test("Failed to get stimulus display")
		return

	stimulus_display.refresh()  # Apply current Config settings
	stimulus_display.show_overlay = true  # Show debug overlay during test
	stimulus_display.start()
	_stimulus_dataset = stimulus_display.get_dataset()

	if _stimulus_dataset == null:
		push_error("TimingDiagnostics: Failed to get dataset from stimulus display")
		_abort_quick_test("Failed to get stimulus dataset")
		return

	if not _stimulus_dataset.is_recording():
		push_error("TimingDiagnostics: Stimulus dataset failed to start recording")
		_abort_quick_test("Stimulus dataset not recording")
		return

	var display_fps := DisplayServer.screen_get_refresh_rate(target_screen)
	print("  Stimulus window on screen %d (%dx%d @ %.1f Hz)" % [target_screen, screen_size.x, screen_size.y, display_fps])


func _abort_quick_test(reason: String) -> void:
	print("TimingDiagnostics: Test aborted - %s" % reason)

	# Clean up any partially created resources
	if _camera_dataset:
		_camera_dataset.stop_recording()
		_camera_dataset = null

	if _test_stimulus_window:
		_test_stimulus_window.queue_free()
		_test_stimulus_window = null

	_stimulus_dataset = null
	_status = Status.IDLE
	_quick_test_button.text = "Quick Test 5s"
	_quick_test_button.disabled = false
	_update_status_display()


func _stop_quick_test() -> void:
	if _status != Status.RUNNING:
		return

	print("TimingDiagnostics: Test complete, analyzing...")

	# Stop camera recording
	if _camera_dataset:
		_camera_dataset.stop_recording()

	# Stop stimulus and get dataset
	if _test_stimulus_window:
		var stimulus_display = _test_stimulus_window.get_display()
		if stimulus_display:
			stimulus_display.stop()
			_stimulus_dataset = stimulus_display.get_dataset()

	# Log recording summary
	print("  Camera frames: %d" % _camera_dataset.frame_count)
	if _camera_dataset.frame_count > 0:
		print("  Camera first ts: %d us" % _camera_dataset.timestamps_us[0])
		print("  Camera last ts: %d us" % _camera_dataset.timestamps_us[_camera_dataset.timestamps_us.size() - 1])

	if _stimulus_dataset:
		print("  Stimulus frames: %d" % _stimulus_dataset.frame_count)
		if _stimulus_dataset.frame_count > 0:
			print("  Stimulus first ts: %d us" % _stimulus_dataset.timestamps_us[0])
			print("  Stimulus last ts: %d us" % _stimulus_dataset.timestamps_us[_stimulus_dataset.timestamps_us.size() - 1])

	# Clean up stimulus window
	if _test_stimulus_window:
		_test_stimulus_window.queue_free()
		_test_stimulus_window = null

	# Update status
	_status = Status.COMPLETE
	_quick_test_button.text = "Quick Test 5s"
	_update_status_display()

	# Run analysis
	_run_analysis()


## Run analysis for Quick Test with both camera and stimulus data
func _run_analysis() -> void:
	var issues: Array[String] = []
	var cam_ok := false
	var stim_ok := false

	# --- Camera Analysis ---
	var cam_stats: Dictionary = {}
	if not _camera_dataset:
		print("TimingDiagnostics: No camera data to analyze")
		issues.append("No camera data")
	elif not _camera_dataset.has_valid_timing_data():
		print("TimingDiagnostics: FAIL - No valid timestamps from camera")
		issues.append("Camera: no valid timestamps")
	else:
		# Use full statistics for overall mean FPS (not rolling average)
		cam_stats = _camera_dataset.get_full_statistics()
		var cam_fps: float = float(cam_stats.get("actual_fps", 0.0))
		var cam_jitter: float = float(cam_stats.get("jitter_us", 0.0))

		var fps_ok := _check_fps_ok(cam_fps, _camera_dataset.expected_fps, 1.0)
		var drops_ok := _camera_dataset.dropped_frame_indices.size() == 0
		var jitter_ok := cam_jitter < 2000.0

		cam_ok = fps_ok and drops_ok and jitter_ok

		if not fps_ok:
			issues.append("Camera FPS: %.2f (expected %.1f)" % [cam_fps, _camera_dataset.expected_fps])
		if not drops_ok:
			issues.append("Camera drops: %d" % _camera_dataset.dropped_frame_indices.size())
		if not jitter_ok:
			issues.append("Camera jitter: %.0f µs" % cam_jitter)

		print("TimingDiagnostics: Camera - %d frames, %.2f FPS, %d drops" % [
			_camera_dataset.frame_count, cam_fps, _camera_dataset.dropped_frame_indices.size()])

	# --- Stimulus Analysis ---
	var stim_stats: Dictionary = {}
	if not _stimulus_dataset:
		print("TimingDiagnostics: No stimulus data to analyze")
		issues.append("No stimulus data")
	elif _stimulus_dataset.frame_count == 0:
		print("TimingDiagnostics: FAIL - No stimulus frames recorded")
		issues.append("Stimulus: no frames recorded")
	elif not _stimulus_dataset._refresh_rate_validated:
		print("TimingDiagnostics: FAIL - Stimulus refresh rate not validated")
		issues.append("Stimulus: refresh rate not validated (not enough frames)")
	else:
		# Use full statistics for overall mean FPS
		stim_stats = _stimulus_dataset.get_full_statistics()
		var stim_fps: float = float(stim_stats.get("actual_fps", 0.0))
		var expected_stim_fps := _stimulus_dataset.get_display_refresh_hz()

		var stim_fps_ok := _check_fps_ok(stim_fps, expected_stim_fps, 1.0)
		var stim_drops_ok := _stimulus_dataset.dropped_frame_indices.size() == 0

		stim_ok = stim_fps_ok and stim_drops_ok

		if not stim_fps_ok:
			issues.append("Stimulus FPS: %.2f (expected %.1f)" % [stim_fps, expected_stim_fps])
		if not stim_drops_ok:
			issues.append("Stimulus drops: %d" % _stimulus_dataset.dropped_frame_indices.size())

		print("TimingDiagnostics: Stimulus - %d frames, %.2f FPS, %d drops" % [
			_stimulus_dataset.frame_count, stim_fps, _stimulus_dataset.dropped_frame_indices.size()])

	# --- Build Report ---
	_last_report = {
		"test_type": "quick_test",
		"duration_sec": 5.0,
		"quality": {
			"camera_ok": cam_ok,
			"stimulus_ok": stim_ok,
			"overall_ok": cam_ok and stim_ok,
			"issues": issues,
		},
	}

	if _camera_dataset and _camera_dataset.has_valid_timing_data():
		_last_report["camera"] = {
			"frame_count": _camera_dataset.frame_count,
			"actual_fps": cam_stats.get("actual_fps", 0.0),
			"expected_fps": _camera_dataset.expected_fps,
			"dropped_count": _camera_dataset.dropped_frame_indices.size(),
			"jitter_us": cam_stats.get("jitter_us", 0.0),
			"mean_delta_us": cam_stats.get("mean_delta_us", 0.0),
		}

	if _stimulus_dataset and _stimulus_dataset._refresh_rate_validated:
		_last_report["stimulus"] = {
			"frame_count": _stimulus_dataset.frame_count,
			"actual_fps": stim_stats.get("actual_fps", 0.0),
			"expected_fps": _stimulus_dataset.get_display_refresh_hz(),
			"dropped_count": _stimulus_dataset.dropped_frame_indices.size(),
			"jitter_us": stim_stats.get("jitter_us", 0.0),
			"mean_delta_us": stim_stats.get("mean_delta_us", 0.0),
		}

	# --- Sync Analysis ---
	var sync_ok := false
	if _timing_analyzer and _camera_dataset and _stimulus_dataset:
		if _camera_dataset.has_valid_timing_data() and _stimulus_dataset.frame_count >= 2:
			var camera_ts := _camera_dataset.get_timestamps()
			var stimulus_ts := _stimulus_dataset.timestamps_us.duplicate()

			# Run sync analysis (Rust normalizes timestamps internally)
			var sync_result: Dictionary = _timing_analyzer.analyze_sync(camera_ts, stimulus_ts)

			if not sync_result.has("error"):
				_last_report["sync"] = sync_result

				# Update sync UI labels
				if sync_result.has("offset_mean_us"):
					var offset_mean: float = float(sync_result["offset_mean_us"])
					_sync_offset_mean_label.text = "%+.0f µs" % offset_mean
				if sync_result.has("offset_max_us"):
					var offset_max: int = int(sync_result["offset_max_us"])
					_sync_offset_max_label.text = "%d µs" % offset_max
					_set_status_color(_sync_offset_status, offset_max < 50000)
				if sync_result.has("offset_sd_us"):
					var offset_sd: float = float(sync_result["offset_sd_us"])
					_sync_offset_sd_label.text = "%.0f µs" % offset_sd

				if sync_result.has("optimal_lag_us"):
					var lag: int = int(sync_result["optimal_lag_us"])
					_sync_lag_label.text = "%+d µs" % lag
				if sync_result.has("correlation"):
					var corr: float = float(sync_result["correlation"])
					_sync_corr_label.text = "%.3f" % corr
					_set_status_color(_sync_align_status, corr > 0.5)
					sync_ok = corr > 0.5

				if sync_result.has("relative_drift_ppm"):
					var drift_ppm: float = float(sync_result["relative_drift_ppm"])
					_sync_drift_label.text = "%+.0f ppm" % drift_ppm
					_set_status_color(_sync_drift_status, absf(drift_ppm) < 1000.0)
					if sync_ok:
						sync_ok = absf(drift_ppm) < 1000.0

				print("TimingDiagnostics: Sync - offset_max %d µs, corr %.3f, drift %.0f ppm" % [
					sync_result.get("offset_max_us", 0),
					sync_result.get("correlation", 0.0),
					sync_result.get("relative_drift_ppm", 0.0)])

	# --- Final UI Update with Complete Statistics ---
	# Update per-stream metrics with final complete data
	if _camera_dataset and _camera_dataset.has_valid_timing_data():
		var stats := _camera_dataset.get_full_statistics()
		_update_column_from_stats(stats, _camera_dataset.expected_fps,
			_cam_fps_label, _cam_fps_status,
			_cam_jitter_label, _cam_jitter_status,
			_cam_drops_label, _cam_drops_status,
			_cam_delta_min_label, _cam_delta_max_label, _cam_delta_mean_label,
			_cam_drift_total_label, _cam_drift_rate_label)

	if _stimulus_dataset and _stimulus_dataset._refresh_rate_validated:
		var stats := _stimulus_dataset.get_full_statistics()
		var expected_fps := _stimulus_dataset.get_display_refresh_hz()
		_update_column_from_stats(stats, expected_fps,
			_stim_fps_label, _stim_fps_status,
			_stim_jitter_label, _stim_jitter_status,
			_stim_drops_label, _stim_drops_status,
			_stim_delta_min_label, _stim_delta_max_label, _stim_delta_mean_label,
			_stim_drift_total_label, _stim_drift_rate_label)

	# --- Print Summary ---
	var overall_ok: bool = cam_ok and stim_ok and sync_ok
	print("TimingDiagnostics: Quick Test %s" % ("PASS" if overall_ok else "FAIL"))
	if issues.size() > 0:
		for issue in issues:
			print("  - %s" % issue)


func _on_export_pressed() -> void:
	if _last_report.is_empty():
		print("TimingDiagnostics: No report to export")
		return

	var output_dir := OS.get_user_data_dir().path_join("timing_reports")
	if not DirAccess.dir_exists_absolute(output_dir):
		DirAccess.make_dir_recursive_absolute(output_dir)

	var timestamp := Time.get_datetime_string_from_system(true).replace(":", "-")
	var file_path := output_dir.path_join("timing_report_%s.json" % timestamp)

	var json_string := JSON.stringify(_last_report, "  ")
	var file := FileAccess.open(file_path, FileAccess.WRITE)
	if file:
		file.store_string(json_string)
		file.close()
		print("TimingDiagnostics: Report exported to %s" % file_path)
	else:
		print("TimingDiagnostics: Failed to export report")


func _on_clear_pressed() -> void:
	# Clean up any running test
	if _test_stimulus_window:
		_test_stimulus_window.queue_free()
		_test_stimulus_window = null

	_status = Status.IDLE
	_camera_dataset = null
	_stimulus_dataset = null
	_last_report = {}

	_update_status_display()
	_duration_label.text = "Duration: --:--"

	# Reset camera metric labels
	_cam_fps_label.text = "—"
	_cam_jitter_label.text = "—"
	_cam_drops_label.text = "—"
	_cam_delta_min_label.text = "—"
	_cam_delta_max_label.text = "—"
	_cam_delta_mean_label.text = "—"
	_cam_drift_total_label.text = "—"
	_cam_drift_rate_label.text = "—"

	# Reset stimulus metric labels
	_stim_fps_label.text = "—"
	_stim_jitter_label.text = "—"
	_stim_drops_label.text = "—"
	_stim_delta_min_label.text = "—"
	_stim_delta_max_label.text = "—"
	_stim_delta_mean_label.text = "—"
	_stim_drift_total_label.text = "—"
	_stim_drift_rate_label.text = "—"

	# Reset sync metric labels
	_sync_offset_mean_label.text = "—"
	_sync_offset_max_label.text = "—"
	_sync_offset_sd_label.text = "—"
	_sync_lag_label.text = "—"
	_sync_corr_label.text = "—"
	_sync_drift_label.text = "—"

	# Reset all status indicators
	for status in [_cam_fps_status, _cam_jitter_status, _cam_drops_status,
				   _stim_fps_status, _stim_jitter_status, _stim_drops_status,
				   _sync_offset_status, _sync_align_status, _sync_drift_status]:
		if status:
			status.color = AppTheme.DIAG_STATUS_IDLE
