// Common utility functions for UI shaders
// Dithering, anti-aliasing, color utilities

// Dithering to reduce gradient banding
// Uses interleaved gradient noise for smooth transitions
// screen_pos: screen-space position (typically FRAGCOORD.xy or UV * size)
float dither(vec2 screen_pos) {
	return fract(52.9829189 * fract(dot(screen_pos, vec2(0.06711056, 0.00583715))));
}


// Anti-aliased edge alpha
// Returns 1.0 inside, 0.0 outside, with smooth transition at edge
// dist: signed distance from edge (negative inside)
// edge_width: width of anti-aliasing transition (typically 1.0)
float aa_edge(float dist, float edge_width) {
	return 1.0 - smoothstep(-edge_width, 0.0, dist);
}
