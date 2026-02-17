class_name ErrorDialog
extends CanvasLayer

## Modal error dialog for displaying errors to users.
## Automatically handles error queue - only shows one dialog at a time.

signal dismissed(error: ErrorHandler.AppError)

# Dialog state
var _current_error: ErrorHandler.AppError = null
var _error_queue: Array[ErrorHandler.AppError] = []
var _is_showing := false

# UI components
var _overlay: ColorRect
var _card: PanelContainer
var _title_label: Label
var _severity_icon: Label
var _message_label: Label
var _details_container: VBoxContainer
var _details_label: RichTextLabel
var _details_toggle: Button
var _dismiss_button: Control  # StyledButton
var _retry_button: Control    # StyledButton
var _button_container: HBoxContainer

# Animation
var _tween: Tween

const FADE_DURATION := AppTheme.ANIM_MICRO


func _ready() -> void:
	layer = AppTheme.Z_INDEX_MODAL
	visible = false
	_build_ui()


func _build_ui() -> void:
	# Modal overlay (darkens background)
	_overlay = ColorRect.new()
	_overlay.name = "Overlay"
	_overlay.color = AppTheme.with_alpha(AppTheme.BG_BASE, 0.0)
	_overlay.set_anchors_preset(Control.PRESET_FULL_RECT)
	_overlay.mouse_filter = Control.MOUSE_FILTER_STOP
	add_child(_overlay)

	# Center container
	var center := CenterContainer.new()
	center.name = "CenterContainer"
	center.set_anchors_preset(Control.PRESET_FULL_RECT)
	center.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_overlay.add_child(center)

	# Card container - uses PanelModal theme variation
	_card = PanelContainer.new()
	_card.name = "Card"
	_card.custom_minimum_size.x = AppTheme.DIALOG_WIDTH
	_card.modulate.a = 0.0
	_card.theme_type_variation = "PanelModal"
	center.add_child(_card)

	# Main layout - uses MarginPanel theme variation
	var margin := MarginContainer.new()
	margin.name = "Margin"
	margin.theme_type_variation = "MarginPanel"
	_card.add_child(margin)

	var vbox := VBoxContainer.new()
	vbox.name = "Content"
	vbox.theme_type_variation = "VBoxMD"
	margin.add_child(vbox)

	# Header (severity icon + title)
	var header := HBoxContainer.new()
	header.name = "Header"
	header.theme_type_variation = "HBoxSM"
	vbox.add_child(header)

	_severity_icon = Label.new()
	_severity_icon.name = "SeverityIcon"
	_severity_icon.theme_type_variation = "LabelIconTitle"
	header.add_child(_severity_icon)

	_title_label = Label.new()
	_title_label.name = "Title"
	_title_label.theme_type_variation = "LabelTitleBold"
	header.add_child(_title_label)

	# Message
	_message_label = Label.new()
	_message_label.name = "Message"
	_message_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_message_label.theme_type_variation = "LabelTitleMuted"
	vbox.add_child(_message_label)

	# Details section (collapsible)
	_details_container = VBoxContainer.new()
	_details_container.name = "DetailsContainer"
	_details_container.visible = false
	_details_container.theme_type_variation = "VBoxXS"
	vbox.add_child(_details_container)

	_details_toggle = Button.new()
	_details_toggle.name = "DetailsToggle"
	_details_toggle.text = "Show Details"
	_details_toggle.flat = true
	_details_toggle.theme_type_variation = "ButtonLink"
	_details_toggle.pressed.connect(_toggle_details)
	_details_container.add_child(_details_toggle)

	_details_label = RichTextLabel.new()
	_details_label.name = "Details"
	_details_label.bbcode_enabled = true
	_details_label.fit_content = true
	_details_label.scroll_active = false
	_details_label.custom_minimum_size.y = AppTheme.DIALOG_DETAILS_HEIGHT
	_details_label.visible = false
	_details_label.theme_type_variation = "RichTextLabelDetails"
	_details_container.add_child(_details_label)

	# Spacer
	var spacer := Control.new()
	spacer.custom_minimum_size.y = AppTheme.SPACING_SM
	vbox.add_child(spacer)

	# Buttons
	_button_container = HBoxContainer.new()
	_button_container.name = "Buttons"
	_button_container.alignment = BoxContainer.ALIGNMENT_END
	_button_container.theme_type_variation = "HBoxSM"
	vbox.add_child(_button_container)

	# Create retry button (hidden by default)
	_retry_button = _create_button("Retry", false)
	_retry_button.visible = false
	_retry_button.pressed.connect(_on_retry_pressed)
	_button_container.add_child(_retry_button)

	# Create dismiss button
	_dismiss_button = _create_button("Dismiss", true)
	_dismiss_button.pressed.connect(_on_dismiss_pressed)
	_button_container.add_child(_dismiss_button)


