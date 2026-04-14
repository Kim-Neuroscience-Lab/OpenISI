//! Stimulus thread — raw win32 window + wgpu + DXGI WaitForVBlank.
//!
//! Runs on its own thread, communicates via crossbeam channels.
//! Owns a fullscreen window on the stimulus monitor and renders at vsync rate.
//!
//! On non-Windows platforms, the stimulus thread reports that hardware stimulus
//! display is not available and waits for shutdown.

#[cfg(not(windows))]
use crossbeam_channel::{Receiver, Sender};
#[cfg(not(windows))]
use crate::messages::{StimulusCmd, StimulusEvt};

#[cfg(windows)]
use std::num::NonZeroIsize;
#[cfg(windows)]
use std::time::{Duration, Instant};

#[cfg(windows)]
use crossbeam_channel::{Receiver, Sender};
#[cfg(windows)]
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle,
};
#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
#[cfg(windows)]
use windows::Win32::Graphics::Dxgi::IDXGIOutput;
#[cfg(windows)]
use windows::Win32::Graphics::Dwm::{DwmGetCompositionTimingInfo, DWM_TIMING_INFO};
#[cfg(windows)]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(windows)]
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::*;

#[cfg(windows)]
use openisi_stimulus::dataset::{DatasetConfig, FrameRecord, FrameState, StimulusDataset};
#[cfg(windows)]
use openisi_stimulus::geometry::DisplayGeometry;
#[cfg(windows)]
use openisi_stimulus::renderer::{direction_to_int, RendererConfig, StimulusRenderer};
#[cfg(windows)]
use openisi_stimulus::sequencer::{Sequencer, SequencerConfig};

#[cfg(windows)]
use crate::config::{Envelope, Experiment, RigGeometry};
#[cfg(windows)]
use crate::messages::{
    AcquisitionCommand, AcquisitionResult, StimulusCmd, StimulusEvt,
    StimulusFrameRecord, StimulusPreviewFrame,
};
#[cfg(windows)]
use crate::monitor::find_dxgi_output;

// =============================================================================
// Non-Windows stub
// =============================================================================

#[cfg(not(windows))]
pub fn run(
    cmd_rx: Receiver<StimulusCmd>,
    evt_tx: Sender<StimulusEvt>,
    _monitor_index: usize,
    _monitor_width_px: u32,
    _monitor_height_px: u32,
    _monitor_position: (i32, i32),
    _system_cfg: crate::config::SystemTuning,
    _initial_bg_luminance: f64,
) {
    eprintln!("[stimulus_thread] Stimulus display is not available on this platform (requires Windows)");
    let _ = evt_tx.send(StimulusEvt::Error(
        "Stimulus display requires Windows (Win32 + DXGI)".into(),
    ));
    // Wait for shutdown command so the thread doesn't exit prematurely.
    loop {
        match cmd_rx.recv() {
            Ok(StimulusCmd::Shutdown) | Err(_) => return,
            _ => {
                let _ = evt_tx.send(StimulusEvt::Error(
                    "Stimulus display is not available on this platform".into(),
                ));
            }
        }
    }
}

// =============================================================================
// Windows implementation
// =============================================================================

// =============================================================================
// QPC helpers
// =============================================================================

#[cfg(windows)]
fn qpc_to_us(qpc: i64, freq: i64) -> i64 {
    ((qpc as i128 * 1_000_000) / freq as i128) as i64
}

#[cfg(windows)]
fn query_qpc() -> i64 {
    let mut qpc = 0i64;
    unsafe {
        let _ = QueryPerformanceCounter(&mut qpc);
    }
    qpc
}

#[cfg(windows)]
fn query_dwm_vsync() -> Option<(i64, u64)> {
    unsafe {
        let mut info: DWM_TIMING_INFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<DWM_TIMING_INFO>() as u32;
        // HWND(null) = desktop window = get compositor-wide timing
        if DwmGetCompositionTimingInfo(HWND(std::ptr::null_mut()), &mut info).is_ok() {
            Some((info.qpcVBlank as i64, info.cFrameDisplayed))
        } else {
            None
        }
    }
}

// =============================================================================
// Win32 window
// =============================================================================

