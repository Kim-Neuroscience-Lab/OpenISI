class_name TextureRenderer
extends StimulusRendererBase
## Unified renderer for all texture-based stimuli.
##
## Handles all texture paradigm envelopes (NONE, BAR, WEDGE, RING) by selecting
## the appropriate shader and setting common/envelope-specific uniforms.
##
## This replaces the individual renderers:
## - CheckerboardRenderer (NONE envelope)
## - DriftingBarRenderer (BAR envelope)
## - RotatingWedgeRenderer (WEDGE envelope)
## - ExpandingRingRenderer (RING envelope)


## Shader paths for each envelope type
const SHADER_PATHS := {
	Envelopes.Type.NONE: "res://src/stimulus/shaders/texture_fullfield.gdshader",
	Envelopes.Type.BAR: "res://src/stimulus/shaders/texture_bar.gdshader",
	Envelopes.Type.WEDGE: "res://src/stimulus/shaders/texture_wedge.gdshader",
	Envelopes.Type.RING: "res://src/stimulus/shaders/texture_ring.gdshader",
}

## Direction mappings for shaders
const BAR_DIRECTION_MAP := {
	"LR": 0, "RL": 1, "TB": 2, "BT": 3,
}
const WEDGE_DIRECTION_MAP := {
	"CW": 0, "CCW": 1,
}
const RING_DIRECTION_MAP := {
	"EXP": 0, "CON": 1,
}

## Shader material
var _material: ShaderMaterial = null

## Current envelope type
var _envelope_type: int = Envelopes.Type.NONE


func get_type_id() -> String:
	return "texture"


func requires_shader() -> bool:
	return true


func _on_initialize() -> void:
	# Get envelope type from params
	_envelope_type = int(get_param("envelope"))

	# Load appropriate shader
	if not SHADER_PATHS.has(_envelope_type):
		push_error("TextureRenderer: Unknown envelope type: %d" % _envelope_type)
		return

	var shader_path: String = SHADER_PATHS[_envelope_type]
	var shader: Shader = load(shader_path) as Shader

	if shader == null:
		push_error("TextureRenderer: Failed to load shader: %s" % shader_path)
		return

	_material = ShaderMaterial.new()
	_material.shader = shader

	# Set all uniforms
	_set_common_uniforms()
	_set_carrier_uniforms()
	_set_modulation_uniforms()
	_set_envelope_uniforms()


## Set common uniforms shared by all texture shaders
func _set_common_uniforms() -> void:
	if _material == null:
		return

	# Display geometry (handled by geometry.apply_to_shader)
	if geometry:
		geometry.apply_to_shader(_material)

	# Visual field (from geometry or Config)
	_material.set_shader_parameter("visual_field_deg", get_visual_field_deg())


## Set carrier-related uniforms
func _set_carrier_uniforms() -> void:
	if _material == null:
		return

	# Carrier type
	var carrier: int = int(get_param("carrier"))
	var carrier_type_shader: int = 0 if carrier == Carriers.Type.SOLID else 1
	_material.set_shader_parameter("carrier_type", carrier_type_shader)

	# Check size (both units - shader picks based on projection)
	_material.set_shader_parameter("check_size_cm", float(get_param("check_size_cm")))
	_material.set_shader_parameter("check_size_deg", float(get_param("check_size_deg")))

	# Contrast and luminance
	_material.set_shader_parameter("contrast", float(get_param("contrast")))
	_material.set_shader_parameter("mean_luminance", float(get_param("mean_luminance")))
	_material.set_shader_parameter("luminance_high", float(get_param("luminance_max")))
	_material.set_shader_parameter("luminance_low", float(get_param("luminance_min")))


## Set modulation uniforms (strobe)
func _set_modulation_uniforms() -> void:
	if _material == null:
		return

	# Strobe frequency (only if enabled)
	var strobe_enabled: bool = bool(get_param("strobe_enabled"))
	var strobe_freq: float = float(get_param("strobe_frequency_hz")) if strobe_enabled else 0.0
	_material.set_shader_parameter("strobe_frequency_hz", strobe_freq)


## Set envelope-specific uniforms based on envelope type
func _set_envelope_uniforms() -> void:
	if _material == null:
		return

	match _envelope_type:
		Envelopes.Type.NONE:
			# Full-field has no envelope-specific uniforms
			pass

		Envelopes.Type.BAR:
			_material.set_shader_parameter("stimulus_width_deg", float(get_param("stimulus_width_deg")))
			_material.set_shader_parameter("background_luminance", float(get_param("background_luminance")))
			_material.set_shader_parameter("rotation_deg", float(get_param("rotation_deg")))
			# Direction and progress set in _on_update

		Envelopes.Type.WEDGE:
			_material.set_shader_parameter("stimulus_width_deg", float(get_param("stimulus_width_deg")))
			_material.set_shader_parameter("background_luminance", float(get_param("background_luminance")))
			_material.set_shader_parameter("rotation_deg", float(get_param("rotation_deg")))
			_material.set_shader_parameter("gradual_seam", true)  # Always use gradual seam for wedge
			# Direction and progress set in _on_update

		Envelopes.Type.RING:
			_material.set_shader_parameter("stimulus_width_deg", float(get_param("stimulus_width_deg")))
			_material.set_shader_parameter("background_luminance", float(get_param("background_luminance")))
			_material.set_shader_parameter("rotation_deg", float(get_param("rotation_deg")))
			# Calculate max eccentricity from geometry
			assert(geometry != null, "TextureRenderer: RING envelope requires geometry")
			var max_ecc := geometry.get_max_eccentricity_deg()
			_material.set_shader_parameter("max_eccentricity_deg", max_ecc)
			# Direction and progress set in _on_update


