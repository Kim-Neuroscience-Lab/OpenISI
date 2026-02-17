class_name NavigationBar
extends BaseCard
## Horizontal navigation bar showing primary application screens.
##
## Displays tabs for Setup, Focus, Stimulus, Acquire, Results with the
## current screen highlighted. Uses the Sleep Punk Night design system.
##
## Screen names are sourced from Session (SSoT).

## Emitted when a screen tab is clicked (for navigation).
signal screen_clicked(screen: Session.Screen)

# Screen names in order - sourced from Session SSoT
var _screen_names: Array[String] = []

# References to screen tabs and their background panels
var _screen_tabs: Array[Button] = []
var _tab_backgrounds: Array[Panel] = []
var _current_screen_idx: int = 0


func _ready() -> void:
	# Configure shader for nav container
	_base_color = AppTheme.SURFACE_COLOR_NAV
	_corner_radius = float(AppTheme.RADIUS_NAV)
	_draw_gradient = true
	_draw_rim_highlight = true
	theme_type_variation = "PanelNavContainer"
	super._ready()

	# Adjust gradient intensity for nav container
	if _shader_material:
		_shader_material.set_shader_parameter("gradient_intensity", AppTheme.RAISED_GRADIENT_INTENSITY)

	# Get screen names from Session SSoT
	_screen_names = Session.get_primary_screen_names()

	_build_ui()
	_apply_style()

	# Connect to Session screen changes
	Session.screen_changed.connect(_on_screen_changed)
	_update_screen_display(_screen_index_from_session())


func _screen_index_from_session() -> int:
	# Find index of current screen in PRIMARY_SCREENS
	return Session.PRIMARY_SCREENS.find(Session.current_screen)


func _build_ui() -> void:
	# Create inner HBox for the tabs
	var hbox := HBoxContainer.new()
	hbox.name = "TabContainer"
	hbox.theme_type_variation = "HBoxXS"
	hbox.alignment = BoxContainer.ALIGNMENT_CENTER
	add_child(hbox)

	# Create a tab for each primary screen
	for i in range(_screen_names.size()):
		# Container to hold background and button
		var container := Control.new()
		container.name = "TabWrapper_" + _screen_names[i]
		container.custom_minimum_size = Vector2(AppTheme.NAV_PILL_MIN_WIDTH, AppTheme.NAV_PILL_HEIGHT)
		hbox.add_child(container)

		# Background panel for shader effect (behind button)
		var bg := Panel.new()
		bg.name = "Background"
		bg.set_anchors_preset(Control.PRESET_FULL_RECT)
		bg.mouse_filter = Control.MOUSE_FILTER_IGNORE
		container.add_child(bg)
		_tab_backgrounds.append(bg)

		# Button on top for text and click handling
		var tab := Button.new()
		tab.name = "Screen_" + _screen_names[i]
		tab.text = _screen_names[i]
		tab.flat = true  # Transparent so background shows through
		tab.set_anchors_preset(Control.PRESET_FULL_RECT)
		tab.pressed.connect(_on_tab_pressed.bind(i))
		container.add_child(tab)
		_screen_tabs.append(tab)


func _apply_style() -> void:
	# Container styling handled by theme_type_variation set in _ready()
	_update_tab_styles()


func _update_tab_styles() -> void:
	var theme_node := AppTheme

	for i in range(_screen_tabs.size()):
		var tab := _screen_tabs[i]
		var bg := _tab_backgrounds[i]
		var is_active := i == _current_screen_idx

		if is_active:
			# Use theme variation for padding - shader handles all visuals
			bg.theme_type_variation = "PanelPillBG"
			bg.visible = true
			# Apply raised surface shader for gradient and rim highlights
			var mat := AppTheme.create_raised_surface_material(
				theme_node.SURFACE_ELEVATED,
				float(theme_node.RADIUS_XL),
				Color.TRANSPARENT,
				theme_node.RIM_LIGHT_STRONG_ALPHA
			)
			mat.set_shader_parameter("rect_size", bg.size)
			bg.material = mat
			# Update size when background resizes
			if not bg.resized.is_connected(_on_bg_resized):
				bg.resized.connect(_on_bg_resized.bind(bg))
			# Use theme variation for active tab button styling
			tab.flat = true
			tab.theme_type_variation = "ButtonNavActive"
		else:
			# Hide background for inactive tabs
			bg.visible = false
			bg.material = null
			# Use theme variation for inactive tab button styling
			tab.flat = false
			tab.theme_type_variation = "ButtonNavInactive"


func _on_bg_resized(bg: Panel) -> void:
	if bg.material is ShaderMaterial:
		bg.material.set_shader_parameter("rect_size", bg.size)


func _on_screen_changed(new_screen: Session.Screen) -> void:
	var idx := Session.PRIMARY_SCREENS.find(new_screen)
	if idx >= 0:
		_update_screen_display(idx)


func _update_screen_display(screen_idx: int) -> void:
	_current_screen_idx = screen_idx
	_update_tab_styles()


func _on_tab_pressed(screen_idx: int) -> void:
	if screen_idx >= 0 and screen_idx < Session.PRIMARY_SCREENS.size():
		var screen: Session.Screen = Session.PRIMARY_SCREENS[screen_idx]
		screen_clicked.emit(screen)


## Set the current screen by index (0-4 for primary screens).
func set_screen(screen_idx: int) -> void:
	_update_screen_display(screen_idx)