#[cfg(windows)]
unsafe extern "system" fn stimulus_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(windows)]
fn create_fullscreen_window(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(HWND, isize), String> {
    unsafe {
        let hmodule = GetModuleHandleW(None).map_err(|e| format!("GetModuleHandleW: {e}"))?;
        let hinstance = windows::Win32::Foundation::HINSTANCE(hmodule.0);

        let class_name: Vec<u16> = "OpenISI_Stimulus\0".encode_utf16().collect();

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(stimulus_wnd_proc),
            hInstance: hinstance,
            lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };

        // Returns 0 if already registered — that's fine
        RegisterClassW(&wc);

        let title: Vec<u16> = "OpenISI Stimulus\0".encode_utf16().collect();

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            windows::core::PCWSTR(class_name.as_ptr()),
            windows::core::PCWSTR(title.as_ptr()),
            WS_POPUP | WS_VISIBLE,
            x,
            y,
            width as i32,
            height as i32,
            None,
            None,
            Some(hinstance),
            None,
        )
        .map_err(|e| format!("CreateWindowExW: {e}"))?;

        Ok((hwnd, hinstance.0 as isize))
    }
}

#[cfg(windows)]
fn create_wgpu_surface(
    instance: &wgpu::Instance,
    hwnd: HWND,
    hinstance: isize,
) -> Result<wgpu::Surface<'static>, String> {
    let hwnd_isize = hwnd.0 as isize;
    let hwnd_nz = NonZeroIsize::new(hwnd_isize).ok_or("HWND is null")?;
    let mut win32_handle = Win32WindowHandle::new(hwnd_nz);
    win32_handle.hinstance = NonZeroIsize::new(hinstance);

    let raw_window = RawWindowHandle::Win32(win32_handle);
    let raw_display = RawDisplayHandle::Windows(WindowsDisplayHandle::new());

    let target = wgpu::SurfaceTargetUnsafe::RawHandle {
        raw_display_handle: raw_display,
        raw_window_handle: raw_window,
    };

    unsafe {
        instance
            .create_surface_unsafe(target)
            .map_err(|e| format!("create_surface_unsafe: {e}"))
    }
}

#[cfg(windows)]
/// Pump win32 messages. Returns false if WM_QUIT received.
fn pump_messages() -> bool {
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                return false;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        true
    }
}

// =============================================================================
// Rendering (Phase 2: solid color only)
// =============================================================================

#[cfg(windows)]
fn render_clear(
    surface: &wgpu::Surface,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    color: wgpu::Color,
) -> Result<(), String> {
    let frame = surface
        .get_current_texture()
        .map_err(|e| format!("get_current_texture: {e}"))?;
    let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
    render_clear_view(device, queue, &view, color);
    frame.present();
    Ok(())
}

#[cfg(windows)]
fn render_clear_view(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &wgpu::TextureView,
    color: wgpu::Color,
) {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("stimulus_clear"),
    });
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("stimulus_clear_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }
    queue.submit(std::iter::once(encoder.finish()));
}

// =============================================================================
// Preview frame capture
// =============================================================================

#[cfg(windows)]
/// Render the current stimulus to the small preview texture (without monitor rotation),
/// copy to staging buffer, read back pixels, and send as PreviewFrame event.
fn capture_preview_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut StimulusRenderer,
    preview_view: &wgpu::TextureView,
    preview_texture: &wgpu::Texture,
    staging_buffer: &wgpu::Buffer,
    preview_w: u32,
    preview_h: u32,
    bytes_per_row: u32,
    is_baseline: bool,
    bg_luminance: f32,
    evt_tx: &Sender<StimulusEvt>,
) {
    if is_baseline {
        let bg = wgpu::Color { r: bg_luminance as f64, g: bg_luminance as f64, b: bg_luminance as f64, a: 1.0 };
        render_clear_view(device, queue, preview_view, bg);
    } else {
        // Temporarily disable monitor rotation for preview (show mouse's perspective)
        let saved_rotation = renderer.get_monitor_rotation();
        if saved_rotation != 0.0 {
            renderer.set_monitor_rotation(queue, 0.0);
        }
        renderer.render(device, queue, preview_view);
        if saved_rotation != 0.0 {
            renderer.set_monitor_rotation(queue, saved_rotation);
        }
    }

    // Copy texture to staging buffer
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("preview_copy"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: preview_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: staging_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(preview_h),
            },
        },
        wgpu::Extent3d { width: preview_w, height: preview_h, depth_or_array_layers: 1 },
    );
    queue.submit(std::iter::once(encoder.finish()));

    // Map and read back
    let buffer_slice = staging_buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device.poll(wgpu::Maintain::Wait);

    if let Ok(Ok(())) = rx.recv() {
        let data = buffer_slice.get_mapped_range();
        // Extract packed RGBA pixels (remove row padding)
        let row_bytes = (preview_w * 4) as usize;
        let mut rgba = Vec::with_capacity(row_bytes * preview_h as usize);
        for row in 0..preview_h as usize {
            let start = row * bytes_per_row as usize;
            rgba.extend_from_slice(&data[start..start + row_bytes]);
        }
        drop(data);
        staging_buffer.unmap();

        let _ = evt_tx.send(StimulusEvt::PreviewFrame(StimulusPreviewFrame {
            rgba_pixels: rgba,
            width: preview_w,
            height: preview_h,
        }));
    } else {
        staging_buffer.unmap();
    }
}

