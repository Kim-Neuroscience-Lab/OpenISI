shader_type canvas_item;

#include "includes/sdf.gdshaderinc"
#include "includes/common.gdshaderinc"
#include "includes/geometry.gdshaderinc"
#include "includes/gradients.gdshaderinc"
#include "includes/rim.gdshaderinc"
#include "includes/outline.gdshaderinc"

// Raised surface shader for cards, nav bars, pills, etc.
// Creates a convex appearance with rim highlights

uniform vec4 base_color : source_color = vec4(0.141, 0.122, 0.180, 1.0);
uniform float corner_radius : hint_range(0.0, 64.0) = 20.0;

// Gradient settings - bidirectional: lightens at top, darkens at bottom
uniform float gradient_intensity : hint_range(0.0, 0.2) = 0.05;

// Rim highlight settings
uniform vec4 rim_top_color : source_color = vec4(0.941, 0.902, 0.863, 0.15);
uniform vec4 rim_bottom_color : source_color = vec4(0.0, 0.0, 0.0, 0.3);
uniform vec4 rim_bottom_highlight : source_color = vec4(0.941, 0.902, 0.863, 0.07);
uniform float rim_width : hint_range(0.5, 5.0) = 2.0;
uniform float rim_fade_pixels : hint_range(0.0, 50.0) = 15.0;

// Thin outline around entire surface
uniform vec4 outline_color : source_color = vec4(0.941, 0.902, 0.863, 0.06);
uniform float outline_width : hint_range(0.5, 2.0) = 1.0;

// Accent border for status indicators (set alpha > 0 to enable)
uniform vec4 accent_border_color : source_color = vec4(0.0, 0.0, 0.0, 0.0);
uniform float accent_border_width : hint_range(0.0, 4.0) = 1.0;

uniform vec2 rect_size = vec2(100.0, 100.0);

void fragment() {
	vec4 pos = get_positions(UV, rect_size);
	vec2 pixel_pos = pos.xy;
	vec2 center_pos = pos.zw;

	// Distance from rounded rectangle edge (negative = inside)
	float dist = rounded_rect_sdf(center_pos, rect_size * 0.5, corner_radius);

	// Discard pixels outside the rounded rectangle
	if (dist > 0.0) {
		discard;
	}

	// Anti-aliased edge
	float edge_alpha = aa_edge(dist, 1.0);

	// Start with base color and apply raised gradient
	vec3 color = raised_gradient(base_color.rgb, UV.y, gradient_intensity);

	// Apply rim highlights
	color = apply_rim_highlight(color, dist, pixel_pos, rect_size, corner_radius,
	                            rim_width, rim_fade_pixels, rim_top_color, rim_bottom_color);

	// Apply bottom edge bright highlight
	color = apply_bottom_highlight(color, dist, pixel_pos, rect_size, corner_radius,
	                               rim_width, rim_fade_pixels, rim_bottom_highlight);

	// Apply outline
	color = apply_outline(color, dist, outline_width, outline_color);

	// Apply accent border (if alpha > 0)
	if (accent_border_color.a > 0.0) {
		float border_band = smoothstep(-accent_border_width - 0.5, -accent_border_width, dist)
		                  * smoothstep(-0.5, 0.0, -dist);
		color = mix(color, accent_border_color.rgb, border_band * accent_border_color.a);
	}

	// Dithering to reduce gradient banding
	color += (dither(FRAGCOORD.xy) - 0.5) / 255.0;

	COLOR = vec4(color, base_color.a * edge_alpha);
}
