//! wgpu-based stimulus renderer.
//!
//! Renders stimulus patterns (bars, wedges, rings, fullfield) to a caller-provided
//! surface using a fullscreen-triangle approach with a uniform buffer driving the
//! WGSL shader.

use bytemuck::{self, Zeroable};
use glyphon::{
    Attrs, Buffer as TextBuffer, Cache as GlyphonCache, Color as TextColor, Family, FontSystem,
    Metrics, Resolution, Shaping, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer,
    Viewport,
};

// ---------------------------------------------------------------------------
// Uniform buffer (must match shaders/stimulus.wgsl exactly)
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct StimulusUniforms {
    // Display geometry (offset 0)
    pub visual_field_deg: [f32; 2],
    pub projection_type: i32,
    pub viewing_distance_cm: f32,
    pub display_size_cm: [f32; 2],
    pub center_offset_deg: [f32; 2],

    // Carrier (offset 32)
    pub carrier_type: i32,
    pub check_size_deg: f32,
    pub check_size_cm: f32,
    pub contrast: f32,
    pub mean_luminance: f32,
    pub luminance_high: f32,
    pub luminance_low: f32,

    // Modulation (offset 60)
    pub strobe_frequency_hz: f32,

    // Timing (offset 64)
    pub time_sec: f32,

    // Envelope (offset 68)
    pub envelope_type: i32,
    pub stimulus_width_deg: f32,
    pub progress: f32,
    pub direction: i32,
    pub rotation_deg: f32,
    pub background_luminance: f32,
    pub max_eccentricity_deg: f32,

    // Monitor physical rotation (offset 96)
    /// Physical rotation of the monitor around the viewing axis, in degrees.
    /// Applied to UV coordinates to compensate for mounted orientation.
    pub monitor_rotation_deg: f32,
    /// Draw 1-cm-spaced calibration ticks along the monitor's four borders,
    /// centered on the monitor geometric center. 0.0 = off, 1.0 = on.
    /// Enabled during preview / disabled during acquisition.
    pub show_ticks: f32,

    // Stimulus angular envelope (offset 104). Declared sweep extent in
    // degrees (Zhuang 140° az × 110° alt canonical) and the visual-space
    // center the envelope is anchored on. The shader uses these to bound
    // the drifting bar's sweep AND to discard pixels outside the
    // declared FOV — so the rendered stimulus actually matches the
    // `azi_angular_range`/`alt_angular_range` recorded in the .oisi file.
    pub swept_range_deg: [f32; 2],
    pub swept_center_deg: [f32; 2],

    // Bisector intercept (offset 120) in cm from the monitor geometric
    // center. Shifts the cm origin used by the pixel→angle transform so
    // the bisector intercept lands at visual angle (0, 0). Mirrors
    // Zhuang's `C2A_cm`/`C2T_cm` (anterior/top offsets to the bisector).
    pub bisector_cm: [f32; 2],

    // FOV envelope as a flat rectangle in monitor cm coords (offset 128,
    // already 16-aligned). (y_min, y_max, z_min, z_max) — the axis-aligned
    // bounding box of the four visual-space FOV corners projected to
    // the monitor face. Used to mask the stimulus to a post-transform
    // rectangle while the drawn outline lines still show the true
    // curved envelope.
    pub fov_mask_cm: [f32; 4],
}

// Compile-time size check: 128 + 16 = 144, multiple of 16.
const _: () = assert!(std::mem::size_of::<StimulusUniforms>() == 144);

// ---------------------------------------------------------------------------
// Configuration (static per-trial values)
// ---------------------------------------------------------------------------

pub struct RendererConfig {
    // Display geometry
    pub visual_field_width_deg: f32,
    pub visual_field_height_deg: f32,
    pub projection_type: i32,
    pub viewing_distance_cm: f32,
    pub display_width_cm: f32,
    pub display_height_cm: f32,
    pub center_azimuth_deg: f32,
    pub center_elevation_deg: f32,

