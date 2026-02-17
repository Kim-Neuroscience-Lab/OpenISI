// Coordinate transforms and zone detection for UI shaders
// Provides consistent position calculations and corner detection

// Calculate pixel position and center-relative position from UV
// Returns: vec4(pixel_pos.x, pixel_pos.y, center_pos.x, center_pos.y)
vec4 get_positions(vec2 uv, vec2 size) {
	vec2 pixel_pos = uv * size;
	vec2 center_pos = pixel_pos - size * 0.5;
	return vec4(pixel_pos, center_pos);
}

// Calculate horizontal fade at left/right edges (pixel-based)
// Returns smooth fade: 0.0 at edges, 1.0 in center
float horizontal_fade(vec2 pixel_pos, vec2 size, float fade_pixels) {
	float left_fade = smoothstep(0.0, fade_pixels, pixel_pos.x);
	float right_fade = smoothstep(0.0, fade_pixels, size.x - pixel_pos.x);
	return left_fade * right_fade;
}

// Determine which corner zone a pixel is in
// Returns: vec4(top_left, top_right, bottom_left, bottom_right) as 0.0 or 1.0
vec4 corner_zones(vec2 pixel_pos, vec2 size, float corner_radius) {
	float in_top_left = (pixel_pos.x < corner_radius && pixel_pos.y < corner_radius) ? 1.0 : 0.0;
	float in_top_right = (pixel_pos.x > size.x - corner_radius && pixel_pos.y < corner_radius) ? 1.0 : 0.0;
	float in_bottom_left = (pixel_pos.x < corner_radius && pixel_pos.y > size.y - corner_radius) ? 1.0 : 0.0;
	float in_bottom_right = (pixel_pos.x > size.x - corner_radius && pixel_pos.y > size.y - corner_radius) ? 1.0 : 0.0;
	return vec4(in_top_left, in_top_right, in_bottom_left, in_bottom_right);
}
