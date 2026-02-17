## StimulusSequencer - Timing state machine for stimulus execution
##
## Manages the execution of a stimulus sequence including:
## - Direction ordering (sequential/interleaved/random)
## - Phase transitions (baseline -> sweep -> interval -> ...)
## - Repetition counting
## - Timing signals for data synchronization
##
## Reads all config values directly from Config (SSoT) - no cached copies.
class_name StimulusSequencer
extends RefCounted

## Sequencer states
enum State {
	IDLE,              ## Not running
	BASELINE_START,    ## Initial baseline period
	SWEEP,             ## Active stimulus sweep
	INTER_STIMULUS,    ## Between sweeps (same direction)
	INTER_DIRECTION,   ## Between direction changes
	BASELINE_END,      ## Final baseline period
	COMPLETE,          ## Sequence finished
}

## State names for debugging
const STATE_NAMES := {
	State.IDLE: "Idle",
	State.BASELINE_START: "Baseline (Start)",
	State.SWEEP: "Sweep",
	State.INTER_STIMULUS: "Inter-Stimulus",
	State.INTER_DIRECTION: "Inter-Direction",
	State.BASELINE_END: "Baseline (End)",
	State.COMPLETE: "Complete",
}

## Signals
signal state_changed(new_state: State, old_state: State)
signal sweep_started(sweep_index: int, direction: String)
signal sweep_completed(sweep_index: int, direction: String)
signal direction_changed(new_direction: String, old_direction: String)
signal sequence_started()
signal sequence_completed()
signal progress_updated(elapsed_sec: float, total_sec: float, percent: float)

## Runtime state only - no config copies
var state: State = State.IDLE
var current_sweep_index: int = 0
var current_direction: String = ""

## Frame timing (computed at start from display refresh rate)
var _refresh_hz: float = 0.0
var _baseline_start_frames: int = 0
var _sweep_duration_frames: int = 0
var _inter_stimulus_frames: int = 0
var _inter_direction_frames: int = 0
var _baseline_end_frames: int = 0
var _total_duration_frames: int = 0

## Runtime frame counters (incremented at vsync boundaries)
var _total_frame_count: int = 0
var _state_frame_count: int = 0

## Sweep sequence - generated at start, locked for duration of run
var _sweep_sequence: Array[String] = []

## Condition occurrence tracking (how many times each condition has been shown)
var _condition_counts: Dictionary = {}  # condition -> count
var _current_condition_occurrence: int = 0


## Start sequence execution
func start() -> void:
	# Generate sweep sequence at start (locked for this run)
	_sweep_sequence = _generate_sweep_sequence()

	if _sweep_sequence.is_empty():
		ErrorHandler.report(
			ErrorHandler.Code.ACQUISITION_START_FAILED,
			"Empty sweep sequence",
			"No conditions configured. Configure at least one direction in the Stimulus screen.",
			ErrorHandler.Severity.ERROR,
			ErrorHandler.Category.STIMULUS
		)
		return

	# Get validated refresh rate (REQUIRED for frame-locked timing)
	assert(Session.display_refresh_validated,
		"Display refresh rate must be validated before starting sequencer")
	_refresh_hz = Session.display_measured_refresh_hz
	assert(_refresh_hz > 0, "Invalid refresh rate: %f" % _refresh_hz)

	# Convert all durations to frame counts (ceil to never cut short)
	_baseline_start_frames = _sec_to_frames(Settings.baseline_start_sec)
	_sweep_duration_frames = _sec_to_frames(Settings.sweep_duration_sec)
	_inter_stimulus_frames = _sec_to_frames(Settings.inter_stimulus_sec)
	_inter_direction_frames = _sec_to_frames(Settings.inter_direction_sec)
	_baseline_end_frames = _sec_to_frames(Settings.baseline_end_sec)
	_total_duration_frames = _compute_total_duration_frames()

	# Initialize frame counters
	_total_frame_count = 0
	_state_frame_count = 0
	current_sweep_index = 0

	# Initialize condition occurrence tracking
	_condition_counts.clear()
	_current_condition_occurrence = 0

	# Start with baseline if configured
	if _baseline_start_frames > 0:
		_transition_to(State.BASELINE_START)
	else:
		_start_next_sweep()

	sequence_started.emit()


