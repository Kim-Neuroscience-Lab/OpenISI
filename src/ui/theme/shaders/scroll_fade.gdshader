shader_type canvas_item;

#include "includes/common.gdshaderinc"

// Scroll fade overlay shader
// Simple gradient: fades from opaque (at header/footer edge) to transparent (into content)
// The overlay is positioned to cover from screen edge into content area

uniform vec4 bg_color : source_color = vec4(0.075, 0.067, 0.102, 1.0);
uniform bool fade_from_top = true;

void fragment() {
	float opacity;
	if (fade_from_top) {
		// Top overlay: opaque at top (UV.y=0), transparent at bottom (UV.y=1)
		opacity = 1.0 - UV.y;
	} else {
		// Bottom overlay: transparent at top (UV.y=0), opaque at bottom (UV.y=1)
		opacity = UV.y;
	}

	// Smooth easing for natural fade
	opacity = opacity * opacity * (3.0 - 2.0 * opacity);

	// Apply dithering to reduce banding (±0.5/255 noise)
	float noise = (dither(FRAGCOORD.xy) - 0.5) / 255.0;
	opacity = clamp(opacity + noise * 2.0, 0.0, 1.0);

	COLOR = vec4(bg_color.rgb, opacity);
}
