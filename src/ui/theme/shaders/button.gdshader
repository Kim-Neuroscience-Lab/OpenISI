shader_type canvas_item;

#include "includes/sdf.gdshaderinc"
#include "includes/common.gdshaderinc"
#include "includes/geometry.gdshaderinc"
#include "includes/gradients.gdshaderinc"
#include "includes/rim.gdshaderinc"
#include "includes/outline.gdshaderinc"

// Button shader with secondary (raised) and nightlight (glowing) modes
//
// Mode 0 (Secondary): Raised surface with subtle gradient and rim highlights
// Mode 1 (Nightlight): Warm amber gradient with glow

// Mode: 0 = secondary, 1 = nightlight
uniform int mode : hint_range(0, 1) = 0;

// Button state: 0 = normal, 1 = hover, 2 = pressed, 3 = disabled
uniform int button_state : hint_range(0, 3) = 0;

// Rim mode: 0 = full (bright top + dark bottom), 1 = dark only (pressed state)
uniform int rim_mode : hint_range(0, 1) = 0;

// Geometry
uniform vec2 rect_size = vec2(200.0, 56.0);
uniform float corner_radius : hint_range(0.0, 32.0) = 12.0;
uniform float button_inset : hint_range(0.0, 16.0) = 4.0;

// Neumorphic inset settings
uniform float inset_depth : hint_range(0.0, 12.0) = 2.5;
uniform float inset_shadow_opacity : hint_range(0.0, 1.0) = 0.4;
uniform float inset_highlight_opacity : hint_range(0.0, 0.5) = 0.15;
uniform float inset_ring_opacity : hint_range(0.0, 1.0) = 0.5;

// Rim highlight settings
uniform float rim_width : hint_range(0.5, 5.0) = 2.0;
uniform float rim_fade_pixels : hint_range(0.0, 30.0) = 10.0;
uniform float rim_top_opacity : hint_range(0.0, 0.5) = 0.15;
uniform float rim_bottom_shadow_opacity : hint_range(0.0, 0.5) = 0.3;
uniform float rim_bottom_highlight_opacity : hint_range(0.0, 0.2) = 0.07;
uniform float outline_opacity : hint_range(0.0, 0.2) = 0.06;

// Secondary mode colors
uniform vec4 surface : source_color = vec4(0.110, 0.094, 0.141, 1.0);
uniform vec4 surface_elevated : source_color = vec4(0.141, 0.122, 0.180, 1.0);
uniform vec4 surface_disabled : source_color = vec4(0.094, 0.078, 0.122, 1.0);
uniform vec4 surface_recessed : source_color = vec4(0.075, 0.067, 0.098, 1.0);
uniform vec4 well : source_color = vec4(0.059, 0.051, 0.078, 1.0);
uniform vec4 cream : source_color = vec4(0.941, 0.902, 0.863, 1.0);
uniform vec4 lavender_deep : source_color = vec4(0.420, 0.353, 0.541, 1.0);

// Nightlight mode colors
uniform vec4 amber_bright : source_color = vec4(0.988, 0.847, 0.682, 1.0);
uniform vec4 amber : source_color = vec4(0.910, 0.769, 0.604, 1.0);
uniform vec4 amber_glow : source_color = vec4(0.831, 0.647, 0.455, 1.0);
uniform vec4 amber_pale : source_color = vec4(0.659, 0.565, 0.502, 1.0);
uniform vec4 amber_glow_pale : source_color = vec4(0.541, 0.471, 0.408, 1.0);

// Glow intensity (for amber mode)
uniform float glow_intensity : hint_range(0.0, 1.0) = 0.15;

