extends Node
## CameraClient autoload: Interface to the Python camera daemon via shared memory.
##
## This wraps the GDExtension SharedMemoryReader and handles daemon lifecycle.
## All daemon operations are non-blocking to keep the UI responsive.
##
## Uses explicit ConnectionState enum for clear state management.

## Connection state machine
enum ConnectionState {
	IDLE,            ## Not connected, not trying to connect
	STARTING_DAEMON, ## Thread is running to start daemon process
	POLLING_SHM,     ## Daemon started, polling for shared memory availability
	CONNECTED,       ## Connected and receiving frames
	RETRYING,        ## Waiting for retry timer after failure
	FAILED,          ## All retries exhausted, gave up
	CLEANUP,         ## Shutting down resources
}

## Emitted when connection state changes.
signal connection_changed(connected: bool)

## Emitted when the daemon process starts or stops.
signal daemon_state_changed(running: bool)

## Emitted when connection attempt fails (for UI to display error).
signal connection_failed(reason: String)

## Emitted when async connection attempt completes.
signal connection_attempt_complete(success: bool)

## Emitted when camera format mismatch is detected (actual differs from selected).
signal format_mismatch_detected(actual: Dictionary)

## Shared memory reader (GDExtension class).
var _shm_reader: RefCounted = null

## Connection retry settings
const MAX_RETRY_ATTEMPTS := 3
const INITIAL_RETRY_DELAY_MS := 500
const RETRY_BACKOFF_MULTIPLIER := 2.0

## Shared memory polling settings
## Poll rapidly initially, then slow down if daemon is still starting up
const SHM_POLL_INTERVAL_MS := 100  # Check every 100ms
const SHM_POLL_MAX_ATTEMPTS := 100  # 100 * 100ms = 10 seconds max wait

## Current connection state
var _state := ConnectionState.IDLE

## Retry counter for connection attempts
var _retry_count: int = 0

## Poll counter for shared memory availability check
var _shm_poll_count: int = 0

## Daemon process ID.
var _daemon_pid: int = -1


## Valid state transitions (from -> [valid to states])
const _VALID_TRANSITIONS := {
	ConnectionState.IDLE: [ConnectionState.STARTING_DAEMON, ConnectionState.CLEANUP],
	ConnectionState.STARTING_DAEMON: [ConnectionState.POLLING_SHM, ConnectionState.FAILED, ConnectionState.CLEANUP],
	ConnectionState.POLLING_SHM: [ConnectionState.CONNECTED, ConnectionState.RETRYING, ConnectionState.FAILED, ConnectionState.CLEANUP],
	ConnectionState.CONNECTED: [ConnectionState.RETRYING, ConnectionState.CLEANUP],
	ConnectionState.RETRYING: [ConnectionState.STARTING_DAEMON, ConnectionState.FAILED, ConnectionState.CLEANUP],
	ConnectionState.FAILED: [ConnectionState.IDLE, ConnectionState.CLEANUP],
	ConnectionState.CLEANUP: [ConnectionState.IDLE],
}


## Transition to a new state with validation
func _transition_to(new_state: ConnectionState) -> bool:
	if new_state == _state:
		return true  # Already in this state

	var valid_targets: Array = _VALID_TRANSITIONS.get(_state, [])
	if new_state not in valid_targets:
		push_error("CameraClient: Invalid state transition: %s -> %s" % [
			ConnectionState.keys()[_state],
			ConnectionState.keys()[new_state]
		])
		return false

	var old_state := _state
	_state = new_state
	print("CameraClient: State %s -> %s" % [
		ConnectionState.keys()[old_state],
		ConnectionState.keys()[new_state]
	])
	return true

## Thread for blocking daemon operations
var _daemon_thread: Thread = null


func _ready() -> void:
	_try_create_shm_reader()
	print("CameraClient autoload initialized")


func _notification(what: int) -> void:
	if what == NOTIFICATION_WM_CLOSE_REQUEST or what == NOTIFICATION_PREDELETE:
		cleanup()


## Returns true if connected to the daemon via shared memory.
func is_daemon_connected() -> bool:
	return _state == ConnectionState.CONNECTED


## Returns true if the GDExtension is loaded.
func is_extension_loaded() -> bool:
	return _shm_reader != null


## Returns true if the daemon process is running.
func is_daemon_running() -> bool:
	return _daemon_pid > 0


