extends Control
## Main window for OpenISI.
##
## Manages the application shell layout (header, content, footer) and
## coordinates screen navigation. Screen-specific content is loaded into
## the content container based on the current screen.
##
## Screen definitions are sourced from Session (SSoT).
##
## Developer tools:
##   Ctrl+Shift+T - Open Timing Diagnostics panel

# Preloaded screen scenes (instant transitions via SceneRegistry)
const _SCREEN_SCENES := {
	Session.Screen.SETUP: SceneRegistry.SetupScreen,
	Session.Screen.FOCUS: SceneRegistry.FocusScreen,
	Session.Screen.STIMULUS: SceneRegistry.StimulusScreen,
	Session.Screen.ACQUIRE: SceneRegistry.AcquireScreen,
	Session.Screen.RESULTS: SceneRegistry.ResultsScreen,
}

# UI References
var _vbox: VBoxContainer = null
var _content_container: MarginContainer = null
var _header: AppHeader = null
var _footer: AppFooter = null

# Current screen scene instance
var _current_screen_scene: Control = null

# Stimulus window
var _stimulus_window: Window = null

# Developer tools
var _timing_diagnostics: TimingDiagnostics = null

# Error dialog
var _error_dialog: ErrorDialog = null

# Update dialog
var _update_dialog: UpdateDialog = null

# Scroll fade effect
var _fade_top: ColorRect = null
var _fade_bottom: ColorRect = null

# Screen content wrapper (provides consistent margins)
var _screen_wrapper: MarginContainer = null


func _ready() -> void:
	# Restore window state from previous session
	Settings.apply_window_state(get_window())

	# Cap framerate (0 = unlimited)
	Engine.max_fps = Settings.target_fps

	# Build UI structure programmatically
	_build_ui()

	# Connect to Session screen changes
	Session.screen_changed.connect(_on_screen_changed)

	# Connect header/footer signals
	_header.screen_clicked.connect(_on_screen_clicked)
	_footer.back_pressed.connect(_on_back_pressed)
	_footer.primary_pressed.connect(_on_primary_pressed)
	_footer.secondary_pressed.connect(_on_secondary_pressed)

	# Connect to CameraClient for status updates (autoload always present)
	CameraClient.connection_changed.connect(_on_camera_connection_changed)

	# Set initial status based on actual state
	_update_status_indicator()

	# Setup error dialog and connect to ErrorHandler
	_setup_error_handling()

	# Setup update dialog
	_update_dialog = UpdateDialog.new()
	_update_dialog.name = "UpdateDialog"
	add_child(_update_dialog)

	# Setup scroll fade overlays
	_setup_scroll_fade()

	# Load initial screen
	_load_screen(Session.current_screen)

	print("Main window ready")


func _build_ui() -> void:
	# Main vertical layout container
	_vbox = VBoxContainer.new()
	_vbox.name = "VBoxContainer"
	_vbox.set_anchors_preset(Control.PRESET_FULL_RECT)
	_vbox.theme_type_variation = "VBoxTight"
	add_child(_vbox)

	# Header
	_header = AppHeader.new()
	_header.name = "AppHeader"
	_vbox.add_child(_header)

	# Content container (expands to fill space between header and footer)
	# Note: MarginContainer has 0 margins by default, no overrides needed
	_content_container = MarginContainer.new()
	_content_container.name = "ContentContainer"
	_content_container.size_flags_vertical = Control.SIZE_EXPAND_FILL
	_vbox.add_child(_content_container)

	# Footer
	_footer = AppFooter.new()
	_footer.name = "AppFooter"
	_vbox.add_child(_footer)


func _notification(what: int) -> void:
	if what == NOTIFICATION_WM_CLOSE_REQUEST:
		_cleanup()
		get_tree().quit()


func _unhandled_input(event: InputEvent) -> void:
	# Ctrl+Shift+T - Toggle Timing Diagnostics
	if event is InputEventKey and event.pressed:
		if event.keycode == KEY_T and event.ctrl_pressed and event.shift_pressed:
			_toggle_timing_diagnostics()
			get_viewport().set_input_as_handled()


func _toggle_timing_diagnostics() -> void:
	if _timing_diagnostics == null:
		_timing_diagnostics = TimingDiagnostics.new()
		_timing_diagnostics.close_requested.connect(_on_timing_diagnostics_closed)
		add_child(_timing_diagnostics)
		_timing_diagnostics.popup_centered()
		print("Timing Diagnostics opened (Ctrl+Shift+T to close)")
	else:
		_timing_diagnostics.hide()
		_timing_diagnostics.queue_free()
		_timing_diagnostics = null


func _on_timing_diagnostics_closed() -> void:
	if _timing_diagnostics:
		_timing_diagnostics.queue_free()
		_timing_diagnostics = null