void fragment() {
	vec4 pos = get_positions(UV, rect_size);
	vec2 pixel_pos = pos.xy;
	vec2 center_pos = pos.zw;

	// Button is inset from shader rect edges
	vec2 button_half_size = rect_size * 0.5 - vec2(button_inset);

	// Distance from button's rounded rectangle edge (negative = inside button)
	float dist = rounded_rect_sdf(center_pos, button_half_size, corner_radius);

	// Anti-aliased edge
	float edge_alpha = aa_edge(dist, 1.0);

	// UV coordinates relative to button area (for gradients)
	vec2 button_uv = (pixel_pos - vec2(button_inset)) / (rect_size - vec2(button_inset * 2.0));
	button_uv = clamp(button_uv, vec2(0.0), vec2(1.0));

	// === NEUMORPHIC INSET ===
	// Soft depression around button like it's set into a surface

	// 1px dark ring at button edge
	float ring_mask = smoothstep(0.0, 0.5, dist) * (1.0 - smoothstep(0.5, 1.5, dist));
	float inset_ring = ring_mask * inset_ring_opacity;

	// Normalized distance into inset area
	float inset_t = clamp(dist / inset_depth, 0.0, 1.0);
	float falloff = 1.0 - pow(inset_t, 0.6);

	// Only apply gradient outside button (but not in ring area)
	float outside_button = smoothstep(-0.5, 0.5, dist);
	float past_ring = smoothstep(0.5, 1.5, dist);
	falloff *= outside_button * past_ring;

	// Inset lighting: top = shadow, bottom = highlight
	float top_shadow = (1.0 - button_uv.y);
	float bottom_highlight = button_uv.y;

	float inset_shadow = falloff * top_shadow * inset_shadow_opacity;
	float inset_highlight = falloff * bottom_highlight * inset_highlight_opacity;

	float gradient_alpha = max(inset_shadow, inset_highlight) *
	                       (1.0 - smoothstep(inset_depth - 0.5, inset_depth, dist));
	float inset_alpha = max(inset_ring, gradient_alpha);

	vec3 color;

	// Button size for pixel-based calculations
	vec2 btn_size = rect_size - vec2(button_inset * 2.0);
	vec2 btn_pixel_pos = pixel_pos - vec2(button_inset);

	if (mode == 0) {
		// === SECONDARY MODE ===
		vec4 base_color = surface;
		if (button_state == 1) {
			base_color = surface_elevated;
		} else if (button_state == 2) {
			base_color = surface_recessed;
		} else if (button_state == 3) {
			base_color = surface_disabled;
		}

		// Subtle vertical gradient for raised surface
		float gradient_strength = 0.06;
		vec3 grad_top = base_color.rgb * (1.0 + gradient_strength);
		vec3 grad_bottom = base_color.rgb * (1.0 - gradient_strength);
		color = mix(grad_top, grad_bottom, button_uv.y);

	} else {
		// === NIGHTLIGHT MODE ===
		vec3 top_col = amber_bright.rgb;
		vec3 mid_col = amber.rgb;
		vec3 bottom_col = amber_glow.rgb;

		if (button_state == 1) {
			top_col *= 1.05;
			mid_col *= 1.05;
		} else if (button_state == 2) {
			top_col = amber.rgb;
			mid_col = amber_glow.rgb;
			bottom_col = amber_glow.rgb * 0.9;
		} else if (button_state == 3) {
			top_col = amber_pale.rgb;
			mid_col = amber_pale.rgb;
			bottom_col = amber_glow_pale.rgb;
		}

		color = three_stop_gradient(top_col, mid_col, bottom_col, button_uv.y, 0.4);
	}

	// === RIM HIGHLIGHTS (both modes) ===
	float band = rim_band(dist, rim_width);
	vec2 rim_info = rim_zones(btn_pixel_pos, btn_size, corner_radius);
	float h_fade = horizontal_fade(btn_pixel_pos, btn_size, rim_fade_pixels);

	// Top rim highlight (only if rim_mode is full)
	if (rim_mode == 0) {
		float top_rim_alpha = band * rim_info.x * h_fade * rim_top_opacity;
		if (button_state == 1) top_rim_alpha *= 1.3;
		color = mix(color, cream.rgb, top_rim_alpha);
	}

	// Bottom rim shadow (always shown)
	float bottom_rim_alpha = band * rim_info.y * h_fade * rim_bottom_shadow_opacity;
	color = mix(color, vec3(0.0), bottom_rim_alpha);

	// Bottom edge bright highlight (straight edge only)
	if (rim_mode == 0 && on_straight_bottom(btn_pixel_pos, btn_size, corner_radius)) {
		float corner_fade_left = smoothstep(corner_radius, corner_radius + rim_fade_pixels, btn_pixel_pos.x);
		float corner_fade_right = smoothstep(corner_radius, corner_radius + rim_fade_pixels, btn_size.x - btn_pixel_pos.x);
		float corner_fade = corner_fade_left * corner_fade_right;
		float bottom_highlight_alpha = band * corner_fade * rim_bottom_highlight_opacity;
		color = mix(color, cream.rgb, bottom_highlight_alpha);
	}

	// Thin outline
	color = apply_outline(color, dist, 1.0, vec4(cream.rgb, outline_opacity));

	// === COMPOSITE OUTPUT ===
	vec3 shadow_color = vec3(0.0);
	vec3 highlight_color = cream.rgb;

	float highlight_ratio = inset_highlight / max(inset_shadow + inset_highlight, 0.001);
	vec3 gradient_color = mix(shadow_color, highlight_color, highlight_ratio);

	float ring_blend = inset_ring / max(inset_alpha, 0.001);
	vec3 inset_color = mix(gradient_color, shadow_color, ring_blend);

	vec3 final_color = mix(inset_color, color, edge_alpha);
	float final_alpha = max(edge_alpha, inset_alpha);

	// Dithering
	final_color += (dither(FRAGCOORD.xy) - 0.5) / 255.0;

	COLOR = vec4(final_color, final_alpha);
}
