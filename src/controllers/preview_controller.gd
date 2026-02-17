class_name PreviewController
extends RefCounted
## PreviewController - Stimulus preview management
##
## Manages the stimulus preview display, including loading,
## configuration updates, and play/stop control.
## Stimulus display reads directly from Config (SSoT).

signal play_state_changed(is_playing: bool)

var _preview_container: Control = null
var _stimulus_display: Control = null
var _play_button: StyledButton = null
var _is_playing: bool = false


## Initialize the preview controller with a container
func initialize(container: Control) -> void:
	_preview_container = container

	# Load and add stimulus display scene
	var stimulus_scene := preload("res://src/stimulus/stimulus_display.tscn")
	_stimulus_display = stimulus_scene.instantiate()
	_stimulus_display.set_anchors_preset(Control.PRESET_FULL_RECT)
	_stimulus_display.show_overlay = false
	_preview_container.add_child(_stimulus_display)


## Set the play button reference for state sync
func set_play_button(button: StyledButton) -> void:
	_play_button = button


## Refresh the stimulus display from Config
func refresh() -> void:
	if _stimulus_display:
		_stimulus_display.refresh()


## Start preview playback
func play() -> void:
	if _stimulus_display and not _is_playing:
		_stimulus_display.start()
		_is_playing = true
		_update_button_state()
		play_state_changed.emit(true)


## Stop preview playback
func stop() -> void:
	if _stimulus_display and _is_playing:
		_stimulus_display.stop()
		_is_playing = false
		_update_button_state()
		play_state_changed.emit(false)


## Toggle play/stop state
func toggle() -> void:
	if _is_playing:
		stop()
	else:
		play()


## Check if preview is currently playing
func is_playing() -> bool:
	return _is_playing


## Update button text/state based on play state
func _update_button_state() -> void:
	if _play_button:
		if _is_playing:
			_play_button.text = "Stop Preview"
			_play_button.button_pressed = false
		else:
			_play_button.text = "Play Preview"
			_play_button.button_pressed = true


## Get the stimulus display node
func get_stimulus_display() -> Control:
	return _stimulus_display
