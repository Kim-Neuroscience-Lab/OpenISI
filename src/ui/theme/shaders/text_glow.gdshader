shader_type canvas_item;

// Text glow shader for Sleep Punk Night aesthetic
// Creates a soft colored glow around text

uniform vec4 glow_color : source_color = vec4(0.722, 0.647, 0.788, 0.25);
uniform float glow_size : hint_range(0.0, 50.0) = 20.0;
uniform float glow_intensity : hint_range(0.0, 2.0) = 1.0;

void fragment() {
	vec4 col = texture(TEXTURE, UV);

	// Sample surrounding pixels for glow
	float glow = 0.0;
	float samples = 0.0;
	float pixel_size = 1.0 / 512.0; // Approximate, adjust based on resolution

	for (float x = -3.0; x <= 3.0; x += 1.0) {
		for (float y = -3.0; y <= 3.0; y += 1.0) {
			vec2 offset = vec2(x, y) * pixel_size * glow_size * 0.1;
			glow += texture(TEXTURE, UV + offset).a;
			samples += 1.0;
		}
	}

	glow = (glow / samples) * glow_intensity;

	// Combine original color with glow
	vec4 glow_layer = vec4(glow_color.rgb, glow * glow_color.a);
	COLOR = mix(glow_layer, col, col.a);
	COLOR.a = max(glow_layer.a, col.a);
}
