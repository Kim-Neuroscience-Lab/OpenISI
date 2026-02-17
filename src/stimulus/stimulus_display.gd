class_name StimulusDisplay
extends Control
## Stimulus display renderer for OpenISI.
##
## Displays visual stimuli using pluggable renderers. Reads config from Config
## autoload (SSoT). No fallbacks - if something is wrong, it fails.

signal sweep_completed(direction: String)
signal state_changed(new_state: String)
signal frame_recorded(frame_index: int)


# -----------------------------------------------------------------------------
# State
# -----------------------------------------------------------------------------

## Current renderer (created from Config)
var _renderer: StimulusRendererBase = null

## Render state passed to renderer
var _render_state: StimulusRendererBase.RenderState = null

## Dataset for capturing per-frame metadata (null when not recording)
var _dataset: StimulusDataset = null

## Display geometry
var _display_geometry: DisplayGeometry = null

## ColorRect for shader-based rendering
var _shader_rect: ColorRect = null

## Sync patch for photodiode detection (hardware timestamps)
## Small white/black square that toggles each frame for photodiode to detect
var _sync_patch: ColorRect = null
var _sync_patch_state: bool = false  # Current state: true=white, false=black
var _sync_patch_enabled: bool = true  # Whether sync patch is shown

## Connected sequencer (if any)
var _sequencer: StimulusSequencer = null

## Whether the stimulus animation is running
var is_running: bool = false

## Whether to show debug overlay
var show_overlay: bool = true

## Current sweep direction
var _direction: String = "LR"

## Progress within current sweep (0.0 to 1.0)
var _sweep_progress: float = 0.0

## Elapsed time since stimulus started
var _elapsed_sec: float = 0.0

## Current sweep index
var _sweep_index: int = 0

## Total sweeps
var _total_sweeps: int = 1

## Whether currently in baseline period
var _is_baseline: bool = false


# -----------------------------------------------------------------------------
# Timing Measurements
# -----------------------------------------------------------------------------

var _frame_count: int = 0
var _sweep_frame_count: int = 0
var _timing_log_interval: float = 2.0
var _timing_log_timer: float = 0.0

## Software timestamp captured at frame_post_draw (for mapping to hardware vsync)
var _software_timestamp_us: int = 0

## Hardware vsync timestamp (mapped from software timestamp via VsyncTimestampProvider)
## This is the TRUE presentation timestamp from Vulkan VK_GOOGLE_display_timing
var _vsync_timestamp_us: int = 0

## VsyncTimestampProvider for hardware vsync timestamps (from GDExtension)
## This provides TRUE hardware timestamps, not software approximations
var _vsync_provider: RefCounted = null

## Whether hardware vsync timestamps are available
var _hardware_timestamps_available: bool = false


# -----------------------------------------------------------------------------
# Lifecycle
# -----------------------------------------------------------------------------

func _ready() -> void:
	# Create a ColorRect for shader-based stimuli
	_shader_rect = ColorRect.new()
	_shader_rect.name = "ShaderRect"
	_shader_rect.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	_shader_rect.color = Color.WHITE
	_shader_rect.visible = false
	_shader_rect.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(_shader_rect)

	# Create sync patch for photodiode hardware timestamps
	_create_sync_patch()

	# Initialize render state
	_render_state = StimulusRendererBase.RenderState.new()

	# Connect to RenderingServer's frame_post_draw signal for timestamp capture
	# This fires AFTER rendering completes - we capture software timestamp here
	# and map to hardware vsync timestamp via VsyncTimestampProvider
	RenderingServer.frame_post_draw.connect(_on_frame_post_draw)

	# Defer initialization until layout is complete
	call_deferred("_deferred_init")


func _deferred_init() -> void:
	# Create renderer from Config
	_create_renderer()

	print("Stimulus display ready")
	print("  Control size: ", size)
	print("  Aspect ratio: ", size.x / size.y if size.y > 0 else 0.0)
	print("  Expected ratio: ", Session.display_width_cm / Session.display_height_cm)
	if _renderer:
		print("  Renderer: ", _renderer.get_type_id())


# -----------------------------------------------------------------------------
# Renderer Management
# -----------------------------------------------------------------------------