// Note: config::Projection IS ProjectionType and config::Envelope IS EnvelopeType
// (re-exported from the stimulus crate), so no conversion helpers are needed.

// =============================================================================
// Config mapping
// =============================================================================

#[cfg(windows)]
fn build_sequencer_config(cfg: &AcquisitionCommand) -> SequencerConfig {
    // Compute sweep duration from stimulus parameters and geometry.
    let sweep_duration_sec = compute_sweep_duration(cfg);

    SequencerConfig {
        conditions: cfg.experiment.presentation.conditions.clone(),
        repetitions: cfg.experiment.presentation.repetitions,
        order: cfg.experiment.presentation.order,
        baseline_start_sec: cfg.experiment.timing.baseline_start_sec,
        baseline_end_sec: cfg.experiment.timing.baseline_end_sec,
        inter_stimulus_sec: cfg.experiment.timing.inter_stimulus_sec,
        inter_direction_sec: cfg.experiment.timing.inter_direction_sec,
        sweep_duration_sec,
    }
}

#[cfg(windows)]
fn compute_sweep_duration(cfg: &AcquisitionCommand) -> f64 {
    let params = &cfg.experiment.stimulus.params;
    let envelope = cfg.experiment.stimulus.envelope;

    let geometry = DisplayGeometry::new(
        cfg.experiment.geometry.projection,
        cfg.geometry.viewing_distance_cm,
        cfg.experiment.geometry.horizontal_offset_deg,
        cfg.experiment.geometry.vertical_offset_deg,
        cfg.monitor.width_cm,
        cfg.monitor.height_cm,
        cfg.monitor.width_px,
        cfg.monitor.height_px,
    );

    match envelope {
        Envelope::Fullfield => {
            // Fullfield: no sweep, duration determined by timing config
            0.0
        }
        Envelope::Bar => {
            // Bar: total_travel = visual_field_width + stimulus_width
            let vf_width = geometry.visual_field_width_deg();
            let total_travel = vf_width + params.stimulus_width_deg;
            total_travel / params.sweep_speed_deg_per_sec
        }
        Envelope::Wedge => {
            // Wedge: full rotation
            360.0 / params.rotation_speed_deg_per_sec
        }
        Envelope::Ring => {
            // Ring: sweep from center to max eccentricity
            let max_ecc = geometry.get_max_eccentricity_deg();
            let total_travel = max_ecc + params.stimulus_width_deg;
            total_travel / params.expansion_speed_deg_per_sec
        }
    }
}

#[cfg(windows)]
fn build_dataset_config(cfg: &AcquisitionCommand) -> DatasetConfig {
    use std::collections::HashMap;

    let envelope = cfg.experiment.stimulus.envelope;

    let geometry = DisplayGeometry::new(
        cfg.experiment.geometry.projection,
        cfg.geometry.viewing_distance_cm,
        cfg.experiment.geometry.horizontal_offset_deg,
        cfg.experiment.geometry.vertical_offset_deg,
        cfg.monitor.width_cm,
        cfg.monitor.height_cm,
        cfg.monitor.width_px,
        cfg.monitor.height_px,
    );

    // Serialize stimulus params to HashMap for dataset metadata
    let stimulus_params: HashMap<String, serde_json::Value> = {
        let json_val = serde_json::to_value(&cfg.experiment.stimulus.params)
            .expect("StimulusParams must serialize to JSON");
        serde_json::from_value(json_val)
            .expect("StimulusParams JSON must deserialize to HashMap")
    };

    DatasetConfig {
        envelope,
        stimulus_params,
        conditions: cfg.experiment.presentation.conditions.clone(),
        repetitions: cfg.experiment.presentation.repetitions,
        order: cfg.experiment.presentation.order,
        baseline_start_sec: cfg.experiment.timing.baseline_start_sec,
        baseline_end_sec: cfg.experiment.timing.baseline_end_sec,
        inter_stimulus_sec: cfg.experiment.timing.inter_stimulus_sec,
        inter_direction_sec: cfg.experiment.timing.inter_direction_sec,
        sweep_duration_sec: compute_sweep_duration(cfg),
        geometry,
        display_physical_source: cfg.monitor.physical_source.clone(),
        reported_refresh_hz: cfg.monitor.refresh_hz as f64,
        measured_refresh_hz: cfg.measured_refresh_hz,
        target_stimulus_fps: cfg.display.target_stimulus_fps,
        drop_detection_warmup_frames: cfg.system.drop_detection_warmup_frames,
        drop_detection_threshold: cfg.system.drop_detection_threshold,
        fps_window_frames: cfg.system.fps_window_frames,
    }
}

