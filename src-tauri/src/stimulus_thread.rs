//! Stimulus thread â€” raw win32 window + wgpu + DXGI WaitForVBlank.
//!
//! Runs on its own thread, communicates via crossbeam channels.
//! Owns a fullscreen window on the stimulus monitor and renders at vsync rate.
//!
//! On non-Windows platforms, the stimulus thread reports that hardware stimulus
//! display is not available and waits for shutdown.

#[cfg(not(windows))]
use crate::messages::{StimulusCmd, StimulusEvt};
#[cfg(not(windows))]
use crossbeam_channel::{Receiver, Sender};

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
use windows::Win32::Graphics::Dwm::{DWM_TIMING_INFO, DwmGetCompositionTimingInfo};
#[cfg(windows)]
use windows::Win32::Graphics::Dxgi::IDXGIOutput;
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
use openisi_stimulus::renderer::{RendererConfig, StimulusRenderer, direction_to_int};
#[cfg(windows)]
use openisi_stimulus::sequencer::{Sequencer, SequencerConfig};

#[cfg(windows)]
use crate::error::AcquisitionError;
#[cfg(windows)]
use crate::messages::{
    AcquisitionResult, StimulusCmd, StimulusEvt, StimulusFrameRecord, StimulusPreviewFrame,
};
#[cfg(windows)]
use crate::monitor::find_dxgi_output;
#[cfg(windows)]
use crate::params::Envelope;
#[cfg(windows)]
use openisi_params::config::ConfigSnapshot;

// =============================================================================
// Shared configuration
// =============================================================================

/// Monitor geometry + preview/timing configuration for the stimulus thread.
/// These values travel together from the spawn site (`AppState`) through `run`
/// into `run_inner`; bundling them is the parameter-object that removes the long
/// argument list the two functions previously duplicated.
#[derive(Debug, Clone, Copy)]
pub struct StimulusConfig {
    pub monitor_index: usize,
    pub monitor_width_px: u32,
    pub monitor_height_px: u32,
    pub monitor_position: (i32, i32),
    pub preview_width_px: u32,
    pub preview_interval_ms: u32,
    pub preview_cycle_sec: f64,
    pub idle_sleep_ms: u32,
    pub drop_detection_warmup_frames: usize,
    pub initial_bg_luminance: f64,
}

// =============================================================================
// Non-Windows stub
// =============================================================================

#[cfg(not(windows))]
pub fn run(cmd_rx: Receiver<StimulusCmd>, evt_tx: Sender<StimulusEvt>, _config: StimulusConfig) {
    tracing::warn!("stimulus display is not available on this platform (requires Windows)");
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
) -> Result<(HWND, isize), AcquisitionError> {
    unsafe {
        let hmodule = GetModuleHandleW(None)
            .map_err(|e| AcquisitionError::Stimulus(format!("GetModuleHandleW: {e}")))?;
        let hinstance = windows::Win32::Foundation::HINSTANCE(hmodule.0);

        let class_name: Vec<u16> = "OpenISI_Stimulus\0".encode_utf16().collect();

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(stimulus_wnd_proc),
            hInstance: hinstance,
            lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };

        // Returns 0 if already registered â€” that's fine
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
        .map_err(|e| AcquisitionError::Stimulus(format!("CreateWindowExW: {e}")))?;

        Ok((hwnd, hinstance.0 as isize))
    }
}

#[cfg(windows)]
fn create_wgpu_surface(
    instance: &wgpu::Instance,
    hwnd: HWND,
    hinstance: isize,
) -> Result<wgpu::Surface<'static>, AcquisitionError> {
    let hwnd_isize = hwnd.0 as isize;
    let hwnd_nz = NonZeroIsize::new(hwnd_isize)
        .ok_or_else(|| AcquisitionError::Stimulus("HWND is null".into()))?;
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
            .map_err(|e| AcquisitionError::Stimulus(format!("create_surface_unsafe: {e}")))
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
// Solid-color surface clears (blanking / background; the sweep stimulus itself
// is rendered by the `openisi-stimulus` WGSL renderer)
// =============================================================================

#[cfg(windows)]
fn render_clear(
    surface: &wgpu::Surface,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    color: wgpu::Color,
) -> Result<(), AcquisitionError> {
    let frame = surface
        .get_current_texture()
        .map_err(|e| AcquisitionError::Stimulus(format!("get_current_texture: {e}")))?;
    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
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

/// The GPU resources + dimensions of the preview render target — the parameter
/// object for [`capture_preview_frame`]. Naming the three `u32` dimensions
/// removes the width/height/bytes-per-row swap hazard; the wgpu handles travel
/// together as one "where the preview renders" concept.
#[cfg(windows)]
struct PreviewTarget<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    view: &'a wgpu::TextureView,
    texture: &'a wgpu::Texture,
    staging_buffer: &'a wgpu::Buffer,
    width: u32,
    height: u32,
    bytes_per_row: u32,
}