## Create renderer from current Config values
func _create_renderer() -> void:
	# Clean up old renderer
	if _renderer:
		_renderer.cleanup()
		_renderer = null

	# Reset shader rect
	if _shader_rect:
		_shader_rect.material = null
		_shader_rect.visible = false

	# Get renderer type from Config
	var renderer_type := _get_renderer_type()

	# Create renderer using factory
	var factory := RendererFactory.get_instance()
	_renderer = factory.create_renderer(renderer_type)

	if _renderer == null:
		push_error("Failed to create renderer for type: %s" % renderer_type)
		return

	# Get geometry from Config
	var geom := DisplayGeometry.from_config()
	geom.display_width_px = int(size.x)
	geom.display_height_px = int(size.y)

	# Build stimulus params from Config
	var stimulus_params := _build_stimulus_params()

	# Initialize renderer (geometry is SSoT for visual field)
	_renderer.initialize(stimulus_params, size, geom)

	# If renderer uses a shader, apply it to the shader rect
	if _renderer.requires_shader():
		var shader_mat := _renderer.get_shader_material()
		if shader_mat and _shader_rect:
			_shader_rect.material = shader_mat
			_shader_rect.visible = true

	# Set initial direction from Config
	var conditions: Array = Settings.conditions
	if conditions.size() > 0:
		_direction = str(conditions[0])


## Create sync patch for photodiode detection
## This small square toggles white/black each frame so a photodiode can
## detect actual display timing (hardware timestamps)
func _create_sync_patch() -> void:
	if _sync_patch:
		return  # Already exists

	_sync_patch = ColorRect.new()
	_sync_patch.name = "SyncPatch"
	# Position in top-left corner
	_sync_patch.position = Vector2(0, 0)
	# Size: 50x50 pixels (visible to photodiode, small enough to not interfere)
	_sync_patch.size = Vector2(50, 50)
	_sync_patch.color = Color.BLACK
	_sync_patch.visible = _sync_patch_enabled
	_sync_patch.mouse_filter = Control.MOUSE_FILTER_IGNORE
	# Add as last child so it's drawn on top
	add_child(_sync_patch)


## Enable or disable the sync patch
func set_sync_patch_enabled(enabled: bool) -> void:
	_sync_patch_enabled = enabled
	if _sync_patch:
		_sync_patch.visible = enabled


## Refresh renderer from Config (call when Config changes)
func refresh() -> void:
	_create_renderer()
	queue_redraw()


## Get renderer type from Config envelope
func _get_renderer_type() -> String:
	var envelope: int = Settings.envelope
	match envelope:
		Envelopes.Type.NONE:
			return "full_field"
		Envelopes.Type.BAR:
			return "drifting_bar"
		Envelopes.Type.WEDGE:
			return "rotating_wedge"
		Envelopes.Type.RING:
			return "expanding_ring"
	push_error("Unknown envelope type: %d" % envelope)
	return ""


## Build stimulus params dictionary from Config
func _build_stimulus_params() -> Dictionary:
	var carrier: int = Settings.carrier
	var envelope: int = Settings.envelope

	var params := {
		"stimulus_width_deg": Settings.stimulus_width_deg,
		"sweep_speed_deg_per_sec": Settings.sweep_speed_deg_per_sec,
		"luminance_min": Settings.luminance_min,
		"luminance_max": Settings.luminance_max,
		"background_luminance": Settings.background_luminance,
		"contrast": Settings.contrast,
		"mean_luminance": Settings.mean_luminance,
		"rotation_deg": Settings.rotation_deg,
		"carrier": carrier,
		"envelope": envelope,
		"strobe_enabled": Settings.strobe_enabled,
		"strobe_frequency_hz": Settings.strobe_frequency_hz,
	}

	# Add carrier-specific params
	match carrier:
		Carriers.Type.CHECKERBOARD:
			params["pattern"] = "checkerboard"
			params["check_size_cm"] = Settings.check_size_cm
			params["check_size_deg"] = Settings.check_size_deg
		Carriers.Type.SOLID:
			params["pattern"] = "solid"

	return params


# -----------------------------------------------------------------------------
# Dataset Management
# -----------------------------------------------------------------------------

## Get the current dataset
func get_dataset() -> StimulusDataset:
	return _dataset


