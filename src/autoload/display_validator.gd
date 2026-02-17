## DisplayValidator - Display refresh rate validation service
##
## Stateless service that validates actual display refresh rate by measuring frame intervals.
## Emits signals with results - does NOT store validation state (Config is SSoT).
##
## Usage:
##   DisplayValidator.validate_display(screen_idx)
##   # SetupScreen listens for signals and updates Config via public API
extends Node


# -----------------------------------------------------------------------------
# Signals
# -----------------------------------------------------------------------------

## Emitted when validation starts
signal validation_started(screen_idx: int)

## Emitted when validation succeeds
## measured_hz: Actual measured refresh rate
## reported_hz: OS-reported refresh rate
## mismatch: True if measured differs significantly from reported
signal validation_completed(measured_hz: float, reported_hz: float, mismatch: bool)

## Emitted when validation fails (measurement itself failed, not mismatch)
## reason: Human-readable error message
signal validation_failed(reason: String)


# -----------------------------------------------------------------------------
# Constants
# -----------------------------------------------------------------------------

## Warmup frames to skip (window settling period)
## Higher value during splash when there's lots of initialization activity
const WARMUP_FRAMES := 60

## Minimum frames for statistical validity (~1 second at 60Hz)
const MIN_SAMPLES := 60

## 95% confidence level z-score
const Z_SCORE := 1.96

## CI must be within ±2% of mean for valid measurement
const MAX_CI_PERCENT := 0.02

## Measured must match reported within 5%
const REPORTED_TOLERANCE := 0.05


# -----------------------------------------------------------------------------
# Validation Process State (transient, not exposed)
# -----------------------------------------------------------------------------

var _is_validating: bool = false
var _screen_idx: int = -1
var _reported_refresh_hz: float = -1.0  # Local to current validation

## Temporary window for validation
var _validation_window: Window = null

## Frame timestamps captured during validation
var _frame_timestamps_us: PackedInt64Array = PackedInt64Array()

## Whether we're connected to frame_post_draw
var _connected_to_frame_post_draw: bool = false

## Warmup frame counter (skip first N frames for window to stabilize)
var _warmup_remaining: int = 0


# -----------------------------------------------------------------------------
# Public API
# -----------------------------------------------------------------------------

## Start validation for the specified screen
func validate_display(screen_idx: int) -> void:
	# Cancel any in-progress validation
	if _is_validating:
		_cleanup_validation()

	_screen_idx = screen_idx
	_is_validating = true
	_frame_timestamps_us.clear()

	# Get reported refresh rate from native API (bypasses Godot's buggy DisplayServer)
	_reported_refresh_hz = _get_refresh_rate_native(screen_idx)
	if _reported_refresh_hz <= 0:
		_fail_validation("Could not query refresh rate for screen %d" % screen_idx)
		return

	validation_started.emit(screen_idx)

	# Defer window creation to next frame so UI can fully render first
	call_deferred("_create_validation_window", screen_idx)


## Get refresh rate using native MonitorInfo API
func _get_refresh_rate_native(screen_idx: int) -> float:
	assert(ClassDB.class_exists("MonitorInfo"), "MonitorInfo extension required")
	var monitor_info: RefCounted = ClassDB.instantiate(&"MonitorInfo")
	assert(monitor_info != null, "Failed to instantiate MonitorInfo")

	var info: Dictionary = monitor_info.call("get_display_info", screen_idx)
	return float(info.get("refresh_rate", 0.0))


## Check if validation is in progress
func is_validating() -> bool:
	return _is_validating


# -----------------------------------------------------------------------------
# Validation Window
# -----------------------------------------------------------------------------

func _create_validation_window(screen_idx: int) -> void:
	# Get screen info from native API (position is in virtual desktop coordinates)
	var screen_info := _get_screen_info_native(screen_idx)
	var screen_size := Vector2i(int(screen_info.get("width", 1920)), int(screen_info.get("height", 1080)))
	var screen_pos := Vector2i(int(screen_info.get("position_x", 0)), int(screen_info.get("position_y", 0)))

	print("DisplayValidator: Creating window at position %s, size %s" % [screen_pos, screen_size])

	_validation_window = Window.new()
	_validation_window.title = ""
	_validation_window.borderless = true
	_validation_window.unresizable = true
	_validation_window.transparent = false
	_validation_window.unfocusable = true  # Don't steal focus from splash

	# Position at origin of target screen using virtual desktop coordinates
	# Do NOT use current_screen - Godot may not know about all monitors
	_validation_window.position = screen_pos
	_validation_window.size = screen_size

	_validation_window.ready.connect(_on_validation_window_ready)

	add_child(_validation_window)


## Get screen info using native MonitorInfo API
func _get_screen_info_native(screen_idx: int) -> Dictionary:
	assert(ClassDB.class_exists("MonitorInfo"), "MonitorInfo extension required")
	var monitor_info: RefCounted = ClassDB.instantiate(&"MonitorInfo")
	assert(monitor_info != null, "Failed to instantiate MonitorInfo")
	return monitor_info.call("get_display_info", screen_idx)