    // Carrier
    pub carrier_type: i32, // 0=solid, 1=checkerboard
    pub check_size_deg: f32,
    pub check_size_cm: f32,
    pub contrast: f32,
    pub mean_luminance: f32,
    pub luminance_high: f32,
    pub luminance_low: f32,

    // Modulation
    pub strobe_frequency_hz: f32,

    // Envelope
    pub envelope_type: i32, // 0=fullfield, 1=bar, 2=wedge, 3=ring
    pub stimulus_width_deg: f32,
    pub rotation_deg: f32,
    pub background_luminance: f32,
    pub max_eccentricity_deg: f32,

    /// Physical rotation of the stimulus monitor (degrees around viewing axis).
    /// 0 = normal, 180 = upside down. Applied to UV before stimulus computation.
    pub monitor_rotation_deg: f32,

    /// Enable the 1-cm-spaced calibration tick overlay (preview only).
    pub show_ticks: bool,

    /// Declared sweep envelope (degrees): width × height of the angular
    /// region the stimulus is presented in. Plumbed from the experiment's
    /// `AziAngularRange` / `AltAngularRange` params. Used by `bar_envelope`
    /// to set sweep extent, and by all envelopes to mask pixels outside
    /// the declared FOV.
    pub swept_range_width_deg: f32,
    pub swept_range_height_deg: f32,

    /// Visual-space center of the swept envelope (degrees). Plumbed from
    /// `OffsetAzi` / `OffsetAlt`. The FOV mask discards pixels where
    /// |az - swept_center.x| > swept_range.x/2 or
    /// |el - swept_center.y| > swept_range.y/2.
    pub swept_center_az_deg: f32,
    pub swept_center_el_deg: f32,

    /// Bisector intercept on the monitor face in cm from monitor geometric
    /// center (+x = anterior, +y = top). Plumbed from `BisectorXCm`/
    /// `BisectorYCm`. The shader subtracts these from the cm coordinates
    /// of each pixel BEFORE the pixel→angle transform, so the bisector
    /// intercept sits at visual angle (0, 0).
    pub bisector_x_cm: f32,
    pub bisector_y_cm: f32,

    /// Axis-aligned bounding box (cm from monitor geometric center) of
    /// the four FOV-envelope corners after spherical projection. Pixels
    /// outside this rectangle are masked to background_luminance.
    /// Computed in stimulus_thread.rs from `DisplayGeometry::angle_to_uv`.
    pub fov_mask_y_min_cm: f32,
    pub fov_mask_y_max_cm: f32,
    pub fov_mask_z_min_cm: f32,
    pub fov_mask_z_max_cm: f32,
}

// ---------------------------------------------------------------------------
// Renderer
// ---------------------------------------------------------------------------

pub struct StimulusRenderer {
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    uniforms: StimulusUniforms,
    /// Anti-aliased TTF text rendering for the preview calibration
    /// labels. cosmic-text + glyphon discover system fonts automatically.
    font_system: FontSystem,
    swash_cache: SwashCache,
    #[allow(dead_code)] // Kept alive: text_viewport/atlas hold references to its GPU state.
    glyphon_cache: GlyphonCache,
    text_viewport: Viewport,
    text_atlas: TextAtlas,
    text_renderer: TextRenderer,
    surface_width_px: u32,
    surface_height_px: u32,
}

