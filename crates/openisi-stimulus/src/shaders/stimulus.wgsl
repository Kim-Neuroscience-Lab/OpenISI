// Combined WGSL shader for all stimulus types.
// Ported from Godot GLSL shaders (4 envelope shaders + 4 include files).

const PI: f32 = 3.14159265358979323846;
const TAU: f32 = 6.28318530717958647692;

struct Uniforms {
    // Display geometry (offset 0)
    visual_field_deg: vec2<f32>,     // 0
    projection_type: i32,            // 8
    viewing_distance_cm: f32,        // 12
    display_size_cm: vec2<f32>,      // 16
    center_offset_deg: vec2<f32>,    // 24

    // Carrier (offset 32)
    carrier_type: i32,               // 32
    check_size_deg: f32,             // 36
    check_size_cm: f32,              // 40
    contrast: f32,                   // 44
    mean_luminance: f32,             // 48
    luminance_high: f32,             // 52
    luminance_low: f32,              // 56

    // Modulation (offset 60)
    strobe_frequency_hz: f32,        // 60

    // Timing (offset 64)
    time_sec: f32,                   // 64

    // Envelope (offset 68)
    envelope_type: i32,              // 68 (0=fullfield, 1=bar, 2=wedge, 3=ring)
    stimulus_width_deg: f32,         // 72
    progress: f32,                   // 76
    direction: i32,                  // 80
    rotation_deg: f32,               // 84
    background_luminance: f32,       // 88
    max_eccentricity_deg: f32,       // 92

    // Monitor physical rotation (offset 96)
    monitor_rotation_deg: f32,       // 96
    _pad0: f32,                      // 100
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

// ---------------------------------------------------------------------------
// Vertex shader
// ---------------------------------------------------------------------------

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    let x = f32(i32(vertex_index) / 2) * 4.0 - 1.0;
    let y = f32(i32(vertex_index) % 2) * 4.0 - 1.0;
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    // UV: map clip space (-1..1) to (0..1), flip Y so top=0
    out.uv = vec2<f32>(x * 0.5 + 0.5, 1.0 - (y * 0.5 + 0.5));
    return out;
}

// ---------------------------------------------------------------------------
// Utility functions (common.gdshaderinc)
// ---------------------------------------------------------------------------

fn rotate_point(p: vec2<f32>, angle_deg: f32) -> vec2<f32> {
    let rad = radians(angle_deg);
    let c = cos(rad);
    let s = sin(rad);
    return vec2<f32>(p.x * c - p.y * s, p.x * s + p.y * c);
}

fn normalize_angle(angle: f32) -> f32 {
    // Normalize to -180..180
    var a = angle;
    a = a - floor(a / 360.0) * 360.0; // modulo into 0..360
    if (a > 180.0) {
        a = a - 360.0;
    }
    return a;
}

fn normalize_angle_positive(angle: f32) -> f32 {
    // Normalize to 0..360
    var a = angle;
    a = a - floor(a / 360.0) * 360.0;
    return a;
}

// ---------------------------------------------------------------------------
// Projection functions (projection.gdshaderinc)
// ---------------------------------------------------------------------------

fn uv_to_equator_angle(uv: vec2<f32>) -> vec2<f32> {
    // Returns (azimuth, elevation) in degrees
    var az: f32;
    var el: f32;

    if (uniforms.projection_type == 0) {
        // Cartesian: simple linear mapping
        az = (uv.x - 0.5) * uniforms.visual_field_deg.x;
        el = (0.5 - uv.y) * uniforms.visual_field_deg.y;  // flip Y: UV y-down, elevation y-up
    } else {
        // Convert UV to physical cm on display
        let y_cm = (uv.x - 0.5) * uniforms.display_size_cm.x;   // horizontal
        let z_cm = (0.5 - uv.y) * uniforms.display_size_cm.y;   // vertical (flipped)
        let xo = uniforms.viewing_distance_cm;

        if (uniforms.projection_type == 1) {
            // Spherical (Marshel et al. 2012 correction)
            let r = sqrt(xo * xo + y_cm * y_cm + z_cm * z_cm);
            az = degrees(atan2(y_cm, xo));
            el = degrees(asin(z_cm / r));
        } else {
            // Cylindrical
            az = degrees(atan2(y_cm, xo));
            el = degrees(atan2(z_cm, xo));
        }
    }

    // Apply center offset
    az = az - uniforms.center_offset_deg.x;
    el = el - uniforms.center_offset_deg.y;

    return vec2<f32>(az, el);
}