func _setup_scroll_fade() -> void:
	var shader := load("res://src/ui/theme/shaders/scroll_fade.gdshader")
	if not shader:
		ErrorHandler.report(
			ErrorHandler.Code.OPERATION_FAILED,
			"Failed to load scroll_fade.gdshader",
			"",
			ErrorHandler.Severity.WARNING,
			ErrorHandler.Category.SYSTEM
		)
		return

	# Create fade overlays after layout is complete
	call_deferred("_create_fade_overlays", shader)


func _create_fade_overlays(shader: Shader) -> void:
	const SCROLLBAR_WIDTH := 16  # Leave room for scrollbar

	# Top fade: positioned just below header, fading into content area
	_fade_top = ColorRect.new()
	_fade_top.name = "FadeTop"
	_fade_top.color = Color.WHITE
	_fade_top.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_fade_top.z_index = AppTheme.Z_INDEX_SCROLL_FADE

	_fade_top.anchor_left = 0.0
	_fade_top.anchor_right = 1.0
	_fade_top.anchor_top = 0.0
	_fade_top.anchor_bottom = 0.0
	_fade_top.offset_left = 0
	_fade_top.offset_right = -SCROLLBAR_WIDTH
	_fade_top.offset_top = AppTheme.HEADER_HEIGHT
	_fade_top.offset_bottom = AppTheme.HEADER_HEIGHT + AppTheme.SCROLL_FADE_HEIGHT

	var mat_top := ShaderMaterial.new()
	mat_top.shader = shader
	mat_top.set_shader_parameter("bg_color", AppTheme.BG_BASE)
	mat_top.set_shader_parameter("fade_from_top", true)
	_fade_top.material = mat_top
	add_child(_fade_top)

	# Bottom fade: positioned just above footer, fading into content area
	_fade_bottom = ColorRect.new()
	_fade_bottom.name = "FadeBottom"
	_fade_bottom.color = Color.WHITE
	_fade_bottom.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_fade_bottom.z_index = AppTheme.Z_INDEX_SCROLL_FADE

	_fade_bottom.anchor_left = 0.0
	_fade_bottom.anchor_right = 1.0
	_fade_bottom.anchor_top = 0.0
	_fade_bottom.anchor_bottom = 0.0
	_fade_bottom.offset_left = 0
	_fade_bottom.offset_right = -SCROLLBAR_WIDTH
	_fade_bottom.offset_top = size.y - AppTheme.FOOTER_HEIGHT - AppTheme.SCROLL_FADE_HEIGHT
	_fade_bottom.offset_bottom = size.y - AppTheme.FOOTER_HEIGHT

	var mat_bottom := ShaderMaterial.new()
	mat_bottom.shader = shader
	mat_bottom.set_shader_parameter("bg_color", AppTheme.BG_BASE)
	mat_bottom.set_shader_parameter("fade_from_top", false)
	_fade_bottom.material = mat_bottom
	add_child(_fade_bottom)

	# Update on resize
	resized.connect(_update_fade_positions)


func _update_fade_positions() -> void:
	if not _fade_top or not _fade_bottom:
		return

	_fade_top.offset_top = AppTheme.HEADER_HEIGHT
	_fade_top.offset_bottom = AppTheme.HEADER_HEIGHT + AppTheme.SCROLL_FADE_HEIGHT

	_fade_bottom.offset_top = size.y - AppTheme.FOOTER_HEIGHT - AppTheme.SCROLL_FADE_HEIGHT
	_fade_bottom.offset_bottom = size.y - AppTheme.FOOTER_HEIGHT


# --- Screen Management ---

func _on_screen_changed(new_screen: Session.Screen) -> void:
	print("Screen changed to: ", Session.get_screen_name(new_screen))
	_load_screen(new_screen)
	_update_status_indicator()


func _load_screen(screen: Session.Screen) -> void:
	# Remove current screen (and its wrapper)
	if _screen_wrapper != null:
		_screen_wrapper.queue_free()
		_screen_wrapper = null
		_current_screen_scene = null

	# Get preloaded screen scene (instant, no disk I/O)
	var scene: PackedScene = _SCREEN_SCENES[screen]
	_current_screen_scene = scene.instantiate()

	# Wrap screen in container - each screen handles its own padding internally
	_screen_wrapper = MarginContainer.new()
	_screen_wrapper.name = "ScreenWrapper"
	_screen_wrapper.set_anchors_preset(Control.PRESET_FULL_RECT)

	_screen_wrapper.add_child(_current_screen_scene)
	_content_container.add_child(_screen_wrapper)

	# Connect screen scene signals if available
	_connect_screen_signals(_current_screen_scene)

	print("Loaded screen: ", Session.Screen.keys()[screen])

	# Update footer for this screen
	_update_footer_for_screen(screen)


func _update_footer_for_screen(screen: Session.Screen) -> void:
	var config: Dictionary = Session.get_screen_config(screen)

	# Set primary button text
	_footer.set_primary_text(str(config["primary_text"]))

	# Primary button is always enabled in workspace model
	_footer.set_primary_enabled(true)
	_footer.set_primary_highlighted(true)

	# Show back button based on screen config
	# (handled by footer listening to screen changes)

	# Clear any secondary actions
	_footer.clear_secondary_actions()


