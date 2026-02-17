extends Node

## Centralized error handling with user feedback and history tracking.
##
## Usage:
##   ErrorHandler.report_camera_error("Connection failed", "Daemon not running")
##   ErrorHandler.report(ErrorHandler.Code.HARDWARE_NOT_FOUND, "Camera not detected")

# Signals
signal error_occurred(error: AppError)
signal error_dismissed(error: AppError)

# Severity levels for filtering and display
enum Severity {
	INFO,      # Informational, no action needed
	WARNING,   # Something unexpected but recoverable
	ERROR,     # Operation failed, user should know
	CRITICAL   # System cannot continue normally
}

# Categories for grouping and filtering errors
enum Category {
	SYSTEM,      # General system errors
	HARDWARE,    # Hardware detection/enumeration
	CAMERA,      # Camera connection and streaming
	DISPLAY,     # Display configuration
	CONFIG,      # Configuration loading/saving
	ACQUISITION, # Data acquisition
	STIMULUS,    # Stimulus rendering
	EXPORT       # Data export
}

# Error codes for programmatic handling
enum Code {
	# Generic (0-99)
	UNKNOWN = 0,
	OPERATION_FAILED = 1,
	INVALID_STATE = 2,

	# Hardware (100-199)
	HARDWARE_NOT_FOUND = 100,
	HARDWARE_ENUMERATION_FAILED = 101,
	HARDWARE_ACCESS_DENIED = 102,

	# Camera (200-299)
	CAMERA_CONNECTION_FAILED = 200,
	CAMERA_DAEMON_NOT_RUNNING = 201,
	CAMERA_STREAM_INTERRUPTED = 202,
	CAMERA_FRAME_DROPPED = 203,
	CAMERA_SHARED_MEMORY_ERROR = 204,

	# Display (300-399)
	DISPLAY_NOT_FOUND = 300,
	DISPLAY_INVALID_REFRESH_RATE = 301,
	DISPLAY_GEOMETRY_UNKNOWN = 302,

	# Config (400-499)
	CONFIG_LOAD_FAILED = 400,
	CONFIG_SAVE_FAILED = 401,
	CONFIG_INVALID_VALUE = 402,
	CONFIG_MIGRATION_FAILED = 403,

	# Acquisition (500-599)
	ACQUISITION_START_FAILED = 500,
	ACQUISITION_INTERRUPTED = 501,
	ACQUISITION_TIMING_ERROR = 502,

	# Stimulus (600-699)
	STIMULUS_WINDOW_FAILED = 600,
	STIMULUS_SHADER_ERROR = 601,

	# Export (700-799)
	EXPORT_WRITE_FAILED = 700,
	EXPORT_DIRECTORY_ERROR = 701
}

## Error instance holding all details
class AppError extends RefCounted:
	var code: Code
	var severity: Severity
	var category: Category
	var message: String
	var details: String
	var timestamp_ms: int
	var recoverable: bool
	var retry_action: Callable
	var dismissed: bool = false

	func _init(
		p_code: Code,
		p_message: String,
		p_details: String = "",
		p_severity: Severity = Severity.ERROR,
		p_category: Category = Category.SYSTEM,
		p_recoverable: bool = false,
		p_retry_action: Callable = Callable()
	) -> void:
		code = p_code
		message = p_message
		details = p_details
		severity = p_severity
		category = p_category
		recoverable = p_recoverable
		retry_action = p_retry_action
		timestamp_ms = Time.get_ticks_msec()

	func get_severity_name() -> String:
		return Severity.keys()[severity]

	func get_category_name() -> String:
		return Category.keys()[category]

	func format_for_log() -> String:
		var msg := "[%s][%s] %s" % [get_severity_name(), get_category_name(), message]
		if details:
			msg += " | " + details
		return msg

# Error history for diagnostics
var _error_history: Array[AppError] = []
const MAX_HISTORY := 100


func _ready() -> void:
	pass


