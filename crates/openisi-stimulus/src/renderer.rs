//! wgpu-based stimulus renderer.
//!
//! Renders stimulus patterns (bars, wedges, rings, fullfield) to a caller-provided
//! surface using a fullscreen-triangle approach with a uniform buffer driving the
//! WGSL shader.

use bytemuck::{self, Zeroable};

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
    pub _pad0: f32,
}

// Compile-time size check: 96 + 8 = 104, multiple of 8 (struct alignment).
const _: () = assert!(std::mem::size_of::<StimulusUniforms>() == 104);

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
}

// ---------------------------------------------------------------------------
// Renderer
// ---------------------------------------------------------------------------

pub struct StimulusRenderer {
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    uniforms: StimulusUniforms,
}

impl StimulusRenderer {
    /// Create a new renderer.
    ///
    /// The caller owns the device and surface; this struct only borrows them
    /// long enough to build GPU objects.
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
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

        Self {
            pipeline,
            uniform_buffer,
            bind_group,
            uniforms: StimulusUniforms::zeroed(),
        }
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
        self.uniforms.center_offset_deg = [
            config.center_azimuth_deg,
            config.center_elevation_deg,
        ];

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
    pub fn render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
    ) {
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
        }
        queue.submit(std::iter::once(encoder.finish()));
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert a direction string to the integer the shader expects.
///
/// - BAR:   "LR"=0, "RL"=1, "TB"=2, "BT"=3
/// - WEDGE: "CW"=0, "CCW"=1
/// - RING:  "expand"=0, "contract"=1
pub fn direction_to_int(direction: &str) -> i32 {
    match direction {
        "LR" => 0,
        "RL" => 1,
        "TB" => 2,
        "BT" => 3,
        "CW" => 0,
        "CCW" => 1,
        "Expand" | "expand" => 0,
        "Contract" | "contract" => 1,
        "On" => 0,
        "" => 0, // No direction during baseline/idle — renderer shows gray, value unused.
        _ => panic!("Invalid direction: {direction}"),
    }
}