## Convert seconds to frames (always rounds UP to never cut short)
func _sec_to_frames(sec: float) -> int:
	if sec <= 0:
		return 0
	return ceili(sec * _refresh_hz)


## Compute total duration in frames
func _compute_total_duration_frames() -> int:
	var total := _baseline_start_frames + _baseline_end_frames

	# Add frames for all sweeps and intervals
	var num_sweeps := _sweep_sequence.size()
	total += num_sweeps * _sweep_duration_frames

	# Inter-stimulus intervals (between sweeps of same direction)
	# This is simplified - actual count depends on direction sequence
	if num_sweeps > 1:
		total += (num_sweeps - 1) * _inter_stimulus_frames

	return total


## Stop sequence execution
func stop() -> void:
	_transition_to(State.IDLE)
	_sweep_sequence.clear()


## Advance frame counter (called by StimulusDisplay at vsync boundary)
## This is the ONLY place timing advances - ensures frame-locked transitions
func advance_frame() -> void:
	if state == State.IDLE or state == State.COMPLETE:
		return

	_total_frame_count += 1
	_state_frame_count += 1

	# Check for state transitions (frame-locked)
	var duration_frames := _get_current_state_duration_frames()
	if _state_frame_count >= duration_frames:
		match state:
			State.BASELINE_START:
				_start_next_sweep()

			State.SWEEP:
				_complete_current_sweep()

			State.INTER_STIMULUS:
				_start_next_sweep()

			State.INTER_DIRECTION:
				_start_next_sweep()

			State.BASELINE_END:
				_transition_to(State.COMPLETE)
				sequence_completed.emit()

	# Emit progress (computed from frame counts)
	var elapsed_sec := float(_total_frame_count) / _refresh_hz
	var total_sec := float(_total_duration_frames) / _refresh_hz
	var percent := 0.0
	if _total_duration_frames > 0:
		percent = clampf(float(_total_frame_count) / float(_total_duration_frames) * 100.0, 0.0, 100.0)
	progress_updated.emit(elapsed_sec, total_sec, percent)


## Generate the sweep sequence from Config
func _generate_sweep_sequence() -> Array[String]:
	var conditions: Array = Settings.conditions
	var reps: int = Settings.repetitions
	var order_str: String = Settings.order

	var sequence: Array[String] = []

	match order_str:
		"interleaved":
			for _i in range(reps):
				for cond in conditions:
					sequence.append(str(cond))
		"randomized":
			for cond in conditions:
				for _i in range(reps):
					sequence.append(str(cond))
			sequence.shuffle()
		"sequential":
			for cond in conditions:
				for _i in range(reps):
					sequence.append(str(cond))
		_:
			assert(false, "Unknown order type '%s' (must be 'sequential', 'interleaved', or 'randomized')" % order_str)

	return sequence


## Transition to a new state (resets state frame counter)
func _transition_to(new_state: State) -> void:
	var old_state := state
	state = new_state
	_state_frame_count = 0  # Reset on every state transition
	state_changed.emit(new_state, old_state)


## Start the next sweep in the sequence
func _start_next_sweep() -> void:
	if current_sweep_index >= _sweep_sequence.size():
		# All sweeps complete
		if _baseline_end_frames > 0:
			_transition_to(State.BASELINE_END)
		else:
			_transition_to(State.COMPLETE)
			sequence_completed.emit()
		return

	var new_direction := _sweep_sequence[current_sweep_index]

	# Handle blank trials
	if new_direction == "BLANK":
		_transition_to(State.INTER_STIMULUS)
		current_sweep_index += 1
		return

	# Check for direction change
	var old_direction := current_direction
	if new_direction != old_direction and old_direction != "":
		direction_changed.emit(new_direction, old_direction)

	current_direction = new_direction

	# Track condition occurrence (how many times this condition has been shown)
	if not _condition_counts.has(new_direction):
		_condition_counts[new_direction] = 0
	_condition_counts[new_direction] += 1
	_current_condition_occurrence = _condition_counts[new_direction]

	_transition_to(State.SWEEP)
	sweep_started.emit(current_sweep_index, current_direction)


