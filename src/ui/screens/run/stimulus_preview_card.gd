## StimulusPreviewCard - Stimulus preview and metrics display
##
## Shows stimulus preview with monitor aspect ratio and stimulus state metrics.
class_name StimulusPreviewCard
extends MarginContainer


# UI references
var _card: Control = null
var _preview: TextureRect = null
var _placeholder: ColorRect = null
var _well_container: SubViewportContainer = null
var _well_viewport: SubViewport = null

# UI references - Stimulus metrics
var _condition_row: InfoRow = null
var _sweep_row: InfoRow = null
var _state_row: InfoRow = null
var _progress_row: InfoRow = null

# Aspect ratio for layout
var _aspect_ratio: float = 16.0 / 9.0


func _ready() -> void:
	_build_ui()


func _build_ui() -> void:
	size_flags_horizontal = Control.SIZE_EXPAND_FILL
	size_flags_vertical = Control.SIZE_EXPAND_FILL

	_card = Card.new()
	_card.title = "Stimulus"
	_card.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_card.size_flags_vertical = Control.SIZE_EXPAND_FILL
	add_child(_card)

	# Vertical layout: preview on top, metrics below
	var vbox := VBoxContainer.new()
	vbox.name = "StimulusLayout"
	vbox.theme_type_variation = "VBoxMD"
	_card.get_content_slot().add_child(vbox)

	# Preview container (takes remaining space)
	var preview_container := Control.new()
	preview_container.name = "PreviewContainer"
	preview_container.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	preview_container.size_flags_vertical = Control.SIZE_EXPAND_FILL
	vbox.add_child(preview_container)

	# Stimulus well (monitor aspect ratio)
	_create_rounded_well()
	preview_container.add_child(_well_container)

	# Stimulus preview rect
	_preview = TextureRect.new()
	_preview.name = "StimulusPreview"
	_preview.set_anchors_preset(Control.PRESET_FULL_RECT)
	_preview.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
	_preview.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_CENTERED
	_well_viewport.add_child(_preview)

	# Stimulus placeholder
	_placeholder = ColorRect.new()
	_placeholder.name = "StimulusPlaceholder"
	_placeholder.set_anchors_preset(Control.PRESET_FULL_RECT)
	_placeholder.color = AppTheme.WELL
	_well_viewport.add_child(_placeholder)

	var placeholder_label := Label.new()
	placeholder_label.text = "Stimulus Preview"
	placeholder_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	placeholder_label.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	placeholder_label.set_anchors_preset(Control.PRESET_CENTER)
	placeholder_label.theme_type_variation = "LabelCaption"
	_placeholder.add_child(placeholder_label)

	# Layout: Maintain monitor aspect ratio, centered
	preview_container.resized.connect(_on_preview_container_resized.bind(preview_container))

	# Stimulus state metrics below preview
	var metrics_grid := GridContainer.new()
	metrics_grid.columns = 2
	metrics_grid.theme_type_variation = "GridMD"
	vbox.add_child(metrics_grid)

	_condition_row = InfoRow.new()
	_condition_row.label_text = "Condition"
	_condition_row.value_text = "—"
	_condition_row.mono_value = true
	metrics_grid.add_child(_condition_row)

	_sweep_row = InfoRow.new()
	_sweep_row.label_text = "Sweep"
	_sweep_row.value_text = "0 / 0"
	_sweep_row.mono_value = true
	metrics_grid.add_child(_sweep_row)

	_state_row = InfoRow.new()
	_state_row.label_text = "State"
	_state_row.value_text = "Idle"
	_state_row.mono_value = true
	metrics_grid.add_child(_state_row)

	_progress_row = InfoRow.new()
	_progress_row.label_text = "Progress"
	_progress_row.value_text = "0%"
	_progress_row.mono_value = true
	metrics_grid.add_child(_progress_row)


func _create_rounded_well() -> void:
	_well_container = SubViewportContainer.new()
	_well_container.name = "StimulusWell"
	_well_container.stretch = true

	var mat := AppTheme.create_rounded_mask_material()
	_well_container.material = mat

	_well_container.resized.connect(func():
		mat.set_shader_parameter("rect_size", _well_container.size)
	)

	_well_viewport = SubViewport.new()
	_well_viewport.name = "Viewport"
	_well_viewport.transparent_bg = true
	_well_viewport.handle_input_locally = false
	_well_viewport.gui_disable_input = true
	_well_container.add_child(_well_viewport)


func _on_preview_container_resized(preview_container: Control) -> void:
	var w := preview_container.size.x
	var h := preview_container.size.y

	# Calculate size maintaining aspect ratio
	var stimulus_width := w
	var stimulus_height := w / _aspect_ratio

	# If height exceeds available, constrain by height
	if stimulus_height > h:
		stimulus_height = h
		stimulus_width = h * _aspect_ratio

	# Center the preview
	var x_offset := (w - stimulus_width) / 2.0
	var y_offset := (h - stimulus_height) / 2.0

	_well_container.position = Vector2(x_offset, y_offset)
	_well_container.size = Vector2(stimulus_width, stimulus_height)
	_well_viewport.size = Vector2i(int(stimulus_width), int(stimulus_height))


# -----------------------------------------------------------------------------
# Public API
# -----------------------------------------------------------------------------

## Set the aspect ratio for the stimulus preview
func set_aspect_ratio(ratio: float) -> void:
	_aspect_ratio = ratio


## Update preview texture
func update_preview(texture: Texture2D) -> void:
	if _preview:
		_preview.texture = texture

	show_placeholder(false)


## Show or hide the placeholder
func show_placeholder(should_show: bool) -> void:
	if _placeholder:
		_placeholder.visible = should_show


## Update stimulus metrics
func update_metrics(condition: String, sweep_current: int, sweep_total: int, state: String, progress: float) -> void:
	if _condition_row:
		var condition_display := DirectionSystem.get_display_name(condition) if condition else "—"
		_condition_row.set_value(condition_display)

	if _sweep_row:
		_sweep_row.set_value("%d / %d" % [sweep_current, sweep_total])

	if _state_row:
		_state_row.set_value(state.capitalize() if state else "—")

	if _progress_row:
		_progress_row.set_value("%.0f%%" % (progress * 100.0))


## Reset to initial state
func reset() -> void:
	show_placeholder(true)
	update_metrics("", 0, 0, "Idle", 0.0)
