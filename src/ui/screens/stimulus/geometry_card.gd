class_name GeometryCard
extends MarginContainer
## GeometryCard - Display geometry configuration UI
##
## Handles display geometry configuration including viewing distance
## and center offset angles. For spherical/cylindrical projections,
## the curvature radius equals viewing_distance_cm (Marshel et al. 2012).
## Note: Projection type is now selected in CompositionCard as "Space".

signal geometry_changed(geometry: DisplayGeometry)

var _card: Control = null
var _distance_input: StyledSpinBox = null
var _azimuth_input: StyledSpinBox = null
var _elevation_input: StyledSpinBox = null


func _ready() -> void:
	_build_ui()
	_connect_signals()


func _build_ui() -> void:
	_card = Card.new()
	_card.title = "Display Geometry"
	add_child(_card)

	var content := VBoxContainer.new()
	content.theme_type_variation = "VBoxMD"
	_card.get_content_slot().add_child(content)

	# Viewing distance row
	var dist_row := HBoxContainer.new()
	dist_row.theme_type_variation = "HBoxSM"
	content.add_child(dist_row)

	var dist_label := Label.new()
	dist_label.text = "Distance"
	dist_label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_MD
	dist_row.add_child(dist_label)

	var dist_contract := Settings.lookup_param_contract("viewing_distance_cm")
	_distance_input = StyledSpinBox.new()
	_distance_input.min_value = dist_contract["min"]
	_distance_input.max_value = dist_contract["max"]
	_distance_input.step = dist_contract["step"]
	_distance_input.value = Settings.viewing_distance_cm
	_distance_input.suffix = " cm"
	_distance_input.custom_minimum_size = Vector2(AppTheme.INPUT_SPINBOX_WIDTH, AppTheme.INPUT_HEIGHT)
	dist_row.add_child(_distance_input)

	var dist_hint := Label.new()
	dist_hint.text = "eye to display"
	dist_hint.theme_type_variation = "LabelSmallDim"
	dist_row.add_child(dist_hint)

	# Divider
	var divider := Divider.new()
	divider.margin = 4
	content.add_child(divider)

	# Center offset section
	var offset_header := SectionHeader.new()
	offset_header.title = "CENTER OFFSET"
	content.add_child(offset_header)

	# Azimuth row
	var az_row := HBoxContainer.new()
	az_row.theme_type_variation = "HBoxSM"
	content.add_child(az_row)

	var az_label := Label.new()
	az_label.text = "Azimuth"
	az_label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_MD
	az_row.add_child(az_label)

	var az_contract := Settings.lookup_param_contract("horizontal_offset_deg")
	_azimuth_input = StyledSpinBox.new()
	_azimuth_input.min_value = az_contract["min"]
	_azimuth_input.max_value = az_contract["max"]
	_azimuth_input.step = az_contract["step"]
	_azimuth_input.value = Settings.horizontal_offset_deg
	_azimuth_input.suffix = " deg"
	_azimuth_input.custom_minimum_size = Vector2(AppTheme.INPUT_SPINBOX_WIDTH, AppTheme.INPUT_HEIGHT)
	az_row.add_child(_azimuth_input)

	var az_hint := Label.new()
	az_hint.text = "+ = right"
	az_hint.theme_type_variation = "LabelSmallDim"
	az_row.add_child(az_hint)

	# Elevation row
	var el_row := HBoxContainer.new()
	el_row.theme_type_variation = "HBoxSM"
	content.add_child(el_row)

	var el_label := Label.new()
	el_label.text = "Elevation"
	el_label.custom_minimum_size.x = AppTheme.LABEL_WIDTH_MD
	el_row.add_child(el_label)

	var el_contract := Settings.lookup_param_contract("vertical_offset_deg")
	_elevation_input = StyledSpinBox.new()
	_elevation_input.min_value = el_contract["min"]
	_elevation_input.max_value = el_contract["max"]
	_elevation_input.step = el_contract["step"]
	_elevation_input.value = Settings.vertical_offset_deg
	_elevation_input.suffix = " deg"
	_elevation_input.custom_minimum_size = Vector2(AppTheme.INPUT_SPINBOX_WIDTH, AppTheme.INPUT_HEIGHT)
	el_row.add_child(_elevation_input)

	var el_hint := Label.new()
	el_hint.text = "+ = up"
	el_hint.theme_type_variation = "LabelSmallDim"
	el_row.add_child(el_hint)


func _connect_signals() -> void:
	_distance_input.value_changed.connect(_on_value_changed)
	_azimuth_input.value_changed.connect(_on_value_changed)
	_elevation_input.value_changed.connect(_on_value_changed)


func _on_value_changed(_value: float) -> void:
	geometry_changed.emit(get_geometry())


## Get the current display geometry configuration
## Note: projection_type is NOT set here - it comes from CompositionCard's space selector
## and is stored in Settings.projection_type
func get_geometry() -> DisplayGeometry:
	var geom := DisplayGeometry.new()

	# Set distance and offsets
	# Note: Curvature radius = viewing_distance_cm for spherical/cylindrical (Marshel et al. 2012)
	geom.viewing_distance_cm = _distance_input.value
	geom.center_azimuth_deg = _azimuth_input.value
	geom.center_elevation_deg = _elevation_input.value

	# Get display dimensions from Config
	geom.display_width_cm = Session.display_width_cm
	geom.display_height_cm = Session.display_height_cm

	return geom


## Set display geometry from a DisplayGeometry object
## Note: projection_type in the geometry object is ignored - it comes from CompositionCard
func set_geometry(geometry: DisplayGeometry) -> void:
	if geometry == null:
		load_from_config()
		return

	# Temporarily disconnect signals
	_distance_input.value_changed.disconnect(_on_value_changed)
	_azimuth_input.value_changed.disconnect(_on_value_changed)
	_elevation_input.value_changed.disconnect(_on_value_changed)

	_distance_input.value = geometry.viewing_distance_cm
	_azimuth_input.value = geometry.center_azimuth_deg
	_elevation_input.value = geometry.center_elevation_deg

	# Reconnect signals
	_distance_input.value_changed.connect(_on_value_changed)
	_azimuth_input.value_changed.connect(_on_value_changed)
	_elevation_input.value_changed.connect(_on_value_changed)


## Load geometry from Config defaults
func load_from_config() -> void:
	# Temporarily disconnect signals
	_distance_input.value_changed.disconnect(_on_value_changed)
	_azimuth_input.value_changed.disconnect(_on_value_changed)
	_elevation_input.value_changed.disconnect(_on_value_changed)

	_distance_input.value = Settings.viewing_distance_cm
	_azimuth_input.value = Settings.horizontal_offset_deg
	_elevation_input.value = Settings.vertical_offset_deg

	# Reconnect signals
	_distance_input.value_changed.connect(_on_value_changed)
	_azimuth_input.value_changed.connect(_on_value_changed)
	_elevation_input.value_changed.connect(_on_value_changed)
