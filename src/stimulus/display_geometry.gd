class_name DisplayGeometry
extends RefCounted
## Display geometry and projection transformation.
##
## Handles mapping between visual angles (degrees) and screen coordinates
## for different display configurations: cartesian (flat screens), spherical,
## and cylindrical displays.

enum ProjectionType {
	CARTESIAN,   ## Cartesian coordinates on flat screen (physical cm units)
	SPHERICAL,   ## Spherical correction for flat screen (Marshel et al. method)
	CYLINDRICAL, ## Cylindrical curved display
}
## Note on SPHERICAL: This applies spherical coordinate correction to a flat monitor,
## as described in Marshel et al. (2012). The "sphere radius" equals the viewing distance.
## This ensures constant spatial/temporal frequency across the visual field.

## Current projection type - must be set from Config
var projection_type: ProjectionType

## Distance from subject's eye to display surface (cm)
## No default - must be set from Config
var viewing_distance_cm: float

## Angular offset of display center from straight ahead (degrees)
## No defaults - must be set from Config
var center_azimuth_deg: float   ## Horizontal offset (+ = right)
var center_elevation_deg: float ## Vertical offset (+ = up)

## Physical display dimensions (cm)
## No defaults - must be set from Config
var display_width_cm: float
var display_height_cm: float

## Display resolution (pixels)
## No defaults - must be set from Config or display detection
var display_width_px: int
var display_height_px: int

## Note on sphere/cylinder radius:
## For both SPHERICAL and CYLINDRICAL projections, the radius equals viewing_distance_cm.
## This follows Marshel et al. (2012) - the flat monitor is tangent to an imaginary
## sphere/cylinder at the viewing distance. There is no separate curvature parameter.


## Computed visual field extents (degrees)
var visual_field_width_deg: float:
	get: return _compute_visual_field_width()

var visual_field_height_deg: float:
	get: return _compute_visual_field_height()


## Create from Config values (SSoT for hardware settings)
static func from_config() -> DisplayGeometry:
	assert(Session.has_selected_display(), "No display selected - configure display in Setup first")

	var geom := DisplayGeometry.new()

	# Get values from Config autoload (SSoT)
	geom.viewing_distance_cm = Settings.viewing_distance_cm
	geom.center_azimuth_deg = Settings.horizontal_offset_deg
	geom.center_elevation_deg = Settings.vertical_offset_deg
	geom.display_width_cm = Session.display_width_cm
	geom.display_height_cm = Session.display_height_cm

	# Use selected display
	var screen_idx: int = Session.display_index
	var screen_count := DisplayServer.get_screen_count()
	assert(screen_idx >= 0 and screen_idx < screen_count,
		"Selected display %d not available (have %d displays)" % [screen_idx, screen_count])
	geom.display_width_px = DisplayServer.screen_get_size(screen_idx).x
	geom.display_height_px = DisplayServer.screen_get_size(screen_idx).y

	# Parse projection type from config (no fallback - must be valid)
	var proj_type: int = Settings.projection_type
	assert(proj_type >= 0 and proj_type <= 2,
		"Invalid projection type %d (must be 0=CARTESIAN, 1=SPHERICAL, 2=CYLINDRICAL)" % proj_type)
	match proj_type:
		0: geom.projection_type = ProjectionType.CARTESIAN
		1: geom.projection_type = ProjectionType.SPHERICAL
		2: geom.projection_type = ProjectionType.CYLINDRICAL

	return geom


## Create with explicit parameters
static func create(
	proj_type: ProjectionType,
	distance_cm: float,
	azimuth_deg: float = 0.0,
	elevation_deg: float = 0.0
) -> DisplayGeometry:
	var geom := DisplayGeometry.new()
	geom.projection_type = proj_type
	geom.viewing_distance_cm = distance_cm
	geom.center_azimuth_deg = azimuth_deg
	geom.center_elevation_deg = elevation_deg
	return geom


# -----------------------------------------------------------------------------
# Visual Field Calculations
# -----------------------------------------------------------------------------

