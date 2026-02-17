## SessionConfigCard - Session directory and name configuration component
##
## Handles session directory selection and session name input.
## Emits signal when configuration changes.
class_name SessionConfigCard
extends MarginContainer


signal config_changed(directory: String, session_name: String)


# UI Controls
var _card: Control = null
var _directory_input: StyledLineEdit = null
var _name_input: StyledLineEdit = null


func _ready() -> void:
	_build_ui()
	_connect_signals()
	_load_state()


func _build_ui() -> void:
	size_flags_horizontal = Control.SIZE_EXPAND_FILL

	_card = Card.new()
	_card.title = "Session"
	add_child(_card)

	var content := VBoxContainer.new()
	content.theme_type_variation = "VBoxLG"
	_card.get_content_slot().add_child(content)

	# Directory row
	var dir_row := HBoxContainer.new()
	dir_row.theme_type_variation = "HBoxSM"
	content.add_child(dir_row)

	var dir_label := Label.new()
	dir_label.text = "Directory"
	dir_label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_SM
	dir_row.add_child(dir_label)

	_directory_input = StyledLineEdit.new()
	_directory_input.placeholder_text = Settings.last_save_directory if Settings.last_save_directory else "~/Documents/OpenISI"
	_directory_input.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_directory_input.custom_minimum_size.y = AppTheme.INPUT_HEIGHT
	dir_row.add_child(_directory_input)

	var browse_btn := StyledButton.new()
	browse_btn.text = "Browse"
	browse_btn.pressed.connect(_on_browse_pressed)
	dir_row.add_child(browse_btn)

	# Name row
	var name_row := HBoxContainer.new()
	name_row.theme_type_variation = "HBoxSM"
	content.add_child(name_row)

	var name_label := Label.new()
	name_label.text = "Name"
	name_label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_SM
	name_row.add_child(name_label)

	_name_input = StyledLineEdit.new()
	_name_input.placeholder_text = "session_name"
	_name_input.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_name_input.custom_minimum_size.y = AppTheme.INPUT_HEIGHT
	name_row.add_child(_name_input)

	var auto_btn := StyledButton.new()
	auto_btn.text = "Auto"
	auto_btn.pressed.connect(generate_auto_name)
	name_row.add_child(auto_btn)


func _connect_signals() -> void:
	if _directory_input:
		_directory_input.text_changed.connect(_on_directory_changed)

	if _name_input:
		_name_input.text_changed.connect(_on_name_changed)


func _load_state() -> void:
	# Load directory from Settings
	if _directory_input:
		_directory_input.text = Settings.last_save_directory

	# Generate auto-name for session
	generate_auto_name()


# -----------------------------------------------------------------------------
# Public API
# -----------------------------------------------------------------------------

## Get the current directory path
var directory: String:
	get:
		if _directory_input:
			return _directory_input.text.strip_edges()
		return ""
	set(value):
		if _directory_input:
			_directory_input.text = value


## Get the current session name
var session_name: String:
	get:
		if _name_input:
			return _name_input.text.strip_edges()
		return ""
	set(value):
		if _name_input:
			_name_input.text = value


## Generate an automatic session name based on current date
func generate_auto_name() -> void:
	if not _name_input:
		return

	var date := Time.get_date_dict_from_system()
	var base_name := "session_%04d-%02d-%02d" % [date.year, date.month, date.day]
	_name_input.text = "%s_%03d" % [base_name, 1]


## Check if the session name is valid (not empty)
func is_valid() -> bool:
	return not session_name.is_empty()


# -----------------------------------------------------------------------------
# Signal Handlers
# -----------------------------------------------------------------------------

func _on_directory_changed(new_text: String) -> void:
	Settings.last_save_directory = new_text
	config_changed.emit(new_text, session_name)


func _on_name_changed(new_text: String) -> void:
	config_changed.emit(directory, new_text)


func _on_browse_pressed() -> void:
	# Create file dialog for directory selection
	var dialog := FileDialog.new()
	dialog.file_mode = FileDialog.FILE_MODE_OPEN_DIR
	dialog.access = FileDialog.ACCESS_FILESYSTEM
	dialog.title = "Select Session Directory"

	# Set initial path
	if directory and DirAccess.dir_exists_absolute(directory):
		dialog.current_dir = directory
	elif Settings.last_save_directory and DirAccess.dir_exists_absolute(Settings.last_save_directory):
		dialog.current_dir = Settings.last_save_directory

	dialog.dir_selected.connect(_on_directory_selected.bind(dialog))
	dialog.canceled.connect(dialog.queue_free)

	# Add to scene tree and show
	get_tree().root.add_child(dialog)
	dialog.popup_centered_ratio(0.6)


func _on_directory_selected(dir: String, dialog: FileDialog) -> void:
	directory = dir
	Settings.last_save_directory = dir
	config_changed.emit(dir, session_name)
	dialog.queue_free()