impl StimulusRenderer {
    /// Create a new renderer.
    ///
    /// The caller owns the device and surface; this struct only borrows them
    /// long enough to build GPU objects.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        surface_width_px: u32,
        surface_height_px: u32,
    ) -> Self {
        // Shader -----------------------------------------------------------
        let shader_src = include_str!("shaders/stimulus.wgsl");
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stimulus_shader"),
            source: wgpu::ShaderSource::Wgsl(shader_src.into()),
        });

        // Uniform buffer ---------------------------------------------------
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stimulus_uniforms"),
            size: std::mem::size_of::<StimulusUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Bind group layout & bind group -----------------------------------
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("stimulus_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stimulus_bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // Pipeline ---------------------------------------------------------
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("stimulus_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stimulus_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Text renderer for calibration labels --------------------------
        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let glyphon_cache = GlyphonCache::new(device);
        let mut text_viewport = Viewport::new(device, &glyphon_cache);
        text_viewport.update(
            queue,
            Resolution {
                width: surface_width_px,
                height: surface_height_px,
            },
        );
        let mut text_atlas = TextAtlas::new(device, queue, &glyphon_cache, surface_format);
        let text_renderer = TextRenderer::new(
            &mut text_atlas,
            device,
            wgpu::MultisampleState::default(),
            None,
        );

        Self {
            pipeline,
            uniform_buffer,
            bind_group,
            uniforms: StimulusUniforms::zeroed(),
            font_system,
            swash_cache,
            glyphon_cache,
            text_viewport,
            text_atlas,
            text_renderer,
            surface_width_px,
            surface_height_px,
        }
    }

    /// Inform the text renderer of a new surface size. Call after a
    /// surface reconfigure so labels stay at the correct pixel positions.
    pub fn resize(&mut self, queue: &wgpu::Queue, width_px: u32, height_px: u32) {
        self.surface_width_px = width_px;
        self.surface_height_px = height_px;
        self.text_viewport.update(
            queue,
            Resolution {
                width: width_px,
                height: height_px,
            },
        );
    }

    /// Apply static per-trial configuration to the uniform buffer.
    pub fn configure(&mut self, config: &RendererConfig) {
        self.uniforms.visual_field_deg = [
            config.visual_field_width_deg,
            config.visual_field_height_deg,
        ];
        self.uniforms.projection_type = config.projection_type;
        self.uniforms.viewing_distance_cm = config.viewing_distance_cm;
        self.uniforms.display_size_cm = [config.display_width_cm, config.display_height_cm];
        self.uniforms.center_offset_deg = [config.center_azimuth_deg, config.center_elevation_deg];

        self.uniforms.carrier_type = config.carrier_type;
        self.uniforms.check_size_deg = config.check_size_deg;
        self.uniforms.check_size_cm = config.check_size_cm;
        self.uniforms.contrast = config.contrast;
        self.uniforms.mean_luminance = config.mean_luminance;
        self.uniforms.luminance_high = config.luminance_high;
        self.uniforms.luminance_low = config.luminance_low;

        self.uniforms.strobe_frequency_hz = config.strobe_frequency_hz;

        self.uniforms.envelope_type = config.envelope_type;
        self.uniforms.stimulus_width_deg = config.stimulus_width_deg;
        self.uniforms.rotation_deg = config.rotation_deg;
        self.uniforms.background_luminance = config.background_luminance;
        self.uniforms.max_eccentricity_deg = config.max_eccentricity_deg;

        self.uniforms.monitor_rotation_deg = config.monitor_rotation_deg;
        self.uniforms.show_ticks = if config.show_ticks { 1.0 } else { 0.0 };

        self.uniforms.swept_range_deg =
            [config.swept_range_width_deg, config.swept_range_height_deg];
        self.uniforms.swept_center_deg = [config.swept_center_az_deg, config.swept_center_el_deg];
        self.uniforms.bisector_cm = [config.bisector_x_cm, config.bisector_y_cm];
        self.uniforms.fov_mask_cm = [
            config.fov_mask_y_min_cm,
            config.fov_mask_y_max_cm,
            config.fov_mask_z_min_cm,
            config.fov_mask_z_max_cm,
        ];
    }

    /// Update per-frame dynamic uniforms and write the buffer to the GPU.
    pub fn update_frame(
        &mut self,
        queue: &wgpu::Queue,
        progress: f32,
        direction: i32,
        time_sec: f32,
    ) {
        self.uniforms.progress = progress;
        self.uniforms.direction = direction;
        self.uniforms.time_sec = time_sec;
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&self.uniforms));
    }

    /// Get the current monitor rotation uniform value.
    pub fn get_monitor_rotation(&self) -> f32 {
        self.uniforms.monitor_rotation_deg
    }

    /// Set the monitor rotation and write to GPU immediately.
    pub fn set_monitor_rotation(&mut self, queue: &wgpu::Queue, deg: f32) {
        self.uniforms.monitor_rotation_deg = deg;
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&self.uniforms));
    }

    /// Execute a render pass, drawing the fullscreen stimulus triangle.
    /// If `show_ticks` is enabled, also overlays anti-aliased calibration
    /// labels at each 5-cm major-tick position along the four monitor
    /// borders.
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, view: &wgpu::TextureView) {
        // Build label buffers + areas for the preview overlay if enabled.
        let label_buffers: Vec<(TextBuffer, f32, f32)> = if self.uniforms.show_ticks > 0.5 {
            self.build_label_buffers()
        } else {
            Vec::new()
        };

        // Prepare text renderer.
        if !label_buffers.is_empty() {
            let areas: Vec<TextArea<'_>> = label_buffers
                .iter()
                .map(|(buf, left, top)| TextArea {
                    buffer: buf,
                    left: *left,
                    top: *top,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: 0,
                        top: 0,
                        right: self.surface_width_px as i32,
                        bottom: self.surface_height_px as i32,
                    },
                    default_color: TextColor::rgba(255, 255, 255, 255),
                    custom_glyphs: &[],
                })
                .collect();

            if let Err(e) = self.text_renderer.prepare(
                device,
                queue,
                &mut self.font_system,
                &mut self.text_atlas,
                &self.text_viewport,
                areas,
                &mut self.swash_cache,
            ) {
                tracing::error!(error = %e, "label prepare failed");
            }
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("stimulus_render"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("stimulus_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1); // Fullscreen triangle, 3 vertices

            if !label_buffers.is_empty()
                && let Err(e) =
                    self.text_renderer
                        .render(&self.text_atlas, &self.text_viewport, &mut pass)
            {
                tracing::error!(error = %e, "label render failed");
            }
        }
        queue.submit(std::iter::once(encoder.finish()));
    }

    /// Build one cosmic-text `Buffer` per major-tick label. Returns
    /// `(buffer, left_px, top_px)` triples. The cm grid is centered on
    /// the monitor geometric center; labels read e.g. "+5", "-10", "+25".
    /// Independent of the bisector intercept (the ticks describe the
    /// physical panel, not the eye's gaze projection).
    fn build_label_buffers(&mut self) -> Vec<(TextBuffer, f32, f32)> {
        let w_cm = self.uniforms.display_size_cm[0];
        let h_cm = self.uniforms.display_size_cm[1];
        if w_cm <= 0.0 || h_cm <= 0.0 {
            return Vec::new();
        }
        let w_px = self.surface_width_px as f32;
        let h_px = self.surface_height_px as f32;
        let px_per_cm_x = w_px / w_cm;
        let px_per_cm_y = h_px / h_cm;

        // ~0.6 cm tall text, scaled to physical cm so labels are visually
        // the same size across monitors of different pixel densities.
        let text_height_cm: f32 = 0.6;
        let font_size_px = text_height_cm * px_per_cm_y;
        // Pad between tick end and label box.
        let major_len_cm: f32 = 1.2;
        let gap_cm: f32 = 0.25;
        let inset_y_px = (major_len_cm + gap_cm) * px_per_cm_y;
        let inset_x_px = (major_len_cm + gap_cm) * px_per_cm_x;

        let half_w_cm = w_cm * 0.5;
        let half_h_cm = h_cm * 0.5;

        let metrics = Metrics::new(font_size_px, font_size_px * 1.2);
        let attrs = Attrs::new().family(Family::SansSerif);
        // Allow each label up to ~3 cm wide (enough for "-25").
        let box_w = (3.0 * px_per_cm_x).max(font_size_px * 3.0);
        let box_h = font_size_px * 1.4;

        let make_buf = |fs: &mut FontSystem, text: &str| -> (TextBuffer, f32, f32) {
            let mut buf = TextBuffer::new(fs, metrics);
            buf.set_size(fs, Some(box_w), Some(box_h));
            buf.set_text(fs, text, attrs, Shaping::Advanced);
            buf.shape_until_scroll(fs, false);
            // Measure: width of the first laid-out line.
            let measured_w = buf
                .layout_runs()
                .next()
                .map(|run| run.line_w)
                .unwrap_or(0.0);
            (buf, measured_w, box_h)
        };

        let mut out: Vec<(TextBuffer, f32, f32)> = Vec::new();

        // ── Horizontal-edge labels (top + bottom): y_cm major positions ─
        let mut y = -((half_w_cm / 5.0).floor() as i32) * 5;
        while (y as f32) <= half_w_cm {
            if y != 0 {
                let label = format_signed(y);
                let (buf, measured_w, _) = make_buf(&mut self.font_system, &label);
                let center_x_px = (y as f32 + half_w_cm) * px_per_cm_x;
                let left = center_x_px - measured_w * 0.5;
                // Top edge.
                out.push((
                    clone_buffer(
                        &buf,
                        &mut self.font_system,
                        &label,
                        metrics,
                        attrs,
                        box_w,
                        box_h,
                    ),
                    left,
                    inset_y_px,
                ));
                // Bottom edge.
                let bottom_top = h_px - inset_y_px - font_size_px;
                out.push((buf, left, bottom_top));
            }
            y += 5;
        }

        // ── Vertical-edge labels (left + right): z_cm major positions ───
        let mut z = -((half_h_cm / 5.0).floor() as i32) * 5;
        while (z as f32) <= half_h_cm {
            if z != 0 {
                let label = format_signed(z);
                let (buf, measured_w, _) = make_buf(&mut self.font_system, &label);
                // +z = upward → smaller screen y.
                let center_y_px = (half_h_cm - z as f32) * px_per_cm_y;
                let top = center_y_px - font_size_px * 0.5;
                // Left edge: text starts at inset_x_px.
                out.push((
                    clone_buffer(
                        &buf,
                        &mut self.font_system,
                        &label,
                        metrics,
                        attrs,
                        box_w,
                        box_h,
                    ),
                    inset_x_px,
                    top,
                ));
                // Right edge: text ends at (w_px - inset_x_px).
                let right_left = w_px - inset_x_px - measured_w;
                out.push((buf, right_left, top));
            }
            z += 5;
        }

        out
    }
}

