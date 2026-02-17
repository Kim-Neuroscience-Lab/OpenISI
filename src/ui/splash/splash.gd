extends Control
## Splash screen shown during application startup.
##
## Displays OpenISI branding while autoloads initialize and hardware is
## enumerated, then transitions to the main application.

signal splash_complete

const MAIN_SCENE_PATH := "res://src/main.tscn"

var _background: ColorRect = null
var _center: CenterContainer = null
var _vbox: VBoxContainer = null
var _title_label: Label = null
var _status_label: Label = null

var _main_scene_loaded := false
var _hardware_ready := false
var _display_validated := false


func _ready() -> void:
	_build_ui()
	_status_label.text = "Initializing..."

	# Start initialization after a short delay to ensure splash renders
	var timer := get_tree().create_timer(0.2)
	timer.timeout.connect(_start_initialization)


func _build_ui() -> void:
	# Background
	_background = ColorRect.new()
	_background.name = "Background"
	_background.set_anchors_preset(Control.PRESET_FULL_RECT)
	_background.color = AppTheme.BG_BASE
	add_child(_background)

	# Center container
	_center = CenterContainer.new()
	_center.name = "CenterContainer"
	_center.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(_center)

	# VBox for title and status
	_vbox = VBoxContainer.new()
	_vbox.name = "VBoxContainer"
	_vbox.theme_type_variation = "VBox2XL"
	_center.add_child(_vbox)

	# Title label
	_title_label = Label.new()
	_title_label.name = "TitleLabel"
	_title_label.text = "OpenISI"
	_title_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_title_label.theme_type_variation = "LabelSplash"
	_vbox.add_child(_title_label)

	# Status label
	_status_label = Label.new()
	_status_label.name = "StatusLabel"
	_status_label.text = "Starting..."
	_status_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_status_label.theme_type_variation = "LabelSmallDim"
	_vbox.add_child(_status_label)


func _start_initialization() -> void:
	# Start hardware enumeration
	_status_label.text = "Detecting hardware..."
	await get_tree().process_frame

	HardwareManager.cameras_enumerated.connect(_on_hardware_ready, CONNECT_ONE_SHOT)
	HardwareManager.enumeration_failed.connect(_on_hardware_ready, CONNECT_ONE_SHOT)

	# Enumerate monitors first (fast, synchronous)
	HardwareManager.enumerate_monitors()

	# Start display validation (validation window shows splash-like UI)
	_status_label.text = "Validating display..."
	await get_tree().process_frame
	_start_display_validation()

	# Camera enumeration runs in background thread (non-blocking)
	HardwareManager.enumerate_cameras_async()

	# Load main scene
	_status_label.text = "Loading..."
	await get_tree().process_frame

	var main_scene := load(MAIN_SCENE_PATH)
	if main_scene == null:
		_status_label.text = "Error: Failed to load main scene"
		push_error("Splash: Failed to load main scene: %s" % MAIN_SCENE_PATH)
		return

	_main_scene_loaded = true
	_check_ready_to_transition()


func _on_hardware_ready(_result = null) -> void:
	_hardware_ready = true
	_check_ready_to_transition()


func _start_display_validation() -> void:
	# Connect to validation signals
	DisplayValidator.validation_completed.connect(_on_display_validated, CONNECT_ONE_SHOT)
	DisplayValidator.validation_failed.connect(_on_display_validation_failed, CONNECT_ONE_SHOT)

	# Validate the stimulus display (using native monitor count from HardwareManager)
	var monitors := HardwareManager.get_detected_monitors()
	var target_idx := HardwareManager.get_stimulus_monitor_index()
	print("Splash: %d monitors detected, validating screen %d" % [monitors.size(), target_idx])
	DisplayValidator.validate_display(target_idx)


func _on_display_validated(measured_hz: float, _reported_hz: float, _mismatch: bool) -> void:
	# Store validation result in Session
	var target_idx := HardwareManager.get_stimulus_monitor_index()
	var monitor := HardwareManager.get_monitor_at_index(target_idx)
	if not monitor.is_empty():
		Session.set_selected_display(monitor)
	Session.set_display_validation(measured_hz)

	_display_validated = true
	_check_ready_to_transition()


func _on_display_validation_failed(_reason: String) -> void:
	# Validation failed, but we can still proceed - Setup screen will show the issue
	_display_validated = true
	_check_ready_to_transition()


func _check_ready_to_transition() -> void:
	if _main_scene_loaded and _hardware_ready and _display_validated:
		_transition_to_main()


func _transition_to_main() -> void:
	splash_complete.emit()
	get_tree().change_scene_to_file(MAIN_SCENE_PATH)