## Create and initialize a dataset for recording (snapshots Config values)
func create_dataset() -> StimulusDataset:
	# Create display geometry from Config
	_display_geometry = DisplayGeometry.from_config()
	_display_geometry.display_width_px = int(size.x)
	_display_geometry.display_height_px = int(size.y)

	# Create and initialize dataset (snapshots Config values at call time)
	_dataset = StimulusDataset.new()
	_dataset.initialize_from_config(_display_geometry)

	# Connect to frame_recorded signal
	_dataset.frame_recorded.connect(_on_dataset_frame_recorded)

	return _dataset


func _on_dataset_frame_recorded(frame_index: int) -> void:
	frame_recorded.emit(frame_index)


## Export the dataset to the specified directory
func export_dataset(output_dir: String) -> Error:
	if _dataset == null:
		push_error("No dataset to export")
		return ERR_DOES_NOT_EXIST

	return DatasetExporter.export_dataset(_dataset, output_dir)


# -----------------------------------------------------------------------------
# Animation Control
# -----------------------------------------------------------------------------

## Start the stimulus animation
func start() -> void:
	# Ensure renderer exists
	if _renderer == null:
		_create_renderer()

	if _renderer == null:
		push_error("Cannot start stimulus: no renderer")
		return

	# Initialize VsyncTimestampProvider for hardware timestamps
	_init_vsync_provider()

	is_running = true
	_elapsed_sec = 0.0
	_sweep_progress = 0.0
	_sweep_frame_count = 0
	_timing_log_timer = 0.0

	# Create and start dataset recording
	if _dataset == null:
		create_dataset()
	if _dataset:
		_dataset.start_recording()

	# Show shader rect if using shader-based renderer
	if _renderer.requires_shader() and _shader_rect:
		_shader_rect.visible = true

	state_changed.emit("running")


## Initialize VsyncTimestampProvider for TRUE hardware vsync timestamps
func _init_vsync_provider() -> void:
	# Clean up any existing provider
	if _vsync_provider:
		_vsync_provider.stop()
		_vsync_provider = null
	_hardware_timestamps_available = false

	# Check if VsyncTimestampProvider class exists (from GDExtension)
	if not ClassDB.class_exists(&"VsyncTimestampProvider"):
		ErrorHandler.report(
			ErrorHandler.Code.HARDWARE_NOT_FOUND,
			"VsyncTimestampProvider not available",
			"GDExtension not loaded. Hardware vsync timestamps UNAVAILABLE - data will NOT be scientifically valid.",
			ErrorHandler.Severity.CRITICAL,
			ErrorHandler.Category.STIMULUS
		)
		return

	# Create provider
	_vsync_provider = ClassDB.instantiate(&"VsyncTimestampProvider")
	if _vsync_provider == null:
		ErrorHandler.report(
			ErrorHandler.Code.HARDWARE_NOT_FOUND,
			"Failed to instantiate VsyncTimestampProvider",
			"Hardware vsync timestamps UNAVAILABLE - data will NOT be scientifically valid.",
			ErrorHandler.Severity.CRITICAL,
			ErrorHandler.Category.STIMULUS
		)
		return

	# Start capturing on the configured display
	var display_index := Session.display_index
	if not _vsync_provider.start(display_index):
		var error_msg: String = _vsync_provider.get_error()
		ErrorHandler.report(
			ErrorHandler.Code.HARDWARE_NOT_FOUND,
			"VsyncTimestampProvider failed to start",
			"%s. Hardware vsync timestamps UNAVAILABLE - data will NOT be scientifically valid." % error_msg,
			ErrorHandler.Severity.CRITICAL,
			ErrorHandler.Category.STIMULUS
		)
		_vsync_provider = null
		return

	_hardware_timestamps_available = true
	print("VsyncTimestampProvider: Started on display %d - TRUE hardware timestamps enabled" % display_index)


## Stop the stimulus animation
func stop() -> void:
	is_running = false

	# Finalize timestamps with hardware mapping before stopping dataset
	if _dataset and _dataset.is_recording():
		_finalize_hardware_timestamps()
		_dataset.stop_recording()

	# Stop vsync provider
	if _vsync_provider:
		_vsync_provider.stop()
		_vsync_provider = null
	_hardware_timestamps_available = false

	state_changed.emit("stopped")


