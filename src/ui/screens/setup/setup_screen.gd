extends BaseScreen
## Setup screen: Configure hardware and session parameters.
##
## Coordinates MonitorCard, CameraCard, and SessionConfigCard components.
## Validates that all requirements are met before allowing navigation.

# Card components
var _monitor_card: MonitorCard = null
var _camera_card: CameraCard = null
var _session_card: SessionConfigCard = null

# Scroll layout
var _scroll_container: SmoothScrollContainer = null
var _scroll_margin: MarginContainer = null


func _ready() -> void:
	super._ready()
	# Adjust right margin after layout is complete to account for actual scrollbar width
	call_deferred("_adjust_scroll_margin")


func _load_state() -> void:
	_validate()


func _adjust_scroll_margin() -> void:
	if not _scroll_container or not _scroll_margin:
		return

	# Custom scrollbar floats over content, so use full padding on both sides
	# Built-in scrollbar is part of layout, so reduce right margin to account for it
	if _scroll_container.has_custom_scrollbar():
		_scroll_margin.theme_type_variation = "MarginScreenContent"
	else:
		_scroll_margin.theme_type_variation = "MarginScreenContentWithScrollbar"


func _build_ui() -> void:
	# Scroll container spans full width - scrollbar at screen edge
	var scroll := SmoothScrollContainer.new()
	scroll.name = "ScrollContainer"
	scroll.set_anchors_preset(Control.PRESET_FULL_RECT)
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	scroll.scrollbar_vertical_inset = AppTheme.SCROLL_FADE_HEIGHT
	add_child(scroll)

	# Inner margin for content - variation set in _adjust_scroll_margin based on scrollbar type
	var inner_margin := MarginContainer.new()
	inner_margin.name = "ScrollContentMargin"
	inner_margin.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	inner_margin.theme_type_variation = "MarginScreenContentWithScrollbar"
	scroll.add_child(inner_margin)

	# Store reference for margin adjustment
	_scroll_margin = inner_margin
	_scroll_container = scroll

	# Main VBox for all sections
	var vbox := VBoxContainer.new()
	vbox.name = "MainContent"
	vbox.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	vbox.theme_type_variation = "VBox2XL"
	inner_margin.add_child(vbox)

	# Session config card
	_session_card = SessionConfigCard.new()
	vbox.add_child(_session_card)

	# Hardware cards container
	var cards := HBoxContainer.new()
	cards.theme_type_variation = "HBox2XL"
	vbox.add_child(cards)

	# Monitor card
	_monitor_card = MonitorCard.new()
	cards.add_child(_monitor_card)

	# Camera card
	_camera_card = CameraCard.new()
	cards.add_child(_camera_card)


func _connect_signals() -> void:
	# Monitor card signals
	if _monitor_card:
		_monitor_card.validation_changed.connect(_on_monitor_validation_changed)

	# Camera card signals
	if _camera_card:
		_camera_card.connection_changed.connect(_on_camera_connection_changed)

	# Session card signals
	if _session_card:
		_session_card.config_changed.connect(_on_session_config_changed)


func _validate() -> void:
	var errors: Array[String] = []

	# Check display validation
	if _monitor_card:
		if _monitor_card.is_validating():
			errors.append("Validating display refresh rate...")
		elif not _monitor_card.is_validated():
			errors.append("Validate display refresh rate")

	# Check camera connection
	if _camera_card:
		if not _camera_card.is_camera_connected():
			errors.append("Connect to camera daemon")

	# Check session name not empty
	if _session_card and not _session_card.is_valid():
		errors.append("Enter a session name")

	_set_valid(errors.is_empty())


# -----------------------------------------------------------------------------
# Signal Handlers
# -----------------------------------------------------------------------------

func _on_monitor_validation_changed(_valid: bool) -> void:
	_validate()


func _on_camera_connection_changed(_connected: bool) -> void:
	_validate()


func _on_session_config_changed(_directory: String, _name: String) -> void:
	_validate()
