extends BaseScreen
## Focus screen: Live preview, adjust exposure, capture anatomical image.
##
## UI-only responsibilities:
## - Layout and control instantiation
## - Connect controller signals to UI updates
## - Route user actions to controller
##
## Domain logic is in FocusController.

# Controller
var _controller: FocusController = null

# Preview area
var _preview_rect: TextureRect = null
var _preview_placeholder: ColorRect = null

# Anatomical preview
var _anatomical_rect: TextureRect = null
var _anatomical_placeholder: ColorRect = null

# Controls
var _exposure_slider: HSlider = null
var _exposure_value: Label = null
var _exposure_minus: StyledButton = null
var _exposure_plus: StyledButton = null

# Head ring controls
var _ring_checkbox: CheckBox = null
var _ring_radius_value: Label = null
var _ring_center_value: Label = null

# Anatomical capture
var _capture_button: StyledButton = null
var _capture_status: Label = null

# Head ring state (UI-only, not persisted)
var _ring_visible := true
var _ring_radius := AppTheme.FOCUS_RING_RADIUS_DEFAULT
var _ring_center := Vector2(AppTheme.FOCUS_PREVIEW_CENTER, AppTheme.FOCUS_PREVIEW_CENTER)


func _ready() -> void:
	_setup_controller()
	super._ready()
	_apply_theme()
	_update_ui()


func _setup_controller() -> void:
	_controller = FocusController.new()
	_controller.initialize()

	# Connect controller signals
	_controller.frame_updated.connect(_on_frame_updated)
	_controller.anatomical_captured.connect(_on_anatomical_captured)
	_controller.anatomical_capture_failed.connect(_on_anatomical_capture_failed)
	_controller.exposure_changed.connect(_on_exposure_changed)


func _load_state() -> void:
	# Load anatomical from Session if already captured
	if _controller.has_anatomical():
		_load_anatomical_image()


func _process(_delta: float) -> void:
	_controller.process(_delta)


func _exit_tree() -> void:
	if _controller:
		_controller.cleanup()


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

	# Main horizontal layout
	var hbox := HBoxContainer.new()
	hbox.name = "MainLayout"
	hbox.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	hbox.size_flags_vertical = Control.SIZE_EXPAND_FILL
	hbox.theme_type_variation = "HBox2XL"
	inner_margin.add_child(hbox)

	# Left side: Preview card with both live preview and anatomical
	_build_preview_section(hbox)

	# Right side: Controls
	_build_controls_section(hbox)


func _build_preview_section(parent: Control) -> void:
	var preview_card := Card.new()
	preview_card.title = "Live Preview"
	preview_card.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	preview_card.size_flags_vertical = Control.SIZE_EXPAND_FILL
	preview_card.size_flags_stretch_ratio = 2.0
	parent.add_child(preview_card)

	# Use a Control as the content container for manual layout
	var content := Control.new()
	content.name = "PreviewContent"
	content.set_anchors_preset(Control.PRESET_FULL_RECT)
	preview_card.get_content_slot().add_child(content)

	# Preview well with rounded clipping
	var preview_well := _create_rounded_well("PreviewWell")
	content.add_child(preview_well.container)

	# Preview rect (inside the viewport)
	_preview_rect = TextureRect.new()
	_preview_rect.name = "PreviewRect"
	_preview_rect.set_anchors_preset(Control.PRESET_FULL_RECT)
	_preview_rect.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
	_preview_rect.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_CENTERED
	preview_well.viewport.add_child(_preview_rect)

	# Placeholder
	_preview_placeholder = ColorRect.new()
	_preview_placeholder.name = "Placeholder"
	_preview_placeholder.set_anchors_preset(Control.PRESET_FULL_RECT)
	_preview_placeholder.color = AppTheme.WELL
	preview_well.viewport.add_child(_preview_placeholder)

	var placeholder_label := Label.new()
	placeholder_label.text = "Camera Preview"
	placeholder_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	placeholder_label.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	placeholder_label.set_anchors_preset(Control.PRESET_CENTER)
	placeholder_label.theme_type_variation = "LabelCaption"
	_preview_placeholder.add_child(placeholder_label)

	# Anatomical well with rounded clipping
	var anatomical_well := _create_rounded_well("AnatomicalWell")
	content.add_child(anatomical_well.container)

	# Anatomical image rect (inside the viewport)
	_anatomical_rect = TextureRect.new()
	_anatomical_rect.name = "AnatomicalRect"
	_anatomical_rect.set_anchors_preset(Control.PRESET_FULL_RECT)
	_anatomical_rect.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
	_anatomical_rect.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_CENTERED
	_anatomical_rect.visible = false
	anatomical_well.viewport.add_child(_anatomical_rect)

	# Placeholder (shown when no anatomical captured)
	_anatomical_placeholder = ColorRect.new()
	_anatomical_placeholder.name = "AnatomicalPlaceholder"
	_anatomical_placeholder.set_anchors_preset(Control.PRESET_FULL_RECT)
	_anatomical_placeholder.color = AppTheme.WELL
	anatomical_well.viewport.add_child(_anatomical_placeholder)

	var anat_placeholder_label := Label.new()
	anat_placeholder_label.text = "Anatomical"
	anat_placeholder_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	anat_placeholder_label.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	anat_placeholder_label.set_anchors_preset(Control.PRESET_CENTER)
	anat_placeholder_label.theme_type_variation = "LabelCaption"
	_anatomical_placeholder.add_child(anat_placeholder_label)

	# Layout: Preview fills vertical space (square), anatomical fills remaining horizontal space
	var spacing := float(AppTheme.SPACING_LG)

	content.resized.connect(func():
		var w := content.size.x
		var h := content.size.y

		# Preview fills vertical space as a square
		var preview_side := h

		# Anatomical fills remaining horizontal space as a square (capped by height)
		var remaining_width := w - preview_side - spacing
		var anat_side := minf(remaining_width, h)

		# Position and size preview
		preview_well.container.position = Vector2.ZERO
		preview_well.container.size = Vector2(preview_side, preview_side)
		preview_well.viewport.size = Vector2i(int(preview_side), int(preview_side))

		# Position and size anatomical
		anatomical_well.container.position = Vector2(preview_side + spacing, 0)
		anatomical_well.container.size = Vector2(anat_side, anat_side)
		anatomical_well.viewport.size = Vector2i(int(anat_side), int(anat_side))
	)


