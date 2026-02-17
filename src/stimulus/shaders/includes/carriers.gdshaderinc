// carriers.gdshaderinc
// Carrier pattern generation functions for texture stimuli.
//
// Carriers are the base patterns that fill the envelope aperture:
// - SOLID: Uniform luminance
// - CHECKERBOARD: 2D checkerboard pattern
// Future: GRATING, NOISE

// ============================================================================
// Carrier Uniforms
// ============================================================================

uniform int carrier_type : hint_range(0, 1) = 1;  // 0=solid, 1=checkerboard

// Checkerboard parameters
uniform float check_size_deg : hint_range(0.1, 90.0) = 5.0;   // Size in degrees (spherical/cylindrical)
uniform float check_size_cm : hint_range(0.1, 100.0) = 1.0;   // Size in cm (cartesian)
uniform float contrast : hint_range(0.0, 1.0) = 1.0;          // Michelson contrast
uniform float mean_luminance : hint_range(0.0, 1.0) = 0.5;    // Mean luminance level

// Solid/luminance parameters
uniform float luminance_high : hint_range(0.0, 1.0) = 1.0;    // High luminance (bar/stimulus)
uniform float luminance_low : hint_range(0.0, 1.0) = 0.0;     // Low luminance (background)

// ============================================================================
// Helper Functions
// ============================================================================

// Get check size in appropriate units for current projection type
float get_check_size() {
	if (projection_type == 0) {
		// Cartesian: use check_size_cm for square checks on screen
		return check_size_cm;
	} else {
		// Spherical/Cylindrical: use check_size_deg for constant spatial frequency
		return check_size_deg;
	}
}

// ============================================================================
// Solid Carrier
// Returns luminance based on strobe polarity.
// ============================================================================

float solid_carrier(float polarity) {
	return (polarity >= 0.0) ? luminance_high : luminance_low;
}

// ============================================================================
// Checkerboard Carrier - Cartesian
// For BAR envelope and full-field stimulus.
// Position is in projection-appropriate units (cm or deg).
// ============================================================================

float checkerboard_cartesian(vec2 position, float polarity) {
	float check_size = get_check_size();

	// Calculate check indices
	ivec2 check_idx = ivec2(floor(position / check_size));

	// Checkerboard pattern: (row + col) % 2
	int pattern = (check_idx.x + check_idx.y) % 2;

	// Apply contrast reversal (polarity from strobe)
	if (polarity < 0.0) {
		pattern = 1 - pattern;
	}

	// Calculate luminance from contrast
	// half_contrast gives the deviation from mean_luminance
	float half_contrast = contrast * mean_luminance;
	if (pattern == 0) {
		return mean_luminance + half_contrast;
	} else {
		return mean_luminance - half_contrast;
	}
}

// ============================================================================
// Checkerboard Carrier - Polar
// For WEDGE and RING envelopes.
// Uses eccentricity and polar_angle in degrees.
// ============================================================================

float checkerboard_polar(float eccentricity, float polar_angle, float polarity) {
	// For polar coordinates, always use degrees
	float check_size = check_size_deg;

	// Calculate check indices in radial and angular directions
	float radial_check = floor(eccentricity / check_size);
	float angular_check = floor(polar_angle / check_size);

	// Checkerboard pattern
	int pattern = int(mod(radial_check + angular_check, 2.0));

	// Apply contrast reversal
	if (polarity < 0.0) {
		pattern = 1 - pattern;
	}

	// Calculate luminance
	float half_contrast = contrast * mean_luminance;
	if (pattern == 0) {
		return mean_luminance + half_contrast;
	} else {
		return mean_luminance - half_contrast;
	}
}