func _on_validation_window_ready() -> void:
	# Enable V-sync on the validation window
	DisplayServer.window_set_vsync_mode(
		DisplayServer.VSYNC_ENABLED,
		_validation_window.get_window_id()
	)

	# Build UI that matches splash screen appearance
	var background := ColorRect.new()
	background.color = AppTheme.BG_BASE
	background.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	_validation_window.add_child(background)

	# Center container for content
	var center := CenterContainer.new()
	center.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	_validation_window.add_child(center)

	# VBox for title and status (matches splash layout)
	var vbox := VBoxContainer.new()
	vbox.theme_type_variation = "VBox2XL"
	center.add_child(vbox)

	# Title label matching splash
	var title := Label.new()
	title.text = "OpenISI"
	title.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	title.theme_type_variation = "LabelSplash"
	vbox.add_child(title)

	# Status label
	var status := Label.new()
	status.text = "Validating display..."
	status.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	status.theme_type_variation = "LabelSmallDim"
	vbox.add_child(status)

	# Start warmup period before collecting samples
	_warmup_remaining = WARMUP_FRAMES

	# Connect to frame_post_draw to capture vsync timestamps
	if not _connected_to_frame_post_draw:
		RenderingServer.frame_post_draw.connect(_on_frame_post_draw)
		_connected_to_frame_post_draw = true

	var actual_pos := _validation_window.position
	var actual_size := _validation_window.size
	print("DisplayValidator: Started vsync validation on screen %d (window at %s, size %s, reported: %.1f Hz)" % [
		_screen_idx, actual_pos, actual_size, _reported_refresh_hz])


func _on_frame_post_draw() -> void:
	if not _is_validating or _validation_window == null:
		return

	# Skip warmup frames to let window stabilize
	if _warmup_remaining > 0:
		_warmup_remaining -= 1
		return

	# Capture timestamp
	var timestamp_us := Time.get_ticks_usec()
	_frame_timestamps_us.append(timestamp_us)

	# Check if we have enough samples
	if _frame_timestamps_us.size() >= MIN_SAMPLES + 1:  # +1 because we need deltas
		_compute_and_validate()


# -----------------------------------------------------------------------------
# Validation Logic
# -----------------------------------------------------------------------------

func _compute_and_validate() -> void:
	# Compute frame deltas
	var deltas: PackedInt64Array = PackedInt64Array()
	for i in range(1, _frame_timestamps_us.size()):
		var delta := _frame_timestamps_us[i] - _frame_timestamps_us[i - 1]
		deltas.append(delta)

	var n := deltas.size()
	assert(n >= MIN_SAMPLES, "Not enough samples for validation")

	# Compute mean interval
	var sum: int = 0
	for i in range(n):
		sum += deltas[i]
	var mean_delta_us := float(sum) / n

	# Compute standard deviation
	var sum_sq: float = 0.0
	for i in range(n):
		var diff := float(deltas[i]) - mean_delta_us
		sum_sq += diff * diff
	var std_dev := sqrt(sum_sq / n)

	# Compute 95% confidence interval as percentage of mean
	var std_error := std_dev / sqrt(n)
	var ci_half_width := Z_SCORE * std_error
	var ci_percent := ci_half_width / mean_delta_us

	# Check statistical validity
	if ci_percent > MAX_CI_PERCENT:
		_fail_validation(
			"Measurement unstable: 95%% CI is ±%.1f%% (max ±%.1f%%). Jitter: %.0f µs" % [
				ci_percent * 100, MAX_CI_PERCENT * 100, std_dev])
		return

	var measured_refresh_hz := 1000000.0 / mean_delta_us

	# Account for fps_divisor: expected = reported / divisor
	var divisor := Settings.display_fps_divisor
	var expected_hz := _reported_refresh_hz / divisor

	# Check if measured differs significantly from expected (accounting for divisor)
	var ratio := measured_refresh_hz / expected_hz
	var mismatch := ratio < (1.0 - REPORTED_TOLERANCE) or ratio > (1.0 + REPORTED_TOLERANCE)

	if mismatch:
		print("DisplayValidator: Mismatch detected - expected %.1f Hz (%.0f / %d), measured %.1f Hz (%.1f%% difference)" % [
			expected_hz, _reported_refresh_hz, divisor, measured_refresh_hz, (ratio - 1.0) * 100])
	elif divisor > 1:
		print("DisplayValidator: Using divisor %d: %.0f Hz / %d = %.1f Hz expected" % [
			divisor, _reported_refresh_hz, divisor, expected_hz])

	# Success - emit result with measured value (SetupScreen will update Config)
	# Mismatch is a warning, not a failure - we use the measured value
	_succeed_validation(measured_refresh_hz, mismatch)


func _succeed_validation(measured_hz: float, mismatch: bool = false) -> void:
	if mismatch:
		print("DisplayValidator: Using measured %.2f Hz (reported: %.0f Hz, n=%d) - MISMATCH" % [
			measured_hz, _reported_refresh_hz, _frame_timestamps_us.size() - 1])
	else:
		print("DisplayValidator: Validated %.2f Hz (reported: %.0f Hz, n=%d)" % [
			measured_hz, _reported_refresh_hz, _frame_timestamps_us.size() - 1])

	var reported_hz := _reported_refresh_hz
	_cleanup_validation()

	# Emit signal - SetupScreen will call Session.set_display_validation()
	# Mismatch is passed so UI can show warning (like camera format mismatch)
	validation_completed.emit(measured_hz, reported_hz, mismatch)


func _fail_validation(reason: String) -> void:
	print("DisplayValidator: FAILED - %s" % reason)

	_cleanup_validation()

	# Emit signal - SetupScreen will call Session.clear_display_validation()
	validation_failed.emit(reason)


func _cleanup_validation() -> void:
	_is_validating = false
	_frame_timestamps_us.clear()
	_reported_refresh_hz = -1.0
	_screen_idx = -1
	_warmup_remaining = 0

	# Disconnect from frame_post_draw
	if _connected_to_frame_post_draw:
		RenderingServer.frame_post_draw.disconnect(_on_frame_post_draw)
		_connected_to_frame_post_draw = false

	# Destroy validation window
	if _validation_window != null:
		_validation_window.queue_free()
		_validation_window = null