func _create_rounded_well(well_name: String) -> Dictionary:
	## Create a well with rounded corner clipping using SubViewport.
	## Returns {container: SubViewportContainer, viewport: SubViewport}

	# SubViewportContainer displays the viewport content with rounded masking
	var container := SubViewportContainer.new()
	container.name = well_name
	container.stretch = true

	# Apply rounded mask shader for clipping and inset styling
	var mat := AppTheme.create_rounded_mask_material()
	container.material = mat

	# Update shader rect_size when container resizes
	container.resized.connect(func():
		mat.set_shader_parameter("rect_size", container.size)
	)

	# SubViewport renders the content
	var viewport := SubViewport.new()
	viewport.name = "Viewport"
	viewport.transparent_bg = true
	viewport.handle_input_locally = false
	viewport.gui_disable_input = true
	container.add_child(viewport)

	return {"container": container, "viewport": viewport}


func _build_controls_section(parent: Control) -> void:
	var controls_card := Card.new()
	controls_card.title = "Controls"
	controls_card.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	controls_card.size_flags_vertical = Control.SIZE_SHRINK_BEGIN
	parent.add_child(controls_card)

	var content := VBoxContainer.new()
	content.theme_type_variation = "VBoxLG"
	controls_card.get_content_slot().add_child(content)

	# Exposure section
	_build_exposure_section(content)

	# Divider
	var divider1 := Divider.new()
	divider1.margin = 8
	content.add_child(divider1)

	# Head ring section
	_build_head_ring_section(content)

	# Divider
	var divider2 := Divider.new()
	divider2.margin = 8
	content.add_child(divider2)

	# Anatomical capture section
	_build_anatomical_section(content)


func _build_exposure_section(parent: Control) -> void:
	var section := VBoxContainer.new()
	section.theme_type_variation = "VBoxMD"
	parent.add_child(section)

	# Section header
	var header := SectionHeader.new()
	header.title = "EXPOSURE"
	section.add_child(header)

	# Slider row
	var slider_row := HBoxContainer.new()
	slider_row.theme_type_variation = "HBoxSM"
	section.add_child(slider_row)

	# Get limits from controller
	var limits: Dictionary = _controller.get_exposure_limits()

	# Minus button
	_exposure_minus = StyledButton.new()
	_exposure_minus.text = "-"
	slider_row.add_child(_exposure_minus)

	# Slider
	_exposure_slider = HSlider.new()
	_exposure_slider.min_value = limits.min
	_exposure_slider.max_value = limits.max
	_exposure_slider.value = _controller.get_exposure()
	_exposure_slider.step = limits.step
	_exposure_slider.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	slider_row.add_child(_exposure_slider)

	# Plus button
	_exposure_plus = StyledButton.new()
	_exposure_plus.text = "+"
	slider_row.add_child(_exposure_plus)

	# Value label
	_exposure_value = Label.new()
	_exposure_value.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	_exposure_value.theme_type_variation = "LabelMono"
	section.add_child(_exposure_value)
	_update_exposure_label()


