shader_type canvas_item;
// Drifting bar texture stimulus (BAR envelope).
//
// Rectangular aperture that sweeps across the visual field in one of
// four cardinal directions (LR, RL, TB, BT). Used for retinotopic mapping
// (Kalatsky & Stryker protocol).
//
// Supports spherical coordinate correction (Marshel et al. 2012) to maintain
// constant spatial frequency across the visual field on a flat monitor.

#include "res://src/stimulus/shaders/includes/common.gdshaderinc"
#include "res://src/stimulus/shaders/includes/projection.gdshaderinc"
#include "res://src/stimulus/shaders/includes/carriers.gdshaderinc"
#include "res://src/stimulus/shaders/includes/modulation.gdshaderinc"

// ============================================================================
// BAR Envelope Uniforms
// ============================================================================

uniform float stimulus_width_deg : hint_range(1.0, 180.0) = 20.0;  // Bar width in degrees
uniform float progress : hint_range(0.0, 1.0) = 0.0;               // Sweep position (0=start, 1=end)
uniform int direction : hint_range(0, 3) = 0;                       // 0=LR, 1=RL, 2=TB, 3=BT
uniform float rotation_deg : hint_range(0.0, 360.0) = 0.0;         // Pattern rotation
uniform float background_luminance : hint_range(0.0, 1.0) = 0.0;   // Background outside bar

void fragment() {
	// Convert UV to visual angle in degrees
	vec2 pos_deg = uv_to_equator_angle(UV);

	// Apply rotation around center point
	if (rotation_deg != 0.0) {
		pos_deg = rotate_point(pos_deg, -rotation_deg);
	}

	// Get sweep extent (visual field size in sweep direction)
	float sweep_extent = (direction == 0 || direction == 1) ? visual_field_deg.x : visual_field_deg.y;

	// Calculate bar position in degrees
	// Bar travels from -bar_width/2 to sweep_extent + bar_width/2 (enters and exits fully)
	float total_travel = sweep_extent + stimulus_width_deg;
	float bar_center;

	if (direction == 0) {
		// LR: bar moves from left to right
		bar_center = -visual_field_deg.x / 2.0 - stimulus_width_deg / 2.0 + progress * total_travel;
	} else if (direction == 1) {
		// RL: bar moves from right to left
		bar_center = visual_field_deg.x / 2.0 + stimulus_width_deg / 2.0 - progress * total_travel;
	} else if (direction == 2) {
		// TB: bar moves from top to bottom (positive to negative elevation)
		bar_center = visual_field_deg.y / 2.0 + stimulus_width_deg / 2.0 - progress * total_travel;
	} else {
		// BT: bar moves from bottom to top
		bar_center = -visual_field_deg.y / 2.0 - stimulus_width_deg / 2.0 + progress * total_travel;
	}

	// Get position along sweep axis
	float pos_along_sweep = (direction == 0 || direction == 1) ? pos_deg.x : pos_deg.y;

	// Check if pixel is inside the bar
	float half_width = stimulus_width_deg / 2.0;
	bool inside_bar = abs(pos_along_sweep - bar_center) < half_width;

	float lum;

	if (!inside_bar) {
		// Background
		lum = background_luminance;
	} else {
		// Inside bar - render carrier pattern
		float polarity = get_strobe_polarity();

		if (carrier_type == 0) {
			// Solid
			lum = solid_carrier(polarity);
		} else {
			// Checkerboard
			vec2 pattern_pos = uv_to_pattern_position(UV);
			lum = checkerboard_cartesian(pattern_pos, polarity);
		}
	}

	// Clamp and output
	lum = clamp(lum, 0.0, 1.0);
	COLOR = vec4(vec3(lum), 1.0);
}
