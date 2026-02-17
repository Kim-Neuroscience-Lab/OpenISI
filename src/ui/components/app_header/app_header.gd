class_name AppHeader
extends PanelContainer
## Application header with centered navigation bar and status badge.
##
## Features the Sleep Punk Night design system with the horizontal
## navigation bar showing available screens.

## Emitted when a screen tab is clicked (for navigation).
signal screen_clicked(screen: Session.Screen)

var _hbox: HBoxContainer = null
var _logo_label: Label = null
var _left_spacer: Control = null
var _nav_slot: CenterContainer = null
var _right_spacer: Control = null
var _status_pill: StatusPill = null
var _navigation_bar: NavigationBar = null


func _ready() -> void:
	_build_ui()

	# Create and add navigation bar
	_navigation_bar = NavigationBar.new()
	_nav_slot.add_child(_navigation_bar)

	# Connect navigation bar clicks
	_navigation_bar.screen_clicked.connect(_on_nav_screen_clicked)

	# Ensure header appears above faded scroll content
	z_index = AppTheme.Z_INDEX_CHROME

	_apply_style()


func _build_ui() -> void:
	_hbox = HBoxContainer.new()
	_hbox.name = "HBoxContainer"
	add_child(_hbox)

	# Logo text (left side)
	_logo_label = Label.new()
	_logo_label.name = "LogoLabel"
	_logo_label.text = "OpenISI v" + Version.CURRENT
	_logo_label.size_flags_vertical = Control.SIZE_SHRINK_BEGIN
	_logo_label.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	_hbox.add_child(_logo_label)

	# Left spacer
	_left_spacer = Control.new()
	_left_spacer.name = "LeftSpacer"
	_left_spacer.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_hbox.add_child(_left_spacer)

	# Navigation slot (center)
	_nav_slot = CenterContainer.new()
	_nav_slot.name = "NavigationSlot"
	_hbox.add_child(_nav_slot)

	# Right spacer
	_right_spacer = Control.new()
	_right_spacer.name = "RightSpacer"
	_right_spacer.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_hbox.add_child(_right_spacer)

	# Status pill (right side)
	_status_pill = StatusPill.new()
	_status_pill.name = "StatusPill"
	_status_pill.size_flags_vertical = Control.SIZE_SHRINK_BEGIN
	_status_pill.status = "info"
	_status_pill.text = "Ready"
	_hbox.add_child(_status_pill)


func _apply_style() -> void:
	# Apply SSoT size constant
	custom_minimum_size.y = AppTheme.HEADER_HEIGHT

	# Transparent header - use theme variation
	theme_type_variation = "PanelHeaderFooter"

	# Style logo text (bold via theme variation)
	_logo_label.theme_type_variation = "LabelLogo"

	# Apply SSoT size constants to status pill
	if _status_pill:
		_status_pill.custom_minimum_size = Vector2(AppTheme.STATUS_PILL_MIN_WIDTH, AppTheme.STATUS_PILL_HEIGHT)


func _on_nav_screen_clicked(screen: Session.Screen) -> void:
	screen_clicked.emit(screen)


## Sets the status displayed in the header badge.
func set_status(status: String, text: String = "") -> void:
	if _status_pill:
		_status_pill.set_status(status, text)


## Sets whether the status pill dot should pulse.
func set_status_pulsing(pulsing: bool) -> void:
	if _status_pill:
		_status_pill.pulsing = pulsing
