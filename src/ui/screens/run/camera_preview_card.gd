## CameraPreviewCard - Camera frame preview display
##
## Shows camera frames in a square aspect ratio with rounded corners.
class_name CameraPreviewCard
extends MarginContainer


# UI references
var _card: Control = null
var _preview: TextureRect = null
var _placeholder: ColorRect = null
var _well_container: SubViewportContainer = null
var _well_viewport: SubViewport = null

# Frame display
var _frame_texture: ImageTexture = null


func _ready() -> void:
	_build_ui()


func _build_ui() -> void:
	size_flags_vertical = Control.SIZE_EXPAND_FILL

	_card = Card.new()
	_card.title = "Camera"
	_card.size_flags_vertical = Control.SIZE_EXPAND_FILL
	add_child(_card)

	# Use Control for manual square layout
	var content := Control.new()
	content.name = "CameraContent"
	content.set_anchors_preset(Control.PRESET_FULL_RECT)
	_card.get_content_slot().add_child(content)

	# Camera well (square)
	_create_rounded_well()
	content.add_child(_well_container)

	# Camera preview rect
	_preview = TextureRect.new()
	_preview.name = "CameraPreview"
	_preview.set_anchors_preset(Control.PRESET_FULL_RECT)
	_preview.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
	_preview.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_CENTERED
	_well_viewport.add_child(_preview)

	# Camera placeholder
	_placeholder = ColorRect.new()
	_placeholder.name = "CameraPlaceholder"
	_placeholder.set_anchors_preset(Control.PRESET_FULL_RECT)
	_placeholder.color = AppTheme.WELL
	_well_viewport.add_child(_placeholder)

	var placeholder_label := Label.new()
	placeholder_label.text = "Camera Preview"
	placeholder_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	placeholder_label.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	placeholder_label.set_anchors_preset(Control.PRESET_CENTER)
	placeholder_label.theme_type_variation = "LabelCaption"
	_placeholder.add_child(placeholder_label)

	# Layout: Square preview filling vertical space
	content.resized.connect(func():
		var h := content.size.y
		var side := h  # Square

		# Center horizontally if content is wider than square
		var x_offset := maxf(0, (content.size.x - side) / 2.0)

		_well_container.position = Vector2(x_offset, 0)
		_well_container.size = Vector2(side, side)
		_well_viewport.size = Vector2i(int(side), int(side))

		# Set card width to match square content plus padding
		_card.custom_minimum_size.x = side + AppTheme.SPACING_2XL * 2
	)


func _create_rounded_well() -> void:
	_well_container = SubViewportContainer.new()
	_well_container.name = "CameraWell"
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


# -----------------------------------------------------------------------------
# Public API
# -----------------------------------------------------------------------------

## Update frame from raw pixel data
func update_frame(data: PackedByteArray, width: int, height: int) -> void:
	if data.is_empty():
		return

	var expected_size: int = width * height
	if data.size() != expected_size:
		return

	var image := Image.create_from_data(width, height, false, Image.FORMAT_L8, data)

	if _frame_texture == null:
		_frame_texture = ImageTexture.create_from_image(image)
	else:
		_frame_texture.update(image)

	if _preview:
		_preview.texture = _frame_texture

	show_placeholder(false)


## Show or hide the placeholder
func show_placeholder(should_show: bool) -> void:
	if _placeholder:
		_placeholder.visible = should_show


## Get the card for width adjustment by parent
func get_card() -> Control:
	return _card
