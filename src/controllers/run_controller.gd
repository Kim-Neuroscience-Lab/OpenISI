## RunController - Domain logic for acquisition lifecycle
##
## Owns all acquisition-related objects and provides clean signals for UI:
## - SequencerController (sequencer state machine)
## - CameraDataset (timing analysis)
## - Stimulus window lifecycle
## - Start time tracking
##
## The UI (RunScreen) only connects signals and routes user actions.
class_name RunController
extends RefCounted


# -----------------------------------------------------------------------------
# Signals for UI
# -----------------------------------------------------------------------------

## Status changed (text, level: "info"/"success"/"warning"/"error", pulsing)
signal status_changed(text: String, level: String, pulsing: bool)

## Progress updated with all display data
signal progress_updated(percent: float, sweep_current: int, sweep_total: int, direction: String, frame_count: int)

## Camera metrics ready for display
signal camera_metrics_updated(stats: Dictionary)

## Stimulus metrics ready for display
signal stimulus_metrics_updated(stats: Dictionary)

## Stimulus preview data ready for display
signal stimulus_preview_updated(condition: String, sweep_current: int, sweep_total: int, state: String, progress: float)

## Elapsed and storage strings ready for display
signal elapsed_updated(elapsed_string: String, storage_string: String)

## Acquisition completed successfully
signal acquisition_complete()

## Acquisition stopped by user
signal acquisition_stopped()


# -----------------------------------------------------------------------------
# Scene References
# -----------------------------------------------------------------------------

const StimulusWindowScene := preload("res://src/stimulus/stimulus_window.tscn")


# -----------------------------------------------------------------------------
# Owned Objects
# -----------------------------------------------------------------------------

var _acquisition_controller: SequencerController = null
var _camera_dataset: CameraDataset = null
var _stimulus_window: Window = null


# -----------------------------------------------------------------------------
# State
# -----------------------------------------------------------------------------

var _start_time: int = 0
var _running := false
var _cam_start_frame_count: int = 0
var _last_frame_count: int = 0


# -----------------------------------------------------------------------------
# Lifecycle
# -----------------------------------------------------------------------------

func _init() -> void:
	_acquisition_controller = SequencerController.new()
	_connect_controller_signals()
	_acquisition_controller.initialize()


func _connect_controller_signals() -> void:
	_acquisition_controller.state_changed.connect(_on_controller_state_changed)
	_acquisition_controller.sequencer_state_changed.connect(_on_sequencer_state_changed)
	_acquisition_controller.sweep_started.connect(_on_sweep_started)
	_acquisition_controller.sweep_completed.connect(_on_sweep_completed)
	_acquisition_controller.acquisition_completed.connect(_on_acquisition_completed)
	_acquisition_controller.acquisition_stopped.connect(_on_acquisition_stopped)


## Clean up all resources
func cleanup() -> void:
	_cleanup_stimulus_window()

	if _acquisition_controller:
		_acquisition_controller.cleanup()

	if _camera_dataset and _camera_dataset.is_recording():
		_camera_dataset.stop_recording()


# -----------------------------------------------------------------------------
# Control API
# -----------------------------------------------------------------------------

## Start acquisition - captures snapshots, creates datasets, starts sequencer
func start() -> void:
	# Capture state snapshots
	Session.state.capture_stimulus_snapshot()
	Session.state.capture_hardware_state()

	_running = true
	Session.acquisition_running = true
	_start_time = Time.get_ticks_msec()

	# Create and start camera dataset
	_camera_dataset = CameraDataset.new()
	_camera_dataset.initialize(Session.camera_fps)
	_camera_dataset.start_recording()

	# Initialize frame tracking
	_last_frame_count = CameraClient.get_frame_count() if CameraClient.is_daemon_connected() else 0
	_cam_start_frame_count = _last_frame_count

	# Create stimulus window and connect to sequencer
	_setup_stimulus_system()

	# Start the acquisition controller
	if _acquisition_controller:
		_acquisition_controller.start()

	status_changed.emit("Acquiring", "success", true)
	print("RunController: Acquisition started")


## Stop acquisition
func stop() -> void:
	if _acquisition_controller:
		_acquisition_controller.stop()


## Update each frame - records camera data, computes metrics, emits signals
func update(delta: float) -> void:
	if not _running:
		return

	if _acquisition_controller:
		_acquisition_controller.update(delta)

	_update_camera_data()
	_emit_metrics()


## Check if acquisition is running
func is_running() -> bool:
	return _running


