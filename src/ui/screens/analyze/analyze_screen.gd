extends BaseScreen
## Analyze screen: Acquisition complete, analyze and review results.
##
## Shows session summary, quality metrics, and output file information.
## Provides analysis tools and allows starting a new session.
## Scene preloads are sourced from SceneRegistry (SSoT).

# UI references
var _status_title: Label = null
var _status_subtitle: Label = null
var _quality_pill: StatusPill = null

# Metrics
var _frames_row: InfoRow = null
var _duration_row: InfoRow = null
var _dropped_row: InfoRow = null
var _size_row: InfoRow = null
var _avg_fps_row: InfoRow = null

# Output
var _output_path_label: Label = null

# Buttons
var _new_session_button: StyledButton = null
var _open_folder_button: StyledButton = null


func _ready() -> void:
	super._ready()
	_apply_theme()

	# Always valid in done screen
	validation_changed.emit(true)



func _build_ui() -> void:
	# ScrollContainer for content that may exceed available space
	var scroll := SmoothScrollContainer.new()
	scroll.name = "ScrollContainer"
	scroll.set_anchors_preset(Control.PRESET_FULL_RECT)
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	scroll.vertical_scroll_mode = ScrollContainer.SCROLL_MODE_AUTO
	scroll.scrollbar_vertical_inset = AppTheme.SCROLL_FADE_HEIGHT
	add_child(scroll)

	# Inner margin for content positioning within scroll area
	var inner_margin := MarginContainer.new()
	inner_margin.name = "InnerMargin"
	inner_margin.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	inner_margin.size_flags_vertical = Control.SIZE_EXPAND_FILL
	inner_margin.theme_type_variation = "MarginScreenContent"
	scroll.add_child(inner_margin)

	# Main vertical layout, centered
	var vbox := VBoxContainer.new()
	vbox.name = "MainLayout"
	vbox.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	vbox.size_flags_vertical = Control.SIZE_EXPAND_FILL
	vbox.theme_type_variation = "VBox2XL"
	vbox.alignment = BoxContainer.ALIGNMENT_CENTER
	inner_margin.add_child(vbox)

	# Success header section
	_build_header_section(vbox)

	# Summary card
	_build_summary_section(vbox)

	# Output card
	_build_output_section(vbox)

	# Action buttons
	_build_actions_section(vbox)


func _build_header_section(parent: Control) -> void:
	var header := VBoxContainer.new()
	header.theme_type_variation = "VBoxSM"
	header.alignment = BoxContainer.ALIGNMENT_CENTER
	parent.add_child(header)

	# Status title with icon
	var title_row := HBoxContainer.new()
	title_row.alignment = BoxContainer.ALIGNMENT_CENTER
	title_row.theme_type_variation = "HBoxMD"
	header.add_child(title_row)

	_status_title = Label.new()
	_status_title.text = "Acquisition Complete"
	_status_title.theme_type_variation = "LabelTitleSuccess"
	title_row.add_child(_status_title)

	# Subtitle
	_status_subtitle = Label.new()
	_status_subtitle.text = "Session data has been saved successfully"
	_status_subtitle.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_status_subtitle.theme_type_variation = "LabelCaption"
	header.add_child(_status_subtitle)


func _build_summary_section(parent: Control) -> void:
	var summary_card := Card.new()
	summary_card.title = "Session Summary"
	summary_card.size_flags_horizontal = Control.SIZE_SHRINK_CENTER
	summary_card.custom_minimum_size = Vector2(AppTheme.CARD_WIDTH_MD, 0)
	parent.add_child(summary_card)

	var content := VBoxContainer.new()
	content.theme_type_variation = "VBoxLG"
	summary_card.get_content_slot().add_child(content)

	# Quality status
	var quality_row := HBoxContainer.new()
	quality_row.alignment = BoxContainer.ALIGNMENT_CENTER
	quality_row.theme_type_variation = "HBoxMD"
	content.add_child(quality_row)

	_quality_pill = StatusPill.new()
	_quality_pill.status = "success"
	_quality_pill.text = "No Errors"
	quality_row.add_child(_quality_pill)

	# Divider
	var divider := Divider.new()
	divider.margin = 8
	content.add_child(divider)

	# Metrics in two columns
	var metrics_row := HBoxContainer.new()
	metrics_row.theme_type_variation = "HBox2XL"
	content.add_child(metrics_row)

	# Left column
	var left_col := VBoxContainer.new()
	left_col.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	left_col.theme_type_variation = "VBoxSM"
	metrics_row.add_child(left_col)

	_frames_row = InfoRow.new()
	_frames_row.label_text = "Total Frames"
	_frames_row.value_text = "48,000"
	_frames_row.mono_value = true
	left_col.add_child(_frames_row)

	_duration_row = InfoRow.new()
	_duration_row.label_text = "Duration"
	_duration_row.value_text = "26:40"
	_duration_row.important_value = true
	left_col.add_child(_duration_row)

	_dropped_row = InfoRow.new()
	_dropped_row.label_text = "Dropped Frames"
	_dropped_row.value_text = "0"
	_dropped_row.status = "success"
	left_col.add_child(_dropped_row)

	# Right column
	var right_col := VBoxContainer.new()
	right_col.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	right_col.theme_type_variation = "VBoxSM"
	metrics_row.add_child(right_col)

	_size_row = InfoRow.new()
	_size_row.label_text = "File Size"
	_size_row.value_text = "23.4 GB"
	_size_row.mono_value = true
	right_col.add_child(_size_row)

	_avg_fps_row = InfoRow.new()
	_avg_fps_row.label_text = "Avg Frame Rate"
	_avg_fps_row.value_text = "30.0 fps"
	_avg_fps_row.mono_value = true
	right_col.add_child(_avg_fps_row)