## Map all software timestamps to hardware vsync timestamps at end of acquisition
func _finalize_hardware_timestamps() -> void:
	if _vsync_provider == null:
		push_error("Cannot finalize timestamps - no VsyncTimestampProvider")
		push_error("WARNING: Timestamps are SOFTWARE timing, NOT hardware vsync")
		if _dataset:
			_dataset.set_hardware_timestamps(false)
			_dataset.set_timestamps_finalized(false)
		return

	if _dataset == null or _dataset.timestamps_us.size() == 0:
		return

	var vsync_times: PackedInt64Array = _vsync_provider.get_vsync_timestamps()
	if vsync_times.size() == 0:
		push_error("No vsync timestamps captured - cannot map to hardware timestamps")
		push_error("WARNING: Timestamps are SOFTWARE timing, NOT hardware vsync")
		_dataset.set_hardware_timestamps(false)
		_dataset.set_timestamps_finalized(false)
		return

	print("VsyncTimestampProvider: Mapping %d frames to %d vsync timestamps" % [
		_dataset.timestamps_us.size(), vsync_times.size()])

	var mapping_failures := 0
	var vsync_idx := 0

	# Map each software timestamp to its corresponding hardware vsync
	for i in range(_dataset.timestamps_us.size()):
		var software_ts: int = _dataset.timestamps_us[i]

		# Find the first vsync timestamp greater than software_ts
		while vsync_idx < vsync_times.size() and vsync_times[vsync_idx] <= software_ts:
			vsync_idx += 1

		if vsync_idx < vsync_times.size():
			# Replace software timestamp with hardware vsync timestamp
			_dataset.timestamps_us[i] = vsync_times[vsync_idx]
		else:
			# No matching vsync found - keep software timestamp but log error
			mapping_failures += 1

	if mapping_failures > 0:
		push_error("Failed to map %d frames to hardware vsync timestamps" % mapping_failures)
		push_error("WARNING: %d timestamps are SOFTWARE timing, NOT hardware vsync" % mapping_failures)
		_dataset.set_hardware_timestamps(false)
		_dataset.set_timestamps_finalized(true)  # Attempted but failed
	else:
		print("VsyncTimestampProvider: Successfully mapped all %d frames to hardware timestamps" % [
			_dataset.timestamps_us.size()])
		_dataset.set_hardware_timestamps(true)
		_dataset.set_timestamps_finalized(true)


## Set current direction
func set_direction(direction: String) -> void:
	_direction = direction


## Set whether currently in baseline period
func set_baseline(baseline: bool) -> void:
	_is_baseline = baseline


## Set sweep progress (0.0 to 1.0)
func set_progress(progress: float) -> void:
	_sweep_progress = clamp(progress, 0.0, 1.0)


## Set sweep metadata
func set_sweep_info(index: int, total: int) -> void:
	_sweep_index = index
	_total_sweeps = total


# -----------------------------------------------------------------------------
# Sequencer Integration
# -----------------------------------------------------------------------------

## Connect display to a sequencer for automatic control
func connect_to_sequencer(sequencer: StimulusSequencer) -> void:
	if _sequencer:
		disconnect_from_sequencer()

	_sequencer = sequencer

	sequencer.state_changed.connect(_on_sequencer_state_changed)
	sequencer.sweep_started.connect(_on_sequencer_sweep_started)
	sequencer.sweep_completed.connect(_on_sequencer_sweep_completed)
	sequencer.progress_updated.connect(_on_sequencer_progress_updated)
	sequencer.sequence_started.connect(_on_sequencer_started)
	sequencer.sequence_completed.connect(_on_sequencer_completed)


## Disconnect from sequencer
func disconnect_from_sequencer() -> void:
	if _sequencer == null:
		return

	if _sequencer.state_changed.is_connected(_on_sequencer_state_changed):
		_sequencer.state_changed.disconnect(_on_sequencer_state_changed)
	if _sequencer.sweep_started.is_connected(_on_sequencer_sweep_started):
		_sequencer.sweep_started.disconnect(_on_sequencer_sweep_started)
	if _sequencer.sweep_completed.is_connected(_on_sequencer_sweep_completed):
		_sequencer.sweep_completed.disconnect(_on_sequencer_sweep_completed)
	if _sequencer.progress_updated.is_connected(_on_sequencer_progress_updated):
		_sequencer.progress_updated.disconnect(_on_sequencer_progress_updated)
	if _sequencer.sequence_started.is_connected(_on_sequencer_started):
		_sequencer.sequence_started.disconnect(_on_sequencer_started)
	if _sequencer.sequence_completed.is_connected(_on_sequencer_completed):
		_sequencer.sequence_completed.disconnect(_on_sequencer_completed)

	_sequencer = null