#[cfg(windows)]
/// Build renderer config for preview mode.
/// Requires a selected monitor — preview cannot run without display geometry.
/// Preview shows the mouse's perspective (no monitor rotation applied).
fn build_preview_renderer_config(
    experiment: &Experiment,
    rig_geometry: &RigGeometry,
    monitor: &crate::session::MonitorInfo,
) -> RendererConfig {
    let (w_cm, h_cm, w_px, h_px) = (monitor.width_cm, monitor.height_cm, monitor.width_px, monitor.height_px);

    let geometry = DisplayGeometry::new(
        experiment.geometry.projection,
        rig_geometry.viewing_distance_cm,
        experiment.geometry.horizontal_offset_deg,
        experiment.geometry.vertical_offset_deg,
        w_cm,
        h_cm,
        w_px,
        h_px,
    );

    let params = &experiment.stimulus.params;
    let max_ecc = geometry.get_max_eccentricity_deg() as f32;

    RendererConfig {
        visual_field_width_deg: geometry.visual_field_width_deg() as f32,
        visual_field_height_deg: geometry.visual_field_height_deg() as f32,
        projection_type: experiment.geometry.projection.to_shader_int(),
        viewing_distance_cm: rig_geometry.viewing_distance_cm as f32,
        display_width_cm: w_cm as f32,
        display_height_cm: h_cm as f32,
        center_azimuth_deg: experiment.geometry.horizontal_offset_deg as f32,
        center_elevation_deg: experiment.geometry.vertical_offset_deg as f32,

        carrier_type: experiment.stimulus.carrier.to_shader_int(),
        check_size_deg: params.check_size_deg as f32,
        check_size_cm: params.check_size_cm as f32,
        contrast: params.contrast as f32,
        mean_luminance: params.mean_luminance as f32,
        luminance_high: params.luminance_high() as f32,
        luminance_low: params.luminance_low() as f32,

        strobe_frequency_hz: if params.strobe_frequency_hz > 0.0 {
            params.strobe_frequency_hz as f32
        } else {
            0.0
        },

        envelope_type: experiment.stimulus.envelope.to_shader_int(),
        stimulus_width_deg: params.stimulus_width_deg as f32,
        rotation_deg: params.rotation_deg as f32,
        background_luminance: params.background_luminance as f32,
        max_eccentricity_deg: max_ecc,

        // Preview shows mouse's perspective — no monitor rotation.
        monitor_rotation_deg: 0.0,
    }
}

#[cfg(windows)]
fn build_renderer_config(cfg: &AcquisitionCommand) -> RendererConfig {
    let geometry = DisplayGeometry::new(
        cfg.experiment.geometry.projection,
        cfg.geometry.viewing_distance_cm,
        cfg.experiment.geometry.horizontal_offset_deg,
        cfg.experiment.geometry.vertical_offset_deg,
        cfg.monitor.width_cm,
        cfg.monitor.height_cm,
        cfg.monitor.width_px,
        cfg.monitor.height_px,
    );

    let params = &cfg.experiment.stimulus.params;

    // Compute max eccentricity from geometry
    let max_ecc = geometry.get_max_eccentricity_deg() as f32;

    RendererConfig {
        visual_field_width_deg: geometry.visual_field_width_deg() as f32,
        visual_field_height_deg: geometry.visual_field_height_deg() as f32,
        projection_type: cfg.experiment.geometry.projection.to_shader_int(),
        viewing_distance_cm: cfg.geometry.viewing_distance_cm as f32,
        display_width_cm: cfg.monitor.width_cm as f32,
        display_height_cm: cfg.monitor.height_cm as f32,
        center_azimuth_deg: cfg.experiment.geometry.horizontal_offset_deg as f32,
        center_elevation_deg: cfg.experiment.geometry.vertical_offset_deg as f32,

        carrier_type: cfg.experiment.stimulus.carrier.to_shader_int(),
        check_size_deg: params.check_size_deg as f32,
        check_size_cm: params.check_size_cm as f32,
        contrast: params.contrast as f32,
        mean_luminance: params.mean_luminance as f32,
        luminance_high: params.luminance_high() as f32,
        luminance_low: params.luminance_low() as f32,

        strobe_frequency_hz: if params.strobe_frequency_hz > 0.0 {
            params.strobe_frequency_hz as f32
        } else {
            0.0
        },

        envelope_type: cfg.experiment.stimulus.envelope.to_shader_int(),
        stimulus_width_deg: params.stimulus_width_deg as f32,
        rotation_deg: params.rotation_deg as f32,
        background_luminance: params.background_luminance as f32,
        max_eccentricity_deg: max_ecc,

        // Apply physical monitor rotation so the mouse sees correct stimulus.
        monitor_rotation_deg: cfg.display.monitor_rotation_deg as f32,
    }
}

