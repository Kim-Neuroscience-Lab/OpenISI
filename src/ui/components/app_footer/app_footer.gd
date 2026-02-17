class_name AppFooter
extends PanelContainer
## Application footer with back navigation, secondary actions, primary action, and status.

signal back_pressed
signal primary_pressed
signal secondary_pressed(action: String)

var _hbox: HBoxContainer = null
var _back_button: StyledButton = null
var _secondary_container: HBoxContainer = null
var _spacer: Control = null
var _primary_section: VBoxContainer = null
var _primary_button: StyledButton = null
var _status_label: Label = null

var _secondary_buttons: Dictionary = {}  # action_id -> StyledButton


func _ready() -> void:
	_build_ui()

	_back_button.pressed.connect(func(): back_pressed.emit())
	_primary_button.pressed.connect(func(): primary_pressed.emit())

	# Connect to Session screen changes for back button visibility
	Session.screen_changed.connect(_on_screen_changed)
	_update_back_button_visibility(Session.current_screen)

	# Ensure footer appears above faded scroll content
	z_index = AppTheme.Z_INDEX_CHROME

	_apply_style()


func _build_ui() -> void:
	_hbox = HBoxContainer.new()
	_hbox.name = "HBoxContainer"
	add_child(_hbox)

	# Back button (left side, hidden by default)
	_back_button = StyledButton.new()
	_back_button.name = "BackButton"
	_back_button.text = "Back"
	_back_button.visible = false
	_hbox.add_child(_back_button)

	# Secondary actions container
	_secondary_container = HBoxContainer.new()
	_secondary_container.name = "SecondaryContainer"
	_hbox.add_child(_secondary_container)

	# Spacer to push primary section to the right
	_spacer = Control.new()
	_spacer.name = "Spacer"
	_spacer.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_hbox.add_child(_spacer)

	# Primary section (button + status label)
	_primary_section = VBoxContainer.new()
	_primary_section.name = "PrimarySection"
	_primary_section.alignment = BoxContainer.ALIGNMENT_CENTER
	_hbox.add_child(_primary_section)

	_primary_button = StyledButton.new()
	_primary_button.name = "PrimaryButton"
	_primary_button.text = "Continue"
	_primary_section.add_child(_primary_button)

	_status_label = Label.new()
	_status_label.name = "StatusLabel"
	_status_label.visible = false
	_status_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	_primary_section.add_child(_status_label)


func _apply_style() -> void:
	# Apply SSoT size constant
	custom_minimum_size.y = AppTheme.FOOTER_HEIGHT

	# Set container separations from theme variations
	if _hbox:
		_hbox.theme_type_variation = "HBoxLG"
	if _secondary_container:
		_secondary_container.theme_type_variation = "HBoxSM"
	if _primary_section:
		_primary_section.theme_type_variation = "VBoxXS"

	# Transparent footer - use theme variation
	theme_type_variation = "PanelFooter"

	# Style status label
	_status_label.theme_type_variation = "LabelSmall"


## Sets the primary action button text.
func set_primary_text(text: String) -> void:
	_primary_button.text = text


## Enables or disables the primary action button.
func set_primary_enabled(enabled: bool) -> void:
	_primary_button.disabled = not enabled


## Sets the primary button highlighted state (nightlight when true).
func set_primary_highlighted(highlighted: bool) -> void:
	_primary_button.button_pressed = highlighted


## Sets the status message below the primary button.
func set_status(status: String, status_type: String = "info") -> void:
	_status_label.text = status
	_status_label.visible = not status.is_empty()

	# Use theme variation for status coloring
	_status_label.theme_type_variation = AppTheme.get_status_label_variation(status_type, true)


## Clears all secondary action buttons.
func clear_secondary_actions() -> void:
	for child in _secondary_container.get_children():
		child.queue_free()
	_secondary_buttons.clear()


## Adds a secondary action button.
func add_secondary_action(action_id: String, text: String) -> void:
	var btn := StyledButton.new()
	btn.text = text

	btn.pressed.connect(func(): secondary_pressed.emit(action_id))

	_secondary_container.add_child(btn)
	_secondary_buttons[action_id] = btn


## Removes a secondary action button.
func remove_secondary_action(action_id: String) -> void:
	if _secondary_buttons.has(action_id):
		_secondary_buttons[action_id].queue_free()
		_secondary_buttons.erase(action_id)


## Shows or hides the back button.
func set_back_button_visible(should_show: bool) -> void:
	if _back_button:
		_back_button.visible = should_show


func _on_screen_changed(new_screen: Session.Screen) -> void:
	_update_back_button_visibility(new_screen)


func _update_back_button_visibility(screen: Session.Screen) -> void:
	if _back_button:
		# Get screen config to determine if back should show
		var config := Session.get_screen_config(screen)
		_back_button.visible = bool(config["show_back"])