## Get the camera dataset (for external access if needed)
func get_camera_dataset() -> CameraDataset:
	return _camera_dataset


# -----------------------------------------------------------------------------
# Data Export
# -----------------------------------------------------------------------------

## Export both camera and stimulus datasets
func export_datasets(output_dir: String) -> void:
	var timestamp: String = Time.get_datetime_string_from_system(true).replace(":", "-")
	var session_dir: String = output_dir.path_join("session_%s" % timestamp)

	_export_camera_dataset(session_dir)
	_export_stimulus_dataset(session_dir)


func _export_camera_dataset(session_dir: String) -> void:
	if _camera_dataset == null or _camera_dataset.frame_count == 0:
		print("RunController: No camera dataset to export")
		return

	# Ensure directory exists
	if not DirAccess.dir_exists_absolute(session_dir):
		DirAccess.make_dir_recursive_absolute(session_dir)

	var camera_data := _camera_dataset.export_data()
	var json_string := JSON.stringify(camera_data, "  ")
	var file_path := session_dir.path_join("camera_timing.json")
	var file := FileAccess.open(file_path, FileAccess.WRITE)
	if file:
		file.store_string(json_string)
		file.close()
		print("RunController: Camera dataset exported to %s" % file_path)
	else:
		ErrorHandler.report(
			ErrorHandler.Code.EXPORT_WRITE_FAILED,
			"Failed to export camera dataset",
			"Could not create file: %s" % file_path,
			ErrorHandler.Severity.WARNING,
			ErrorHandler.Category.EXPORT
		)


func _export_stimulus_dataset(session_dir: String) -> void:
	var dataset := _get_stimulus_dataset()
	if dataset == null:
		print("RunController: No stimulus dataset to export")
		return

	var stimulus_display = _stimulus_window.get_display() if _stimulus_window else null
	if stimulus_display and stimulus_display.has_method("export_dataset"):
		var err: Error = stimulus_display.export_dataset(session_dir)
		if err == OK:
			print("RunController: Stimulus dataset exported to %s" % session_dir)
		else:
			ErrorHandler.report(
				ErrorHandler.Code.EXPORT_WRITE_FAILED,
				"Failed to export stimulus dataset",
				"Error: %s\nDirectory: %s" % [error_string(err), session_dir],
				ErrorHandler.Severity.WARNING,
				ErrorHandler.Category.EXPORT
			)


# -----------------------------------------------------------------------------
# Stimulus Window Management
# -----------------------------------------------------------------------------

func _setup_stimulus_system() -> void:
	if _acquisition_controller == null:
		ErrorHandler.report_acquisition_error(
			"Acquisition controller not initialized",
			"Cannot setup stimulus system without acquisition controller.",
			ErrorHandler.Code.ACQUISITION_START_FAILED,
			ErrorHandler.Severity.ERROR
		)
		return

	_create_stimulus_window()

	print("RunController: Stimulus system initialized")
	print("  Total sweeps: ", _acquisition_controller.get_total_sweeps())
	print("  Total duration: %.1f sec" % _acquisition_controller.get_total_duration_sec())


func _create_stimulus_window() -> void:
	if not ResourceLoader.exists("res://src/stimulus/stimulus_window.tscn"):
		push_warning("RunController: stimulus_window.tscn not found")
		return

	_stimulus_window = StimulusWindowScene.instantiate() as Window

	if _stimulus_window == null:
		ErrorHandler.report(
			ErrorHandler.Code.STIMULUS_WINDOW_FAILED,
			"Failed to create stimulus window",
			"Could not instantiate stimulus_window.tscn",
			ErrorHandler.Severity.ERROR,
			ErrorHandler.Category.STIMULUS
		)
		return

	# Configure window for secondary display
	var screen_count := DisplayServer.get_screen_count()
	var target_screen := 1 if screen_count > 1 else 0

	var screen_size := DisplayServer.screen_get_size(target_screen)
	var screen_pos := DisplayServer.screen_get_position(target_screen)

	_stimulus_window.position = screen_pos
	_stimulus_window.size = screen_size
	_stimulus_window.current_screen = target_screen
	_stimulus_window.mode = Window.MODE_EXCLUSIVE_FULLSCREEN

	var stimulus_display = _stimulus_window.get_display()
	if stimulus_display and _acquisition_controller:
		stimulus_display.refresh()
		stimulus_display.connect_to_sequencer(_acquisition_controller.get_sequencer())
		stimulus_display.show_overlay = false

	# Add to scene tree (requires a node reference - we'll use Engine.get_main_loop())
	var tree := Engine.get_main_loop() as SceneTree
	if tree:
		tree.root.add_child(_stimulus_window)
		_stimulus_window.show()

	print("RunController: Stimulus window created on screen %d (%dx%d)" % [target_screen, screen_size.x, screen_size.y])


