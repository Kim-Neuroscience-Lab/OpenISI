// Outline and border calculations for UI elements
// Creates thin borders and shadow rings at element edges

// Calculate outline mask from SDF distance
// Returns intensity in [0, 1] range for blending
// dist: signed distance from edge (negative inside)
// outline_width: width of the outline in pixels
float outline_mask(float dist, float outline_width) {
	float inner = smoothstep(-outline_width - 0.5, -outline_width, dist);
	float outer = 1.0 - smoothstep(-0.5, 0.0, dist);
	return inner * outer;
}

// Apply outline to color
// Returns modified color with outline blended in
vec3 apply_outline(vec3 color, float dist, float outline_width, vec4 outline_color) {
	float mask = outline_mask(dist, outline_width);
	return mix(color, outline_color.rgb, mask * outline_color.a);
}

// Inner shadow ring mask (for inset surfaces like input fields)
// Creates a thin dark ring right at the element edge
// Returns intensity in [0, 1] range
float inner_shadow_mask(float dist) {
	float inner = smoothstep(-1.5, -1.0, dist);
	float outer = 1.0 - smoothstep(-0.5, 0.0, dist);
	return inner * outer;
}

// Apply inner shadow ring
// For recessed elements where the edge should appear to drop into shadow
vec3 apply_inner_shadow(vec3 color, float dist, float opacity) {
	float mask = inner_shadow_mask(dist);
	return mix(color, vec3(0.0), mask * opacity);
}
