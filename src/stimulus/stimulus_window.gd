extends Window
## Stimulus window for OpenISI.
##
## Window container for StimulusDisplay. Manages fullscreen presentation
## on the secondary monitor during acquisition.

## Emitted when a sweep completes (forwarded from StimulusDisplay)
signal sweep_completed(direction: String)

## Reference to the StimulusDisplay child
var _display: StimulusDisplay = null


func _ready() -> void:
	title = "Stimulus"

	# Enable V-sync for frame-locked presentation
	# This ensures frames are presented at display refresh boundaries
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_ENABLED, get_window_id())

	# Create StimulusDisplay child
	_display = StimulusDisplay.new()
	_display.name = "StimulusDisplay"
	_display.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(_display)

	# Forward sweep_completed signal
	if _display.has_signal("sweep_completed"):
		_display.sweep_completed.connect(func(dir): sweep_completed.emit(dir))

	var vsync_mode := DisplayServer.window_get_vsync_mode(get_window_id())
	print("Stimulus window ready (V-sync: %s)" % _vsync_mode_name(vsync_mode))


func _vsync_mode_name(vsync_mode: DisplayServer.VSyncMode) -> String:
	match vsync_mode:
		DisplayServer.VSYNC_DISABLED: return "DISABLED"
		DisplayServer.VSYNC_ENABLED: return "ENABLED"
		DisplayServer.VSYNC_ADAPTIVE: return "ADAPTIVE"
		DisplayServer.VSYNC_MAILBOX: return "MAILBOX"
	assert(false, "Unknown vsync mode: %d" % vsync_mode)
	return ""


func _notification(what: int) -> void:
	if what == NOTIFICATION_WM_CLOSE_REQUEST:
		print("Stimulus window closing")
		queue_free()


## Get the StimulusDisplay for configuration
func get_display() -> StimulusDisplay:
	return _display


## Convenience: Get the dataset from the display
func get_dataset() -> StimulusDataset:
	if _display and _display.has_method("get_dataset"):
		return _display.get_dataset()
	return null


## Convenience: Export the dataset from the display
func export_dataset(output_dir: String) -> Error:
	if _display and _display.has_method("export_dataset"):
		return _display.export_dataset(output_dir)
	return ERR_DOES_NOT_EXIST