fn format_signed(v: i32) -> String {
    if v > 0 {
        format!("+{v}")
    } else {
        format!("{v}")
    }
}

fn clone_buffer(
    _src: &TextBuffer,
    fs: &mut FontSystem,
    text: &str,
    metrics: Metrics,
    attrs: Attrs<'_>,
    box_w: f32,
    box_h: f32,
) -> TextBuffer {
    let mut buf = TextBuffer::new(fs, metrics);
    buf.set_size(fs, Some(box_w), Some(box_h));
    buf.set_text(fs, text, attrs, Shaping::Advanced);
    buf.shape_until_scroll(fs, false);
    buf
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert a direction string to the integer the shader expects.
///
/// - BAR:   "LR"=0, "RL"=1, "TB"=2, "BT"=3
/// - WEDGE: "CW"=0, "CCW"=1
/// - RING:  "expand"=0, "contract"=1
///
/// Returns `None` for unrecognized direction strings. Callers that
/// receive `None` should treat it as a programmer error at the
/// sequencer (the renderer can fall back to a neutral value).
pub fn direction_to_int(direction: &str) -> Option<i32> {
    match direction {
        "LR" => Some(0),
        "RL" => Some(1),
        "TB" => Some(2),
        "BT" => Some(3),
        "CW" => Some(0),
        "CCW" => Some(1),
        "Expand" | "expand" => Some(0),
        "Contract" | "contract" => Some(1),
        "On" => Some(0),
        "" => Some(0), // No direction during baseline/idle — renderer shows gray, value unused.
        _ => None,
    }
}