func _compute_visual_field_width() -> float:
	match projection_type:
		ProjectionType.CARTESIAN:
			# Standard flat screen: 2 * atan(half_width / distance)
			var half_width := display_width_cm / 2.0
			return 2.0 * rad_to_deg(atan(half_width / viewing_distance_cm))

		ProjectionType.SPHERICAL:
			# Spherical correction: radius = viewing distance (Marshel et al.)
			# For flat screen with spherical correction, visual field = 2 * atan(half_width / distance)
			# Same as CARTESIAN, but the coordinate transformation differs
			var half_width := display_width_cm / 2.0
			return 2.0 * rad_to_deg(atan(half_width / viewing_distance_cm))

		ProjectionType.CYLINDRICAL:
			# Cylindrical: horizontal arc, flat vertical
			# Radius = viewing distance (tangent to imaginary cylinder)
			var arc_length := display_width_cm
			var angle_rad := arc_length / viewing_distance_cm
			return rad_to_deg(angle_rad)

	assert(false, "Unknown projection type: %d" % projection_type)
	return 0.0  # Unreachable


func _compute_visual_field_height() -> float:
	match projection_type:
		ProjectionType.CARTESIAN:
			var half_height := display_height_cm / 2.0
			return 2.0 * rad_to_deg(atan(half_height / viewing_distance_cm))

		ProjectionType.SPHERICAL:
			# Spherical correction: same visual field as flat, transformation differs
			var half_height := display_height_cm / 2.0
			return 2.0 * rad_to_deg(atan(half_height / viewing_distance_cm))

		ProjectionType.CYLINDRICAL:
			# Cylindrical: flat in vertical direction
			var half_height := display_height_cm / 2.0
			return 2.0 * rad_to_deg(atan(half_height / viewing_distance_cm))

	assert(false, "Unknown projection type: %d" % projection_type)
	return 0.0  # Unreachable


# -----------------------------------------------------------------------------
# Coordinate Transformations
# -----------------------------------------------------------------------------

## Convert visual angle (degrees from center) to normalized screen UV (0-1)
## Returns Vector2(u, v) where (0,0) is top-left, (1,1) is bottom-right
func angle_to_uv(azimuth_deg: float, elevation_deg: float) -> Vector2:
	# Apply center offset
	var local_az := azimuth_deg - center_azimuth_deg
	var local_el := elevation_deg - center_elevation_deg

	match projection_type:
		ProjectionType.CARTESIAN:
			return _flat_angle_to_uv(local_az, local_el)
		ProjectionType.SPHERICAL:
			return _spherical_angle_to_uv(local_az, local_el)
		ProjectionType.CYLINDRICAL:
			return _cylindrical_angle_to_uv(local_az, local_el)

	return Vector2(0.5, 0.5)


## Convert normalized screen UV to visual angle (degrees from center)
## Returns Vector2(azimuth, elevation) in degrees
func uv_to_angle(uv: Vector2) -> Vector2:
	var local_angle: Vector2

	match projection_type:
		ProjectionType.CARTESIAN:
			local_angle = _flat_uv_to_angle(uv)
		ProjectionType.SPHERICAL:
			local_angle = _spherical_uv_to_angle(uv)
		ProjectionType.CYLINDRICAL:
			local_angle = _cylindrical_uv_to_angle(uv)
		_:
			local_angle = Vector2.ZERO

	# Apply center offset
	return Vector2(
		local_angle.x + center_azimuth_deg,
		local_angle.y + center_elevation_deg
	)


## Convert visual angle to pixel coordinates
func angle_to_px(azimuth_deg: float, elevation_deg: float) -> Vector2:
	var uv := angle_to_uv(azimuth_deg, elevation_deg)
	return Vector2(uv.x * display_width_px, uv.y * display_height_px)


## Convert pixel coordinates to visual angle
func px_to_angle(px: Vector2) -> Vector2:
	var uv := Vector2(px.x / display_width_px, px.y / display_height_px)
	return uv_to_angle(uv)


## Convert degrees to pixels (simple linear, for compatibility)
func deg_to_px(deg: float, horizontal: bool = true) -> float:
	if horizontal:
		return (deg / visual_field_width_deg) * display_width_px
	else:
		return (deg / visual_field_height_deg) * display_height_px


## Convert pixels to degrees (simple linear, for compatibility)
func px_to_deg(px: float, horizontal: bool = true) -> float:
	if horizontal:
		return (px / display_width_px) * visual_field_width_deg
	else:
		return (px / display_height_px) * visual_field_height_deg


# -----------------------------------------------------------------------------
# Flat Projection
# -----------------------------------------------------------------------------

func _flat_angle_to_uv(az_deg: float, el_deg: float) -> Vector2:
	# Convert angle to position on flat screen
	var x_cm := viewing_distance_cm * tan(deg_to_rad(az_deg))
	var y_cm := viewing_distance_cm * tan(deg_to_rad(el_deg))

	# Normalize to UV (center is 0.5, 0.5)
	var u := 0.5 + (x_cm / display_width_cm)
	var v := 0.5 - (y_cm / display_height_cm)  # Y inverted for screen coords

	return Vector2(u, v)