fn uv_to_polar(uv: vec2<f32>) -> vec2<f32> {
    // Returns (eccentricity, polar_angle) in degrees
    let y_cm = (uv.x - 0.5) * uniforms.display_size_cm.x;   // horizontal
    let z_cm = (0.5 - uv.y) * uniforms.display_size_cm.y;   // vertical (flipped)
    let xo = uniforms.viewing_distance_cm;

    // Apply center offset in cm (convert degrees to cm at viewing distance)
    let offset_y_cm = xo * tan(radians(uniforms.center_offset_deg.x));
    let offset_z_cm = xo * tan(radians(uniforms.center_offset_deg.y));
    let cy = y_cm - offset_y_cm;
    let cz = z_cm - offset_z_cm;

    let polar_angle = degrees(atan2(cz, cy));
    let radial_cm = sqrt(cy * cy + cz * cz);
    let eccentricity = degrees(atan2(radial_cm, xo));

    return vec2<f32>(eccentricity, polar_angle);
}

fn uv_to_pattern_position(uv: vec2<f32>) -> vec2<f32> {
    // Position for checkerboard pattern in projection-appropriate units.
    let y_cm = (uv.x - 0.5) * uniforms.display_size_cm.x;
    let z_cm = (0.5 - uv.y) * uniforms.display_size_cm.y;
    let xo = uniforms.viewing_distance_cm;

    // Apply center offset in cm
    let offset_y_cm = tan(radians(uniforms.center_offset_deg.x)) * xo;
    let offset_z_cm = tan(radians(uniforms.center_offset_deg.y)) * xo;
    let cy = y_cm - offset_y_cm;
    let cz = z_cm - offset_z_cm;

    if (uniforms.projection_type == 1) {
        // Spherical: return visual angles for constant spatial frequency
        let r = sqrt(xo * xo + cy * cy + cz * cz);
        return vec2<f32>(degrees(atan2(cy, xo)), degrees(asin(cz / r)));
    } else if (uniforms.projection_type == 2) {
        // Cylindrical: spherical horizontal, flat vertical
        return vec2<f32>(degrees(atan2(cy, xo)), degrees(atan2(cz, xo)));
    } else {
        // Cartesian: return physical cm for square checks on screen
        return vec2<f32>(cy, cz);
    }
}

// ---------------------------------------------------------------------------
// Carrier functions (carriers.gdshaderinc)
// ---------------------------------------------------------------------------

fn get_check_size() -> f32 {
    if (uniforms.projection_type == 0) {
        return uniforms.check_size_cm;
    }
    return uniforms.check_size_deg;
}

fn solid_carrier(polarity: f32) -> f32 {
    if (polarity >= 0.0) {
        return uniforms.luminance_high;
    }
    return uniforms.luminance_low;
}

fn checkerboard_cartesian(position: vec2<f32>, polarity: f32) -> f32 {
    let cs = get_check_size();
    if (cs <= 0.0) {
        return uniforms.mean_luminance;
    }
    let ix = i32(floor(position.x / cs));
    let iy = i32(floor(position.y / cs));
    var pattern = (ix + iy) % 2;
    // Apply contrast reversal (polarity from strobe)
    if (polarity < 0.0) {
        pattern = 1 - pattern;
    }
    // Michelson contrast: luminance = mean ± contrast * mean
    let half_contrast = uniforms.contrast * uniforms.mean_luminance;
    if (pattern == 0) {
        return uniforms.mean_luminance + half_contrast;
    }
    return uniforms.mean_luminance - half_contrast;
}

