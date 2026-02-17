## SequencerController - Manages stimulus sequencer and acquisition metrics
##
## Wraps StimulusSequencer with state machine and metrics tracking.
## Used by RunController to orchestrate acquisition.
##
## Reads all config values directly from Settings (SSoT) - no cached copies.
class_name SequencerController
extends RefCounted


# -----------------------------------------------------------------------------
# Signals
# -----------------------------------------------------------------------------

signal state_changed(state: State)
signal progress_updated(elapsed_sec: float, total_sec: float, percent: float)
signal sweep_started(index: int, direction: String)
signal sweep_completed(index: int, direction: String)
signal direction_changed(new_direction: String, old_direction: String)
signal sequencer_state_changed(new_state: StimulusSequencer.State, old_state: StimulusSequencer.State)
signal acquisition_started()
signal acquisition_completed(metrics: AcquisitionMetrics)
signal acquisition_stopped(metrics: AcquisitionMetrics)


# -----------------------------------------------------------------------------
# State
# -----------------------------------------------------------------------------

enum State { IDLE, RUNNING, PAUSED, COMPLETE }

var _sequencer: StimulusSequencer = null
var _state: State = State.IDLE
var _metrics: AcquisitionMetrics = null
var _start_time_msec: int = 0


# -----------------------------------------------------------------------------
# Lifecycle
# -----------------------------------------------------------------------------

func _init() -> void:
	_metrics = AcquisitionMetrics.new()


## Initialize controller - creates sequencer and connects signals
func initialize() -> void:
	# Create sequencer (reads from Config directly)
	_sequencer = StimulusSequencer.new()

	# Connect sequencer signals
	_sequencer.state_changed.connect(_on_sequencer_state_changed)
	_sequencer.sweep_started.connect(_on_sequencer_sweep_started)
	_sequencer.sweep_completed.connect(_on_sequencer_sweep_completed)
	_sequencer.direction_changed.connect(_on_sequencer_direction_changed)
	_sequencer.progress_updated.connect(_on_sequencer_progress_updated)
	_sequencer.sequence_completed.connect(_on_sequence_completed)

	_state = State.IDLE
	state_changed.emit(_state)


## Clean up resources
func cleanup() -> void:
	if _sequencer:
		_sequencer.stop()
		_sequencer = null

	_state = State.IDLE


# -----------------------------------------------------------------------------
# Control Methods
# -----------------------------------------------------------------------------

## Start acquisition
func start() -> void:
	if _state == State.RUNNING:
		return

	if _sequencer == null:
		ErrorHandler.report(
			ErrorHandler.Code.ACQUISITION_START_FAILED,
			"Acquisition controller not initialized",
			"Call initialize() before start()",
			ErrorHandler.Severity.ERROR,
			ErrorHandler.Category.ACQUISITION
		)
		return

	_start_time_msec = Time.get_ticks_msec()
	_metrics.reset()
	_metrics.total_duration_sec = Settings.total_duration_sec

	_sequencer.start()

	_metrics.total_sweeps = _sequencer.get_total_sweeps()

	_state = State.RUNNING
	state_changed.emit(_state)
	acquisition_started.emit()


## Stop acquisition
func stop() -> void:
	if _state != State.RUNNING:
		return

	if _sequencer:
		_sequencer.stop()

	_update_metrics_from_sequencer()

	_state = State.IDLE
	state_changed.emit(_state)
	acquisition_stopped.emit(_metrics)


## Pause acquisition (if supported)
func pause() -> void:
	if _state != State.RUNNING:
		return

	# Note: StimulusSequencer doesn't currently support pause
	_state = State.PAUSED
	state_changed.emit(_state)


## Resume paused acquisition
func resume() -> void:
	if _state != State.PAUSED:
		return

	_state = State.RUNNING
	state_changed.emit(_state)


## Update controller (call each frame during acquisition)
## Note: Sequencer timing is now frame-locked - advanced by StimulusDisplay at vsync
func update(_delta: float) -> void:
	if _state != State.RUNNING:
		return

	# Sequencer is advanced by StimulusDisplay._on_frame_post_draw() at vsync boundary
	# We just update metrics here
	_update_metrics_from_sequencer()


# -----------------------------------------------------------------------------
# State Queries
# -----------------------------------------------------------------------------

## Get current state
func get_state() -> State:
	return _state


## Check if acquisition is running
func is_running() -> bool:
	return _state == State.RUNNING


## Check if acquisition is complete
func is_complete() -> bool:
	return _state == State.COMPLETE


## Get current metrics
func get_metrics() -> AcquisitionMetrics:
	return _metrics


## Get the sequencer (for connecting to stimulus display)
func get_sequencer() -> StimulusSequencer:
	return _sequencer


## Get total sweeps
func get_total_sweeps() -> int:
	if _sequencer:
		return _sequencer.get_total_sweeps()
	return 0


## Get total duration in seconds
func get_total_duration_sec() -> float:
	return Settings.total_duration_sec


## Get elapsed time in seconds
func get_elapsed_sec() -> float:
	if _start_time_msec == 0:
		return 0.0
	return (Time.get_ticks_msec() - _start_time_msec) / 1000.0