func _on_sequencer_state_changed(new_state: StimulusSequencer.State, _old_state: StimulusSequencer.State) -> void:
	match new_state:
		StimulusSequencer.State.BASELINE_START, StimulusSequencer.State.BASELINE_END:
			set_baseline(true)
		StimulusSequencer.State.SWEEP:
			set_baseline(false)
		StimulusSequencer.State.INTER_STIMULUS, StimulusSequencer.State.INTER_DIRECTION:
			set_baseline(true)
		StimulusSequencer.State.COMPLETE:
			set_baseline(true)
			stop()


func _on_sequencer_sweep_started(sweep_index: int, direction: String) -> void:
	set_direction(direction)
	set_sweep_info(sweep_index, _sequencer.get_total_sweeps() if _sequencer else 1)
	_sweep_frame_count = 0
	_sweep_progress = 0.0


func _on_sequencer_sweep_completed(_idx: int, _dir: String) -> void:
	sweep_completed.emit(_direction)


func _on_sequencer_progress_updated(_elapsed: float, _total_sec: float, _percent: float) -> void:
	if _sequencer and _sequencer.state == StimulusSequencer.State.SWEEP:
		_sweep_progress = _sequencer.get_state_progress()


func _on_sequencer_started() -> void:
	start()


func _on_sequencer_completed() -> void:
	stop()
	state_changed.emit("complete")


# -----------------------------------------------------------------------------
# Frame Processing
# -----------------------------------------------------------------------------

func _process(delta: float) -> void:
	if not is_running:
		# Still update renderer for preview
		if _renderer:
			_update_render_state()
			_renderer.update(0.0, _render_state)
		queue_redraw()
		return

	# Periodic timing log (uses dataset's vsync timestamps)
	_timing_log_timer += delta
	if _timing_log_timer >= _timing_log_interval:
		_log_timing_stats()
		_timing_log_timer = 0.0

	# Get timing from sequencer (frame-locked) or use preview mode
	if _sequencer and _sequencer.is_running():
		# Frame-locked timing: sequencer is advanced in _on_frame_post_draw()
		_elapsed_sec = _sequencer.get_elapsed_time()
		if _sequencer.state == StimulusSequencer.State.SWEEP:
			_sweep_progress = _sequencer.get_state_progress()
		else:
			_sweep_progress = 0.0
	else:
		# Preview mode: calculate progress from elapsed time (for UI preview only)
		_elapsed_sec += delta
		var sweep_duration := Settings.sweep_duration_sec
		if sweep_duration > 0:
			# Loop the animation
			var sweep_elapsed := fmod(_elapsed_sec, sweep_duration)
			_sweep_progress = sweep_elapsed / sweep_duration
		else:
			_sweep_progress = 0.0

	# Update render state
	_update_render_state()

	# Update renderer
	if _renderer:
		_renderer.update(delta, _render_state)

	# NOTE: Frame recording and sequencer advancement happen in _on_frame_post_draw()
	# This ensures timing is locked to vsync boundaries

	queue_redraw()


func _update_render_state() -> void:
	_render_state.direction = _direction
	_render_state.progress = _sweep_progress
	_render_state.is_baseline = _is_baseline
	_render_state.elapsed_sec = _elapsed_sec
	_render_state.sweep_index = _sweep_index
	_render_state.total_sweeps = _total_sweeps