## Returns true if currently attempting to connect.
func is_connecting() -> bool:
	return _state in [ConnectionState.STARTING_DAEMON, ConnectionState.POLLING_SHM, ConnectionState.RETRYING]


## Get the current connection state.
func get_state() -> ConnectionState:
	return _state


## Get the current connection state as a string.
func get_state_name() -> String:
	return ConnectionState.keys()[_state]


## Connect to the daemon asynchronously, starting it if necessary.
## Emits connection_attempt_complete when done.
## Returns immediately - use the signal or await connect_to_daemon_async() for result.
func connect_to_daemon_async() -> void:
	if _state == ConnectionState.CONNECTED:
		connection_attempt_complete.emit(true)
		return

	if is_connecting():
		return  # Already in progress

	_retry_count = 0
	_start_connection_attempt()


## Start an async connection attempt
func _start_connection_attempt() -> void:
	if _shm_reader == null:
		_try_create_shm_reader()
		if _shm_reader == null:
			var err_msg := "SharedMemoryReader extension not loaded"
			ErrorHandler.report_camera_error(
				"Camera extension not loaded",
				"The SharedMemoryReader GDExtension is not available. Build the Rust extension first.",
				ErrorHandler.Code.CAMERA_SHARED_MEMORY_ERROR
			)
			_transition_to(ConnectionState.FAILED)
			connection_failed.emit(err_msg)
			connection_attempt_complete.emit(false)
			return

	# Start daemon in background thread (includes killing orphans)
	_start_daemon_async()


## Start daemon asynchronously
func _start_daemon_async() -> void:
	if _daemon_pid > 0:
		print("Daemon already running with PID: ", _daemon_pid)
		_on_daemon_started(true)
		return

	# Validate camera is selected on main thread before starting
	if not Session.has_selected_camera():
		_report_camera_selection_error()
		_on_daemon_start_failed("No camera selected")
		return

	_transition_to(ConnectionState.STARTING_DAEMON)

	# Snapshot all configuration values on main thread (thread-safe)
	var config := {
		"frame_width": Session.camera_width_px,
		"frame_height": Session.camera_height_px,
		"bits_per_pixel": Session.camera_bits_per_pixel,
		"shm_name": Settings.daemon_shm_name,
		"shm_num_buffers": Settings.daemon_shm_num_buffers,
		"camera_type": Session.camera_type,
		"camera_device": Session.camera_device_index,
		"target_fps": Session.camera_fps,
		"project_path": ProjectSettings.globalize_path("res://").rstrip("/"),
		"python_path": PythonUtils.get_venv_python_path(),
		"shell_exe": PythonUtils.get_shell_exe(),
		"daemon_executable": PythonUtils.get_daemon_executable(),
		"daemon_available": PythonUtils.daemon_available(),
		"is_exported": PythonUtils.is_exported(),
		"user_data_dir": OS.get_user_data_dir(),
	}

	# Clean up any previous thread
	if _daemon_thread != null and _daemon_thread.is_started():
		_daemon_thread.wait_to_finish()

	# Run blocking operations in background thread with snapshotted config
	_daemon_thread = Thread.new()
	_daemon_thread.start(_start_daemon_threaded.bind(config))


