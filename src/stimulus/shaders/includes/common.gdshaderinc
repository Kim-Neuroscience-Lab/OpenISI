// common.gdshaderinc
// Shared uniforms and utility functions for all texture stimuli.
//
// This file is included by all texture shaders to provide:
// - Display geometry uniforms (set by DisplayGeometry.apply_to_shader())
// - Time uniform (updated by renderer each frame)
// - Common utility functions

// ============================================================================
// Display Geometry Uniforms
// These are set by DisplayGeometry.apply_to_shader() and define the
// physical display configuration and projection parameters.
// ============================================================================

uniform vec2 visual_field_deg = vec2(120.0, 90.0);
uniform int projection_type = 0;  // 0=cartesian, 1=spherical, 2=cylindrical
uniform float viewing_distance_cm = 10.0;
uniform vec2 display_size_cm = vec2(34.89, 13.74);
uniform vec2 center_offset_deg = vec2(0.0, 0.0);

// ============================================================================
// Timing Uniform
// Updated by the renderer each frame.
// ============================================================================

uniform float time_sec : hint_range(0.0, 100000.0) = 0.0;

// ============================================================================
// Utility Functions
// Note: PI and TAU are built-in Godot shader constants
// ============================================================================

// Rotate a 2D point around the origin by angle in degrees
vec2 rotate_point(vec2 p, float angle_deg) {
	float angle_rad = radians(angle_deg);
	float c = cos(angle_rad);
	float s = sin(angle_rad);
	return vec2(c * p.x - s * p.y, s * p.x + c * p.y);
}

// Normalize angle to -180 to 180 range
float normalize_angle(float angle) {
	return mod(angle + 180.0, 360.0) - 180.0;
}

// Normalize angle to 0 to 360 range
float normalize_angle_positive(float angle) {
	return mod(angle, 360.0);
}