## Called by RenderingServer.frame_post_draw signal AFTER the frame is rendered.
## This is the vsync boundary - ALL timing decisions happen here.
## Captures software timestamp and maps to hardware vsync via VsyncTimestampProvider.
func _on_frame_post_draw() -> void:
	# Toggle sync patch for photodiode detection
	# This creates a visual signal that the photodiode can detect with hardware timing
	if _sync_patch and _sync_patch_enabled and is_running:
		_sync_patch_state = not _sync_patch_state
		_sync_patch.color = Color.WHITE if _sync_patch_state else Color.BLACK

	# Capture software timestamp immediately when frame_post_draw fires
	# This is AFTER rendering completes, BEFORE presentation/vsync
	_software_timestamp_us = Time.get_ticks_usec()

	# CRITICAL: Advance sequencer at vsync boundary
	# This ensures state transitions are frame-locked (never happen mid-frame)
	if _sequencer and _sequencer.is_running():
		_sequencer.advance_frame()

	# For real-time display, try to get hardware timestamp immediately
	# (Final mapping happens at stop() for all frames)
	if _vsync_provider and _hardware_timestamps_available:
		var hw_ts: int = _vsync_provider.map_to_hardware_timestamp(_software_timestamp_us)
		if hw_ts > 0:
			_vsync_timestamp_us = hw_ts
		else:
			# Hardware timestamp not yet available - use software for now
			# Will be properly mapped at finalization
			_vsync_timestamp_us = _software_timestamp_us
	else:
		# No hardware timestamps - use software (with warning already logged)
		_vsync_timestamp_us = _software_timestamp_us

	# Record frame - uses _software_timestamp_us which will be mapped at finalization
	_record_frame_with_timestamp()


func _record_frame_with_timestamp() -> void:
	if _dataset == null or not _dataset.is_recording():
		return
	if not is_running:
		return

	# Use the software timestamp captured in frame_post_draw callback
	# This will be mapped to hardware vsync at finalization
	assert(_software_timestamp_us > 0, "Software timestamp not captured - frame_post_draw not called")

	# Increment frame counters (moved from _process to ensure counts match recorded frames)
	_frame_count += 1
	_sweep_frame_count += 1

	var condition := _direction
	if _sequencer:
		condition = _sequencer.current_direction

	# Get sequence-agnostic metadata from sequencer
	var condition_occurrence := 0
	var baseline := _is_baseline
	if _sequencer:
		condition_occurrence = _sequencer.get_current_condition_occurrence()
		baseline = _sequencer.is_baseline()

	var paradigm_state := {}
	if _renderer:
		paradigm_state = _renderer.get_paradigm_state()

	# Record with software timestamp - will be mapped to hardware at finalization
	_dataset.record_frame(
		_software_timestamp_us,
		condition,
		_sweep_index,
		_sweep_frame_count,
		_sweep_progress,
		_get_state_name(),
		condition_occurrence,
		baseline,
		paradigm_state
	)
	# NOTE: Refresh rate validation happens at display selection time via DisplayValidator
	# Hardware timestamp mapping happens at stop() via _finalize_hardware_timestamps()


func _get_state_name() -> String:
	if _sequencer:
		match _sequencer.state:
			StimulusSequencer.State.IDLE:
				return "idle"
			StimulusSequencer.State.BASELINE_START:
				return "baseline_start"
			StimulusSequencer.State.SWEEP:
				return "stimulus"
			StimulusSequencer.State.INTER_STIMULUS:
				return "inter_stimulus"
			StimulusSequencer.State.INTER_DIRECTION:
				return "inter_direction"
			StimulusSequencer.State.BASELINE_END:
				return "baseline_end"
			StimulusSequencer.State.COMPLETE:
				return "complete"
	return "stimulus" if not _is_baseline else "baseline"


# -----------------------------------------------------------------------------
# Drawing
# -----------------------------------------------------------------------------

func _draw() -> void:
	if _renderer == null:
		# No renderer - draw error state
		draw_rect(Rect2(0, 0, size.x, size.y), Color.MAGENTA)
		return

	if _renderer.requires_shader():
		# Shader-based renderer: ColorRect handles drawing
		if _shader_rect:
			_shader_rect.visible = not _is_baseline
			if _is_baseline:
				var bg_lum: float = Settings.background_luminance
				draw_rect(Rect2(0, 0, size.x, size.y), Color(bg_lum, bg_lum, bg_lum))
	else:
		# Immediate-mode renderer
		_renderer.render(self)

	if show_overlay:
		_draw_overlay()