func _cleanup_stimulus_window() -> void:
	if _stimulus_window:
		_stimulus_window.queue_free()
		_stimulus_window = null


func _get_stimulus_dataset() -> StimulusDataset:
	if _stimulus_window:
		var stimulus_display = _stimulus_window.get_display()
		if stimulus_display and stimulus_display.has_method("get_dataset"):
			return stimulus_display.get_dataset()
	return null


# -----------------------------------------------------------------------------
# Camera Data Recording
# -----------------------------------------------------------------------------

func _update_camera_data() -> void:
	if not CameraClient.is_daemon_connected():
		return

	var frame_count := CameraClient.get_frame_count()
	if frame_count == _last_frame_count:
		return

	# Record hardware timestamps during acquisition
	if _camera_dataset:
		var timestamp_us := CameraClient.get_latest_timestamp_us()
		var frames_since_last := frame_count - _last_frame_count

		for i in range(frames_since_last):
			_camera_dataset.record_frame(frame_count - frames_since_last + i + 1, timestamp_us)

		if _acquisition_controller:
			for i in range(frames_since_last):
				_acquisition_controller.record_frame()

			if frames_since_last > 1:
				var dropped := frames_since_last - 1
				for i in range(dropped):
					_acquisition_controller.record_dropped_frame()

			var bytes_per_frame: int = Session.camera_width_px * Session.camera_height_px
			var metrics := _acquisition_controller.get_metrics()
			_acquisition_controller.update_storage(metrics.total_frames * bytes_per_frame)

	_last_frame_count = frame_count


# -----------------------------------------------------------------------------
# Metrics Computation
# -----------------------------------------------------------------------------

func _emit_metrics() -> void:
	if not _acquisition_controller:
		return

	var metrics := _acquisition_controller.get_metrics()

	# Emit progress
	progress_updated.emit(
		metrics.get_progress_percent(),
		metrics.current_sweep,
		metrics.total_sweeps,
		metrics.current_direction,
		metrics.total_frames
	)

	# Emit elapsed and storage
	elapsed_updated.emit(metrics.get_elapsed_string(), metrics.get_storage_string())

	# Emit camera metrics
	camera_metrics_updated.emit(_compute_camera_metrics())

	# Emit stimulus metrics
	stimulus_metrics_updated.emit(_compute_stimulus_metrics())

	# Emit stimulus preview data
	_emit_stimulus_preview()


func _compute_camera_metrics() -> Dictionary:
	var stats := {}

	if _camera_dataset and _camera_dataset.frame_count >= 10:
		var cam_metrics := _camera_dataset.get_current_metrics()

		stats["fps"] = _camera_dataset.get_current_fps()

		if cam_metrics.has("jitter_us"):
			stats["jitter_ms"] = float(cam_metrics["jitter_us"]) / 1000.0

		var expected_fps := Session.camera_fps
		if expected_fps > 0 and cam_metrics.has("mean_delta_us"):
			var mean_delta_us: float = float(cam_metrics["mean_delta_us"])
			var expected_us := 1000000.0 / expected_fps
			var drift_us := mean_delta_us - expected_us
			stats["drift_pct"] = (drift_us / expected_us) * 100.0

		if _running and expected_fps > 0 and CameraClient.is_daemon_connected():
			var elapsed_sec := (Time.get_ticks_msec() - _start_time) / 1000.0
			var expected_frames := elapsed_sec * expected_fps
			var actual_frames := CameraClient.get_frame_count() - _cam_start_frame_count
			stats["total_drift_frames"] = actual_frames - expected_frames

		stats["dropped"] = _camera_dataset.dropped_frame_indices.size()

	return stats


func _compute_stimulus_metrics() -> Dictionary:
	var stats := {}
	var dataset := _get_stimulus_dataset()

	if dataset and dataset.is_recording():
		var current_fps := dataset.get_current_fps()
		stats["fps"] = current_fps

		var frame_data := dataset.get_current_frame_data()
		if frame_data.has("jitter_us"):
			stats["jitter_ms"] = float(frame_data["jitter_us"]) / 1000.0

		if dataset._refresh_rate_validated and current_fps > 0:
			var expected_fps := dataset.get_display_refresh_hz()
			stats["drift_pct"] = ((current_fps - expected_fps) / expected_fps) * 100.0

		if dataset._refresh_rate_validated and _running:
			var expected_fps := dataset.get_display_refresh_hz()
			var elapsed_sec := (Time.get_ticks_msec() - _start_time) / 1000.0
			var expected_frames := elapsed_sec * expected_fps
			stats["total_drift_frames"] = dataset.frame_count - expected_frames

	return stats