fn checkerboard_polar(eccentricity: f32, polar_angle: f32, polarity: f32) -> f32 {
    let cs = uniforms.check_size_deg;
    if (cs <= 0.0) {
        return uniforms.mean_luminance;
    }
    // Simple grid: uniform angular spacing (matches GDShader reference)
    let radial_check = floor(eccentricity / cs);
    let angular_check = floor(polar_angle / cs);
    var pattern = i32(radial_check + angular_check) % 2;
    // Handle negative modulo
    if (pattern < 0) {
        pattern = pattern + 2;
    }
    // Apply contrast reversal
    if (polarity < 0.0) {
        pattern = 1 - pattern;
    }
    let half_contrast = uniforms.contrast * uniforms.mean_luminance;
    if (pattern == 0) {
        return uniforms.mean_luminance + half_contrast;
    }
    return uniforms.mean_luminance - half_contrast;
}

// ---------------------------------------------------------------------------
// Modulation functions (modulation.gdshaderinc)
// ---------------------------------------------------------------------------

fn get_strobe_polarity() -> f32 {
    if (uniforms.strobe_frequency_hz <= 0.0) {
        return 1.0;
    }
    let phase = uniforms.time_sec * uniforms.strobe_frequency_hz;
    let cycle_pos = phase - floor(phase); // fract
    if (cycle_pos < 0.5) {
        return 1.0;
    }
    return -1.0;
}

// ---------------------------------------------------------------------------
// Envelope functions
// ---------------------------------------------------------------------------

fn fullfield_envelope(uv: vec2<f32>) -> f32 {
    let polarity = get_strobe_polarity();
    if (uniforms.carrier_type == 0) {
        return solid_carrier(polarity);
    }
    return checkerboard_cartesian(uv_to_pattern_position(uv), polarity);
}

fn bar_envelope(uv: vec2<f32>) -> f32 {
    var pos_deg = uv_to_equator_angle(uv);

    if (uniforms.rotation_deg != 0.0) {
        pos_deg = rotate_point(pos_deg, -uniforms.rotation_deg);
    }

    let half_width = uniforms.stimulus_width_deg * 0.5;

    // Determine sweep axis based on direction:
    // 0=LR, 1=RL, 2=TB, 3=BT
    var sweep_extent: f32;
    var pos_along_sweep: f32;
    var bar_center: f32;

    if (uniforms.direction <= 1) {
        // Horizontal sweep (left-right or right-left)
        sweep_extent = uniforms.visual_field_deg.x;
        pos_along_sweep = pos_deg.x;
    } else {
        // Vertical sweep (top-bottom or bottom-top)
        sweep_extent = uniforms.visual_field_deg.y;
        pos_along_sweep = pos_deg.y;
    }

    let total_travel = sweep_extent + uniforms.stimulus_width_deg;
    let start = -sweep_extent * 0.5 - half_width;

    if (uniforms.direction == 0 || uniforms.direction == 3) {
        // LR or BT: sweep in positive direction (left→right, bottom→top)
        bar_center = start + uniforms.progress * total_travel;
    } else {
        // RL or TB: sweep in negative direction (right→left, top→bottom)
        bar_center = -start - uniforms.progress * total_travel;
    }

    let dist = abs(pos_along_sweep - bar_center);

    if (dist < half_width) {
        // Inside bar
        let polarity = get_strobe_polarity();
        if (uniforms.carrier_type == 0) {
            return solid_carrier(polarity);
        }
        return checkerboard_cartesian(uv_to_pattern_position(uv), polarity);
    }
    return uniforms.background_luminance;
}