func _draw_overlay() -> void:
	var font := ThemeDB.fallback_font
	var font_size := 24
	var text_color := Color.YELLOW
	var line_height := 28

	draw_string(font, Vector2(10, 30), "Frame: %d" % _frame_count, HORIZONTAL_ALIGNMENT_LEFT, -1, font_size, text_color)
	draw_string(font, Vector2(10, 30 + line_height), "Sweep: %d" % _sweep_frame_count, HORIZONTAL_ALIGNMENT_LEFT, -1, font_size, text_color)

	# Use dataset's frame deltas (consistent µs units)
	if _dataset and _dataset.frame_deltas_us.size() > 0:
		var last_delta_us: int = _dataset.frame_deltas_us[_dataset.frame_deltas_us.size() - 1]
		draw_string(font, Vector2(10, 30 + line_height * 2), "Delta: %d us" % last_delta_us, HORIZONTAL_ALIGNMENT_LEFT, -1, font_size, text_color)

	if _renderer:
		draw_string(font, Vector2(10, 30 + line_height * 3), "Type: %s" % _renderer.get_type_id(), HORIZONTAL_ALIGNMENT_LEFT, -1, font_size, text_color)

	# Show hardware timestamp status
	var hw_status := "HW vsync: "
	if _hardware_timestamps_available:
		hw_status += "ACTIVE"
		draw_string(font, Vector2(10, 30 + line_height * 4), hw_status, HORIZONTAL_ALIGNMENT_LEFT, -1, font_size, Color.GREEN)
	else:
		hw_status += "UNAVAILABLE"
		draw_string(font, Vector2(10, 30 + line_height * 4), hw_status, HORIZONTAL_ALIGNMENT_LEFT, -1, font_size, Color.RED)

	# Show sync patch status (for photodiode hardware timestamps)
	if _sync_patch_enabled:
		var sync_status := "Sync patch: ON"
		draw_string(font, Vector2(10, 30 + line_height * 5), sync_status, HORIZONTAL_ALIGNMENT_LEFT, -1, font_size, Color.CYAN)


# -----------------------------------------------------------------------------
# Utilities
# -----------------------------------------------------------------------------

func _log_timing_stats() -> void:
	# Use dataset's frame deltas (software timestamps during acquisition,
	# mapped to hardware vsync at finalization)
	if _dataset == null or _dataset.frame_deltas_us.size() < 10:
		return

	# Only log after refresh rate has been validated
	if not _dataset._refresh_rate_validated:
		return

	var deltas := _dataset.frame_deltas_us
	var n := deltas.size()

	# Compute stats from last 60 frames (or all if fewer)
	var start_idx := maxi(0, n - 60)
	var count := n - start_idx

	var sum: int = 0
	var min_delta: int = deltas[start_idx]
	var max_delta: int = deltas[start_idx]

	for i in range(start_idx, n):
		var delta: int = deltas[i]
		sum += delta
		min_delta = mini(min_delta, delta)
		max_delta = maxi(max_delta, delta)

	var mean_us := float(sum) / count
	var expected_delta_us := _dataset._expected_delta_us

	var variance_sum: float = 0.0
	for i in range(start_idx, n):
		var diff := float(deltas[i]) - mean_us
		variance_sum += diff * diff
	var std_dev_us := sqrt(variance_sum / count)

	var jitter_us := maxf(abs(float(max_delta) - mean_us), abs(float(min_delta) - mean_us))

	var timestamp_type := "HW vsync" if _hardware_timestamps_available else "SOFTWARE"
	print("--- Stimulus Frame Timing (%s) ---" % timestamp_type)
	print("  Mean:   %.0f us (expected: %d us)" % [mean_us, expected_delta_us])
	print("  StdDev: %.0f us, Jitter: %.0f us" % [std_dev_us, jitter_us])


func _notification(what: int) -> void:
	if what == NOTIFICATION_RESIZED:
		if _renderer:
			_create_renderer()
	elif what == NOTIFICATION_PREDELETE:
		# Disconnect from RenderingServer signal to prevent errors
		if RenderingServer.frame_post_draw.is_connected(_on_frame_post_draw):
			RenderingServer.frame_post_draw.disconnect(_on_frame_post_draw)
		# Clean up vsync provider
		if _vsync_provider:
			_vsync_provider.stop()
			_vsync_provider = null
	elif what == NOTIFICATION_VISIBILITY_CHANGED:
		if is_visible_in_tree() and _renderer == null:
			call_deferred("_create_renderer")
