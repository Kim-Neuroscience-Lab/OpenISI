shader_type canvas_item;

#include "includes/sdf.gdshaderinc"
#include "includes/common.gdshaderinc"
#include "includes/geometry.gdshaderinc"
#include "includes/gradients.gdshaderinc"
#include "includes/outline.gdshaderinc"

// Inset surface shader for input fields, spinboxes, dropdowns
// Creates a recessed/concave appearance with lip highlights

uniform vec2 rect_size = vec2(200.0, 40.0);
uniform float corner_radius : hint_range(0.0, 16.0) = 8.0;

// Base colors
uniform vec4 base_color : source_color = vec4(0.059, 0.051, 0.078, 1.0);
uniform vec4 cream : source_color = vec4(0.941, 0.902, 0.863, 1.0);

// Outer dark ring
uniform float dark_ring_opacity : hint_range(0.0, 1.0) = 0.6;

// Lip settings (inset from edge - dark top, light bottom for recessed look)
uniform float lip_inset : hint_range(1.0, 6.0) = 2.0;
uniform float lip_width : hint_range(0.5, 4.0) = 1.5;
uniform float lip_top_shadow_opacity : hint_range(0.0, 0.6) = 0.5;
uniform float lip_bottom_highlight_opacity : hint_range(0.0, 0.4) = 0.08;
uniform float lip_fade_pixels : hint_range(0.0, 30.0) = 12.0;

// Gradient intensity (inverted for recessed look)
uniform float gradient_intensity : hint_range(0.0, 0.15) = 0.03;

void fragment() {
	vec4 pos = get_positions(UV, rect_size);
	vec2 pixel_pos = pos.xy;
	vec2 center_pos = pos.zw;

	// Distance from rounded rectangle edge (negative = inside)
	float dist = rounded_rect_sdf(center_pos, rect_size * 0.5, corner_radius);

	// Discard pixels outside
	if (dist > 0.0) {
		discard;
	}

	// Anti-aliased edge
	float edge_alpha = aa_edge(dist, 1.0);

	// Start with base color and apply inset gradient
	vec3 color = inset_gradient(base_color.rgb, UV.y, gradient_intensity);

	// Lip highlights (inset from edge)
	// The lip creates a thin ridge around the inner text area
	float lip_outer = -lip_inset;
	float lip_inner = lip_outer - lip_width;

	// Lip band: visible when dist is between lip_inner and lip_outer
	float lip_band_inner = smoothstep(lip_inner - 0.5, lip_inner + 0.5, dist);
	float lip_band_outer = 1.0 - smoothstep(lip_outer - 0.5, lip_outer + 0.5, dist);
	float lip_band = lip_band_inner * lip_band_outer;

	// Determine if on top or bottom of the lip (including corners)
	float inset_corner = corner_radius + lip_inset;
	bool in_top_zone = pixel_pos.y < inset_corner;
	bool in_bottom_zone = pixel_pos.y > rect_size.y - inset_corner;

	// Horizontal fade (from inset position, not edge)
	float left_fade = smoothstep(lip_inset, lip_inset + lip_fade_pixels, pixel_pos.x);
	float right_fade = smoothstep(lip_inset, lip_inset + lip_fade_pixels, rect_size.x - pixel_pos.x);
	float h_fade = left_fade * right_fade;

	// Apply top lip shadow (dark - recessed top catches shadow)
	if (in_top_zone) {
		float top_lip_alpha = lip_band * h_fade * lip_top_shadow_opacity;
		color = mix(color, vec3(0.0), top_lip_alpha);
	}

	// Apply bottom lip highlight (light - recessed bottom catches light)
	if (in_bottom_zone) {
		float bottom_lip_alpha = lip_band * h_fade * lip_bottom_highlight_opacity;
		color = mix(color, cream.rgb, bottom_lip_alpha);
	}

	// Outer dark ring (at the very edge)
	color = apply_inner_shadow(color, dist, dark_ring_opacity);

	// Dithering to reduce gradient banding
	color += (dither(FRAGCOORD.xy) - 0.5) / 255.0;

	COLOR = vec4(color, base_color.a * edge_alpha);
}