## Report an error with full control over all parameters.
## Returns the created AppError for chaining or inspection.
func report(
	code: Code,
	message: String,
	details: String = "",
	severity: Severity = Severity.ERROR,
	category: Category = Category.SYSTEM,
	recoverable: bool = false,
	retry_action: Callable = Callable()
) -> AppError:
	var error := AppError.new(code, message, details, severity, category, recoverable, retry_action)

	# Add to history
	_error_history.append(error)
	if _error_history.size() > MAX_HISTORY:
		_error_history.pop_front()

	# Log to console (always, for debugging)
	match severity:
		Severity.INFO:
			print(error.format_for_log())
		Severity.WARNING:
			push_warning(error.format_for_log())
		Severity.ERROR, Severity.CRITICAL:
			push_error(error.format_for_log())

	# Emit signal for UI handling
	error_occurred.emit(error)

	return error


## Convenience: Report a hardware error
func report_hardware_error(
	message: String,
	details: String = "",
	code: Code = Code.HARDWARE_NOT_FOUND,
	recoverable: bool = false,
	retry_action: Callable = Callable()
) -> AppError:
	return report(code, message, details, Severity.ERROR, Category.HARDWARE, recoverable, retry_action)


## Convenience: Report a camera error
func report_camera_error(
	message: String,
	details: String = "",
	code: Code = Code.CAMERA_CONNECTION_FAILED,
	recoverable: bool = true,
	retry_action: Callable = Callable()
) -> AppError:
	return report(code, message, details, Severity.ERROR, Category.CAMERA, recoverable, retry_action)


## Convenience: Report a display error
func report_display_error(
	message: String,
	details: String = "",
	code: Code = Code.DISPLAY_NOT_FOUND,
	recoverable: bool = false,
	retry_action: Callable = Callable()
) -> AppError:
	return report(code, message, details, Severity.ERROR, Category.DISPLAY, recoverable, retry_action)


## Convenience: Report a config error
func report_config_error(
	message: String,
	details: String = "",
	code: Code = Code.CONFIG_SAVE_FAILED,
	severity: Severity = Severity.WARNING
) -> AppError:
	return report(code, message, details, severity, Category.CONFIG, false, Callable())


## Convenience: Report an acquisition error (typically critical)
func report_acquisition_error(
	message: String,
	details: String = "",
	code: Code = Code.ACQUISITION_INTERRUPTED,
	severity: Severity = Severity.CRITICAL
) -> AppError:
	return report(code, message, details, severity, Category.ACQUISITION, false, Callable())


## Convenience: Report a warning (non-blocking)
func report_warning(
	message: String,
	details: String = "",
	category: Category = Category.SYSTEM
) -> AppError:
	return report(Code.UNKNOWN, message, details, Severity.WARNING, category, false, Callable())


## Convenience: Report info (non-blocking, just logged)
func report_info(
	message: String,
	details: String = "",
	category: Category = Category.SYSTEM
) -> AppError:
	return report(Code.UNKNOWN, message, details, Severity.INFO, category, false, Callable())


## Mark an error as dismissed (called by error dialog)
func dismiss_error(error: AppError) -> void:
	error.dismissed = true
	error_dismissed.emit(error)


## Get recent errors for diagnostics
func get_recent_errors(count: int = 10) -> Array[AppError]:
	var start: int = maxi(0, _error_history.size() - count)
	var result: Array[AppError] = []
	for i in range(start, _error_history.size()):
		result.append(_error_history[i])
	return result


## Get all errors of a specific category
func get_errors_by_category(category: Category) -> Array[AppError]:
	var result: Array[AppError] = []
	for error in _error_history:
		if error.category == category:
			result.append(error)
	return result


## Get all errors at or above a severity level
func get_errors_by_severity(min_severity: Severity) -> Array[AppError]:
	var result: Array[AppError] = []
	for error in _error_history:
		if error.severity >= min_severity:
			result.append(error)
	return result


## Clear error history
func clear_history() -> void:
	_error_history.clear()


## Check if there are any undismissed errors at or above severity
func has_active_errors(min_severity: Severity = Severity.ERROR) -> bool:
	for error in _error_history:
		if error.severity >= min_severity and not error.dismissed:
			return true
	return false
