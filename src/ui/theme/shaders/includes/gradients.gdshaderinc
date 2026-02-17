// Gradient functions for surface depth effects
// Creates convex (raised) or concave (inset) appearance through lighting

// Raised gradient: lightens at top, darkens at bottom
// Creates convex/raised surface appearance (light comes from above)
vec3 raised_gradient(vec3 surface_rgb, float uv_y, float intensity) {
	vec3 color = surface_rgb;
	color += (1.0 - uv_y) * intensity;  // Lighten top
	color -= uv_y * intensity;           // Darken bottom
	return color;
}

// Inset gradient: darkens at top, lightens at bottom
// Creates concave/recessed surface appearance (shadow falls into top)
vec3 inset_gradient(vec3 surface_rgb, float uv_y, float intensity) {
	vec3 color = surface_rgb;
	color -= (1.0 - uv_y) * intensity;  // Darken top
	color += uv_y * intensity;           // Lighten bottom
	return color;
}

// Three-stop vertical gradient
// Useful for warm/amber glowing surfaces
// mid_point: where middle color is (0-1, typically 0.3-0.5)
vec3 three_stop_gradient(vec3 top_color, vec3 mid_color, vec3 bottom_color, float uv_y, float mid_point) {
	if (uv_y < mid_point) {
		float t = uv_y / mid_point;
		return mix(top_color, mid_color, t);
	} else {
		float t = (uv_y - mid_point) / (1.0 - mid_point);
		return mix(mid_color, bottom_color, t);
	}
}