fn wedge_envelope(uv: vec2<f32>) -> f32 {
    let polar = uv_to_polar(uv);
    let eccentricity = polar.x;
    let polar_angle = normalize_angle(polar.y - uniforms.rotation_deg);

    // Gradual seam: wedge enters at start and exits at end of rotation.
    // The wedge travels from 0 to (360 + stimulus_width_deg), so it
    // gradually appears at the seam and gradually disappears.
    let raw_rotation = uniforms.progress * 360.0;
    let total_travel = 360.0 + uniforms.stimulus_width_deg;
    let leading_edge_travel = raw_rotation * (total_travel / 360.0);
    let leading_edge = leading_edge_travel;
    let trailing_edge = leading_edge - uniforms.stimulus_width_deg;

    // Calculate pixel's angular distance from seam (0 degrees = right side)
    var angle_from_seam: f32;
    if (uniforms.direction == 0) {
        // CW: measure clockwise from seam
        angle_from_seam = -polar_angle;
    } else {
        // CCW: measure counter-clockwise from seam
        angle_from_seam = polar_angle;
    }
    angle_from_seam = normalize_angle_positive(angle_from_seam);

    // Pixel is inside if between trailing and leading edges
    let inside_wedge = (angle_from_seam >= trailing_edge && angle_from_seam < leading_edge);

    if (inside_wedge) {
        let polarity = get_strobe_polarity();
        if (uniforms.carrier_type == 0) {
            return solid_carrier(polarity);
        }
        return checkerboard_polar(eccentricity, polar.y, polarity);
    }
    return uniforms.background_luminance;
}

fn ring_envelope(uv: vec2<f32>) -> f32 {
    let polar = uv_to_polar(uv);
    let eccentricity = polar.x;
    let polar_angle = normalize_angle(polar.y - uniforms.rotation_deg);

    let half_width = uniforms.stimulus_width_deg * 0.5;
    let total_travel = uniforms.max_eccentricity_deg + uniforms.stimulus_width_deg;

    // Ring center from progress and direction (0=expand, 1=contract)
    var ring_center: f32;
    if (uniforms.direction == 0) {
        // Expand: start from center outward
        ring_center = -half_width + uniforms.progress * total_travel;
    } else {
        // Contract: start from periphery inward
        ring_center = uniforms.max_eccentricity_deg + half_width - uniforms.progress * total_travel;
    }

    let inner_edge = max(0.0, ring_center - half_width);
    let outer_edge = ring_center + half_width;

    if (eccentricity >= inner_edge && eccentricity < outer_edge) {
        // Inside ring
        let polarity = get_strobe_polarity();
        if (uniforms.carrier_type == 0) {
            return solid_carrier(polarity);
        }
        return checkerboard_polar(eccentricity, polar.y, polarity);
    }
    return uniforms.background_luminance;
}

// ---------------------------------------------------------------------------
// Fragment shader entry point
// ---------------------------------------------------------------------------

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Apply physical monitor rotation to compensate for mounted orientation.
    // Rotates UV around screen center so the mouse sees the correct stimulus.
    var uv = in.uv;
    if (uniforms.monitor_rotation_deg != 0.0) {
        let centered = uv - vec2<f32>(0.5, 0.5);
        let rotated = rotate_point(centered, uniforms.monitor_rotation_deg);
        uv = rotated + vec2<f32>(0.5, 0.5);
    }

    var lum: f32;
    if (uniforms.envelope_type == 1) {
        lum = bar_envelope(uv);
    } else if (uniforms.envelope_type == 2) {
        lum = wedge_envelope(uv);
    } else if (uniforms.envelope_type == 3) {
        lum = ring_envelope(uv);
    } else {
        lum = fullfield_envelope(uv);
    }
    lum = clamp(lum, 0.0, 1.0);
    return vec4<f32>(lum, lum, lum, 1.0);
}
