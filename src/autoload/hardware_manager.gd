## HardwareManager - Hardware enumeration and connection management (Autoload)
##
## Handles camera enumeration (via Python subprocess) and monitor detection
## (via DisplayServer API + native MonitorInfo extension).
## Instantiated once at startup and persists across screen changes.
extends Node


signal cameras_enumerated(cameras: Array[Dictionary])
signal monitors_enumerated(monitors: Array[Dictionary])
signal enumeration_failed(error: String)


## Cached camera devices from last enumeration
var _camera_devices: Array[Dictionary] = []

## Mutex for thread-safe access to _camera_devices
var _camera_mutex := Mutex.new()

## Cached monitor info from last detection
var _detected_monitors: Array[Dictionary] = []

## Flag to prevent concurrent camera scans
var _is_scanning_cameras := false

## Tracks if initial enumeration has been performed
var _has_done_camera_scan := false
var _has_done_monitor_scan := false

## Thread for async camera enumeration
var _camera_thread: Thread = null


func _notification(what: int) -> void:
	if what == NOTIFICATION_PREDELETE:
		# Clean up thread on exit
		if _camera_thread != null and _camera_thread.is_started():
			_camera_thread.wait_to_finish()


# -----------------------------------------------------------------------------
# Camera Enumeration
# -----------------------------------------------------------------------------

## Get cached camera devices (thread-safe copy)
func get_camera_devices() -> Array[Dictionary]:
	_camera_mutex.lock()
	var result := _camera_devices.duplicate()
	_camera_mutex.unlock()
	return result


## Check if cameras have been enumerated at least once
func has_enumerated_cameras() -> bool:
	_camera_mutex.lock()
	var has_devices := _camera_devices.size() > 0
	_camera_mutex.unlock()
	return has_devices or _has_done_camera_scan


## Check if currently scanning for cameras
func is_scanning_cameras() -> bool:
	return _is_scanning_cameras


## Enumerate available cameras asynchronously (runs in background thread)
## Emits cameras_enumerated when complete, or enumeration_failed on error
func enumerate_cameras_async() -> void:
	if _is_scanning_cameras:
		return

	_is_scanning_cameras = true

	# Clean up any previous thread (non-blocking check)
	if _camera_thread != null:
		if _camera_thread.is_alive():
			# Previous scan still running - shouldn't happen due to flag check
			push_warning("HardwareManager: Previous camera thread still alive")
			_is_scanning_cameras = false
			return
		# Thread finished, safe to wait and clean up
		_camera_thread.wait_to_finish()
		_camera_thread = null

	# Start enumeration in background thread
	_camera_thread = Thread.new()
	_camera_thread.start(_enumerate_cameras_threaded)


## Thread worker function for async camera enumeration
func _enumerate_cameras_threaded() -> void:
	var enum_result := _enumerate_cameras()
	var devices := _build_device_list(enum_result)

	# Update state and emit signal on main thread
	call_deferred("_on_camera_enumeration_complete", devices)


## Thread-safe helper to report venv not found error
func _report_venv_not_found(path: String) -> void:
	ErrorHandler.report_hardware_error(
		"Python environment not found",
		"Python venv not found at %s. Run 'poetry install' to set up the environment." % path,
		ErrorHandler.Code.HARDWARE_ENUMERATION_FAILED
	)


## Thread-safe helper to report enumeration failure
func _report_enumeration_failed(exit_code: int) -> void:
	ErrorHandler.report_hardware_error(
		"Camera enumeration failed",
		"Exit code %d. Check that the Python environment is properly configured." % exit_code,
		ErrorHandler.Code.HARDWARE_ENUMERATION_FAILED
	)


## Thread-safe helper to report JSON parse error
func _report_json_parse_error(error_msg: String) -> void:
	ErrorHandler.report_hardware_error(
		"Camera enumeration parse error",
		"Failed to parse response: %s" % error_msg,
		ErrorHandler.Code.HARDWARE_ENUMERATION_FAILED
	)


## Thread-safe helper to report invalid format
func _report_invalid_format() -> void:
	ErrorHandler.report_hardware_error(
		"Camera enumeration format error",
		"Received invalid response format from enumeration script.",
		ErrorHandler.Code.HARDWARE_ENUMERATION_FAILED
	)


