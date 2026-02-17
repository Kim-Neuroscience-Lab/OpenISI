shader_type canvas_item;
// Rotating wedge texture stimulus (WEDGE envelope).
//
// Pie-wedge aperture that rotates around the fixation point to map polar angle.
// Standard stimulus for retinotopic mapping alongside drifting bar and
// expanding ring stimuli.
//
// Supports spherical coordinate correction (Marshel et al. 2012) to maintain
// constant spatial frequency across the visual field on a flat monitor.

#include "res://src/stimulus/shaders/includes/common.gdshaderinc"
#include "res://src/stimulus/shaders/includes/projection.gdshaderinc"
#include "res://src/stimulus/shaders/includes/carriers.gdshaderinc"
#include "res://src/stimulus/shaders/includes/modulation.gdshaderinc"

// ============================================================================
// WEDGE Envelope Uniforms
// ============================================================================

uniform float stimulus_width_deg : hint_range(1.0, 359.0) = 45.0;    // Angular width of wedge
uniform float progress : hint_range(0.0, 1.0) = 0.0;                  // Rotation progress (0=start, 1=end)
uniform int direction : hint_range(0, 1) = 0;                         // 0=CW, 1=CCW
uniform float rotation_deg : hint_range(0.0, 360.0) = 0.0;           // Starting angle offset
uniform float background_luminance : hint_range(0.0, 1.0) = 0.0;     // Background outside wedge
uniform bool gradual_seam = true;                                     // Fade in/out at rotation seam

void fragment() {
	// Convert UV to polar coordinates (eccentricity, polar_angle) in degrees
	vec2 polar = uv_to_polar(UV);
	float eccentricity = polar.x;
	float polar_angle = polar.y;  // -180 to +180

	// Apply rotation offset
	polar_angle = normalize_angle(polar_angle - rotation_deg);

	// Calculate wedge position based on progress and direction
	float raw_rotation = progress * 360.0;
	float wedge_center;

	if (direction == 0) {
		// Clockwise: decreasing angle
		wedge_center = -raw_rotation;
	} else {
		// Counter-clockwise: increasing angle
		wedge_center = raw_rotation;
	}
	wedge_center = normalize_angle(wedge_center);

	// Calculate angular distance from wedge center
	float angle_diff = normalize_angle(polar_angle - wedge_center);

	// Check if pixel is inside the wedge
	float half_wedge = stimulus_width_deg / 2.0;
	bool inside_wedge = false;

	if (gradual_seam) {
		// Gradual seam: wedge enters at start and exits at end of rotation
		// The wedge travels from 0 to (360 + stimulus_width)
		float total_travel = 360.0 + stimulus_width_deg;
		float leading_edge_travel = raw_rotation * (total_travel / 360.0);
		float leading_edge = leading_edge_travel;
		float trailing_edge = leading_edge - stimulus_width_deg;

		// Calculate pixel's angular distance from seam (0 degrees = right side)
		float angle_from_seam;
		if (direction == 0) {
			// Clockwise: measure clockwise from seam
			angle_from_seam = -polar_angle;
		} else {
			// Counter-clockwise: measure counter-clockwise from seam
			angle_from_seam = polar_angle;
		}
		angle_from_seam = normalize_angle_positive(angle_from_seam);

		// Pixel is inside if between trailing and leading edges
		inside_wedge = (angle_from_seam >= trailing_edge && angle_from_seam < leading_edge);
	} else {
		// Standard mode: hard cut at seam
		inside_wedge = abs(angle_diff) < half_wedge;
	}

	float lum;

	if (!inside_wedge) {
		// Background
		lum = background_luminance;
	} else {
		// Inside wedge - render carrier pattern
		float polarity = get_strobe_polarity();

		if (carrier_type == 0) {
			// Solid
			lum = solid_carrier(polarity);
		} else {
			// Checkerboard (polar coordinates)
			lum = checkerboard_polar(eccentricity, polar_angle, polarity);
		}
	}

	// Clamp and output
	lum = clamp(lum, 0.0, 1.0);
	COLOR = vec4(vec3(lum), 1.0);
}