func _flat_uv_to_angle(uv: Vector2) -> Vector2:
	# Convert UV to position in cm from center
	var x_cm := (uv.x - 0.5) * display_width_cm
	var y_cm := (0.5 - uv.y) * display_height_cm

	# Convert to angle
	var az_deg := rad_to_deg(atan(x_cm / viewing_distance_cm))
	var el_deg := rad_to_deg(atan(y_cm / viewing_distance_cm))

	return Vector2(az_deg, el_deg)


# -----------------------------------------------------------------------------
# Spherical Projection
# -----------------------------------------------------------------------------

func _spherical_angle_to_uv(az_deg: float, el_deg: float) -> Vector2:
	# Spherical correction for flat screen (Marshel et al. 2012)
	# Given spherical angles, find the (y, z) position on a flat screen at distance xo
	# that would stimulate that point on an imaginary sphere of radius = xo
	#
	# The transformation maps spherical coordinates to flat screen coordinates
	# such that spatial/temporal frequencies remain constant across the visual field.
	#
	# For altitude-corrected stimuli (horizontal gratings/bars):
	#   y = xo * tan(azimuth)
	#   z = sqrt(xo² + y²) * tan(elevation)
	#
	# This preserves iso-altitude lines as curved bands on the flat screen.

	var xo := viewing_distance_cm
	var az_rad := deg_to_rad(az_deg)
	var el_rad := deg_to_rad(el_deg)

	# Horizontal position from azimuth
	var y_cm := xo * tan(az_rad)

	# Vertical position from elevation (depends on horizontal eccentricity)
	var r_horizontal := sqrt(xo * xo + y_cm * y_cm)
	var z_cm := r_horizontal * tan(el_rad)

	# Normalize to UV (center is 0.5, 0.5)
	var u := 0.5 + (y_cm / display_width_cm)
	var v := 0.5 - (z_cm / display_height_cm)  # Y inverted for screen coords

	return Vector2(u, v)


func _spherical_uv_to_angle(uv: Vector2) -> Vector2:
	# Inverse of spherical correction (Marshel et al. 2012)
	# Given (y, z) position on flat screen, find the spherical angles
	#
	# From the paper:
	#   azimuth = atan(-y / xo)  [note: sign convention may vary]
	#   altitude = π/2 - acos(z / sqrt(xo² + y² + z²))
	#            = asin(z / sqrt(xo² + y² + z²))

	var xo := viewing_distance_cm

	# Convert UV to position in cm from center
	var y_cm := (uv.x - 0.5) * display_width_cm
	var z_cm := (0.5 - uv.y) * display_height_cm

	# Azimuth from horizontal position
	var az_rad := atan(y_cm / xo)

	# Elevation using full 3D distance
	var r := sqrt(xo * xo + y_cm * y_cm + z_cm * z_cm)
	var el_rad := asin(z_cm / r) if r > 0 else 0.0

	return Vector2(rad_to_deg(az_rad), rad_to_deg(el_rad))


# -----------------------------------------------------------------------------
# Cylindrical Projection
# -----------------------------------------------------------------------------

func _cylindrical_angle_to_uv(az_deg: float, el_deg: float) -> Vector2:
	# Horizontal: curved (radius = viewing distance)
	var az_rad := deg_to_rad(az_deg)
	var x_arc := viewing_distance_cm * az_rad
	var u := 0.5 + (x_arc / display_width_cm)

	# Vertical: flat (like flat screen)
	var y_cm := viewing_distance_cm * tan(deg_to_rad(el_deg))
	var v := 0.5 - (y_cm / display_height_cm)

	return Vector2(u, v)


func _cylindrical_uv_to_angle(uv: Vector2) -> Vector2:
	# Horizontal: curved (radius = viewing distance)
	var x_arc := (uv.x - 0.5) * display_width_cm
	var az_deg := rad_to_deg(x_arc / viewing_distance_cm)

	# Vertical: flat
	var y_cm := (0.5 - uv.y) * display_height_cm
	var el_deg := rad_to_deg(atan(y_cm / viewing_distance_cm))

	return Vector2(az_deg, el_deg)


# -----------------------------------------------------------------------------
# Maximum Eccentricity (Corner Distance)
# -----------------------------------------------------------------------------