func _build_output_section(parent: Control) -> void:
	var output_card := Card.new()
	output_card.title = "Output Location"
	output_card.size_flags_horizontal = Control.SIZE_SHRINK_CENTER
	output_card.custom_minimum_size = Vector2(AppTheme.CARD_WIDTH_MD, 0)
	parent.add_child(output_card)

	var content := VBoxContainer.new()
	content.theme_type_variation = "VBoxMD"
	output_card.get_content_slot().add_child(content)

	# Output path
	_output_path_label = Label.new()
	_output_path_label.text = "/data/sessions/session_2026-01-19_001/"
	_output_path_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_output_path_label.theme_type_variation = "LabelMonoMuted"
	content.add_child(_output_path_label)

	# Files list (compact)
	var files_section := VBoxContainer.new()
	files_section.theme_type_variation = "VBoxXS"
	content.add_child(files_section)

	var files_header := Label.new()
	files_header.text = "Contents:"
	files_header.theme_type_variation = "LabelSmallDim"
	files_section.add_child(files_header)

	var files := ["frames.h5 (23.4 GB)", "anatomical.png", "metadata.json", "stimulus_log.csv"]
	for file_name in files:
		var file_label := Label.new()
		file_label.text = file_name
		file_label.theme_type_variation = "LabelSmall"
		files_section.add_child(file_label)


func _build_actions_section(parent: Control) -> void:
	var actions := VBoxContainer.new()
	actions.theme_type_variation = "VBoxMD"
	actions.alignment = BoxContainer.ALIGNMENT_CENTER
	parent.add_child(actions)

	# Button row
	var button_row := HBoxContainer.new()
	button_row.alignment = BoxContainer.ALIGNMENT_CENTER
	button_row.theme_type_variation = "HBoxLG"
	actions.add_child(button_row)

	_open_folder_button = StyledButton.new()
	_open_folder_button.text = "Open Folder"
	button_row.add_child(_open_folder_button)

	_new_session_button = StyledButton.new()
	_new_session_button.text = "New Session"
	_new_session_button.button_pressed = true  # Nightlight - primary action
	button_row.add_child(_new_session_button)


func _apply_theme() -> void:
	# Most styling is applied during _build_ui
	pass


func _connect_signals() -> void:
	if _new_session_button:
		_new_session_button.pressed.connect(_on_new_session_pressed)

	if _open_folder_button:
		_open_folder_button.pressed.connect(_on_open_folder_pressed)


func _load_state() -> void:
	_load_results()


func _load_results() -> void:
	# Access typed acquisition results from SessionState
	var acquisition := Session.state.acquisition

	# Total frames
	if _frames_row:
		_frames_row.set_value(FormatUtils.format_number(acquisition.total_frames))

	# Duration
	var duration_sec := acquisition.get_duration_sec()
	if _duration_row:
		var minutes := int(duration_sec / 60)
		var seconds := int(duration_sec) % 60
		_duration_row.set_value("%d:%02d" % [minutes, seconds])

	# Average FPS (using typed helper method)
	if _avg_fps_row:
		_avg_fps_row.set_value("%.1f fps" % acquisition.get_avg_fps())

	# Update quality indicator based on dropped frames
	var dropped := acquisition.dropped_frames
	if _quality_pill:
		if dropped == 0:
			_quality_pill.status = "success"
			_quality_pill.text = "No Errors"
		elif dropped < 10:
			_quality_pill.status = "warning"
			_quality_pill.text = "%d Dropped" % dropped
		else:
			_quality_pill.status = "error"
			_quality_pill.text = "%d Dropped" % dropped

	if _dropped_row:
		_dropped_row.set_value(str(dropped))
		_dropped_row.status = "success" if dropped == 0 else "warning"


func _on_new_session_pressed() -> void:
	Session.reset_session()


func _on_open_folder_pressed() -> void:
	# Open the output folder in system file browser
	var path := "/data/sessions/session_2026-01-19_001/"
	OS.shell_open(path)