## Complete the current sweep
func _complete_current_sweep() -> void:
	sweep_completed.emit(current_sweep_index, current_direction)
	current_sweep_index += 1

	if current_sweep_index >= _sweep_sequence.size():
		# All sweeps complete
		if _baseline_end_frames > 0:
			_transition_to(State.BASELINE_END)
		else:
			_transition_to(State.COMPLETE)
			sequence_completed.emit()
		return

	# Determine next interval state
	var next_direction := _sweep_sequence[current_sweep_index]
	if next_direction == "BLANK":
		next_direction = ""

	if next_direction != current_direction and next_direction != "":
		# Direction change - use inter-direction interval
		if _inter_direction_frames > 0:
			_transition_to(State.INTER_DIRECTION)
		else:
			_start_next_sweep()
	else:
		# Same direction - use inter-stimulus interval
		if _inter_stimulus_frames > 0:
			_transition_to(State.INTER_STIMULUS)
		else:
			_start_next_sweep()


## Get current state name
func get_state_name() -> String:
	return STATE_NAMES[state]


## Get progress within current state (0-1), quantized to frames
func get_state_progress() -> float:
	var duration_frames := _get_current_state_duration_frames()
	if duration_frames <= 0:
		return 1.0
	return clampf(float(_state_frame_count) / float(duration_frames), 0.0, 1.0)


## Get frame index within current state
func get_state_frame_index() -> int:
	return _state_frame_count


## Get total frames in current state
func get_current_state_duration_frames() -> int:
	return _get_current_state_duration_frames()


## Get duration of current state in frames
func _get_current_state_duration_frames() -> int:
	match state:
		State.BASELINE_START:
			return _baseline_start_frames
		State.SWEEP:
			return _sweep_duration_frames
		State.INTER_STIMULUS:
			return _inter_stimulus_frames
		State.INTER_DIRECTION:
			return _inter_direction_frames
		State.BASELINE_END:
			return _baseline_end_frames
	return 0


## Get total sweep count
func get_total_sweeps() -> int:
	return _sweep_sequence.size()


## Get completed sweep count
func get_completed_sweeps() -> int:
	if state == State.COMPLETE:
		return _sweep_sequence.size()
	return current_sweep_index


## Check if protocol is running
func is_running() -> bool:
	return state != State.IDLE and state != State.COMPLETE


## Check if protocol is complete
func is_complete() -> bool:
	return state == State.COMPLETE


## Get sweep direction at index
func get_sweep_direction(index: int) -> String:
	if index >= 0 and index < _sweep_sequence.size():
		return _sweep_sequence[index]
	return ""


## Get remaining time in seconds (computed from frame counts)
func get_remaining_time() -> float:
	if not is_running():
		return 0.0
	var remaining_frames := _total_duration_frames - _total_frame_count
	return maxf(0.0, float(remaining_frames) / _refresh_hz)


## Get elapsed time in seconds (computed from frame counts)
func get_elapsed_time() -> float:
	if state == State.IDLE:
		return 0.0
	return float(_total_frame_count) / _refresh_hz


## Get the total sequence duration in seconds
func get_total_duration() -> float:
	if _refresh_hz <= 0:
		return Settings.total_duration_sec  # Fallback before start()
	return float(_total_duration_frames) / _refresh_hz


## Get total frame count
func get_total_frame_count() -> int:
	return _total_frame_count


## Get refresh rate used for frame timing
func get_refresh_hz() -> float:
	return _refresh_hz


## Get the occurrence count for the current condition (1-indexed)
## Returns how many times this condition has been shown so far
func get_current_condition_occurrence() -> int:
	return _current_condition_occurrence


## Check if currently in a baseline state (not actively showing stimulus)
func is_baseline() -> bool:
	return state != State.SWEEP