## Called on main thread when camera enumeration completes
func _on_camera_enumeration_complete(devices: Array[Dictionary]) -> void:
	_camera_mutex.lock()
	_camera_devices = devices
	_camera_mutex.unlock()

	_is_scanning_cameras = false
	_has_done_camera_scan = true

	# Clean up thread
	if _camera_thread != null:
		_camera_thread.wait_to_finish()
		_camera_thread = null

	# Emit with a copy to prevent external modification
	cameras_enumerated.emit(get_camera_devices())


## Synchronous camera enumeration (for immediate results)
func enumerate_cameras_sync() -> Array[Dictionary]:
	if _is_scanning_cameras:
		return get_camera_devices()

	_is_scanning_cameras = true
	var enum_result := _enumerate_cameras()
	var devices := _build_device_list(enum_result)

	_camera_mutex.lock()
	_camera_devices = devices
	_camera_mutex.unlock()

	_is_scanning_cameras = false
	_has_done_camera_scan = true

	return get_camera_devices()


## Run camera enumeration and parse results.
## In exported builds, uses the bundled daemon executable.
## In development, uses the Python venv.
func _enumerate_cameras() -> Dictionary:
	var output: Array = []
	var exit_code: int

	if PythonUtils.has_bundled_daemon():
		# Exported build: use bundled daemon with --enumerate-cameras flag
		var daemon_exe := PythonUtils.get_daemon_executable()
		exit_code = OS.execute(daemon_exe, ["--enumerate-cameras"], output, false)
	else:
		# Development: use Python venv
		var project_path := ProjectSettings.globalize_path("res://").rstrip("/")
		var python_path := PythonUtils.get_venv_python_path()
		var shell_exe := PythonUtils.get_shell_exe()
		var shell_args: Array

		if not PythonUtils.venv_exists():
			call_deferred("_report_venv_not_found", python_path)
			enumeration_failed.emit("Python venv not found - run 'poetry install'")
			return {}

		var cmd: String
		if OS.get_name() == "Windows":
			cmd = 'cd /d "%s" && "%s" -m daemon.camera.enumerate 2>nul' % [project_path, python_path]
			shell_args = ["/c", cmd]
		else:
			cmd = "cd '%s' && '%s' -m daemon.camera.enumerate 2>/dev/null" % [project_path, python_path]
			shell_args = ["-c", cmd]

		exit_code = OS.execute(shell_exe, shell_args, output, false)

	if exit_code != 0 or output.is_empty():
		call_deferred("_report_enumeration_failed", exit_code)
		enumeration_failed.emit("Camera enumeration failed with exit code %d" % exit_code)
		return {}

	# Parse JSON output
	var json := JSON.new()
	var json_str: String = output[0].strip_edges()
	var parse_result := json.parse(json_str)
	if parse_result != OK:
		call_deferred("_report_json_parse_error", json.get_error_message())
		enumeration_failed.emit("JSON parse error: %s" % json.get_error_message())
		return {}

	var data: Variant = json.get_data()
	if data is Dictionary:
		return data
	else:
		call_deferred("_report_invalid_format")
		enumeration_failed.emit("Invalid response format from enumeration")
		return {}


## Build flat list of all camera devices from enumeration result.
## Passes through exactly what Python returns - no defaults, no fallbacks.
func _build_device_list(enum_result: Dictionary) -> Array[Dictionary]:
	var devices: Array[Dictionary] = []

	for backend_type in enum_result:
		var backend_info: Variant = enum_result[backend_type]
		if not backend_info is Dictionary:
			continue
		if not backend_info.has("available") or not backend_info["available"]:
			continue
		if not backend_info.has("devices") or not backend_info["devices"] is Array:
			continue

		for device in backend_info["devices"]:
			if not device is Dictionary:
				continue
			# Pass through device info, adding only the backend type
			var device_entry: Dictionary = device.duplicate()
			device_entry["type"] = backend_type
			devices.append(device_entry)

	return devices


## Get device at index from cached list (thread-safe)
func get_device_at_index(index: int) -> Dictionary:
	_camera_mutex.lock()
	var result: Dictionary = {}
	if index >= 0 and index < _camera_devices.size():
		result = _camera_devices[index].duplicate()
	_camera_mutex.unlock()
	return result


## Find device index matching type and device_index (thread-safe)
## Returns -1 if not found
func find_device_index(device_type: String, device_index: int) -> int:
	_camera_mutex.lock()
	var found_index := -1
	for i in range(_camera_devices.size()):
		var device: Dictionary = _camera_devices[i]
		if str(device["type"]) == device_type and int(device["index"]) == device_index:
			found_index = i
			break
	_camera_mutex.unlock()
	return found_index


