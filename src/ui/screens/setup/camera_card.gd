## CameraCard - Camera selection and connection component
##
## Handles camera enumeration, format selection (for AVFoundation cameras),
## connection management, and format mismatch handling.
## Interacts directly with Session (SSoT) and CameraClient for operations.
## Emits signals when selection or connection state changes.
class_name CameraCard
extends MarginContainer


signal camera_selected(device: Dictionary)
signal connection_changed(connected: bool)
signal format_changed(format: Dictionary)


# UI Controls
var _card: Control = null
var _camera_selector: StyledOptionButton = null
var _format_selector: StyledOptionButton = null
var _format_row: HBoxContainer = null
var _camera_resolution_row: InfoRow = null
var _camera_status_row: InfoRow = null
var _camera_refresh_button: StyledButton = null
var _camera_scan_label: Label = null
var _camera_format_hint_label: Label = null
var _connect_button: StyledButton = null

# State
var _format_locked_due_to_mismatch := false
var _is_connecting := false


func _ready() -> void:
	_build_ui()
	_connect_signals()
	_load_state()


func _build_ui() -> void:
	size_flags_horizontal = Control.SIZE_EXPAND_FILL

	_card = Card.new()
	_card.title = "Camera"
	_card.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	add_child(_card)

	var content := VBoxContainer.new()
	content.theme_type_variation = "VBoxMD"
	_card.get_content_slot().add_child(content)

	# Device selector row
	var device_row := HBoxContainer.new()
	device_row.theme_type_variation = "HBoxSM"
	content.add_child(device_row)

	var device_label := Label.new()
	device_label.text = "Device"
	device_label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_SM
	device_row.add_child(device_label)

	_camera_selector = StyledOptionButton.new()
	_camera_selector.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_camera_selector.custom_minimum_size.y = AppTheme.INPUT_HEIGHT
	device_row.add_child(_camera_selector)

	# Format selector row (for cameras with discrete formats like AVFoundation)
	_format_row = HBoxContainer.new()
	_format_row.theme_type_variation = "HBoxSM"
	_format_row.visible = false  # Hidden until camera with formats is selected
	content.add_child(_format_row)

	var format_label := Label.new()
	format_label.text = "Format"
	format_label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_SM
	_format_row.add_child(format_label)

	_format_selector = StyledOptionButton.new()
	_format_selector.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_format_selector.custom_minimum_size.y = AppTheme.INPUT_HEIGHT
	_format_row.add_child(_format_selector)

	# Button row - both buttons right-aligned: Connect, Refresh
	var button_row := HBoxContainer.new()
	button_row.theme_type_variation = "HBoxSM"
	content.add_child(button_row)

	var button_spacer := Control.new()
	button_spacer.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	button_row.add_child(button_spacer)

	_connect_button = StyledButton.new()
	_connect_button.text = "Connect"
	_connect_button.button_pressed = true  # Nightlight - required action
	button_row.add_child(_connect_button)

	_camera_refresh_button = StyledButton.new()
	_camera_refresh_button.text = "Refresh"
	button_row.add_child(_camera_refresh_button)

	# Scan status label (hidden by default)
	_camera_scan_label = Label.new()
	_camera_scan_label.text = "Scanning for cameras..."
	_camera_scan_label.theme_type_variation = "LabelSmallDim"
	_camera_scan_label.visible = false
	content.add_child(_camera_scan_label)

	# Format hint label (shown when camera delivers different format than selected)
	_camera_format_hint_label = Label.new()
	_camera_format_hint_label.theme_type_variation = "LabelSmallError"
	_camera_format_hint_label.visible = false
	content.add_child(_camera_format_hint_label)

	# Divider
	var divider := Divider.new()
	divider.margin = 4
	content.add_child(divider)

	# Specs
	_camera_status_row = InfoRow.new()
	_camera_status_row.label_text = "Status"
	_camera_status_row.value_text = "Disconnected"
	_camera_status_row.status = "error"
	content.add_child(_camera_status_row)

	_camera_resolution_row = InfoRow.new()
	_camera_resolution_row.label_text = "Resolution"
	_camera_resolution_row.value_text = "Not selected"
	_camera_resolution_row.mono_value = true
	content.add_child(_camera_resolution_row)