## Quantize elapsed time to frame boundaries for phase-locked modulation.
## This ensures strobe/temporal modulation is locked to vsync.
func _quantize_to_frame(elapsed_sec: float, refresh_hz: float) -> float:
	if refresh_hz <= 0:
		return elapsed_sec
	var frame_count := floori(elapsed_sec * refresh_hz)
	return frame_count / refresh_hz


func _on_update(_delta: float) -> void:
	if _material == null:
		return

	# Quantize time to frame boundaries for phase-locked modulation
	var refresh_hz := float(Session.display_refresh_hz)
	var quantized_time := _quantize_to_frame(state.elapsed_sec, refresh_hz)

	# Update time for all shaders (use frame-quantized time for phase-locked strobe)
	_material.set_shader_parameter("time_sec", quantized_time)

	# Update envelope-specific dynamic uniforms
	match _envelope_type:
		Envelopes.Type.NONE:
			pass  # No dynamic uniforms

		Envelopes.Type.BAR:
			var direction_int: int = BAR_DIRECTION_MAP[state.direction]
			_material.set_shader_parameter("direction", direction_int)
			_material.set_shader_parameter("progress", state.progress)

		Envelopes.Type.WEDGE:
			var direction_int: int = WEDGE_DIRECTION_MAP[state.direction]
			_material.set_shader_parameter("direction", direction_int)
			_material.set_shader_parameter("progress", state.progress)

		Envelopes.Type.RING:
			var direction_int: int = RING_DIRECTION_MAP[state.direction]
			_material.set_shader_parameter("direction", direction_int)
			_material.set_shader_parameter("progress", state.progress)


func _on_render(_canvas: CanvasItem) -> void:
	# Shader-based rendering is handled by the ColorRect in StimulusDisplay
	# This method is not called for shader renderers
	pass


func _render_baseline(canvas: CanvasItem) -> void:
	# During baseline, show background luminance
	var bg_lum: float = float(get_param("background_luminance"))
	canvas.draw_rect(Rect2(Vector2.ZERO, display_size), Color(bg_lum, bg_lum, bg_lum))


func get_shader_material() -> ShaderMaterial:
	return _material


func get_paradigm_state() -> Dictionary:
	var visual_field := get_visual_field_deg()
	var envelope_x := 0.0
	var envelope_y := 0.0
	var rotate_angle := 0.0
	var expand_eccentricity := 0.0

	match _envelope_type:
		Envelopes.Type.BAR:
			# Bar position depends on direction
			match state.direction:
				"LR":
					envelope_x = -visual_field.x / 2.0 + state.progress * visual_field.x
				"RL":
					envelope_x = visual_field.x / 2.0 - state.progress * visual_field.x
				"TB":
					envelope_y = -visual_field.y / 2.0 + state.progress * visual_field.y
				"BT":
					envelope_y = visual_field.y / 2.0 - state.progress * visual_field.y

		Envelopes.Type.WEDGE:
			# Wedge angle
			var angle := state.progress * 360.0
			if state.direction == "CW":
				angle = -angle
			rotate_angle = angle
			envelope_x = angle

		Envelopes.Type.RING:
			# Ring eccentricity
			assert(geometry != null, "TextureRenderer: RING envelope requires geometry for paradigm state")
			var max_ecc := geometry.get_max_eccentricity_deg()
			if state.direction == "EXP":
				expand_eccentricity = state.progress * max_ecc
			else:
				expand_eccentricity = max_ecc - state.progress * max_ecc
			envelope_x = expand_eccentricity

	# Calculate strobe phase (contrast reversal) - use frame-quantized time
	var refresh_hz := float(Session.display_refresh_hz)
	var quantized_time := _quantize_to_frame(state.elapsed_sec, refresh_hz)
	var strobe_freq: float = float(get_param("strobe_frequency_hz"))
	var strobe_phase := fmod(quantized_time * strobe_freq, 1.0)
	var carrier_phase := strobe_phase * 360.0

	return {
		"envelope_x": envelope_x,
		"envelope_y": envelope_y,
		"carrier_phase": carrier_phase,
		"strobe_phase": strobe_phase,
		"rotate_angle": rotate_angle,
		"expand_eccentricity": expand_eccentricity,
		"direction_value": state.progress,
	}


func _on_cleanup() -> void:
	_material = null