func _create_button(text: String, is_primary: bool) -> Control:
	# Use StyledButton if available, otherwise fallback to basic Button
	if ClassDB.class_exists("StyledButton"):
		var btn := StyledButton.new()
		btn.text = text
		if is_primary:
			btn.button_pressed = true  # Nightlight style for primary
		return btn
	else:
		var btn := Button.new()
		btn.text = text
		# Uses default Button theme styling (FONT_SM defined in theme)
		return btn


## Show an error in the dialog. Queues if another error is showing.
func show_error(error: ErrorHandler.AppError) -> void:
	if _is_showing:
		_error_queue.append(error)
		return

	_current_error = error
	_update_content()
	_show_animated()


func _update_content() -> void:
	if not _current_error:
		return

	# Severity icon and color - use theme variations
	var icon_text := ""
	var icon_variation := "LabelIconTitle"
	var title_variation := "LabelTitleBold"

	match _current_error.severity:
		ErrorHandler.Severity.INFO:
			icon_text = "i"
			icon_variation = "LabelIconTitleSuccess"
			title_variation = "LabelTitleSuccess"
		ErrorHandler.Severity.WARNING:
			icon_text = "!"
			icon_variation = "LabelIconTitleError"
			title_variation = "LabelTitleError"
		ErrorHandler.Severity.ERROR:
			icon_text = "x"
			icon_variation = "LabelIconTitleError"
			title_variation = "LabelTitleError"
		ErrorHandler.Severity.CRITICAL:
			icon_text = "!!"
			icon_variation = "LabelIconTitleError"
			title_variation = "LabelTitleError"

	_severity_icon.text = icon_text
	_severity_icon.theme_type_variation = icon_variation

	# Title based on category
	_title_label.text = _get_title_for_category(_current_error.category)
	_title_label.theme_type_variation = title_variation

	# Message
	_message_label.text = _current_error.message

	# Details
	if _current_error.details.is_empty():
		_details_container.visible = false
	else:
		_details_container.visible = true
		_details_label.text = _current_error.details
		_details_label.visible = false
		_details_toggle.text = "Show Details"

	# Retry button
	_retry_button.visible = _current_error.recoverable and _current_error.retry_action.is_valid()


func _get_title_for_category(category: ErrorHandler.Category) -> String:
	match category:
		ErrorHandler.Category.HARDWARE:
			return "Hardware Error"
		ErrorHandler.Category.CAMERA:
			return "Camera Error"
		ErrorHandler.Category.DISPLAY:
			return "Display Error"
		ErrorHandler.Category.CONFIG:
			return "Configuration Error"
		ErrorHandler.Category.ACQUISITION:
			return "Acquisition Error"
		ErrorHandler.Category.STIMULUS:
			return "Stimulus Error"
		ErrorHandler.Category.EXPORT:
			return "Export Error"
		_:
			return "Error"


func _toggle_details() -> void:
	_details_label.visible = not _details_label.visible
	_details_toggle.text = "Hide Details" if _details_label.visible else "Show Details"


func _show_animated() -> void:
	visible = true
	_is_showing = true

	if _tween:
		_tween.kill()
	_tween = create_tween()
	_tween.set_parallel(true)
	_tween.tween_property(_overlay, "color:a", AppTheme.SHADOW_ALPHA_MODAL, FADE_DURATION)
	_tween.tween_property(_card, "modulate:a", 1.0, FADE_DURATION)


func _hide_animated() -> void:
	if _tween:
		_tween.kill()
	_tween = create_tween()
	_tween.set_parallel(true)
	_tween.tween_property(_overlay, "color:a", 0.0, FADE_DURATION)
	_tween.tween_property(_card, "modulate:a", 0.0, FADE_DURATION)
	_tween.chain().tween_callback(_on_hide_complete)


func _on_hide_complete() -> void:
	visible = false
	_is_showing = false

	if _current_error:
		dismissed.emit(_current_error)
		ErrorHandler.dismiss_error(_current_error)
		_current_error = null

	# Show next queued error
	if not _error_queue.is_empty():
		var next_error: ErrorHandler.AppError = _error_queue.pop_front()
		call_deferred("show_error", next_error)


func _on_dismiss_pressed() -> void:
	_hide_animated()


func _on_retry_pressed() -> void:
	if _current_error and _current_error.retry_action.is_valid():
		var action := _current_error.retry_action
		_hide_animated()
		# Call retry action after dialog closes
		await dismissed
		action.call()


func _input(event: InputEvent) -> void:
	if not visible:
		return

	# Close on Escape
	if event is InputEventKey and event.pressed and event.keycode == KEY_ESCAPE:
		_on_dismiss_pressed()
		get_viewport().set_input_as_handled()