## Compute the maximum eccentricity from the center offset to the furthest corner.
## This is used by the ring stimulus to ensure it fully enters/exits the screen.
## The ring's inner edge must travel past this distance to be fully off-screen.
func get_max_eccentricity_deg() -> float:
	# The four corners of the display in UV coordinates
	var corners := [
		Vector2(0.0, 0.0),  # Top-left
		Vector2(1.0, 0.0),  # Top-right
		Vector2(0.0, 1.0),  # Bottom-left
		Vector2(1.0, 1.0),  # Bottom-right
	]

	var max_ecc := 0.0

	for corner_uv in corners:
		var ecc := _compute_eccentricity_at_uv(corner_uv)
		if ecc > max_ecc:
			max_ecc = ecc

	return max_ecc


## Compute eccentricity (angle from viewing axis through center offset) at a UV position.
## For ring/wedge stimuli, eccentricity is the angular distance from the center offset point.
func _compute_eccentricity_at_uv(uv: Vector2) -> float:
	# Convert UV to position in cm from screen center
	var y_cm := (uv.x - 0.5) * display_width_cm   # horizontal
	var z_cm := (0.5 - uv.y) * display_height_cm  # vertical (flipped for screen coords)

	# Apply center offset in cm
	var xo := viewing_distance_cm
	var offset_y_cm := tan(deg_to_rad(center_azimuth_deg)) * xo
	var offset_z_cm := tan(deg_to_rad(center_elevation_deg)) * xo
	y_cm -= offset_y_cm
	z_cm -= offset_z_cm

	# Eccentricity is the angle from the central viewing axis (pole-centered)
	# Using atan for all projection types ensures consistency with polar angle calculation
	var radial_cm := sqrt(y_cm * y_cm + z_cm * z_cm)
	return rad_to_deg(atan2(radial_cm, xo))


# -----------------------------------------------------------------------------
# Shader Parameters
# -----------------------------------------------------------------------------

## Get parameters to pass to shader for coordinate transformation
func get_shader_params() -> Dictionary:
	# Note: Curvature radius always equals viewing_distance_cm for both spherical
	# and cylindrical projections (Marshel et al. 2012)
	return {
		"projection_type": projection_type,
		"viewing_distance_cm": viewing_distance_cm,
		"center_azimuth_deg": center_azimuth_deg,
		"center_elevation_deg": center_elevation_deg,
		"display_width_cm": display_width_cm,
		"display_height_cm": display_height_cm,
		"visual_field_deg": Vector2(visual_field_width_deg, visual_field_height_deg),
	}


## Apply geometry parameters to a shader material
func apply_to_shader(material: ShaderMaterial) -> void:
	if material == null:
		return

	material.set_shader_parameter("projection_type", projection_type)
	material.set_shader_parameter("viewing_distance_cm", viewing_distance_cm)
	material.set_shader_parameter("center_offset_deg", Vector2(center_azimuth_deg, center_elevation_deg))
	material.set_shader_parameter("display_size_cm", Vector2(display_width_cm, display_height_cm))
	material.set_shader_parameter("visual_field_deg", Vector2(visual_field_width_deg, visual_field_height_deg))
	# Note: Curvature radius = viewing_distance_cm for both spherical and cylindrical
	# projections on a flat monitor (Marshel et al. 2012). Shaders use viewing_distance_cm directly.


# -----------------------------------------------------------------------------
# Serialization
# -----------------------------------------------------------------------------

func to_dict() -> Dictionary:
	return {
		"projection_type": ProjectionType.keys()[projection_type],
		"viewing_distance_cm": viewing_distance_cm,
		"center_azimuth_deg": center_azimuth_deg,
		"center_elevation_deg": center_elevation_deg,
		"display_width_cm": display_width_cm,
		"display_height_cm": display_height_cm,
		"display_width_px": display_width_px,
		"display_height_px": display_height_px,
	}


static func from_dict(data: Dictionary) -> DisplayGeometry:
	var geom := DisplayGeometry.from_config()  # Start with config defaults

	var type_str: String = data["projection_type"]
	match type_str:
		"CARTESIAN": geom.projection_type = ProjectionType.CARTESIAN
		"SPHERICAL": geom.projection_type = ProjectionType.SPHERICAL
		"CYLINDRICAL": geom.projection_type = ProjectionType.CYLINDRICAL

	# Override with dict values
	geom.viewing_distance_cm = data["viewing_distance_cm"]
	geom.center_azimuth_deg = data["center_azimuth_deg"]
	geom.center_elevation_deg = data["center_elevation_deg"]
	geom.display_width_cm = data["display_width_cm"]
	geom.display_height_cm = data["display_height_cm"]
	geom.display_width_px = data["display_width_px"]
	geom.display_height_px = data["display_height_px"]

	return geom