## Thread worker for daemon startup (handles blocking operations)
## [param config] Snapshotted configuration from main thread (thread-safe)
func _start_daemon_threaded(config: Dictionary) -> void:
	# Kill orphaned daemons (blocking subprocess calls)
	_kill_orphaned_daemons_sync()

	# Extract snapshotted config values (no autoload access in thread)
	var frame_width: int = config["frame_width"]
	var frame_height: int = config["frame_height"]
	var bits_per_pixel: int = config["bits_per_pixel"]
	var shm_name: String = config["shm_name"]
	var shm_num_buffers: int = config["shm_num_buffers"]
	var camera_type: String = config["camera_type"]
	var camera_device: int = config["camera_device"]
	var target_fps: float = config["target_fps"]
	var project_path: String = config["project_path"]
	var daemon_executable: String = config["daemon_executable"]

	# Check if daemon can be run at all
	if not config["daemon_available"]:
		call_deferred("_on_daemon_start_failed", "No daemon available (no bundled executable or Python venv)")
		return

	# Build daemon arguments - all required, no defaults
	var daemon_args_arr: PackedStringArray = [
		"--width", str(frame_width),
		"--height", str(frame_height),
		"--fps", "%.2f" % target_fps,
		"--shm-name", shm_name,
		"--num-buffers", str(shm_num_buffers),
		"--camera", camera_type,
		"--camera-device", str(camera_device),
		"--bits-per-pixel", str(bits_per_pixel),
	]

	var pid: int
	var log_file: String
	if config["is_exported"]:
		log_file = config["user_data_dir"] + "/daemon.log"
	else:
		log_file = project_path + "/daemon.log"

	if daemon_executable != "":
		# Bundled mode: run the daemon executable directly
		print("Starting bundled daemon: %s" % daemon_executable)
		var shell_exe: String = config["shell_exe"]
		var daemon_args_str := " ".join(daemon_args_arr)
		var shell_args: Array
		if OS.get_name() == "Windows":
			var cmd := '"%s" %s > "%s" 2>&1' % [daemon_executable, daemon_args_str, log_file]
			shell_args = ["/c", cmd]
		else:
			var cmd := "'%s' %s > '%s' 2>&1" % [daemon_executable, daemon_args_str, log_file]
			shell_args = ["-c", cmd]
		pid = OS.create_process(shell_exe, shell_args, false)
	else:
		# Development mode: run via Python venv
		var python_path: String = config["python_path"]
		var shell_exe: String = config["shell_exe"]
		var daemon_args_str := " ".join(daemon_args_arr)
		var shell_args: Array
		if OS.get_name() == "Windows":
			var cmd := '"%s" -u -m daemon.main %s > "%s" 2>&1' % [python_path, daemon_args_str, log_file]
			shell_args = ["/c", "cd /d \"%s\" && %s" % [project_path, cmd]]
		else:
			var cmd := "cd '%s' && '%s' -u -m daemon.main %s > '%s' 2>&1" % [
				project_path, python_path, daemon_args_str, log_file
			]
			shell_args = ["-c", cmd]
		pid = OS.create_process(shell_exe, shell_args, false)

	call_deferred("_on_daemon_process_created", pid)


## Called on main thread when daemon process is created
func _on_daemon_process_created(pid: int) -> void:
	# Clean up thread
	if _daemon_thread != null:
		_daemon_thread.wait_to_finish()
		_daemon_thread = null

	if pid <= 0:
		ErrorHandler.report_camera_error(
			"Failed to create daemon process",
			"OS.create_process() returned an invalid PID.",
			ErrorHandler.Code.CAMERA_DAEMON_NOT_RUNNING
		)
		_on_daemon_started(false)
		return

	_daemon_pid = pid
	print("Daemon started with PID: ", _daemon_pid)
	daemon_state_changed.emit(true)
	_on_daemon_started(true)


## Thread-safe helper to report camera selection error (called via call_deferred)
func _report_camera_selection_error() -> void:
	ErrorHandler.report_camera_error(
		"No camera selected",
		"Please select a camera in Setup before connecting.",
		ErrorHandler.Code.CAMERA_CONNECTION_FAILED
	)


## Called when daemon startup fails with a specific reason
func _on_daemon_start_failed(reason: String) -> void:
	ErrorHandler.report_camera_error(
		"Camera daemon failed to start",
		reason,
		ErrorHandler.Code.CAMERA_DAEMON_NOT_RUNNING
	)
	_transition_to(ConnectionState.FAILED)
	connection_failed.emit(reason)
	connection_attempt_complete.emit(false)


## Called when daemon startup completes (success or failure)
func _on_daemon_started(success: bool) -> void:
	if not success:
		_on_daemon_start_failed("Failed to start daemon process")
		return

	# Start polling for shared memory availability
	# This is robust: we wait exactly as long as needed, checking if daemon is still running
	print("Polling for shared memory availability...")
	_transition_to(ConnectionState.POLLING_SHM)
	_shm_poll_count = 0
	_poll_for_shm()


