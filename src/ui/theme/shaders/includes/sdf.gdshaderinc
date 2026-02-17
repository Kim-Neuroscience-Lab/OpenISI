// Signed Distance Functions for UI shapes
// Used by raised surface cards, buttons, input fields, and masks

// Signed distance function for rounded rectangle
// Returns negative values inside the shape, positive outside
// center_pos: position relative to rectangle center
// half_size: half of the rectangle dimensions
// radius: corner radius
float rounded_rect_sdf(vec2 center_pos, vec2 half_size, float radius) {
	vec2 d = abs(center_pos) - half_size + radius;
	return length(max(d, 0.0)) + min(max(d.x, d.y), 0.0) - radius;
}