# -----------------------------------------------------------------------------
# Monitor Enumeration
# -----------------------------------------------------------------------------

## Get cached monitor info
func get_detected_monitors() -> Array[Dictionary]:
	return _detected_monitors


## Check if monitors have been enumerated at least once
func has_enumerated_monitors() -> bool:
	return _has_done_monitor_scan


## Enumerate available monitors synchronously
## Returns array of monitor dictionaries with detailed info
## Uses native APIs (CoreGraphics/WinAPI/DRM) - requires MonitorInfo extension
func enumerate_monitors() -> Array[Dictionary]:
	_detected_monitors.clear()

	assert(ClassDB.class_exists("MonitorInfo"), "MonitorInfo extension required for display enumeration")

	var monitor_info_obj: RefCounted = ClassDB.instantiate(&"MonitorInfo")
	assert(monitor_info_obj != null, "Failed to instantiate MonitorInfo")
	assert(monitor_info_obj.has_method("get_display_count"), "MonitorInfo missing get_display_count method")

	var native_count: int = monitor_info_obj.call("get_display_count")
	print("HardwareManager: Native API detected %d displays" % native_count)

	for i in range(native_count):
		var info := _get_monitor_info_native(monitor_info_obj, i)
		_detected_monitors.append(info)

	_has_done_monitor_scan = true
	monitors_enumerated.emit(_detected_monitors)
	return _detected_monitors


## Get monitor info using native APIs (CoreGraphics/WinAPI/DRM)
func _get_monitor_info_native(monitor_info_obj: RefCounted, index: int) -> Dictionary:
	var native_info: Dictionary = monitor_info_obj.call("get_display_info", index)

	# Convert native format to our standard format
	var width: int = int(native_info.get("width", 0))
	var height: int = int(native_info.get("height", 0))
	var refresh: float = float(native_info.get("refresh_rate", 0.0))
	var pos_x: int = int(native_info.get("position_x", 0))
	var pos_y: int = int(native_info.get("position_y", 0))
	var width_mm: int = int(native_info.get("width_mm", 0))
	var height_mm: int = int(native_info.get("height_mm", 0))
	var is_primary: bool = bool(native_info.get("is_primary", false))

	# Physical dimensions in cm
	var width_cm := float(width_mm) / 10.0
	var height_cm := float(height_mm) / 10.0
	var physical_source := "edid" if width_mm > 0 else "none"

	# Calculate DPI if we have physical dimensions
	var dpi := 0
	if width_cm > 0 and width > 0:
		dpi = int(float(width) / (width_cm / 2.54))

	return {
		"index": index,
		"size": Vector2i(width, height),
		"refresh": refresh,
		"position": Vector2i(pos_x, pos_y),
		"dpi": dpi,
		"width_cm": width_cm,
		"height_cm": height_cm,
		"is_primary": is_primary,
		"physical_source": physical_source,
	}


## Get monitor info using Godot's DisplayServer (fallback)
## Get the best monitor for stimulus display (prefers non-primary)
func get_stimulus_monitor_index() -> int:
	if _detected_monitors.is_empty():
		enumerate_monitors()

	if _detected_monitors.size() <= 1:
		return 0

	# Prefer first non-primary monitor
	for monitor in _detected_monitors:
		if not bool(monitor["is_primary"]):
			return int(monitor["index"])

	# All monitors are primary (shouldn't happen), return first
	return int(_detected_monitors[0]["index"])


## Check if only one monitor is available (testing mode)
func is_single_monitor() -> bool:
	if _detected_monitors.is_empty():
		enumerate_monitors()
	return _detected_monitors.size() == 1


## Get monitor by index from cached list
func get_monitor_at_index(index: int) -> Dictionary:
	for monitor in _detected_monitors:
		if int(monitor["index"]) == index:
			return monitor
	return {}


## Get display label for a monitor (for UI dropdowns)
func get_monitor_display_label(monitor: Dictionary) -> String:
	var idx: int = int(monitor["index"])
	var size: Vector2i = monitor["size"]
	var is_primary: bool = bool(monitor["is_primary"])
	var label := "Display %d: %dx%d" % [idx + 1, size.x, size.y]
	if is_primary:
		label += " (Primary)"
	return label