#[cfg(windows)]
/// Render the current stimulus to the small preview texture (without monitor rotation),
/// copy to staging buffer, read back pixels, and send as PreviewFrame event.
fn capture_preview_frame(
    target: &PreviewTarget,
    renderer: &mut StimulusRenderer,
    is_baseline: bool,
    bg_luminance: f32,
    evt_tx: &Sender<StimulusEvt>,
) {
    // Bind the target fields to the names the body already uses.
    let &PreviewTarget {
        device,
        queue,
        view: preview_view,
        texture: preview_texture,
        staging_buffer,
        width: preview_w,
        height: preview_h,
        bytes_per_row,
    } = target;

    if is_baseline {
        let bg = wgpu::Color {
            r: bg_luminance as f64,
            g: bg_luminance as f64,
            b: bg_luminance as f64,
            a: 1.0,
        };
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
        wgpu::Extent3d {
            width: preview_w,
            height: preview_h,
            depth_or_array_layers: 1,
        },
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
fn build_sequencer_config(
    cs: &ConfigSnapshot,
    monitor: &crate::session::MonitorInfo,
) -> SequencerConfig {
    let sweep_duration_sec = compute_sweep_duration(cs, monitor);

    SequencerConfig {
        conditions: cs.experiment.presentation.conditions.clone(),
        repetitions: cs.experiment.presentation.repetitions,
        order: cs.experiment.presentation.order,
        baseline_start_sec: cs.experiment.timing.baseline_start_sec,
        baseline_end_sec: cs.experiment.timing.baseline_end_sec,
        inter_stimulus_sec: cs.experiment.timing.inter_stimulus_sec,
        inter_direction_sec: cs.experiment.timing.inter_direction_sec,
        sweep_duration_sec,
    }
}

#[cfg(windows)]
fn compute_sweep_duration(cs: &ConfigSnapshot, monitor: &crate::session::MonitorInfo) -> f64 {
    let p = &cs.experiment.stimulus.params;
    let geometry = DisplayGeometry::new(
        cs.experiment.geometry.projection,
        cs.rig.geometry.viewing_distance_cm,
        cs.experiment.geometry.horizontal_offset_deg,
        cs.experiment.geometry.vertical_offset_deg,
        cs.rig.geometry.bisector_x_cm,
        cs.rig.geometry.bisector_y_cm,
        // Effective panel cm: user-calibrated monitor cm if explicitly set,
        // else the EDID hardware value.
        cs.effective_monitor_width_cm().unwrap_or(monitor.width_cm),
        cs.effective_monitor_height_cm().unwrap_or(monitor.height_cm),
        monitor.width_px,
        monitor.height_px,
    );

    match cs.experiment.stimulus.envelope {
        Envelope::Fullfield => 0.0,
        Envelope::Bar => {
            let total_travel = geometry.visual_field_width_deg() + p.stimulus_width_deg;
            total_travel / p.sweep_speed_deg_per_sec
        }
        Envelope::Wedge => 360.0 / p.rotation_speed_deg_per_sec,
        Envelope::Ring => {
            let total_travel = geometry.get_max_eccentricity_deg() + p.stimulus_width_deg;
            total_travel / p.expansion_speed_deg_per_sec
        }
    }
}

#[cfg(windows)]
fn build_dataset_config(
    cs: &ConfigSnapshot,
    monitor: &crate::session::MonitorInfo,
    measured_refresh_hz: f64,
) -> DatasetConfig {
    use std::collections::HashMap;

    let p = &cs.experiment.stimulus.params;
    let geometry = DisplayGeometry::new(
        cs.experiment.geometry.projection,
        cs.rig.geometry.viewing_distance_cm,
        cs.experiment.geometry.horizontal_offset_deg,
        cs.experiment.geometry.vertical_offset_deg,
        cs.rig.geometry.bisector_x_cm,
        cs.rig.geometry.bisector_y_cm,
        cs.effective_monitor_width_cm().unwrap_or(monitor.width_cm),
        cs.effective_monitor_height_cm().unwrap_or(monitor.height_cm),
        monitor.width_px,
        monitor.height_px,
    );

    // Build stimulus params map from the typed config values.
    let mut stimulus_params: HashMap<String, serde_json::Value> = HashMap::new();
    stimulus_params.insert("contrast".into(), serde_json::json!(p.contrast));
    stimulus_params.insert("mean_luminance".into(), serde_json::json!(p.mean_luminance));
    stimulus_params.insert(
        "background_luminance".into(),
        serde_json::json!(p.background_luminance),
    );
    stimulus_params.insert("check_size_deg".into(), serde_json::json!(p.check_size_deg));
    stimulus_params.insert("check_size_cm".into(), serde_json::json!(p.check_size_cm));
    stimulus_params.insert(
        "strobe_frequency_hz".into(),
        serde_json::json!(p.strobe_frequency_hz),
    );
    stimulus_params.insert(
        "stimulus_width_deg".into(),
        serde_json::json!(p.stimulus_width_deg),
    );
    stimulus_params.insert(
        "sweep_speed_deg_per_sec".into(),
        serde_json::json!(p.sweep_speed_deg_per_sec),
    );
    stimulus_params.insert(
        "rotation_speed_deg_per_sec".into(),
        serde_json::json!(p.rotation_speed_deg_per_sec),
    );
    stimulus_params.insert(
        "expansion_speed_deg_per_sec".into(),
        serde_json::json!(p.expansion_speed_deg_per_sec),
    );
    stimulus_params.insert("rotation_deg".into(), serde_json::json!(p.rotation_deg));

    DatasetConfig {
        envelope: cs.experiment.stimulus.envelope,
        stimulus_params,
        conditions: cs.experiment.presentation.conditions.clone(),
        repetitions: cs.experiment.presentation.repetitions,
        order: cs.experiment.presentation.order,
        baseline_start_sec: cs.experiment.timing.baseline_start_sec,
        baseline_end_sec: cs.experiment.timing.baseline_end_sec,
        inter_stimulus_sec: cs.experiment.timing.inter_stimulus_sec,
        inter_direction_sec: cs.experiment.timing.inter_direction_sec,
        sweep_duration_sec: compute_sweep_duration(cs, monitor),
        geometry,
        display_physical_source: monitor.physical_source.clone(),
        reported_refresh_hz: monitor.refresh_hz as f64,
        measured_refresh_hz,
        target_stimulus_fps: cs.rig.display.target_stimulus_fps,
        drop_detection_warmup_frames: cs.rig.system.drop_detection_warmup_frames,
        drop_detection_threshold: cs.rig.system.drop_detection_threshold,
        fps_window_frames: cs.rig.system.fps_window_frames,
    }
}

#[cfg(windows)]
/// Build renderer config from a snapshot + monitor info.
/// Used for both acquisition (with monitor rotation) and preview (without).
fn build_renderer_config_from_snapshot(
    cs: &ConfigSnapshot,
    monitor: &crate::session::MonitorInfo,
    apply_rotation: bool,
) -> RendererConfig {
    // EDID gives reliable pixel dimensions and a usable auto-detected
    // cm size. The config's MonitorWidthCm/MonitorHeightCm only override
    // the EDID value when the user has explicitly calibrated them via UI.
    //
    // Monitor yaw/pitch: the projection currently assumes the monitor normal
    // is the eye-perpendicular axis (yaw = pitch = 0). `MonitorYawDeg` /
    // `MonitorPitchDeg` are config params, so a configured non-zero value
    // would otherwise be *silently ignored* — a no-op setting with real
    // scientific consequences (the presented visual angles would be wrong for
    // a physically-tilted monitor). We refuse to be silent about it: a
    // non-zero value is surfaced as a loud warning here. Actually applying the
    // tilt means rotating the monitor plane in the spherical projection (CPU
    // geometry + the WGSL shader) and validating the rendered angles *on the
    // physical display* — that on-hardware validation can't be faked, so the
    // feature is deferred with cause rather than shipped unvalidated.
    let yaw_deg = cs.rig.geometry.monitor_yaw_deg;
    let pitch_deg = cs.rig.geometry.monitor_pitch_deg;
    if yaw_deg.abs() > f64::EPSILON || pitch_deg.abs() > f64::EPSILON {
        tracing::warn!(
            yaw_deg,
            pitch_deg,
            "monitor yaw/pitch are configured but NOT yet applied to the \
             stimulus projection — presented visual angles assume a \
             perpendicular monitor. Treat retinotopy from a tilted monitor as \
             uncalibrated until yaw/pitch projection lands."
        );
    }

    let (w_cm, h_cm, w_px, h_px) = (
        // Effective panel cm: user-calibrated monitor cm if explicitly set,
        // else the EDID hardware value.
        cs.effective_monitor_width_cm().unwrap_or(monitor.width_cm),
        cs.effective_monitor_height_cm().unwrap_or(monitor.height_cm),
        monitor.width_px,
        monitor.height_px,
    );

    let geometry = DisplayGeometry::new(
        cs.experiment.geometry.projection,
        cs.rig.geometry.viewing_distance_cm,
        cs.experiment.geometry.horizontal_offset_deg,
        cs.experiment.geometry.vertical_offset_deg,
        cs.rig.geometry.bisector_x_cm,
        cs.rig.geometry.bisector_y_cm,
        w_cm,
        h_cm,
        w_px,
        h_px,
    );

    let max_ecc = geometry.get_max_eccentricity_deg() as f32;
    let p = &cs.experiment.stimulus.params;
    let strobe_hz = p.strobe_frequency_hz;
    let fov_mask = compute_fov_mask_bounds(&geometry, cs);

    RendererConfig {
        visual_field_width_deg: geometry.visual_field_width_deg() as f32,
        visual_field_height_deg: geometry.visual_field_height_deg() as f32,
        projection_type: cs.experiment.geometry.projection.to_shader_int(),
        viewing_distance_cm: cs.rig.geometry.viewing_distance_cm as f32,
        display_width_cm: w_cm as f32,
        display_height_cm: h_cm as f32,
        center_azimuth_deg: cs.experiment.geometry.horizontal_offset_deg as f32,
        center_elevation_deg: cs.experiment.geometry.vertical_offset_deg as f32,

        carrier_type: cs.experiment.stimulus.carrier.to_shader_int(),
        check_size_deg: p.check_size_deg as f32,
        check_size_cm: p.check_size_cm as f32,
        contrast: p.contrast as f32,
        mean_luminance: p.mean_luminance as f32,
        luminance_high: cs.luminance_high() as f32,
        luminance_low: cs.luminance_low() as f32,

        strobe_frequency_hz: if strobe_hz > 0.0 {
            strobe_hz as f32
        } else {
            0.0
        },

        envelope_type: cs.experiment.stimulus.envelope.to_shader_int(),
        stimulus_width_deg: p.stimulus_width_deg as f32,
        rotation_deg: p.rotation_deg as f32,
        background_luminance: p.background_luminance as f32,
        max_eccentricity_deg: max_ecc,

        monitor_rotation_deg: if apply_rotation {
            cs.rig.display.monitor_rotation_deg as f32
        } else {
            0.0
        },

        // Calibration ticks during preview only â€” for rig setup and bisector
        // alignment. Disabled during acquisition so the recorded stimulus is
        // unaltered.
        show_ticks: !apply_rotation,

        // Declared sweep envelope (deg) â€” Zhuang canonical 140Â° az Ã— 110Â° alt.
        // The shader uses these to bound the bar's sweep and to mask any
        // bar/wedge/ring pixel outside the declared (az, el) box. Center
        // comes from `OffsetAzi`/`OffsetAlt` so the masked region tracks
        // the experimenter-declared visual-field center.
        swept_range_width_deg: cs.experiment.stimulus_geometry.azi_angular_range as f32,
        swept_range_height_deg: cs.experiment.stimulus_geometry.alt_angular_range as f32,
        swept_center_az_deg: cs.experiment.stimulus_geometry.offset_azi as f32,
        swept_center_el_deg: cs.experiment.stimulus_geometry.offset_alt as f32,

        bisector_x_cm: cs.rig.geometry.bisector_x_cm as f32,
        bisector_y_cm: cs.rig.geometry.bisector_y_cm as f32,

        // Project the four FOV envelope corners (in visual deg) to
        // monitor cm via the inverse spherical transform, then take
        // their axis-aligned bounding box. The shader masks anything
        // outside this rectangle â€” a flat post-transform mask whose
        // corners ARE the projected FOV corners.
        fov_mask_y_min_cm: fov_mask.y_min as f32,
        fov_mask_y_max_cm: fov_mask.y_max as f32,
        fov_mask_z_min_cm: fov_mask.z_min as f32,
        fov_mask_z_max_cm: fov_mask.z_max as f32,
    }
}

#[cfg(windows)]
struct FovMaskBounds {
    y_min: f64,
    y_max: f64,
    z_min: f64,
    z_max: f64,
}

#[cfg(windows)]
fn compute_fov_mask_bounds(geometry: &DisplayGeometry, cs: &ConfigSnapshot) -> FovMaskBounds {
    let sg = &cs.experiment.stimulus_geometry;
    let half_az = sg.azi_angular_range * 0.5;
    let half_el = sg.alt_angular_range * 0.5;
    let cx = sg.offset_azi;
    let cy = sg.offset_alt;
    let corners = [
        (cx - half_az, cy - half_el),
        (cx + half_az, cy - half_el),
        (cx - half_az, cy + half_el),
        (cx + half_az, cy + half_el),
    ];
    let dw = geometry.display_width_cm;
    let dh = geometry.display_height_cm;
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;
    let mut z_min = f64::INFINITY;
    let mut z_max = f64::NEG_INFINITY;
    for (az, el) in corners {
        let (u, v) = geometry.angle_to_uv(az, el);
        let y_cm = (u - 0.5) * dw;
        let z_cm = (0.5 - v) * dh;
        if y_cm < y_min {
            y_min = y_cm;
        }
        if y_cm > y_max {
            y_max = y_cm;
        }
        if z_cm < z_min {
            z_min = z_cm;
        }
        if z_cm > z_max {
            z_max = z_cm;
        }
    }
    FovMaskBounds {
        y_min,
        y_max,
        z_min,
        z_max,
    }
}

// =============================================================================
// Thread entry point
// =============================================================================

/// Joins the calling thread to the Windows Multimedia Class Scheduler Service
/// ("Pro Audio" task) for its lifetime, then reverts on drop. This is the
/// Microsoft-sanctioned mechanism for presentation/audio threads: it schedules
/// the vsync loop at real-time priority so it is not preempted across a flip by
/// the analysis worker, the event forwarder, or OS background work — *without*
/// the system-starvation risk of a raw `THREAD_PRIORITY_TIME_CRITICAL` spin
/// (and our loop blocks on `WaitForVBlank`, yielding every frame). Best-effort:
/// if MMCSS is unavailable, we warn and run at normal priority rather than fail.
#[cfg(windows)]
struct MmcssRealtime(Option<windows::Win32::Foundation::HANDLE>);

#[cfg(windows)]
impl MmcssRealtime {
    fn acquire() -> Self {
        use windows::Win32::System::Threading::AvSetMmThreadCharacteristicsW;
        let task: Vec<u16> = "Pro Audio\0".encode_utf16().collect();
        let mut task_index: u32 = 0;
        // SAFETY: `task` is a valid NUL-terminated UTF-16 buffer and
        // `task_index` a valid out-pointer; the returned handle is reverted on drop.
        match unsafe {
            AvSetMmThreadCharacteristicsW(windows::core::PCWSTR(task.as_ptr()), &mut task_index)
        } {
            Ok(handle) if !handle.is_invalid() => {
                tracing::info!("stimulus thread joined MMCSS 'Pro Audio' (real-time scheduling)");
                MmcssRealtime(Some(handle))
            }
            _ => {
                tracing::warn!(
                    "could not elevate the stimulus thread to MMCSS real-time priority — \
                     running at normal priority; vsync timing may be less robust under load"
                );
                MmcssRealtime(None)
            }
        }
    }
}

#[cfg(windows)]
impl Drop for MmcssRealtime {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            use windows::Win32::System::Threading::AvRevertMmThreadCharacteristics;
            // SAFETY: `handle` came from a successful AvSetMmThreadCharacteristicsW.
            unsafe {
                let _ = AvRevertMmThreadCharacteristics(handle);
            }
        }
    }
}