func _connect_signals() -> void:
	# HardwareManager signals
	HardwareManager.cameras_enumerated.connect(_on_cameras_enumerated)
	HardwareManager.enumeration_failed.connect(_on_enumeration_failed)

	# CameraClient signals (direct connection to SSoT)
	CameraClient.connection_changed.connect(_on_camera_connection_changed)
	CameraClient.connection_failed.connect(_on_camera_connection_failed)
	CameraClient.connection_attempt_complete.connect(_on_camera_attempt_complete)
	CameraClient.format_mismatch_detected.connect(_on_camera_format_mismatch)

	# UI signals
	if _connect_button:
		_connect_button.pressed.connect(_on_connect_button_pressed)

	if _camera_selector:
		_camera_selector.item_selected.connect(_on_camera_selected)

	if _format_selector:
		_format_selector.item_selected.connect(_on_format_selected)

	if _camera_refresh_button:
		_camera_refresh_button.pressed.connect(_on_refresh_pressed)


func _load_state() -> void:
	# Load camera devices
	if HardwareManager.has_enumerated_cameras():
		var cameras := HardwareManager.get_camera_devices()
		_populate_camera_selector(cameras)
	else:
		_scan_cameras()

	_update_camera_status()


# -----------------------------------------------------------------------------
# Public API
# -----------------------------------------------------------------------------

## Get the currently selected device dictionary
func get_selected_device() -> Dictionary:
	if not _camera_selector or _camera_selector.item_count == 0:
		return {}

	var device_idx := _camera_selector.get_selected_id()
	return HardwareManager.get_device_at_index(device_idx)


## Connect to the currently selected camera
func connect_camera() -> void:
	if CameraClient.is_daemon_connected():
		return
	if not Session.has_selected_camera():
		return

	_is_connecting = true
	CameraClient.connect_to_daemon_async()
	_update_camera_status()


## Disconnect from the camera daemon
func disconnect_camera() -> void:
	if CameraClient.is_daemon_connected():
		CameraClient.disconnect_from_daemon()


## Refresh the camera device list
func refresh_devices() -> void:
	_format_locked_due_to_mismatch = false
	if _camera_format_hint_label:
		_camera_format_hint_label.visible = false
	_scan_cameras()


## Check if camera is connected
func is_camera_connected() -> bool:
	return CameraClient.is_daemon_connected()


## Check if connection is in progress
func is_connecting() -> bool:
	return _is_connecting or CameraClient.is_connecting()


# -----------------------------------------------------------------------------
# Internal Methods
# -----------------------------------------------------------------------------

func _scan_cameras() -> void:
	if HardwareManager.is_scanning_cameras():
		return

	# Show scanning state
	if _camera_scan_label:
		_camera_scan_label.text = "Scanning for cameras..."
		_camera_scan_label.visible = true
	if _camera_refresh_button:
		_camera_refresh_button.disabled = true
	if _camera_selector:
		_camera_selector.disabled = true

	# Trigger async enumeration
	HardwareManager.enumerate_cameras_async()


func _populate_camera_selector(devices: Array[Dictionary]) -> void:
	if not _camera_selector:
		return

	_camera_selector.clear()

	# Find currently selected camera
	var selected_idx := -1
	if Session.has_selected_camera():
		var current_type := Session.camera_type
		var current_index := Session.camera_device_index
		selected_idx = HardwareManager.find_device_index(current_type, current_index)
		if selected_idx < 0:
			push_warning("CameraCard: Previously selected camera not found: %s[%d]" % [current_type, current_index])

	for i in range(devices.size()):
		var device: Dictionary = devices[i]
		var device_name: String = str(device["name"])
		var has_formats := device.has("formats") and device["formats"] is Array

		# Build display label - just name for format-based cameras, name + resolution for others
		var label := device_name
		if not has_formats and device.has("width") and device.has("height"):
			var width: int = int(device["width"])
			var height: int = int(device["height"])
			if width > 0 and height > 0:
				label += " (%dx%d)" % [width, height]

		_camera_selector.add_item(label, i)

	if _camera_selector.item_count > 0:
		_camera_selector.disabled = false
		# Restore previous selection if valid, otherwise select first available
		if selected_idx >= 0 and selected_idx < devices.size():
			_camera_selector.select(selected_idx)
			_on_camera_device_changed(devices[selected_idx])
		else:
			# No previous selection - select first item
			_camera_selector.select(0)
			_on_camera_device_changed(devices[0])
		_update_camera_status()
	else:
		_camera_selector.add_item("No cameras found", 0)
		_camera_selector.disabled = true
		# Hide format selector when no cameras
		if _format_row:
			_format_row.visible = false


