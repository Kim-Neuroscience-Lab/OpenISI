## BaseScreen - Abstract base class for all workflow screens
##
## Provides common signals, validation, and lifecycle hooks.
## All screen implementations should extend this class.
class_name BaseScreen
extends Control

signal validation_changed(is_valid: bool)
@warning_ignore("unused_signal")  # Emitted by subclasses
signal request_next_screen

var _is_valid := false

func _ready() -> void:
	_build_ui()
	_connect_signals()
	_load_state()
	_validate()

## Override to build the screen UI
func _build_ui() -> void:
	pass

## Override to connect UI signals
func _connect_signals() -> void:
	pass

## Override to load initial state from config/session
func _load_state() -> void:
	pass

## Override to validate screen state - call _set_valid() at end
func _validate() -> void:
	pass

## Call this to update validation state
func _set_valid(valid: bool) -> void:
	if _is_valid != valid:
		_is_valid = valid
		validation_changed.emit(valid)

## Override to return accumulated screen data
func get_screen_data() -> Dictionary:
	return {}

## Check if screen is currently valid
func is_valid() -> bool:
	return _is_valid