func _emit_stimulus_preview() -> void:
	if not _acquisition_controller:
		return

	var sequencer := _acquisition_controller.get_sequencer()
	if not sequencer:
		return

	var dataset := _get_stimulus_dataset()
	if dataset and dataset.is_recording():
		var frame_data := dataset.get_current_frame_data()
		if not frame_data.is_empty():
			stimulus_preview_updated.emit(
				frame_data["condition"],
				frame_data["sweep_index"] + 1,
				frame_data["total_sweeps"],
				frame_data["state"],
				frame_data["progress"]
			)
			return

	# Fall back to sequencer data
	stimulus_preview_updated.emit(
		sequencer.current_direction,
		sequencer.current_sweep_index + 1,
		sequencer.get_total_sweeps(),
		sequencer.get_state_name(),
		sequencer.get_state_progress()
	)


# -----------------------------------------------------------------------------
# Controller Signal Handlers
# -----------------------------------------------------------------------------

func _on_controller_state_changed(state: SequencerController.State) -> void:
	match state:
		SequencerController.State.IDLE:
			status_changed.emit("Ready", "info", false)
		SequencerController.State.RUNNING:
			status_changed.emit("Acquiring", "success", true)
		SequencerController.State.COMPLETE:
			status_changed.emit("Complete", "success", false)


func _on_sequencer_state_changed(new_state: StimulusSequencer.State, _old_state: StimulusSequencer.State) -> void:
	match new_state:
		StimulusSequencer.State.BASELINE_START:
			status_changed.emit("Baseline", "success", true)
		StimulusSequencer.State.SWEEP:
			status_changed.emit("Acquiring", "success", true)
		StimulusSequencer.State.INTER_STIMULUS, StimulusSequencer.State.INTER_DIRECTION:
			status_changed.emit("Interval", "success", true)
		StimulusSequencer.State.BASELINE_END:
			status_changed.emit("Baseline", "success", true)
		StimulusSequencer.State.COMPLETE:
			status_changed.emit("Complete", "success", false)


func _on_sweep_started(sweep_index: int, direction: String) -> void:
	print("Sweep %d started: %s" % [sweep_index + 1, direction])


func _on_sweep_completed(sweep_index: int, direction: String) -> void:
	print("Sweep %d completed: %s" % [sweep_index + 1, direction])


func _on_acquisition_completed(metrics: SequencerController.AcquisitionMetrics) -> void:
	_running = false
	Session.acquisition_running = false

	if _camera_dataset:
		_camera_dataset.stop_recording()

	status_changed.emit("Complete", "success", false)

	# Export datasets
	var output_dir: String = Settings.last_save_directory
	if output_dir.is_empty():
		output_dir = "user://acquisitions"
	export_datasets(output_dir)

	_cleanup_stimulus_window()

	var elapsed_ms := int(metrics.elapsed_sec * 1000)
	Session.set_acquisition_complete(metrics.total_frames, elapsed_ms)

	print("RunController: Acquisition complete - %d frames" % metrics.total_frames)
	acquisition_complete.emit()


func _on_acquisition_stopped(metrics: SequencerController.AcquisitionMetrics) -> void:
	_running = false
	Session.acquisition_running = false

	if _camera_dataset:
		_camera_dataset.stop_recording()

	status_changed.emit("Stopped", "warning", false)

	# Export camera dataset only (stimulus may be incomplete)
	var output_dir: String = Settings.last_save_directory
	if output_dir.is_empty():
		output_dir = "user://acquisitions"
	var timestamp: String = Time.get_datetime_string_from_system(true).replace(":", "-")
	var session_dir: String = output_dir.path_join("session_%s" % timestamp)
	_export_camera_dataset(session_dir)

	_cleanup_stimulus_window()

	var elapsed_ms := int(metrics.elapsed_sec * 1000)
	Session.set_acquisition_complete(metrics.total_frames, elapsed_ms)

	print("RunController: Acquisition stopped - %d frames" % metrics.total_frames)
	acquisition_stopped.emit()
