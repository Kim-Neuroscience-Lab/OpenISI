shader_type canvas_item;

#include "includes/sdf.gdshaderinc"
#include "includes/geometry.gdshaderinc"
#include "includes/rim.gdshaderinc"
#include "includes/outline.gdshaderinc"

// Rounded rectangle mask shader for inset wells
// Clips content to rounded corners with solid rim and highlights

uniform float corner_radius : hint_range(0.0, 100.0) = 12.0;
uniform vec2 rect_size = vec2(100.0, 100.0);

// Rim base color (the "wall" of the well)
uniform vec4 rim_base_color : source_color = vec4(0.118, 0.098, 0.149, 1.0);
uniform float rim_width : hint_range(0.5, 5.0) = 2.0;

// Rim highlight settings (inverted from raised: dark on top, light on bottom)
uniform vec4 rim_top_color : source_color = vec4(0.0, 0.0, 0.0, 0.3);
uniform vec4 rim_bottom_color : source_color = vec4(0.941, 0.902, 0.863, 0.15);
uniform float rim_fade_pixels : hint_range(0.0, 50.0) = 15.0;

// Thin outline around entire well
uniform vec4 outline_color : source_color = vec4(0.941, 0.902, 0.863, 0.06);
uniform float outline_width : hint_range(0.5, 2.0) = 1.0;

void fragment() {
	vec4 pos = get_positions(UV, rect_size);
	vec2 pixel_pos = pos.xy;
	vec2 center_pos = pos.zw;

	// Distance from rounded rectangle edge (negative = inside)
	float dist = rounded_rect_sdf(center_pos, rect_size * 0.5, corner_radius);

	// Get the texture color
	vec4 tex_color = texture(TEXTURE, UV);

	// Anti-aliased outer edge for clipping
	float aa = fwidth(dist) * 1.5;
	float clip_alpha = 1.0 - smoothstep(-aa, aa, dist);

	// Discard pixels outside
	if (clip_alpha < 0.001) {
		discard;
	}

	// Solid rim band (wall of the well)
	// Content is visible only inside the rim
	float content_edge = smoothstep(-rim_width - 0.5, -rim_width + 0.5, dist);

	// Start with content inside, rim base at edge
	vec3 color = mix(tex_color.rgb, rim_base_color.rgb, content_edge);

	// Rim highlights (only on the rim band)
	float outer_edge = 1.0 - smoothstep(-0.5, 0.5, dist);
	float band = content_edge * outer_edge;

	vec2 rim_info = rim_zones(pixel_pos, rect_size, corner_radius);
	float show_rim = max(rim_info.x, rim_info.y);
	float h_fade = horizontal_fade(pixel_pos, rect_size, rim_fade_pixels);

	// Select rim color (inverted: dark on top, light on bottom)
	vec4 rim_color = mix(rim_bottom_color, rim_top_color, rim_info.x);

	// Apply rim highlight
	float rim_alpha = band * show_rim * h_fade * rim_color.a;
	color = mix(color, rim_color.rgb, rim_alpha);

	// Apply outline
	color = apply_outline(color, dist, outline_width, outline_color);

	COLOR = vec4(color, clip_alpha);
}