// =============================================================================
// Thread entry point
// =============================================================================

#[cfg(windows)]
pub fn run(
    cmd_rx: Receiver<StimulusCmd>,
    evt_tx: Sender<StimulusEvt>,
    monitor_index: usize,
    monitor_width_px: u32,
    monitor_height_px: u32,
    monitor_position: (i32, i32),
    system_cfg: crate::config::SystemTuning,
    initial_bg_luminance: f64,
) {
    if let Err(e) = run_inner(
        cmd_rx,
        &evt_tx,
        monitor_index,
        monitor_width_px,
        monitor_height_px,
        monitor_position,
        system_cfg,
        initial_bg_luminance,
    ) {
        eprintln!("[stimulus_thread] fatal error: {e}");
        let _ = evt_tx.send(StimulusEvt::Error(e));
    }
}

#[cfg(windows)]
fn run_inner(
    cmd_rx: Receiver<StimulusCmd>,
    evt_tx: &Sender<StimulusEvt>,
    monitor_index: usize,
    monitor_width_px: u32,
    monitor_height_px: u32,
    monitor_position: (i32, i32),
    system_cfg: crate::config::SystemTuning,
    initial_bg_luminance: f64,
) -> Result<(), String> {
    // --- Create win32 window ---
    let (hwnd, hinstance) = create_fullscreen_window(
        monitor_position.0,
        monitor_position.1,
        monitor_width_px,
        monitor_height_px,
    )?;
    eprintln!(
        "[stimulus_thread] window created at ({}, {}), {}x{}",
        monitor_position.0, monitor_position.1, monitor_width_px, monitor_height_px
    );

    // --- Create wgpu instance, surface, device, queue ---
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..Default::default()
    });

    let surface = create_wgpu_surface(&instance, hwnd, hinstance)?;

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
    }))
    .ok_or("No suitable GPU adapter found")?;

    eprintln!("[stimulus_thread] adapter: {}", adapter.get_info().name);

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("stimulus_device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
        },
        None,
    ))
    .map_err(|e| format!("request_device: {e}"))?;

    let surface_caps = surface.get_capabilities(&adapter);
    let surface_format = surface_caps
        .formats
        .iter()
        .find(|f| f.is_srgb())
        .copied()
        .unwrap_or(surface_caps.formats[0]);

    surface.configure(
        &device,
        &wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: monitor_width_px,
            height: monitor_height_px,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        },
    );

    // --- Create stimulus renderer ---
    let mut renderer = StimulusRenderer::new(&device, surface_format);
    eprintln!("[stimulus_thread] renderer created");

    // --- Preview capture: small offscreen texture for scientist's sidebar ---
    let preview_w = system_cfg.preview_width_px;
    let preview_h = (preview_w as f64 * monitor_height_px as f64 / monitor_width_px as f64) as u32;
    let preview_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("preview_capture"),
        size: wgpu::Extent3d { width: preview_w, height: preview_h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: surface_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let preview_view = preview_texture.create_view(&wgpu::TextureViewDescriptor::default());
    // Staging buffer: 4 bytes per pixel (RGBA), row-aligned to 256 bytes (COPY_BYTES_PER_ROW_ALIGNMENT)
    let bytes_per_row = ((preview_w * 4 + 255) / 256) * 256;
    let staging_buf_size = (bytes_per_row * preview_h) as u64;
    let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("preview_staging"),
        size: staging_buf_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut last_preview_sent = Instant::now();
    let preview_interval = Duration::from_millis(system_cfg.preview_interval_ms as u64);

    // --- Find DXGI output for WaitForVBlank ---
    let dxgi_output: IDXGIOutput = find_dxgi_output(monitor_index)?;

    // --- QPC frequency ---
    let mut qpc_freq = 0i64;
    unsafe {
        let _ = QueryPerformanceFrequency(&mut qpc_freq);
    }
    if qpc_freq == 0 {
        return Err("QueryPerformanceFrequency returned 0".into());
    }

    // --- Signal ready ---
    evt_tx
        .send(StimulusEvt::Ready)
        .map_err(|_| "evt_tx closed before Ready".to_string())?;
    eprintln!("[stimulus_thread] ready");

    // --- State ---
    let mut sequencer = Sequencer::new();
    let mut dataset: Option<StimulusDataset> = None;
    let mut acquiring = false;
    let mut previewing = false;
    let mut last_qpc_us: i64 = 0;    // QPC clock for sequencer timing (monotonic)
    let mut last_vsync_us: i64 = 0;  // DWM vsync for dataset frame deltas
    let mut start_time_us: i64 = 0;
    let mut preview_start_us: i64 = 0;
    let mut last_present_count: u64 = 0;

    // Sweep schedule — records when each sweep started and ended.
    let mut sweep_sequence: Vec<String> = Vec::new();
    let mut sweep_start_us: Vec<i64> = Vec::new();
    let mut sweep_end_us: Vec<i64> = Vec::new();

    // Background luminance from experiment config. Updated when acq/preview starts.
    let mut bg_luminance: f64 = initial_bg_luminance;
    let mut bg_color = wgpu::Color { r: bg_luminance, g: bg_luminance, b: bg_luminance, a: 1.0 };

    // --- Main loop ---
    loop {
        // Drain commands
        loop {
            match cmd_rx.try_recv() {
                Ok(StimulusCmd::Shutdown) => {
                    eprintln!("[stimulus_thread] shutdown requested");
                    if acquiring {
                        sequencer.stop();
                        if let Some(ref mut ds) = dataset {
                            ds.stop_recording();
                        }
                    }
                    unsafe { let _ = DestroyWindow(hwnd); }
                    return Ok(());
                }
                Ok(StimulusCmd::StartAcquisition(acq_cfg)) => {
                    eprintln!("[stimulus_thread] starting acquisition");
                    previewing = false;

                    // Update background from experiment config.
                    bg_luminance = acq_cfg.experiment.stimulus.params.background_luminance;
                    bg_color = wgpu::Color { r: bg_luminance, g: bg_luminance, b: bg_luminance, a: 1.0 };

                    let seq_cfg = build_sequencer_config(&acq_cfg);
                    sequencer.start(&seq_cfg);

                    let render_cfg = build_renderer_config(&acq_cfg);
                    renderer.configure(&render_cfg);

                    let ds_cfg = build_dataset_config(&acq_cfg);
                    let mut ds = StimulusDataset::new(ds_cfg);
                    ds.start_recording();
                    let now_us = qpc_to_us(query_qpc(), qpc_freq);
                    last_qpc_us = now_us;
                    last_vsync_us = 0;
                    start_time_us = now_us;
                    dataset = Some(ds);
                    acquiring = true;
                }
                Ok(StimulusCmd::Stop) => {
                    eprintln!("[stimulus_thread] stop requested");
                    if acquiring {
                        sequencer.stop();
                        if let Some(mut ds) = dataset.take() {
                            ds.stop_recording();
                            let _ = evt_tx.send(StimulusEvt::Complete(AcquisitionResult {
                                dataset: ds,
                                sweep_sequence: std::mem::take(&mut sweep_sequence),
                                sweep_start_us: std::mem::take(&mut sweep_start_us),
                                sweep_end_us: std::mem::take(&mut sweep_end_us),
                                completed_normally: false,
                            }));
                        }
                        acquiring = false;
                        let _ = evt_tx.send(StimulusEvt::Stopped);
                    }
                }
                Ok(StimulusCmd::Preview(preview_cfg)) => {
                    eprintln!("[stimulus_thread] starting preview");
                    bg_luminance = preview_cfg.experiment.stimulus.params.background_luminance;
                    bg_color = wgpu::Color { r: bg_luminance, g: bg_luminance, b: bg_luminance, a: 1.0 };
                    let render_cfg = build_preview_renderer_config(
                        &preview_cfg.experiment,
                        &preview_cfg.geometry,
                        &preview_cfg.monitor,
                    );
                    eprintln!("[stimulus_thread] preview config: vf={:.1}x{:.1} proj={} env={} carrier={} width={:.1} contrast={:.2} lum_hi={:.2} lum_lo={:.2} bg={:.2} ecc={:.1}",
                        render_cfg.visual_field_width_deg, render_cfg.visual_field_height_deg,
                        render_cfg.projection_type, render_cfg.envelope_type, render_cfg.carrier_type,
                        render_cfg.stimulus_width_deg, render_cfg.contrast,
                        render_cfg.luminance_high, render_cfg.luminance_low,
                        render_cfg.background_luminance, render_cfg.max_eccentricity_deg);
                    renderer.configure(&render_cfg);
                    previewing = true;
                    preview_start_us = qpc_to_us(query_qpc(), qpc_freq);
                }
                Ok(StimulusCmd::StopPreview) => {
                    if previewing {
                        eprintln!("[stimulus_thread] stopping preview");
                        previewing = false;
                    }
                }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    eprintln!("[stimulus_thread] command channel disconnected");
                    unsafe { let _ = DestroyWindow(hwnd); }
                    return Ok(());
                }
            }
        }

        // Pump win32 messages
        if !pump_messages() {
            eprintln!("[stimulus_thread] WM_QUIT received");
            unsafe { let _ = DestroyWindow(hwnd); }
            return Ok(());
        }

        if acquiring {
            // WaitForVBlank — blocks until vertical blank on target monitor.
            // This provides frame pacing. The actual timestamp comes from DWM below.
            unsafe {
                dxgi_output
                    .WaitForVBlank()
                    .map_err(|e| format!("WaitForVBlank: {e}"))?;
            }

            // Use QPC for sequencer timing (monotonic, no DWM jitter).
            let qpc_us = qpc_to_us(query_qpc(), qpc_freq);
            let delta_sec = if last_qpc_us > 0 {
                (qpc_us - last_qpc_us) as f64 / 1_000_000.0
            } else {
                0.0
            };
            last_qpc_us = qpc_us;
            if !sequencer.is_complete() {
                sequencer.advance(delta_sec);
            }

            // Record sweep schedule from sequencer events.
            for event in sequencer.drain_events() {
                match event {
                    openisi_stimulus::sequencer::Event::SweepStarted { direction, .. } => {
                        sweep_sequence.push(direction);
                        sweep_start_us.push(qpc_us);
                    }
                    openisi_stimulus::sequencer::Event::SweepCompleted { .. } => {
                        sweep_end_us.push(qpc_us);
                    }
                    _ => {}
                }
            }

            // Render and present
            let elapsed_sec = if start_time_us > 0 {
                (qpc_us - start_time_us) as f32 / 1_000_000.0
            } else {
                0.0
            };
            let dir_int = direction_to_int(&sequencer.current_direction);
            let progress = if sequencer.state == openisi_stimulus::sequencer::State::Sweep {
                sequencer.get_state_progress() as f32
            } else {
                0.0
            };
            renderer.update_frame(&queue, progress, dir_int, elapsed_sec);

            match surface.get_current_texture() {
                Ok(frame) => {
                    let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
                    if sequencer.is_baseline() {
                        render_clear_view(&device, &queue, &view, bg_color);
                    } else {
                        renderer.render(&device, &queue, &view);
                    }
                    frame.present();
                }
                Err(e) => {
                    eprintln!("[stimulus_thread] get_current_texture: {e}");
                }
            }

            // Query DWM for the actual hardware vsync timestamp.
            // This is the QPC value AT the vsync interrupt, not after OS scheduling delay.
            let (vsync_us, _present_count) = if let Some((qpc_vblank, frame_count)) = query_dwm_vsync() {
                let us = qpc_to_us(qpc_vblank, qpc_freq);
                // Detect GPU-level frame drops from present count gaps
                if last_present_count > 0 && frame_count > last_present_count + 1 {
                    let dropped = frame_count - last_present_count - 1;
                    eprintln!("[stimulus_thread] DWM present count gap: {} frames dropped", dropped);
                }
                last_present_count = frame_count;
                (us, frame_count)
            } else {
                // DWM not available (unlikely on Windows 7+), fall back to QPC
                (qpc_us, 0)
            };

            let frame_delta_us = if last_vsync_us > 0 { vsync_us - last_vsync_us } else { 0 };
            last_vsync_us = vsync_us;

            // Record frame into dataset with hardware vsync timestamp
            if let Some(ref mut ds) = dataset {
                let frame_state = FrameState::from_sequencer_state(sequencer.state);
                // Resolve condition index from current direction in sweep sequence.
                let cond_idx = {
                    let dir = &sequencer.current_direction;
                    ds.config().conditions.iter()
                        .position(|c| c == dir)
                        .map(|i| i as u8)
                        .unwrap_or(0)
                };
                let record = FrameRecord {
                    timestamp_us: vsync_us,
                    condition_index: cond_idx,
                    sweep_index: sequencer.current_sweep_index as u32,
                    frame_in_sweep: sequencer.get_state_frame_index() as u32,
                    sweep_progress: sequencer.get_state_progress() as f32,
                    state_id: frame_state,
                    condition_occurrence: sequencer.get_current_condition_occurrence(),
                    is_baseline: sequencer.is_baseline(),
                };
                ds.record_frame(&record);
            }

            // Send frame event to UI
            let condition = if let Some(ref ds) = dataset {
                let sweep_idx = sequencer.current_sweep_index;
                if sweep_idx < ds.sweep_sequence.len() {
                    ds.sweep_sequence[sweep_idx].clone()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            let record = StimulusFrameRecord {
                timestamp_us: vsync_us,
                state: sequencer.state.name().to_string(),
                sweep_index: sequencer.current_sweep_index,
                total_sweeps: sequencer.get_total_sweeps(),
                state_progress: sequencer.get_state_progress(),
                frame_delta_us,
                elapsed_sec: sequencer.get_elapsed_time(),
                remaining_sec: sequencer.get_remaining_time(),
                condition,
                condition_occurrence: sequencer.get_current_condition_occurrence(),
            };
            let _ = evt_tx.send(StimulusEvt::Frame(record));

            // Capture preview frame for scientist's sidebar (~10 fps)
            if last_preview_sent.elapsed() >= preview_interval {
                last_preview_sent = Instant::now();
                capture_preview_frame(
                    &device, &queue, &mut renderer,
                    &preview_view, &preview_texture, &staging_buffer,
                    preview_w, preview_h, bytes_per_row,
                    sequencer.is_baseline(), bg_luminance as f32, evt_tx,
                );
            }

            // Check completion
            if sequencer.is_complete() {
                eprintln!("[stimulus_thread] acquisition complete");
                if let Some(mut ds) = dataset.take() {
                    ds.stop_recording();
                    let _ = evt_tx.send(StimulusEvt::Complete(AcquisitionResult {
                        dataset: ds,
                        sweep_sequence: std::mem::take(&mut sweep_sequence),
                        sweep_start_us: std::mem::take(&mut sweep_start_us),
                        sweep_end_us: std::mem::take(&mut sweep_end_us),
                        completed_normally: true,
                    }));
                }
                acquiring = false;
            }
        } else if previewing {
            // Preview: render stimulus pattern with cycling progress (no recording).
            // Use a 10-second cycle so the user sees the full sweep range.
            let now_us = qpc_to_us(query_qpc(), qpc_freq);
            let elapsed_sec = (now_us - preview_start_us) as f32 / 1_000_000.0;
            let cycle_sec = system_cfg.preview_cycle_sec as f32;
            let progress = (elapsed_sec % cycle_sec) / cycle_sec;
            renderer.update_frame(&queue, progress, 0, elapsed_sec);

            match surface.get_current_texture() {
                Ok(frame) => {
                    let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
                    renderer.render(&device, &queue, &view);
                    frame.present();
                }
                Err(e) => {
                    eprintln!("[stimulus_thread] preview render error: {e}");
                }
            }

            // Capture preview frame for scientist's sidebar
            if last_preview_sent.elapsed() >= preview_interval {
                last_preview_sent = Instant::now();
                capture_preview_frame(
                    &device, &queue, &mut renderer,
                    &preview_view, &preview_texture, &staging_buffer,
                    preview_w, preview_h, bytes_per_row,
                    false, bg_luminance as f32, evt_tx,
                );
            }

            // Wait for vsync to avoid tearing
            unsafe { let _ = dxgi_output.WaitForVBlank(); }
        } else {
            // Idle — render gray, sleep to avoid busy-waiting
            if let Err(e) = render_clear(&surface, &device, &queue, bg_color) {
                eprintln!("[stimulus_thread] idle render error: {e}");
            }
            std::thread::sleep(Duration::from_millis(system_cfg.idle_sleep_ms as u64));
        }
    }
}
