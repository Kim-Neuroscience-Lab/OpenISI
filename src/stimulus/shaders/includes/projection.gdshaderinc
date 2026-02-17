// projection.gdshaderinc
// Coordinate transformation functions for different projection types.
//
// Implements Marshel et al. (2012) spherical correction for maintaining
// constant spatial frequency across the visual field on a flat monitor.
//
// Two coordinate systems are provided:
// - Equator-centered: (azimuth, elevation) for BAR envelope
// - Pole-centered: (eccentricity, polar_angle) for WEDGE and RING envelopes

// ============================================================================
// Equator-Centered Coordinates (for BAR envelope)
// Returns (azimuth, elevation) in degrees, relative to screen center.
// ============================================================================

vec2 uv_to_equator_angle(vec2 uv) {
	// Convert UV (0-1) to position in cm from screen center
	// UV origin is top-left, y increases downward
	float y_cm = (uv.x - 0.5) * display_size_cm.x;  // horizontal
	float z_cm = (0.5 - uv.y) * display_size_cm.y;  // vertical (flipped)

	vec2 angle_deg;

	if (projection_type == 1) {
		// Spherical correction (Marshel et al. 2012)
		// The flat monitor is tangent to an imaginary sphere at viewing_distance
		float xo = viewing_distance_cm;

		// Azimuth from horizontal position
		float az_rad = atan(y_cm, xo);

		// Altitude using full 3D distance from eye to point on screen
		float r = sqrt(xo * xo + y_cm * y_cm + z_cm * z_cm);
		float el_rad = asin(z_cm / r);

		angle_deg = vec2(degrees(az_rad), degrees(el_rad));
	} else if (projection_type == 2) {
		// Cylindrical: spherical in horizontal, flat in vertical
		float xo = viewing_distance_cm;

		float az_rad = atan(y_cm, xo);
		float el_rad = atan(z_cm, xo);

		angle_deg = vec2(degrees(az_rad), degrees(el_rad));
	} else {
		// Flat (linear) - simple linear mapping
		angle_deg = (uv - 0.5) * visual_field_deg;
	}

	// Apply center offset
	angle_deg -= center_offset_deg;

	return angle_deg;
}

// ============================================================================
// Pole-Centered Coordinates (for WEDGE and RING envelopes)
// Returns (eccentricity, polar_angle) in degrees.
// - Eccentricity: angle from central viewing axis (0 at fixation)
// - Polar angle: angle around viewing axis (-180 to +180, 0 = right)
//
// IMPORTANT: polar_angle is computed from screen position (y_cm, z_cm)
// to ensure straight radial edges. Do NOT use atan(el_rad, az_rad)
// which creates curved lines.
// ============================================================================

vec2 uv_to_polar(vec2 uv) {
	// Convert UV (0-1) to position in cm from screen center
	float y_cm = (uv.x - 0.5) * display_size_cm.x;  // horizontal
	float z_cm = (0.5 - uv.y) * display_size_cm.y;  // vertical (flipped)

	// Apply center offset in cm
	float xo = viewing_distance_cm;
	float offset_y_cm = tan(radians(center_offset_deg.x)) * xo;
	float offset_z_cm = tan(radians(center_offset_deg.y)) * xo;
	y_cm -= offset_y_cm;
	z_cm -= offset_z_cm;

	// Polar angle: computed from screen position for straight radial lines
	// This is the angle in the y-z plane (screen plane)
	float polar_angle_deg = degrees(atan(z_cm, y_cm));

	// Eccentricity: angle from the central viewing axis
	// For a point (y_cm, z_cm) on the screen at distance xo,
	// eccentricity = atan(radial_distance / viewing_distance)
	float radial_cm = sqrt(y_cm * y_cm + z_cm * z_cm);
	float eccentricity_deg = degrees(atan(radial_cm, xo));

	return vec2(eccentricity_deg, polar_angle_deg);
}

// ============================================================================
// Pattern Position (for Checkerboard patterns)
// Returns position in projection-appropriate units:
// - Cartesian: physical cm (for square checks on screen)
// - Spherical/Cylindrical: visual degrees (for constant spatial frequency)
// ============================================================================

vec2 uv_to_pattern_position(vec2 uv) {
	// Convert UV to position in cm from screen center
	float y_cm = (uv.x - 0.5) * display_size_cm.x;
	float z_cm = (0.5 - uv.y) * display_size_cm.y;

	float xo = viewing_distance_cm;

	// Apply center offset in cm
	float offset_y_cm = tan(radians(center_offset_deg.x)) * xo;
	float offset_z_cm = tan(radians(center_offset_deg.y)) * xo;
	y_cm -= offset_y_cm;
	z_cm -= offset_z_cm;

	if (projection_type == 1) {
		// Spherical: return visual angles for constant spatial frequency
		float az_rad = atan(y_cm, xo);
		float r = sqrt(xo * xo + y_cm * y_cm + z_cm * z_cm);
		float el_rad = asin(z_cm / r);
		return vec2(degrees(az_rad), degrees(el_rad));
	} else if (projection_type == 2) {
		// Cylindrical: spherical horizontal, flat vertical
		float az_rad = atan(y_cm, xo);
		float el_rad = atan(z_cm, xo);
		return vec2(degrees(az_rad), degrees(el_rad));
	} else {
		// Cartesian: return physical cm for square checks on screen
		return vec2(y_cm, z_cm);
	}
}