func _on_camera_device_changed(device: Dictionary) -> void:
	# Reset format lock when switching cameras
	_format_locked_due_to_mismatch = false
	if _camera_format_hint_label:
		_camera_format_hint_label.visible = false

	var has_formats := device.has("formats") and device["formats"] is Array

	if has_formats:
		# Camera with discrete formats (e.g., AVFoundation)
		_populate_format_selector(device)
		if _format_row:
			_format_row.visible = true
	else:
		# Camera with direct width/height (e.g., OpenCV, PCO)
		if _format_row:
			_format_row.visible = false
		# Set the full device info in Session (SSoT)
		Session.set_selected_camera(device)

	camera_selected.emit(device)


func _populate_format_selector(device: Dictionary) -> void:
	if not _format_selector:
		return

	_format_selector.clear()

	var formats: Array = device["formats"]
	if formats.is_empty():
		_format_selector.add_item("No formats available", 0)
		_format_selector.disabled = true
		return

	# Build unique format entries (resolution + fps range)
	var unique_formats: Array[Dictionary] = []
	var seen_keys: Dictionary = {}

	for fmt in formats:
		var width: int = int(fmt["width"])
		var height: int = int(fmt["height"])
		var min_fps: float = float(fmt["min_fps"])
		var max_fps: float = float(fmt["max_fps"])

		# Create unique key for deduplication
		var key := "%dx%d@%.0f-%.0f" % [width, height, min_fps, max_fps]
		if seen_keys.has(key):
			continue
		seen_keys[key] = true

		var format_entry := {
			"width": width,
			"height": height,
			"min_fps": min_fps,
			"max_fps": max_fps,
		}
		# Include pixel format info if present
		if fmt.has("bits_per_pixel"):
			format_entry["bits_per_pixel"] = int(fmt["bits_per_pixel"])
		if fmt.has("bits_per_component"):
			format_entry["bits_per_component"] = int(fmt["bits_per_component"])
		unique_formats.append(format_entry)

	# Sort by resolution (largest first) then by max fps
	unique_formats.sort_custom(func(a: Dictionary, b: Dictionary) -> bool:
		var a_pixels: int = int(a["width"]) * int(a["height"])
		var b_pixels: int = int(b["width"]) * int(b["height"])
		if a_pixels != b_pixels:
			return a_pixels > b_pixels
		return float(a["max_fps"]) > float(b["max_fps"])
	)

	# Add items to selector
	for i in range(unique_formats.size()):
		var fmt: Dictionary = unique_formats[i]
		var width: int = int(fmt["width"])
		var height: int = int(fmt["height"])
		var min_fps: float = float(fmt["min_fps"])
		var max_fps: float = float(fmt["max_fps"])

		var label: String
		if absf(min_fps - max_fps) < 0.1:
			label = "%dx%d @ %.0f fps" % [width, height, max_fps]
		else:
			label = "%dx%d @ %.0f-%.0f fps" % [width, height, min_fps, max_fps]

		_format_selector.add_item(label, i)
		# Store format data in metadata
		_format_selector.set_item_metadata(i, fmt)

	_format_selector.disabled = false

	# Select first format by default and apply it
	if _format_selector.item_count > 0:
		_format_selector.select(0)
		_apply_selected_format(device, 0)


func _apply_selected_format(device: Dictionary, format_index: int) -> void:
	if not _format_selector or format_index < 0 or format_index >= _format_selector.item_count:
		return

	var fmt: Dictionary = _format_selector.get_item_metadata(format_index)
	if fmt.is_empty():
		return

	# Update Session (SSoT) with device and format merged
	Session.set_selected_camera_with_format(device, fmt)

	format_changed.emit(fmt)


func _update_camera_status() -> void:
	var camera_connected := CameraClient.is_daemon_connected()
	var connecting := _is_connecting or CameraClient.is_connecting()

	if camera_connected:
		if _camera_status_row:
			_camera_status_row.set_value("Connected")
			_camera_status_row.status = "success"

		if _camera_resolution_row and Session.has_selected_camera():
			_camera_resolution_row.set_value(_get_resolution_string())

		if _connect_button:
			_connect_button.text = "Disconnect"
			_connect_button.disabled = false
			_connect_button.button_pressed = false  # Default style when connected

		# Disable selection and refresh while connected
		if _camera_selector:
			_camera_selector.disabled = true
		if _camera_refresh_button:
			_camera_refresh_button.disabled = true
		if _format_selector:
			_format_selector.disabled = true
	else:
		if _camera_status_row:
			_camera_status_row.set_value("Disconnected")
			_camera_status_row.status = "error"

		if _camera_resolution_row and Session.has_selected_camera():
			_camera_resolution_row.set_value(_get_resolution_string())

		if _connect_button:
			if connecting:
				_connect_button.text = "Connecting..."
				_connect_button.disabled = true
			elif not Session.has_selected_camera():
				_connect_button.text = "Select Camera"
				_connect_button.disabled = true
			else:
				_connect_button.text = "Connect"
				_connect_button.disabled = false
				_connect_button.button_pressed = true  # Nightlight - required action

		# Enable selection and refresh while disconnected
		if _camera_selector:
			_camera_selector.disabled = false
		if _camera_refresh_button:
			_camera_refresh_button.disabled = false
		# Format selector stays locked if mismatch was detected
		if _format_selector and not _format_locked_due_to_mismatch:
			_format_selector.disabled = false