func _connect_screen_signals(screen_scene: Control) -> void:
	# Connect common signals that screen scenes might emit
	if screen_scene.has_signal("validation_changed"):
		screen_scene.validation_changed.connect(_on_screen_validation_changed)
	if screen_scene.has_signal("request_next_screen"):
		screen_scene.request_next_screen.connect(_on_request_next_screen)


func _on_screen_validation_changed(_valid: bool) -> void:
	# In workspace model, validation doesn't gate navigation
	# But we can still update visual feedback if desired
	pass


func _on_request_next_screen() -> void:
	_on_primary_pressed()


# --- Button Handlers ---

func _on_screen_clicked(screen: Session.Screen) -> void:
	# Navigate directly to clicked screen
	Session.navigate_to(screen)


func _on_back_pressed() -> void:
	Session.navigate_back()


func _on_primary_pressed() -> void:
	var current := Session.current_screen

	match current:
		Session.Screen.RESULTS:
			# Reset to new session
			Session.reset_session()
		Session.Screen.ACQUIRE:
			# Stop acquisition early
			_stop_acquisition()
		_:
			# Advance to next screen
			Session.navigate_next()


func _on_secondary_pressed(action: String) -> void:
	match action:
		"preview":
			_open_stimulus_preview()
		"test":
			_start_test_mode()
		_:
			print("Unknown secondary action: ", action)


# --- Stimulus Management ---

func _open_stimulus_preview() -> void:
	if _stimulus_window != null:
		_close_stimulus_window()
		return

	if not Session.has_selected_display():
		ErrorHandler.report_display_error(
			"No display selected",
			"Please select a display in Setup before opening stimulus preview."
		)
		return

	var screen_count := DisplayServer.get_screen_count()
	var target_screen := Session.display_index
	print("Opening stimulus preview on monitor %d (%d monitors total)" % [target_screen, screen_count])

	_stimulus_window = Window.new()
	_stimulus_window.title = "Stimulus Preview"
	_stimulus_window.transient = false

	var stimulus_scene := preload("res://src/stimulus/stimulus_display.tscn")
	var stimulus := stimulus_scene.instantiate()
	_stimulus_window.add_child(stimulus)

	add_child(_stimulus_window)

	# Use configured monitor
	if screen_count > 1:
		assert(target_screen >= 0 and target_screen < screen_count,
			"Configured display %d not available (only %d displays detected)" % [target_screen, screen_count])
		_stimulus_window.current_screen = target_screen
		_stimulus_window.mode = Window.MODE_FULLSCREEN
	else:
		# Single monitor - open windowed for testing
		var screen_size := DisplayServer.screen_get_size(0)
		_stimulus_window.size = Vector2i(screen_size.x >> 1, screen_size.y - 100)
		_stimulus_window.position = Vector2i(screen_size.x >> 1, 50)
		print("  (Single monitor mode - windowed for testing)")


func _close_stimulus_window() -> void:
	if _stimulus_window != null:
		_stimulus_window.queue_free()
		_stimulus_window = null


func _start_test_mode() -> void:
	print("Test mode not yet implemented")


func _stop_acquisition() -> void:
	# Stop the camera acquisition and finalize session
	Session.set_acquisition_complete(0, 0)
	Session.navigate_to(Session.Screen.RESULTS)


# --- Status Management ---

func _on_camera_connection_changed(_connected: bool) -> void:
	_update_status_indicator()


func _update_status_indicator() -> void:
	var camera_connected := CameraClient.is_daemon_connected()
	var screen_count := DisplayServer.get_screen_count()
	var single_monitor := screen_count == 1

	_header.set_status_pulsing(false)

	if not camera_connected:
		_header.set_status("warning", "Camera Disconnected")
	elif Session.current_screen == Session.Screen.ACQUIRE:
		# During acquisition, show active status
		_header.set_status("info", "Recording")
		_header.set_status_pulsing(true)
	elif single_monitor:
		_header.set_status("warning", "Single Monitor (Test Mode)")
	else:
		_header.set_status("success", "Ready")


# --- Utilities ---

func _cleanup() -> void:
	print("Running cleanup...")
	# Save window state before closing
	Settings.update_window_state(get_window())
	_close_stimulus_window()
	CameraClient.cleanup()
	print("Cleanup complete")


# --- Error Handling ---

func _setup_error_handling() -> void:
	_error_dialog = ErrorDialog.new()
	_error_dialog.name = "ErrorDialog"
	add_child(_error_dialog)

	# Connect to ErrorHandler signals
	ErrorHandler.error_occurred.connect(_on_error_occurred)


func _on_error_occurred(error: ErrorHandler.AppError) -> void:
	# Only show dialog for ERROR and CRITICAL severity
	if error.severity >= ErrorHandler.Severity.ERROR:
		_error_dialog.show_error(error)