#[cfg(windows)]
pub fn run(cmd_rx: Receiver<StimulusCmd>, evt_tx: Sender<StimulusEvt>, config: StimulusConfig) {
    // Real-time scheduling for the whole thread lifetime (preview + acquisition).
    let _mmcss = MmcssRealtime::acquire();

    if let Err(e) = run_inner(cmd_rx, &evt_tx, &config) {
        tracing::error!(error = %e, "stimulus thread fatal error");
        let _ = evt_tx.send(StimulusEvt::Fatal(e));
    }
}

#[cfg(windows)]
fn run_inner(
    cmd_rx: Receiver<StimulusCmd>,
    evt_tx: &Sender<StimulusEvt>,
    config: &StimulusConfig,
) -> Result<(), AcquisitionError> {
    // Bind every field as a local (all `Copy`) so the rest of the body — which
    // refers to these by their original names — stays unchanged.
    let StimulusConfig {
        monitor_index,
        monitor_width_px,
        monitor_height_px,
        monitor_position,
        preview_width_px,
        preview_interval_ms,
        preview_cycle_sec,
        idle_sleep_ms,
        drop_detection_warmup_frames,
        initial_bg_luminance,
    } = *config;

    // --- Create win32 window ---
    let (hwnd, hinstance) = create_fullscreen_window(
        monitor_position.0,
        monitor_position.1,
        monitor_width_px,
        monitor_height_px,
    )?;
    tracing::info!(
        x = monitor_position.0,
        y = monitor_position.1,
        width = monitor_width_px,
        height = monitor_height_px,
        "window created",
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
    .ok_or_else(|| AcquisitionError::Stimulus("No suitable GPU adapter found".into()))?;

    tracing::info!(adapter = %adapter.get_info().name, "GPU adapter selected");

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("stimulus_device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
        },
        None,
    ))
    .map_err(|e| AcquisitionError::Stimulus(format!("request_device: {e}")))?;

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
    let mut renderer = StimulusRenderer::new(
        &device,
        &queue,
        surface_format,
        monitor_width_px,
        monitor_height_px,
    );
    tracing::debug!("renderer created");

    // --- Preview capture: small offscreen texture for scientist's sidebar ---
    let preview_w = preview_width_px;
    let preview_h = (preview_w as f64 * monitor_height_px as f64 / monitor_width_px as f64) as u32;
    let preview_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("preview_capture"),
        size: wgpu::Extent3d {
            width: preview_w,
            height: preview_h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: surface_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let preview_view = preview_texture.create_view(&wgpu::TextureViewDescriptor::default());
    // Staging buffer: 4 bytes per pixel (RGBA), row-aligned to 256 bytes (COPY_BYTES_PER_ROW_ALIGNMENT)
    let bytes_per_row = (preview_w * 4).div_ceil(256) * 256;
    let staging_buf_size = (bytes_per_row * preview_h) as u64;
    let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("preview_staging"),
        size: staging_buf_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut last_preview_sent = Instant::now();
    let preview_interval = Duration::from_millis(preview_interval_ms as u64);
    // The per-vsync UI progress event (`StimulusEvt::Frame`) is throttled to
    // ~30 fps: a progress bar doesn't need every flip, and building the event's
    // heap `String`s every frame is avoidable allocation on the real-time
    // thread. The authoritative per-frame timing is recorded in the dataset,
    // not this UI event, so throttling loses no scientific data.
    let mut last_frame_event_sent = Instant::now();
    let frame_event_interval = Duration::from_millis(33);

    // --- Find DXGI output for WaitForVBlank ---
    let dxgi_output: IDXGIOutput =
        find_dxgi_output(monitor_index).map_err(|e| AcquisitionError::Stimulus(e.to_string()))?;

    // --- QPC frequency ---
    let mut qpc_freq = 0i64;
    unsafe {
        let _ = QueryPerformanceFrequency(&mut qpc_freq);
    }
    if qpc_freq == 0 {
        return Err(AcquisitionError::Stimulus(
            "QueryPerformanceFrequency returned 0".to_string(),
        ));
    }

    // --- Signal ready ---
    evt_tx
        .send(StimulusEvt::Ready)
        .map_err(|_| AcquisitionError::ChannelClosed {
            context: "evt_tx closed before Ready",
        })?;
    tracing::info!("stimulus thread ready");

    // --- State ---
    let mut sequencer = Sequencer::new();
    let mut dataset: Option<StimulusDataset> = None;
    let mut acquiring = false;
    let mut previewing = false;
    let mut last_qpc_us: i64 = 0; // QPC clock for sequencer timing (monotonic)
    let mut last_vsync_us: i64 = 0; // DWM vsync for dataset frame deltas
    let mut start_time_us: i64 = 0;
    let mut preview_start_us: i64 = 0;
    // Cycle duration for the preview loop. Computed from the live
    // snapshot's `sweep_speed_deg_per_sec` at StartPreview so changes to
    // sweep speed in the UI are visible during preview. Falls back to
    // `preview_cycle_sec` if sweep duration can't be computed.
    let mut preview_cycle_sec_actual: f32 = preview_cycle_sec as f32;

    // Drop detection state.
    //
    // `drop_detection_warmup_frames` â€” ignore drops in the first N frames
    // (composition warm-up, first-render JIT). The DWM present-count
    // gap is a direct count of missed flips, so we use the gap count
    // directly; the per-frame Î”us-vs-expected ratio test (which would
    // consume `drop_detection_threshold`) is post-hoc-only and lives
    // in `crates/openisi-stimulus/src/dataset.rs`.
    //
    // Catastrophic-threshold policy lives in the pure helper
    // `is_catastrophic_drop` (module-level + unit-tested); the per-run state +
    // its reset live in `DropMonitor`.
    let mut drops = DropMonitor::new(drop_detection_warmup_frames as u64);

    // Sweep schedule â€” records when each sweep started and ended.
    let mut sweep_sequence: Vec<String> = Vec::new();
    let mut sweep_start_us: Vec<i64> = Vec::new();
    let mut sweep_end_us: Vec<i64> = Vec::new();

    // Background luminance from experiment config. Updated when acq/preview starts.
    let mut bg_luminance: f64 = initial_bg_luminance;
    let mut bg_color = wgpu::Color {
        r: bg_luminance,
        g: bg_luminance,
        b: bg_luminance,
        a: 1.0,
    };

    // --- Main loop ---
    loop {
        // Drain commands
        loop {
            match cmd_rx.try_recv() {
                Ok(StimulusCmd::Shutdown) => {
                    tracing::info!("shutdown requested");
                    if acquiring {
                        sequencer.stop();
                        if let Some(ref mut ds) = dataset {
                            ds.stop_recording();
                        }
                    }
                    unsafe {
                        let _ = DestroyWindow(hwnd);
                    }
                    return Ok(());
                }
                Ok(StimulusCmd::StartAcquisition(acq_cfg)) => {
                    tracing::info!("starting acquisition");
                    previewing = false;

                    // The acquisition command carries the typed config directly.
                    let cs = acq_cfg.snapshot;

                    // Update background from the typed config.
                    bg_luminance = cs.experiment.stimulus.params.background_luminance;
                    bg_color = wgpu::Color {
                        r: bg_luminance,
                        g: bg_luminance,
                        b: bg_luminance,
                        a: 1.0,
                    };

                    let seq_cfg = build_sequencer_config(&cs, &acq_cfg.monitor);
                    sequencer.start(&seq_cfg);

                    let render_cfg =
                        build_renderer_config_from_snapshot(&cs, &acq_cfg.monitor, true);
                    renderer.configure(&render_cfg);

                    let ds_cfg =
                        build_dataset_config(&cs, &acq_cfg.monitor, acq_cfg.measured_refresh_hz);
                    let mut ds = StimulusDataset::new(ds_cfg);
                    ds.start_recording();
                    let now_us = qpc_to_us(query_qpc(), qpc_freq);
                    last_qpc_us = now_us;
                    last_vsync_us = 0;
                    start_time_us = now_us;
                    dataset = Some(ds);
                    acquiring = true;

                    // Re-entrancy: each acquisition starts its drop accounting
                    // fresh (counters + present-count baseline). Without this,
                    // run 2 charges every idle-time DWM present since run 1 as
                    // dropped frames and trips the catastrophic-drop abort — see
                    // `DropMonitor` / `present_count_gap`.
                    drops.reset();
                }
                Ok(StimulusCmd::Stop) => {
                    tracing::info!("stop requested");
                    if acquiring {
                        sequencer.stop();
                        finalize_acquisition(
                            evt_tx,
                            &mut dataset,
                            &mut sweep_sequence,
                            &mut sweep_start_us,
                            &mut sweep_end_us,
                            false,
                        );
                        acquiring = false;
                        let _ = evt_tx.send(StimulusEvt::Stopped);
                    }
                }
                Ok(StimulusCmd::Preview(preview_cfg)) => {
                    tracing::info!("starting preview");
                    // The preview command carries the typed config directly.
                    let cs = preview_cfg.snapshot;
                    bg_luminance = cs.experiment.stimulus.params.background_luminance;
                    bg_color = wgpu::Color {
                        r: bg_luminance,
                        g: bg_luminance,
                        b: bg_luminance,
                        a: 1.0,
                    };
                    let render_cfg = build_renderer_config_from_snapshot(
                        &cs,
                        &preview_cfg.monitor,
                        false, // Preview shows mouse's perspective â€” no monitor rotation.
                    );
                    tracing::debug!(
                        "preview config: vf={:.1}x{:.1} swept={:.1}x{:.1}@({:.1},{:.1}) proj={} env={} carrier={} width={:.1} contrast={:.2} lum_hi={:.2} lum_lo={:.2} bg={:.2} ecc={:.1}",
                        render_cfg.visual_field_width_deg,
                        render_cfg.visual_field_height_deg,
                        render_cfg.swept_range_width_deg,
                        render_cfg.swept_range_height_deg,
                        render_cfg.swept_center_az_deg,
                        render_cfg.swept_center_el_deg,
                        render_cfg.projection_type,
                        render_cfg.envelope_type,
                        render_cfg.carrier_type,
                        render_cfg.stimulus_width_deg,
                        render_cfg.contrast,
                        render_cfg.luminance_high,
                        render_cfg.luminance_low,
                        render_cfg.background_luminance,
                        render_cfg.max_eccentricity_deg
                    );
                    renderer.configure(&render_cfg);
                    previewing = true;
                    preview_start_us = qpc_to_us(query_qpc(), qpc_freq);
                    // Drive the preview at the same cycle duration the
                    // acquisition would use, so changes to sweep_speed
                    // (or rotation_speed / expansion_speed for wedge/ring)
                    // are visible. Clamp to â‰¥ 0.5 s to keep the preview
                    // useful even at very fast configured speeds.
                    let sweep_duration = compute_sweep_duration(&cs, &preview_cfg.monitor);
                    preview_cycle_sec_actual = (sweep_duration.max(0.5)) as f32;
                    tracing::debug!(
                        cycle_sec = preview_cycle_sec_actual,
                        "preview cycle (from sweep_speed_deg_per_sec)",
                    );
                }
                Ok(StimulusCmd::StopPreview) => {
                    if previewing {
                        tracing::info!("stopping preview");
                        previewing = false;
                    }
                }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    tracing::warn!("command channel disconnected");
                    unsafe {
                        let _ = DestroyWindow(hwnd);
                    }
                    return Ok(());
                }
            }
        }

        // Pump win32 messages
        if !pump_messages() {
            tracing::info!("WM_QUIT received");
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            return Ok(());
        }

        if acquiring {
            // WaitForVBlank â€” blocks until vertical blank on target monitor.
            // This provides frame pacing. The actual timestamp comes from DWM below.
            unsafe {
                dxgi_output
                    .WaitForVBlank()
                    .map_err(|e| AcquisitionError::Stimulus(format!("WaitForVBlank: {e}")))?;
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
            let dir_int = direction_to_int(&sequencer.current_direction).unwrap_or_else(|| {
                tracing::warn!(
                    direction = %sequencer.current_direction,
                    "unrecognized direction — rendering baseline",
                );
                0
            });
            let progress = if sequencer.state == openisi_stimulus::sequencer::State::Sweep {
                sequencer.get_state_progress() as f32
            } else {
                0.0
            };
            renderer.update_frame(&queue, progress, dir_int, elapsed_sec);

            match surface.get_current_texture() {
                Ok(frame) => {
                    let view = frame
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());
                    if sequencer.is_baseline() {
                        render_clear_view(&device, &queue, &view, bg_color);
                    } else {
                        renderer.render(&device, &queue, &view);
                    }
                    frame.present();
                }
                Err(e) => {
                    tracing::warn!(error = %e, "get_current_texture failed");
                }
            }

            // Query DWM for the actual hardware vsync timestamp.
            // This is the QPC value AT the vsync interrupt, not after OS scheduling delay.
            let (vsync_us, _present_count) = if let Some((qpc_vblank, frame_count)) =
                query_dwm_vsync()
            {
                let us = qpc_to_us(qpc_vblank, qpc_freq);
                // `DropMonitor` owns per-run drop accounting (counters + warmup +
                // present-count baseline); it returns the frames missed before
                // this one (0 within warmup / no drop).
                let gap = drops.observe(frame_count);
                if gap > 0 {
                    tracing::warn!(
                        gap,
                        cumulative = drops.cumulative_drops(),
                        observed = drops.observed_frames(),
                        "DWM present-count gap: frames dropped",
                    );
                    // Transient drop event â€” non-fatal, UI logs it.
                    let _ = evt_tx.send(StimulusEvt::Error(format!(
                        "Dropped {gap} stimulus frame(s) (cumulative {})",
                        drops.cumulative_drops()
                    )));

                    // Catastrophic-threshold check (pure helper, see
                    // `is_catastrophic_drop` for the policy + tests). A
                    // catastrophic drop ABORTS THIS ACQUISITION but must NOT
                    // terminate the long-lived stimulus thread — it owns the GPU
                    // device, window, and DXGI output created once at startup,
                    // and the next run must reuse them. Returning here is what
                    // forced a full app restart before every second acquisition.
                    // Mirror the `Stop` flow: surface why (transient error),
                    // hand back the partial run for the user's save decision,
                    // mark stopped, set `acquiring = false`, and keep looping.
                    if drops.is_catastrophic(gap) {
                        tracing::error!(
                            cumulative = drops.cumulative_drops(),
                            observed = drops.observed_frames(),
                            "stimulus drops exceeded catastrophic threshold — aborting acquisition",
                        );
                        let _ = evt_tx.send(StimulusEvt::Error(format!(
                            "stimulus drops exceeded catastrophic threshold: \
                             last gap={gap}, cumulative={}/{} \
                             ({:.2}%) — acquisition aborted",
                            drops.cumulative_drops(),
                            drops.observed_frames(),
                            drops.drop_fraction() * 100.0
                        )));
                        sequencer.stop();
                        finalize_acquisition(
                            evt_tx,
                            &mut dataset,
                            &mut sweep_sequence,
                            &mut sweep_start_us,
                            &mut sweep_end_us,
                            false,
                        );
                        acquiring = false;
                        let _ = evt_tx.send(StimulusEvt::Stopped);
                        continue;
                    }
                }
                (us, frame_count)
            } else {
                // DWM not available (unlikely on Windows 7+), fall back to QPC
                (qpc_us, 0)
            };

            let frame_delta_us = if last_vsync_us > 0 {
                vsync_us - last_vsync_us
            } else {
                0
            };
            last_vsync_us = vsync_us;

            // Record frame into dataset with hardware vsync timestamp
            if let Some(ref mut ds) = dataset {
                let frame_state = FrameState::from_sequencer_state(sequencer.state);
                // Resolve condition index from current direction in sweep sequence.
                let cond_idx = {
                    let dir = &sequencer.current_direction;
                    ds.config()
                        .conditions
                        .iter()
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

            // Send frame progress event to the UI — throttled to ~30 fps (see
            // `frame_event_interval`); building the record's heap Strings every
            // vsync is avoidable work on the real-time thread.
            if last_frame_event_sent.elapsed() >= frame_event_interval {
                last_frame_event_sent = Instant::now();
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
            }

            // Capture preview frame for scientist's sidebar (~10 fps)
            if last_preview_sent.elapsed() >= preview_interval {
                last_preview_sent = Instant::now();
                capture_preview_frame(
                    &PreviewTarget {
                        device: &device,
                        queue: &queue,
                        view: &preview_view,
                        texture: &preview_texture,
                        staging_buffer: &staging_buffer,
                        width: preview_w,
                        height: preview_h,
                        bytes_per_row,
                    },
                    &mut renderer,
                    sequencer.is_baseline(),
                    bg_luminance as f32,
                    evt_tx,
                );
            }

            // Check completion
            if sequencer.is_complete() {
                tracing::info!("acquisition complete");
                finalize_acquisition(
                    evt_tx,
                    &mut dataset,
                    &mut sweep_sequence,
                    &mut sweep_start_us,
                    &mut sweep_end_us,
                    true,
                );
                acquiring = false;
            }
        } else if previewing {
            // Preview: render stimulus pattern with cycling progress
            // (no recording). Cycle duration mirrors the configured sweep
            // speed so the preview reflects the actual acquisition rate.
            let now_us = qpc_to_us(query_qpc(), qpc_freq);
            let elapsed_sec = (now_us - preview_start_us) as f32 / 1_000_000.0;
            let cycle_sec = preview_cycle_sec_actual;
            let progress = (elapsed_sec % cycle_sec) / cycle_sec;
            renderer.update_frame(&queue, progress, 0, elapsed_sec);

            match surface.get_current_texture() {
                Ok(frame) => {
                    let view = frame
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());
                    renderer.render(&device, &queue, &view);
                    frame.present();
                }
                Err(e) => {
                    tracing::warn!(error = %e, "preview render error");
                }
            }

            // Capture preview frame for scientist's sidebar
            if last_preview_sent.elapsed() >= preview_interval {
                last_preview_sent = Instant::now();
                capture_preview_frame(
                    &PreviewTarget {
                        device: &device,
                        queue: &queue,
                        view: &preview_view,
                        texture: &preview_texture,
                        staging_buffer: &staging_buffer,
                        width: preview_w,
                        height: preview_h,
                        bytes_per_row,
                    },
                    &mut renderer,
                    false,
                    bg_luminance as f32,
                    evt_tx,
                );
            }

            // Wait for vsync to avoid tearing
            unsafe {
                let _ = dxgi_output.WaitForVBlank();
            }
        } else {
            // Idle â€” render gray, sleep to avoid busy-waiting.
            // Errors during idle render (e.g. "surface has changed" when
            // the OS resizes/reconfigures the swap chain) are throttled
            // to once-per-second so we don't drown stderr at 60Hz and
            // back-pressure other prints from the analysis worker.
            if let Err(e) = render_clear(&surface, &device, &queue, bg_color) {
                static LAST_LOG: std::sync::OnceLock<std::sync::Mutex<Option<Instant>>> =
                    std::sync::OnceLock::new();
                let cell = LAST_LOG.get_or_init(|| std::sync::Mutex::new(None));
                if let Ok(mut last) = cell.lock() {
                    let now = Instant::now();
                    if last
                        .map(|t| now.duration_since(t).as_secs() >= 1)
                        .unwrap_or(true)
                    {
                        tracing::warn!(error = %e, "idle render error (throttled)");
                        *last = Some(now);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(idle_sleep_ms as u64));
        }
    }
}

// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// Pure drop-detection policy (testable, no Windows/threading deps).
// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Cumulative-drop fraction above which the run is declared catastrophic.
/// 5% loss = ~10Ã— the noise floor we see in healthy runs.
#[cfg(any(windows, test))]
const CATASTROPHIC_DROP_FRACTION: f64 = 0.05;

/// Single-gap size (in frames) above which the run is declared catastrophic.
/// ~1 second of blackout at 60 Hz is a clear hardware failure, not a glitch.
#[cfg(any(windows, test))]
const CATASTROPHIC_GAP_FRAMES: u64 = 60;

/// Catastrophic-drop policy. Returns `true` when the current acquisition
/// should be aborted (the stimulus thread surfaces the loss, hands back the
/// partial run, and stays alive for the next acquisition) instead of continuing.
///
/// Two independent triggers, either fires:
/// - cumulative loss fraction >= `CATASTROPHIC_DROP_FRACTION` (5%)
/// - last gap size >= `CATASTROPHIC_GAP_FRAMES` (60 frames â‰ˆ 1 s @ 60 Hz)
///
/// Pure function â€” extracted out of the Windows-only render loop so
/// the policy can be unit-tested on every platform. Available on
/// Windows for the actual stimulus thread; available everywhere for
/// tests via `#[cfg(any(windows, test))]`.
#[cfg(any(windows, test))]
pub(crate) fn is_catastrophic_drop(
    cumulative_drops: u64,
    observed_frames: u64,
    last_gap_frames: u64,
) -> bool {
    if observed_frames == 0 {
        return false; // can't compute a fraction; not enough info
    }
    let fraction = cumulative_drops as f64 / observed_frames as f64;
    last_gap_frames >= CATASTROPHIC_GAP_FRAMES || fraction >= CATASTROPHIC_DROP_FRACTION
}

/// Frames missed between two successive DWM present counts. `prev == 0` means
/// "no baseline established yet" (the first frame of a run) and never reports a
/// gap; `current <= prev + 1` is the no-drop case.
///
/// The DWM present count is monotonic for the lifetime of the *process*, not
/// per acquisition. Each `StartAcquisition` therefore zeroes the baseline
/// (`last_present_count = 0`) so a fresh run re-establishes it on its first
/// frame — otherwise run N+1's first frame would charge every DWM present that
/// occurred while the app sat idle between runs as "dropped frames", trip the
/// catastrophic-drop abort, and (formerly) kill the stimulus thread, which is
/// what forced a full app restart before every second acquisition.
///
/// Pure function, extracted out of the Windows-only render loop so the reset
/// semantics are unit-testable on every platform.
#[cfg(any(windows, test))]
pub(crate) fn present_count_gap(prev: u64, current: u64) -> u64 {
    if prev == 0 || current <= prev + 1 {
        0
    } else {
        current - prev - 1
    }
}

/// Per-acquisition DWM frame-drop accounting. Owns the three counters that must
/// be reset together at the start of every run, so "forget to reset a field"
/// (the bug behind the every-second-acquisition failure) is impossible — the
/// caller resets one object, not three loose variables. The drop *policy* stays
/// in the pure [`present_count_gap`] / [`is_catastrophic_drop`] helpers this
/// calls; this only owns the mutable state plus the (per-process) warmup window.
#[cfg(any(windows, test))]
struct DropMonitor {
    /// Frames ignored at run start (composition / first-render JIT spuriously
    /// "drops" a few). Configuration, not per-run state — survives `reset`.
    warmup_frames: u64,
    observed_frames: u64,
    cumulative_drops: u64,
    last_present_count: u64,
}

#[cfg(any(windows, test))]
impl DropMonitor {
    fn new(warmup_frames: u64) -> Self {
        Self {
            warmup_frames,
            observed_frames: 0,
            cumulative_drops: 0,
            last_present_count: 0,
        }
    }

    /// Begin a fresh acquisition: zero the counters AND the present-count
    /// baseline (so the first frame re-establishes it — see [`present_count_gap`]).
    /// The warmup window is configuration, not per-run state, so it persists.
    fn reset(&mut self) {
        self.observed_frames = 0;
        self.cumulative_drops = 0;
        self.last_present_count = 0;
    }

    /// Observe one presented frame at DWM `present_count`. Returns the number of
    /// frames dropped immediately before it (0 within the warmup window or when
    /// none were missed), accumulating any drop into `cumulative_drops`.
    fn observe(&mut self, present_count: u64) -> u64 {
        self.observed_frames += 1;
        let gap = present_count_gap(self.last_present_count, present_count);
        self.last_present_count = present_count;
        if self.observed_frames > self.warmup_frames && gap > 0 {
            self.cumulative_drops += gap;
            gap
        } else {
            0
        }
    }

    fn observed_frames(&self) -> u64 {
        self.observed_frames
    }

    fn cumulative_drops(&self) -> u64 {
        self.cumulative_drops
    }

    fn drop_fraction(&self) -> f64 {
        if self.observed_frames == 0 {
            0.0
        } else {
            self.cumulative_drops as f64 / self.observed_frames as f64
        }
    }

    fn is_catastrophic(&self, last_gap: u64) -> bool {
        is_catastrophic_drop(self.cumulative_drops, self.observed_frames, last_gap)
    }
}

/// Finalize the in-flight acquisition: stop recording and hand the dataset +
/// realized sweep schedule back as a `Complete` event (the sweep vectors are
/// drained so the next run starts empty), tagged with whether it ended normally.
/// The single place a `Complete` is emitted — the normal-completion, user-`Stop`,
/// and catastrophic-abort paths all route through here so the handoff can't drift
/// between them. The caller owns the `acquiring` flag, the `Stopped` event, and
/// any `sequencer.stop()` (those differ per path).
#[cfg(windows)]
fn finalize_acquisition(
    evt_tx: &Sender<StimulusEvt>,
    dataset: &mut Option<StimulusDataset>,
    sweep_sequence: &mut Vec<String>,
    sweep_start_us: &mut Vec<i64>,
    sweep_end_us: &mut Vec<i64>,
    completed_normally: bool,
) {
    if let Some(mut ds) = dataset.take() {
        ds.stop_recording();
        let _ = evt_tx.send(StimulusEvt::Complete(Box::new(AcquisitionResult {
            dataset: ds,
            sweep_sequence: std::mem::take(sweep_sequence),
            sweep_start_us: std::mem::take(sweep_start_us),
            sweep_end_us: std::mem::take(sweep_end_us),
            completed_normally,
        })));
    }
}

#[cfg(test)]
mod tests {
    use super::{is_catastrophic_drop, present_count_gap, DropMonitor};

    #[test]
    fn fresh_baseline_reports_no_gap() {
        // prev == 0 is the per-run reset state: the first frame of a run
        // establishes the baseline and must never report a gap, no matter how
        // far the process-lifetime DWM present count has advanced.
        assert_eq!(present_count_gap(0, 1), 0);
        assert_eq!(present_count_gap(0, 50_000), 0);
    }

    #[test]
    fn consecutive_presents_have_no_gap() {
        assert_eq!(present_count_gap(100, 101), 0); // next frame
        assert_eq!(present_count_gap(100, 100), 0); // same (defensive)
    }

    #[test]
    fn one_missed_present_is_a_gap_of_one() {
        assert_eq!(present_count_gap(100, 102), 1);
    }

    #[test]
    fn drop_monitor_reset_clears_state_and_baseline() {
        let mut m = DropMonitor::new(0); // no warmup
        m.observe(100); // baseline
        m.observe(200); // gap of 99 charged
        assert!(m.cumulative_drops() > 0);
        assert_eq!(m.observed_frames(), 2);

        m.reset();
        assert_eq!(m.observed_frames(), 0);
        assert_eq!(m.cumulative_drops(), 0);
        // After reset the first frame re-establishes the baseline → no phantom
        // gap, however far the process-lifetime present count has advanced.
        // This is the every-second-acquisition bug, now impossible to forget.
        assert_eq!(m.observe(51_800), 0);
        assert_eq!(m.cumulative_drops(), 0);
    }

    #[test]
    fn drop_monitor_ignores_gaps_within_warmup() {
        let mut m = DropMonitor::new(5);
        assert_eq!(m.observe(10), 0); // frame 1: baseline
        assert_eq!(m.observe(20), 0); // frame 2 (≤ warmup): gap not charged
        assert_eq!(m.cumulative_drops(), 0);
    }

    #[test]
    fn drop_monitor_charges_gaps_past_warmup() {
        let mut m = DropMonitor::new(1);
        assert_eq!(m.observe(10), 0); // frame 1: baseline
        assert_eq!(m.observe(13), 2); // frame 2 (> warmup): 10→13 = 2 missed
        assert_eq!(m.cumulative_drops(), 2);
        assert_eq!(m.observed_frames(), 2);
    }

    #[test]
    fn idle_between_runs_without_reset_would_be_a_huge_gap() {
        // The bug: run 2's first frame, baseline NOT reset, present count
        // advanced by ~thousands while idle → a catastrophic phantom gap. The
        // reset (prev == 0) is what prevents this; this documents the failure
        // mode the reset guards against.
        assert_eq!(present_count_gap(50_000, 51_800), 1_799);
        assert!(super::is_catastrophic_drop(1_799, 10_001, 1_799));
    }

    #[test]
    fn zero_observed_frames_is_not_catastrophic() {
        // Defensive: can't divide by zero; can't conclude catastrophe.
        assert!(!is_catastrophic_drop(0, 0, 0));
        assert!(!is_catastrophic_drop(10, 0, 5));
    }

    #[test]
    fn small_drops_under_fraction_threshold_are_safe() {
        // 4 drops / 100 frames = 4%; gap 1 frame; both below threshold.
        assert!(!is_catastrophic_drop(4, 100, 1));
    }

    #[test]
    fn at_fraction_boundary_is_catastrophic() {
        // 5 / 100 = exactly 5% â†’ catastrophic (>=, not >).
        assert!(is_catastrophic_drop(5, 100, 1));
    }

    #[test]
    fn over_fraction_threshold_is_catastrophic() {
        assert!(is_catastrophic_drop(10, 100, 1)); // 10%
    }

    #[test]
    fn single_gap_below_threshold_is_safe() {
        assert!(!is_catastrophic_drop(59, 10_000, 59)); // 0.59% fraction, gap 59
    }

    #[test]
    fn single_gap_at_threshold_is_catastrophic() {
        // gap == CATASTROPHIC_GAP_FRAMES (60) â†’ catastrophic.
        assert!(is_catastrophic_drop(60, 10_000, 60));
    }

    #[test]
    fn single_gap_over_threshold_is_catastrophic_even_with_tiny_fraction() {
        // 120 drops / 1M frames = 0.012%, but the single gap of 120
        // is itself catastrophic. Either trigger fires.
        assert!(is_catastrophic_drop(120, 1_000_000, 120));
    }
}