## Poll for shared memory availability
## This is self-synchronizing: polls until shm is ready, daemon exits, or timeout
func _poll_for_shm() -> void:
	_shm_poll_count += 1
	var shm_name = Settings.daemon_shm_name

	# Try to open shared memory
	if _shm_reader.open(shm_name):
		_transition_to(ConnectionState.CONNECTED)
		_retry_count = 0
		connection_changed.emit(true)
		connection_attempt_complete.emit(true)
		print("Connected to shared memory: %s (after %d polls)" % [shm_name, _shm_poll_count])
		return

	# Check if daemon is still running
	var daemon_running := _daemon_pid > 0 and OS.is_process_running(_daemon_pid)

	if not daemon_running:
		# Daemon has exited - check log for format mismatch
		print("Daemon exited during startup (after %d polls)" % _shm_poll_count)
		_handle_daemon_exit()
		return

	# Daemon still running, check if we've exceeded max polls
	if _shm_poll_count >= SHM_POLL_MAX_ATTEMPTS:
		# Timeout - daemon is running but shm never appeared
		var reason := "Shared memory not available after %ds" % [(SHM_POLL_MAX_ATTEMPTS * SHM_POLL_INTERVAL_MS) / 1000.0]
		ErrorHandler.report_camera_error(
			"Camera connection timeout",
			"Daemon is running but shared memory was not created. Check daemon.log for errors.",
			ErrorHandler.Code.CAMERA_SHARED_MEMORY_ERROR
		)
		_stop_daemon_async()
		_transition_to(ConnectionState.FAILED)
		_retry_count = 0
		connection_failed.emit(reason)
		connection_attempt_complete.emit(false)
		return

	# Schedule next poll
	var timer := get_tree().create_timer(SHM_POLL_INTERVAL_MS / 1000.0)
	timer.timeout.connect(_poll_for_shm)


## Handle daemon exit during connection - check for mismatch or genuine failure
func _handle_daemon_exit() -> void:
	# Check log for format mismatch
	var actual = _parse_daemon_log_for_actual()
	if not actual.is_empty():
		# Format mismatch detected - emit signal for consumer to update Session
		print("Format mismatch detected. Camera delivers %dx%d @ %dbpp" % [
			actual["width"], actual["height"], actual["bits_per_pixel"]])

		# Emit signal for UI to handle (must update Session before retry)
		# Note: Signal handlers should call Session.camera_width_px = actual["width"], etc.
		print("CameraClient: Emitting format_mismatch_detected signal")
		format_mismatch_detected.emit(actual)

		# Clear PID since daemon already exited
		_daemon_pid = -1
		daemon_state_changed.emit(false)

		# Reset and auto-retry with corrected values
		# Use call_deferred so signal handlers update Session first
		_retry_count = 0
		_transition_to(ConnectionState.IDLE)  # Reset to idle before retrying
		call_deferred("_start_connection_attempt")
		return

	# No mismatch - this is a genuine startup failure
	_retry_count += 1

	# Clear PID since daemon already exited
	_daemon_pid = -1
	daemon_state_changed.emit(false)

	if _retry_count < MAX_RETRY_ATTEMPTS:
		_transition_to(ConnectionState.RETRYING)
		var retry_delay := int(INITIAL_RETRY_DELAY_MS * pow(RETRY_BACKOFF_MULTIPLIER, _retry_count - 1))
		print("Daemon startup failed, retrying in %d ms (attempt %d/%d)..." % [
			retry_delay, _retry_count + 1, MAX_RETRY_ATTEMPTS])
		_schedule_retry(retry_delay)
	else:
		# All retries exhausted - genuine failure
		var reason := "Daemon failed to start after %d attempts" % MAX_RETRY_ATTEMPTS
		ErrorHandler.report_camera_error(
			"Camera connection failed",
			"Daemon failed to start after %d attempts. Check daemon.log for details." % MAX_RETRY_ATTEMPTS,
			ErrorHandler.Code.CAMERA_DAEMON_NOT_RUNNING
		)
		_transition_to(ConnectionState.FAILED)
		_retry_count = 0
		connection_failed.emit(reason)
		connection_attempt_complete.emit(false)


## Schedule a retry attempt after delay
func _schedule_retry(delay_ms: int) -> void:
	var timer := get_tree().create_timer(delay_ms / 1000.0)
	timer.timeout.connect(_start_connection_attempt)


## Check if currently retrying connection
func is_retrying_connection() -> bool:
	return _state == ConnectionState.RETRYING


## Get current retry count
func get_retry_count() -> int:
	return _retry_count


