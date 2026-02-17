// modulation.gdshaderinc
// Temporal modulation functions for texture stimuli.
//
// Modulation controls how the carrier pattern changes over time:
// - STROBE: Contrast reversal at specified frequency
// Future: DRIFT (for drifting gratings)

// ============================================================================
// Modulation Uniforms
// ============================================================================

uniform float strobe_frequency_hz : hint_range(0.0, 30.0) = 4.0;  // Contrast reversal frequency

// ============================================================================
// Strobe (Contrast Reversal)
// Returns +1.0 or -1.0 based on strobe phase.
// Positive polarity = normal pattern, negative = inverted pattern.
// ============================================================================

float get_strobe_polarity() {
	if (strobe_frequency_hz <= 0.0) {
		// No strobe - always positive polarity
		return 1.0;
	}

	// Calculate phase based on time and frequency
	// At strobe_frequency_hz, contrast reverses that many times per second
	float phase = time_sec * strobe_frequency_hz * TAU;
	return sign(sin(phase));
}

// ============================================================================
// Strobe Phase (0.0 to 1.0)
// Useful for recording/analysis to track where in the strobe cycle we are.
// ============================================================================

float get_strobe_phase() {
	if (strobe_frequency_hz <= 0.0) {
		return 0.0;
	}
	return fract(time_sec * strobe_frequency_hz);
}