func _get_resolution_string() -> String:
	if not Session.has_selected_camera():
		return "Not selected"
	return "%d x %d" % [Session.camera_width_px, Session.camera_height_px]


# -----------------------------------------------------------------------------
# Signal Handlers - HardwareManager
# -----------------------------------------------------------------------------

func _on_cameras_enumerated(cameras: Array[Dictionary]) -> void:
	_populate_camera_selector(cameras)

	# Hide scanning state
	if _camera_scan_label:
		_camera_scan_label.visible = false
	if _camera_refresh_button:
		_camera_refresh_button.disabled = false


func _on_enumeration_failed(error: String) -> void:
	push_warning("CameraCard: Hardware enumeration failed: %s" % error)
	# Reset UI state so user can retry
	if _camera_scan_label:
		_camera_scan_label.visible = false
	if _camera_refresh_button:
		_camera_refresh_button.disabled = false


# -----------------------------------------------------------------------------
# Signal Handlers - UI
# -----------------------------------------------------------------------------

func _on_connect_button_pressed() -> void:
	if CameraClient.is_daemon_connected():
		disconnect_camera()
	else:
		connect_camera()


func _on_camera_selected(index: int) -> void:
	var device := HardwareManager.get_device_at_index(index)
	if device.is_empty():
		return

	_on_camera_device_changed(device)
	_update_camera_status()


func _on_format_selected(index: int) -> void:
	var device_idx := _camera_selector.get_selected_id()
	var device := HardwareManager.get_device_at_index(device_idx)
	if device.is_empty():
		return

	_apply_selected_format(device, index)
	_update_camera_status()


func _on_refresh_pressed() -> void:
	refresh_devices()
	_update_camera_status()


# -----------------------------------------------------------------------------
# Signal Handlers - CameraClient
# -----------------------------------------------------------------------------

func _on_camera_connection_changed(connected: bool) -> void:
	_is_connecting = false
	_update_camera_status()
	connection_changed.emit(connected)


func _on_camera_connection_failed(reason: String) -> void:
	_is_connecting = false
	_update_camera_status()

	# Display error to user
	if _camera_status_row:
		_camera_status_row.set_value("Error: %s" % reason)
		_camera_status_row.status = "error"

	print("CameraCard: Camera connection failed: %s" % reason)


func _on_camera_attempt_complete(_success: bool) -> void:
	_is_connecting = false
	_update_camera_status()


func _on_camera_format_mismatch(actual: Dictionary) -> void:
	print("CameraCard: Format mismatch detected - %s" % actual)

	# Update Session (SSoT) with actual format
	if actual.has("width"):
		Session.camera_width_px = actual["width"]
	if actual.has("height"):
		Session.camera_height_px = actual["height"]
	if actual.has("bits_per_pixel"):
		Session.camera_bits_per_pixel = actual["bits_per_pixel"]

	# Set flag to prevent re-enabling the selector
	_format_locked_due_to_mismatch = true

	if _camera_format_hint_label:
		_camera_format_hint_label.text = "Camera delivers %dx%d @ %dbpp only." % [
			actual["width"], actual["height"], actual["bits_per_pixel"]]
		_camera_format_hint_label.visible = true

	# Find and select the matching format in the dropdown
	if _format_selector:
		var found_index := -1
		for i in range(_format_selector.item_count):
			var fmt: Dictionary = _format_selector.get_item_metadata(i)
			if fmt and int(fmt.get("width", 0)) == actual["width"] and int(fmt.get("height", 0)) == actual["height"]:
				found_index = i
				break

		if found_index >= 0:
			_format_selector.select(found_index)

		# Lock the selector since the camera only delivers this format
		_format_selector.disabled = true