func _build_head_ring_section(parent: Control) -> void:
	var section := VBoxContainer.new()
	section.theme_type_variation = "VBoxMD"
	parent.add_child(section)

	# Section header
	var header := SectionHeader.new()
	header.title = "HEAD RING"
	section.add_child(header)

	# Checkbox row
	var checkbox_row := HBoxContainer.new()
	checkbox_row.theme_type_variation = "HBoxLG"
	section.add_child(checkbox_row)

	_ring_checkbox = CheckBox.new()
	_ring_checkbox.text = "Show overlay"
	_ring_checkbox.button_pressed = _ring_visible
	checkbox_row.add_child(_ring_checkbox)

	# Radius value
	var radius_row := InfoRow.new()
	radius_row.label_text = "Radius"
	radius_row.mono_value = true
	section.add_child(radius_row)
	_ring_radius_value = radius_row._value
	_update_ring_labels()

	# Center value
	var center_row := InfoRow.new()
	center_row.label_text = "Center"
	center_row.mono_value = true
	section.add_child(center_row)
	_ring_center_value = center_row._value
	_update_ring_labels()


func _build_anatomical_section(parent: Control) -> void:
	var section := VBoxContainer.new()
	section.theme_type_variation = "VBoxMD"
	parent.add_child(section)

	# Section header
	var header := SectionHeader.new()
	header.title = "ANATOMICAL REFERENCE"
	section.add_child(header)

	# Status
	_capture_status = Label.new()
	section.add_child(_capture_status)

	# Capture button
	_capture_button = StyledButton.new()
	_capture_button.button_pressed = true  # Nightlight - required action
	section.add_child(_capture_button)


func _apply_theme() -> void:
	pass


func _connect_signals() -> void:
	if _exposure_slider:
		_exposure_slider.value_changed.connect(_on_slider_exposure_changed)

	if _exposure_minus:
		_exposure_minus.pressed.connect(_on_exposure_minus)

	if _exposure_plus:
		_exposure_plus.pressed.connect(_on_exposure_plus)

	if _ring_checkbox:
		_ring_checkbox.toggled.connect(_on_ring_visibility_changed)

	if _capture_button:
		_capture_button.pressed.connect(_on_capture_pressed)


func _update_ui() -> void:
	_update_exposure_label()
	_update_ring_labels()
	_update_anatomical_ui()


func _update_exposure_label() -> void:
	if _exposure_value:
		_exposure_value.text = "%s us" % FormatUtils.format_number(_controller.get_exposure())


func _update_ring_labels() -> void:
	if _ring_radius_value:
		_ring_radius_value.text = "%d px" % _ring_radius

	if _ring_center_value:
		_ring_center_value.text = "(%d, %d)" % [int(_ring_center.x), int(_ring_center.y)]


func _update_anatomical_ui() -> void:
	var has_anat := _controller.has_anatomical()

	if has_anat:
		if _anatomical_placeholder:
			_anatomical_placeholder.visible = false
		if _anatomical_rect:
			_anatomical_rect.visible = true
		if _capture_status:
			_capture_status.text = "Captured"
			_capture_status.theme_type_variation = "LabelSuccess"
		if _capture_button:
			_capture_button.text = "Recapture Anatomical"
			_capture_button.button_pressed = false
	else:
		if _anatomical_placeholder:
			_anatomical_placeholder.visible = true
		if _anatomical_rect:
			_anatomical_rect.visible = false
		if _capture_status:
			_capture_status.text = "Not yet captured"
			_capture_status.theme_type_variation = "LabelError"
		if _capture_button:
			_capture_button.text = "Capture Anatomical"
			_capture_button.button_pressed = true


# -----------------------------------------------------------------------------
# Controller Signal Handlers
# -----------------------------------------------------------------------------

func _on_frame_updated(texture: ImageTexture) -> void:
	if _preview_rect:
		_preview_rect.texture = texture
	if _preview_placeholder:
		_preview_placeholder.visible = false


func _on_anatomical_captured(texture: ImageTexture, _path: String) -> void:
	if _anatomical_rect:
		_anatomical_rect.texture = texture

	_update_anatomical_ui()
	validation_changed.emit(true)


func _on_anatomical_capture_failed(_reason: String) -> void:
	# Could show error toast here if needed
	pass


func _on_exposure_changed(us: int) -> void:
	if _exposure_slider and int(_exposure_slider.value) != us:
		_exposure_slider.value = us
	_update_exposure_label()


# -----------------------------------------------------------------------------
# UI Signal Handlers
# -----------------------------------------------------------------------------

func _on_slider_exposure_changed(value: float) -> void:
	_controller.set_exposure(int(value))


func _on_exposure_minus() -> void:
	_controller.decrement_exposure()


func _on_exposure_plus() -> void:
	_controller.increment_exposure()


func _on_ring_visibility_changed(pressed: bool) -> void:
	_ring_visible = pressed


func _load_anatomical_image() -> void:
	var texture := _controller.get_anatomical_texture()
	if texture and _anatomical_rect:
		_anatomical_rect.texture = texture
		_update_anatomical_ui()


func _on_capture_pressed() -> void:
	_controller.capture_anatomical()