## Parse daemon.log for actual camera output when format mismatch occurs.
## Returns dictionary with width, height, bits_per_pixel or empty dict if not found.
func _parse_daemon_log_for_actual() -> Dictionary:
	var log_path: String
	if PythonUtils.is_exported():
		log_path = OS.get_user_data_dir() + "/daemon.log"
	else:
		log_path = ProjectSettings.globalize_path("res://").rstrip("/") + "/daemon.log"

	if not FileAccess.file_exists(log_path):
		return {}

	var file = FileAccess.open(log_path, FileAccess.READ)
	if file == null:
		return {}

	var content = file.get_as_text()
	file.close()

	# Look for CAMERA_ACTUAL:width:height:bpp pattern
	var regex = RegEx.new()
	regex.compile("CAMERA_ACTUAL:(\\d+):(\\d+):(\\d+)")
	var result = regex.search(content)

	if result:
		return {
			"width": int(result.get_string(1)),
			"height": int(result.get_string(2)),
			"bits_per_pixel": int(result.get_string(3))
		}
	return {}


## Disconnect from the daemon and stop it.
func disconnect_from_daemon() -> void:
	if _state != ConnectionState.CONNECTED and _daemon_pid <= 0:
		return

	if _shm_reader != null and _state == ConnectionState.CONNECTED:
		_shm_reader.close()

	_transition_to(ConnectionState.CLEANUP)
	connection_changed.emit(false)
	print("Disconnected from shared memory")

	_stop_daemon_async()


## Get the latest frame as uint8 grayscale data.
## Returns empty PackedByteArray if no frame available or not connected.
func get_frame() -> PackedByteArray:
	if _state != ConnectionState.CONNECTED or _shm_reader == null:
		return PackedByteArray()
	return _shm_reader.get_latest_frame_u8()


## Get the daemon's frame counter (for drop detection).
func get_frame_count() -> int:
	if _state != ConnectionState.CONNECTED or _shm_reader == null:
		return 0
	return _shm_reader.get_frame_count()


## Get the hardware timestamp of the most recently received frame in microseconds.
## Returns 0 if no frame available or timestamp unavailable.
func get_latest_timestamp_us() -> int:
	if _state != ConnectionState.CONNECTED or _shm_reader == null:
		return 0
	return _shm_reader.get_latest_timestamp_us()


## Cleanup resources. Called automatically on exit.
func cleanup() -> void:
	if _state == ConnectionState.CLEANUP:
		return  # Already cleaning up

	var was_connected := _state == ConnectionState.CONNECTED
	_transition_to(ConnectionState.CLEANUP)

	print("CameraClient cleanup...")
	# Use sync version during cleanup since we're exiting anyway
	if _shm_reader != null and was_connected:
		_shm_reader.close()
	_stop_daemon_sync()

	# Clean up thread
	if _daemon_thread != null and _daemon_thread.is_started():
		_daemon_thread.wait_to_finish()

	_transition_to(ConnectionState.IDLE)
	print("CameraClient cleanup complete")


# --- Private Methods ---

func _try_create_shm_reader() -> void:
	if ClassDB.class_exists(&"SharedMemoryReader"):
		_shm_reader = ClassDB.instantiate(&"SharedMemoryReader")
		print("SharedMemoryReader extension loaded successfully")
	else:
		push_warning("SharedMemoryReader extension not found. Build the Rust extension first.")


## Kill orphaned daemons synchronously (called from thread)
func _kill_orphaned_daemons_sync() -> void:
	var shm_name: String = Settings.daemon_shm_name
	print("Checking for orphaned daemon processes...")

	if OS.get_name() == "Windows":
		OS.execute("taskkill", ["/F", "/IM", "openisi-daemon.exe"])
		OS.execute("taskkill", ["/F", "/IM", "python.exe", "/FI", "WINDOWTITLE eq *daemon.main*"])
	else:
		var output: Array = []
		# Kill bundled daemon processes
		OS.execute("pkill", ["-f", "openisi-daemon.*" + shm_name], output, true)
		# Kill Python-based daemon processes
		OS.execute("pkill", ["-f", "python.*-m daemon.main.*" + shm_name], output, true)
		OS.delay_msec(100)
		OS.execute("pkill", ["-9", "-f", "openisi-daemon.*" + shm_name], output, true)
		OS.execute("pkill", ["-9", "-f", "python.*-m daemon.main.*" + shm_name], output, true)


