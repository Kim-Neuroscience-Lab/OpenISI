shader_type canvas_item;

#include "includes/sdf.gdshaderinc"
#include "includes/common.gdshaderinc"
#include "includes/gradients.gdshaderinc"

// Surface gradient shader for simple raised surfaces
// Creates subtle vertical gradient suggesting convex curvature

uniform vec4 base_color : source_color = vec4(0.11, 0.094, 0.141, 1.0);
uniform float gradient_intensity : hint_range(0.0, 0.2) = 0.05;
uniform float rim_intensity : hint_range(0.0, 0.3) = 0.08;
uniform float rim_width : hint_range(0.0, 0.1) = 0.02;
uniform float corner_radius : hint_range(0.0, 64.0) = 16.0;

void fragment() {
	// Get size from texture pixel size
	vec2 size = 1.0 / TEXTURE_PIXEL_SIZE;
	vec2 half_size = size * 0.5;
	vec2 pixel_pos = UV * size;
	vec2 center_pos = pixel_pos - half_size;

	// Calculate distance from rounded rectangle edge
	float dist = rounded_rect_sdf(center_pos, half_size, corner_radius);

	// Discard pixels outside the rounded rectangle
	if (dist > 0.0) {
		discard;
	}

	// Anti-aliased edge
	float alpha = aa_edge(dist, 1.0);

	// Apply raised gradient
	vec3 color = raised_gradient(base_color.rgb, UV.y, gradient_intensity);

	// Rim highlight at top edge
	float rim_y = rim_width;
	if (UV.y < rim_y && dist < -corner_radius * 0.5) {
		float rim_factor = 1.0 - (UV.y / rim_y);
		color += rim_factor * rim_intensity;
	}

	// Dithering to reduce gradient banding
	color += (dither(FRAGCOORD.xy) - 0.5) / 255.0;

	COLOR = vec4(color, base_color.a * alpha);
}
