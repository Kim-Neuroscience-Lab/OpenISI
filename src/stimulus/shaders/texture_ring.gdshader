shader_type canvas_item;
// Expanding/contracting ring texture stimulus (RING envelope).
//
// Annular aperture that expands outward or contracts inward from fixation.
// Used for eccentricity mapping in retinotopic experiments.
//
// Supports spherical coordinate correction (Marshel et al. 2012) to maintain
// constant spatial frequency across the visual field on a flat monitor.

#include "res://src/stimulus/shaders/includes/common.gdshaderinc"
#include "res://src/stimulus/shaders/includes/projection.gdshaderinc"
#include "res://src/stimulus/shaders/includes/carriers.gdshaderinc"
#include "res://src/stimulus/shaders/includes/modulation.gdshaderinc"

// ============================================================================
// RING Envelope Uniforms
// ============================================================================

uniform float stimulus_width_deg : hint_range(1.0, 180.0) = 15.0;     // Ring width (thickness)
uniform float max_eccentricity_deg : hint_range(5.0, 180.0) = 60.0;  // Maximum radial extent
uniform float progress : hint_range(0.0, 1.0) = 0.0;                  // Sweep position (0=start, 1=end)
uniform int direction : hint_range(0, 1) = 0;                         // 0=expand, 1=contract
uniform float rotation_deg : hint_range(0.0, 360.0) = 0.0;           // Checkerboard rotation
uniform float background_luminance : hint_range(0.0, 1.0) = 0.0;     // Background outside ring

void fragment() {
	// Convert UV to polar coordinates (eccentricity, polar_angle) in degrees
	vec2 polar = uv_to_polar(UV);
	float eccentricity = polar.x;
	float polar_angle = normalize_angle(polar.y - rotation_deg);

	// Calculate ring position based on progress and direction
	// Ring travels from -half_width to max_eccentricity + half_width
	float half_width = stimulus_width_deg / 2.0;
	float total_travel = max_eccentricity_deg + stimulus_width_deg;
	float ring_center;

	if (direction == 0) {
		// Expanding: starts at center, moves outward
		ring_center = -half_width + progress * total_travel;
	} else {
		// Contracting: starts at edge, moves inward
		ring_center = max_eccentricity_deg + half_width - progress * total_travel;
	}

	// Check if pixel is inside ring
	float inner_edge = ring_center - half_width;
	float outer_edge = ring_center + half_width;
	bool inside_ring = (eccentricity >= max(0.0, inner_edge) && eccentricity < outer_edge);

	float lum;

	if (!inside_ring) {
		// Background
		lum = background_luminance;
	} else {
		// Inside ring - render carrier pattern
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
