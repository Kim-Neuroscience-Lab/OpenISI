## FocusController - Coordinates camera preview, exposure, and anatomical capture
##
## Manages the focus screen domain logic separate from UI concerns:
## - Frame polling and texture creation from CameraClient
## - Anatomical image capture (save to disk, update Session state)
## - Exposure control state
##
## UI components connect to signals and call simple methods.
class_name FocusController
extends RefCounted


# -----------------------------------------------------------------------------
# Signals
# -----------------------------------------------------------------------------

## Emitted when a new frame texture is ready for display
signal frame_updated(texture: ImageTexture)

## Emitted when anatomical image is successfully captured
signal anatomical_captured(texture: ImageTexture, path: String)

## Emitted when anatomical capture fails
signal anatomical_capture_failed(reason: String)

## Emitted when exposure value changes
signal exposure_changed(us: int)


# -----------------------------------------------------------------------------
# State
# -----------------------------------------------------------------------------

## Current frame texture (reused to avoid allocations)
var _frame_texture: ImageTexture = null

## Frame counter for change detection
var _last_frame_count: int = 0

## Current exposure value in microseconds
var _exposure_us: int = 0

## Exposure limits
const EXPOSURE_MIN_US := 1000    # 1ms
const EXPOSURE_MAX_US := 100000  # 100ms
const EXPOSURE_STEP_US := 1000   # 1ms steps


# -----------------------------------------------------------------------------
# Lifecycle
# -----------------------------------------------------------------------------

## Initialize controller - load state from Settings
func initialize() -> void:
	_exposure_us = Settings.camera_exposure_us
	_last_frame_count = 0
	_frame_texture = null


## Clean up resources
func cleanup() -> void:
	_frame_texture = null


# -----------------------------------------------------------------------------
# Frame Processing
# -----------------------------------------------------------------------------

## Process frame updates - call each frame from _process
## Returns true if a new frame was processed
func process(_delta: float) -> bool:
	if not CameraClient.is_daemon_connected():
		return false

	# Check if new frame available
	var frame_count := CameraClient.get_frame_count()
	if frame_count == _last_frame_count:
		return false
	_last_frame_count = frame_count

	# Get frame data
	var frame_data := CameraClient.get_frame()
	if frame_data.is_empty():
		return false

	# Create image from grayscale data
	var width: int = Session.camera_width_px
	var height: int = Session.camera_height_px
	var expected_size: int = width * height
	if frame_data.size() != expected_size:
		push_error("Frame size mismatch: got %d bytes, expected %d (%dx%d)" % [
			frame_data.size(), expected_size, width, height
		])
		return false

	var image := Image.create_from_data(width, height, false, Image.FORMAT_L8, frame_data)

	# Create or update texture
	if _frame_texture == null:
		_frame_texture = ImageTexture.create_from_image(image)
	else:
		_frame_texture.update(image)

	frame_updated.emit(_frame_texture)
	return true


## Get the current frame texture (may be null if no frames received)
func get_frame_texture() -> ImageTexture:
	return _frame_texture


## Check if camera is connected and streaming
func is_camera_connected() -> bool:
	return CameraClient.is_daemon_connected()


# -----------------------------------------------------------------------------
# Anatomical Capture
# -----------------------------------------------------------------------------

## Capture current frame as anatomical reference image
## Saves to session directory and updates Session state
func capture_anatomical() -> void:
	# Validate camera connection
	if not CameraClient.is_daemon_connected():
		anatomical_capture_failed.emit("Camera not connected")
		push_warning("Cannot capture anatomical - camera not connected")
		return

	# Get current frame data
	var frame_data := CameraClient.get_frame()
	if frame_data.is_empty():
		anatomical_capture_failed.emit("No frame data available")
		push_warning("Cannot capture anatomical - no frame data")
		return

	# Create image from frame data
	var width: int = Session.camera_width_px
	var height: int = Session.camera_height_px
	var image := Image.create_from_data(width, height, false, Image.FORMAT_L8, frame_data)

	# Determine save directory (use absolute path)
	var save_dir: String = ProjectSettings.globalize_path("res://").path_join(Settings.last_save_directory)
	if not DirAccess.dir_exists_absolute(save_dir):
		var dir_err := DirAccess.make_dir_recursive_absolute(save_dir)
		if dir_err != OK:
			anatomical_capture_failed.emit("Failed to create save directory")
			push_error("Failed to create anatomical save directory: ", dir_err)
			return

	# Save image to disk
	var save_path := save_dir.path_join("anatomical.png")
	var save_err := image.save_png(save_path)
	if save_err != OK:
		anatomical_capture_failed.emit("Failed to save image file")
		push_error("Failed to save anatomical image: ", save_err)
		return

	# Create texture and update Session state
	var texture := ImageTexture.create_from_image(image)
	Session.set_anatomical_captured(save_path, texture)

	anatomical_captured.emit(texture, save_path)
	print("Anatomical image captured: ", save_path)


## Check if anatomical image has been captured
func has_anatomical() -> bool:
	return Session.state.has_anatomical


## Get the anatomical texture from Session (may be null)
func get_anatomical_texture() -> ImageTexture:
	return Session.state.anatomical_texture as ImageTexture


## Get the anatomical image path from Session
func get_anatomical_path() -> String:
	return Session.state.anatomical_path


# -----------------------------------------------------------------------------
# Exposure Control
# -----------------------------------------------------------------------------

## Set exposure value in microseconds
## Clamps to valid range and persists to Settings
func set_exposure(us: int) -> void:
	var clamped := clampi(us, EXPOSURE_MIN_US, EXPOSURE_MAX_US)
	if clamped == _exposure_us:
		return

	_exposure_us = clamped
	Settings.camera_exposure_us = clamped
	exposure_changed.emit(clamped)

	# TODO: Send exposure command to CameraClient when supported


## Get current exposure value in microseconds
func get_exposure() -> int:
	return _exposure_us


## Increment exposure by one step
func increment_exposure() -> void:
	set_exposure(_exposure_us + EXPOSURE_STEP_US)


## Decrement exposure by one step
func decrement_exposure() -> void:
	set_exposure(_exposure_us - EXPOSURE_STEP_US)


## Get exposure limits for UI (min, max, step)
func get_exposure_limits() -> Dictionary:
	return {
		"min": EXPOSURE_MIN_US,
		"max": EXPOSURE_MAX_US,
		"step": EXPOSURE_STEP_US,
	}