## Get progress as a percentage (0-100)
func get_progress_percent() -> float:
	var total := get_total_duration_sec()
	if total <= 0:
		return 0.0
	return clampf(get_elapsed_sec() / total * 100.0, 0.0, 100.0)


# -----------------------------------------------------------------------------
# Metrics Management
# -----------------------------------------------------------------------------

## Record a captured frame from the camera
func record_frame() -> void:
	_metrics.total_frames += 1


## Record a dropped frame
func record_dropped_frame() -> void:
	_metrics.dropped_frames += 1


## Update storage usage
func update_storage(bytes: int) -> void:
	_metrics.storage_bytes = bytes


## Get current frame rate based on elapsed time
func get_current_fps() -> float:
	var elapsed := get_elapsed_sec()
	if elapsed <= 0:
		return 0.0
	return float(_metrics.total_frames) / elapsed


# -----------------------------------------------------------------------------
# Internal Methods
# -----------------------------------------------------------------------------

func _update_metrics_from_sequencer() -> void:
	if _sequencer == null:
		return

	_metrics.elapsed_sec = get_elapsed_sec()
	_metrics.current_sweep = _sequencer.current_sweep_index + 1
	_metrics.current_direction = _sequencer.current_direction
	_metrics.sequencer_state = _sequencer.state


func _on_sequencer_state_changed(new_state: StimulusSequencer.State, old_state: StimulusSequencer.State) -> void:
	_metrics.sequencer_state = new_state
	sequencer_state_changed.emit(new_state, old_state)


func _on_sequencer_sweep_started(sweep_index: int, direction: String) -> void:
	_metrics.current_sweep = sweep_index + 1
	_metrics.current_direction = direction
	sweep_started.emit(sweep_index, direction)


func _on_sequencer_sweep_completed(sweep_index: int, direction: String) -> void:
	sweep_completed.emit(sweep_index, direction)


func _on_sequencer_direction_changed(new_direction: String, old_direction: String) -> void:
	_metrics.current_direction = new_direction
	direction_changed.emit(new_direction, old_direction)


func _on_sequencer_progress_updated(elapsed_sec: float, total_sec: float, percent: float) -> void:
	_metrics.elapsed_sec = elapsed_sec
	progress_updated.emit(elapsed_sec, total_sec, percent)


func _on_sequence_completed() -> void:
	_update_metrics_from_sequencer()
	_metrics.elapsed_sec = get_elapsed_sec()

	_state = State.COMPLETE
	state_changed.emit(_state)
	acquisition_completed.emit(_metrics)


# -----------------------------------------------------------------------------
# AcquisitionMetrics - Data class for acquisition metrics
# -----------------------------------------------------------------------------

class AcquisitionMetrics:
	## Time elapsed since acquisition start
	var elapsed_sec: float = 0.0

	## Total expected duration
	var total_duration_sec: float = 0.0

	## Total frames captured
	var total_frames: int = 0

	## Number of dropped frames
	var dropped_frames: int = 0

	## Current sweep index (1-based for display)
	var current_sweep: int = 0

	## Total number of sweeps
	var total_sweeps: int = 0

	## Current direction being presented
	var current_direction: String = ""

	## Storage used in bytes
	var storage_bytes: int = 0

	## Current sequencer state
	var sequencer_state: StimulusSequencer.State = StimulusSequencer.State.IDLE

	## Reset all metrics
	func reset() -> void:
		elapsed_sec = 0.0
		total_frames = 0
		dropped_frames = 0
		current_sweep = 0
		current_direction = ""
		storage_bytes = 0
		sequencer_state = StimulusSequencer.State.IDLE

	## Get storage in MB
	func get_storage_mb() -> float:
		return storage_bytes / (1024.0 * 1024.0)

	## Get storage in GB
	func get_storage_gb() -> float:
		return storage_bytes / (1024.0 * 1024.0 * 1024.0)

	## Get formatted storage string
	func get_storage_string() -> String:
		var mb := get_storage_mb()
		if mb < 1024:
			return "%.0f MB" % mb
		else:
			return "%.1f GB" % get_storage_gb()

	## Get formatted elapsed time string (M:SS)
	func get_elapsed_string() -> String:
		var minutes := int(elapsed_sec / 60)
		var seconds := int(elapsed_sec) % 60
		return "%d:%02d" % [minutes, seconds]

	## Get progress percentage
	func get_progress_percent() -> float:
		if total_duration_sec <= 0:
			return 0.0
		return clampf(elapsed_sec / total_duration_sec * 100.0, 0.0, 100.0)

	## Get completion status
	func is_complete() -> bool:
		return sequencer_state == StimulusSequencer.State.COMPLETE

	## Convert to dictionary for serialization/logging
	func to_dict() -> Dictionary:
		return {
			"elapsed_sec": elapsed_sec,
			"total_duration_sec": total_duration_sec,
			"total_frames": total_frames,
			"dropped_frames": dropped_frames,
			"current_sweep": current_sweep,
			"total_sweeps": total_sweeps,
			"current_direction": current_direction,
			"storage_bytes": storage_bytes,
			"sequencer_state": StimulusSequencer.STATE_NAMES[sequencer_state],
		}
