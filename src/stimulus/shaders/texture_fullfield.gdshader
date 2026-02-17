shader_type canvas_item;
// Full-field texture stimulus (NONE envelope).
//
// Generates checkerboard or solid pattern across the entire visual field.
// Used for baseline stimuli and full-field flicker.
//
// Supports spherical coordinate correction (Marshel et al. 2012) to maintain
// constant spatial frequency across the visual field on a flat monitor.

#include "res://src/stimulus/shaders/includes/common.gdshaderinc"
#include "res://src/stimulus/shaders/includes/projection.gdshaderinc"
#include "res://src/stimulus/shaders/includes/carriers.gdshaderinc"
#include "res://src/stimulus/shaders/includes/modulation.gdshaderinc"

void fragment() {
	// Get strobe polarity for contrast reversal
	float polarity = get_strobe_polarity();

	float lum;

	if (carrier_type == 0) {
		// Solid pattern
		lum = solid_carrier(polarity);
	} else {
		// Checkerboard pattern
		vec2 pos = uv_to_pattern_position(UV);
		lum = checkerboard_cartesian(pos, polarity);
	}

	// Clamp and output
	lum = clamp(lum, 0.0, 1.0);
	COLOR = vec4(vec3(lum), 1.0);
}
