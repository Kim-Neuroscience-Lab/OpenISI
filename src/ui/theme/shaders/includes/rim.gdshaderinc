// Rim highlight calculations for UI surfaces
// Creates the characteristic edge highlighting for raised/inset elements
// Requires: geometry.gdshaderinc (for corner_zones, horizontal_fade)

// Determine which rim zone a pixel is in (top, bottom, or neither)
// Returns: vec2(is_top_rim, is_bottom_rim)
vec2 rim_zones(vec2 pixel_pos, vec2 size, float corner_radius) {
	vec4 corners = corner_zones(pixel_pos, size, corner_radius);

	float is_top_rim = 0.0;
	float is_bottom_rim = 0.0;

	// Corner zones: top corners = top rim, bottom corners = bottom rim
	if (corners.x > 0.5 || corners.y > 0.5) {
		// Top-left or top-right corner
		is_top_rim = 1.0;
	} else if (corners.z > 0.5 || corners.w > 0.5) {
		// Bottom-left or bottom-right corner
		is_bottom_rim = 1.0;
	} else {
		// Straight edges (not in corner)
		if (pixel_pos.y < corner_radius) {
			is_top_rim = 1.0;
		} else if (pixel_pos.y > size.y - corner_radius) {
			is_bottom_rim = 1.0;
		}
	}

	return vec2(is_top_rim, is_bottom_rim);
}

// Calculate rim band intensity based on SDF distance
// Returns intensity in [0, 1] range
// dist: signed distance from edge (negative inside)
// rim_width: width of the rim band in pixels
float rim_band(float dist, float rim_width) {
	float inner_edge = smoothstep(-rim_width - 0.5, -rim_width + 0.5, dist);
	float outer_edge = 1.0 - smoothstep(-0.5, 0.5, dist);
	return inner_edge * outer_edge;
}

// Apply rim highlight to color
// Blends top_color or bottom_color based on rim zone
vec3 apply_rim_highlight(
	vec3 color,
	float dist,
	vec2 pixel_pos,
	vec2 size,
	float corner_radius,
	float rim_width,
	float fade_pixels,
	vec4 top_color,
	vec4 bottom_color
) {
	float band = rim_band(dist, rim_width);
	vec2 rim_info = rim_zones(pixel_pos, size, corner_radius);
	float show_rim = max(rim_info.x, rim_info.y);
	float h_fade = horizontal_fade(pixel_pos, size, fade_pixels);

	// Select color based on top/bottom (rim_info.x = is_top)
	vec4 rim_color = mix(bottom_color, top_color, rim_info.x);

	// Apply rim
	float rim_alpha = band * show_rim * h_fade * rim_color.a;
	return mix(color, rim_color.rgb, rim_alpha);
}

// Check if on straight bottom edge (excluding corners)
bool on_straight_bottom(vec2 pixel_pos, vec2 size, float corner_radius) {
	return pixel_pos.y > size.y - corner_radius &&
	       pixel_pos.x > corner_radius &&
	       pixel_pos.x < size.x - corner_radius;
}

// Apply bottom edge bright highlight (straight portion only)
// Creates a subtle catch-light on the bottom rim
vec3 apply_bottom_highlight(
	vec3 color,
	float dist,
	vec2 pixel_pos,
	vec2 size,
	float corner_radius,
	float rim_width,
	float fade_pixels,
	vec4 highlight_color
) {
	if (!on_straight_bottom(pixel_pos, size, corner_radius)) {
		return color;
	}

	float band = rim_band(dist, rim_width);

	// Fade toward corners
	float corner_fade_left = smoothstep(corner_radius, corner_radius + fade_pixels, pixel_pos.x);
	float corner_fade_right = smoothstep(corner_radius, corner_radius + fade_pixels, size.x - pixel_pos.x);
	float corner_fade = corner_fade_left * corner_fade_right;

	float highlight_alpha = band * corner_fade * highlight_color.a;
	return mix(color, highlight_color.rgb, highlight_alpha);
}