## Stop daemon asynchronously (runs blocking operations in thread)
func _stop_daemon_async(on_complete: Callable = Callable()) -> void:
	# Get the actual daemon PID from shared memory (more reliable than shell PID)
	var actual_pid := 0
	if _shm_reader != null and _state == ConnectionState.CONNECTED:
		actual_pid = _shm_reader.get_daemon_pid()
		_shm_reader.send_stop_command()

	# Use shared memory PID if available, otherwise fall back to tracked PID
	var pid_to_kill := actual_pid if actual_pid > 0 else _daemon_pid
	if pid_to_kill <= 0:
		if on_complete.is_valid():
			on_complete.call()
		return

	print("Stopping daemon (PID: ", pid_to_kill, ")...")
	_daemon_pid = -1

	# Clean up any previous thread
	if _daemon_thread != null and _daemon_thread.is_started():
		_daemon_thread.wait_to_finish()

	_daemon_thread = Thread.new()
	_daemon_thread.start(_stop_daemon_threaded.bind(pid_to_kill, on_complete))


## Thread worker for daemon stop
func _stop_daemon_threaded(pid: int, on_complete: Callable) -> void:
	# Brief wait for stop command to be processed
	OS.delay_msec(100)

	# Kill the shell process
	OS.kill(pid)

	# Kill any remaining daemon processes (bundled or Python-based)
	var shm_name: String = Settings.daemon_shm_name
	if OS.get_name() == "Windows":
		OS.execute("taskkill", ["/F", "/IM", "openisi-daemon.exe"])
		OS.execute("taskkill", ["/F", "/IM", "python.exe", "/FI", "WINDOWTITLE eq *daemon.main*"])
	else:
		OS.execute("pkill", ["-f", "openisi-daemon.*" + shm_name])
		OS.execute("pkill", ["-f", "python.*-m daemon.main.*" + shm_name])
		OS.delay_msec(200)
		OS.execute("pkill", ["-9", "-f", "openisi-daemon.*" + shm_name])
		OS.execute("pkill", ["-9", "-f", "python.*-m daemon.main.*" + shm_name])

	call_deferred("_on_daemon_stopped", on_complete)


## Called on main thread when daemon stop completes
func _on_daemon_stopped(on_complete: Callable) -> void:
	# Clean up thread
	if _daemon_thread != null:
		_daemon_thread.wait_to_finish()
		_daemon_thread = null

	# Only transition to IDLE if we're in CLEANUP state
	if _state == ConnectionState.CLEANUP:
		_transition_to(ConnectionState.IDLE)

	daemon_state_changed.emit(false)
	print("Daemon stopped")

	if on_complete.is_valid():
		on_complete.call()


## Stop daemon synchronously (for cleanup/exit)
func _stop_daemon_sync() -> void:
	# Get the actual daemon PID from shared memory (more reliable than shell PID)
	var actual_pid := 0
	if _shm_reader != null and _state == ConnectionState.CONNECTED:
		actual_pid = _shm_reader.get_daemon_pid()
		_shm_reader.send_stop_command()
		OS.delay_msec(100)

	# Use shared memory PID if available, otherwise fall back to tracked PID
	var pid_to_kill := actual_pid if actual_pid > 0 else _daemon_pid
	if pid_to_kill <= 0:
		return

	print("Stopping daemon (PID: ", pid_to_kill, ")...")

	var kill_result := OS.kill(pid_to_kill)
	if kill_result != OK:
		print("OS.kill() returned error code: ", kill_result)

	# Platform-specific fallback to ensure cleanup (bundled and Python-based)
	var shm_name: String = Settings.daemon_shm_name
	if OS.get_name() == "Windows":
		OS.execute("taskkill", ["/F", "/IM", "openisi-daemon.exe"])
		OS.execute("taskkill", ["/F", "/IM", "python.exe", "/FI", "WINDOWTITLE eq *daemon.main*"])
	else:
		OS.execute("pkill", ["-f", "openisi-daemon.*" + shm_name])
		OS.execute("pkill", ["-f", "python.*-m daemon.main.*" + shm_name])
		OS.delay_msec(200)
		OS.execute("pkill", ["-9", "-f", "openisi-daemon.*" + shm_name])
		OS.execute("pkill", ["-9", "-f", "python.*-m daemon.main.*" + shm_name])

	_daemon_pid = -1
	daemon_state_changed.emit(false)
	print("Daemon stopped")
