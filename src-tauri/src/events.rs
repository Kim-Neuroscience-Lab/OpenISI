//! Event forwarder — bridges crossbeam channels to Tauri events.
//!
//! Runs on its own thread. Drains events from stimulus and camera threads,
//! updates workspace state, and emits Tauri events to the frontend.
//! This is the only place where crossbeam Receiver channels are consumed.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::export::AccumulatedData;
use crate::messages::{CameraEvt, StimulusEvt};
use crate::state::{AcquisitionSummary, AppState, CameraFrameCache};

/// Interval for checking channels (10ms — responsive but not busy).
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Minimum interval between camera frame events to the frontend (~10fps).
const CAMERA_FRAME_INTERVAL: Duration = Duration::from_millis(100);

// ── Tauri event payloads ───────────────────────────────────────────────

#[derive(Clone, Serialize)]
pub struct CameraStatusPayload {
    pub connected: bool,
    pub model: Option<String>,
    pub width_px: Option<u32>,
    pub height_px: Option<u32>,
}

#[derive(Clone, Serialize)]
pub struct CameraFramePayload {
    /// PNG-encoded 8-bit grayscale image bytes.
    pub png_bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub sequence_number: u64,
}

#[derive(Clone, Serialize)]
pub struct StimulusFramePayload {
    pub state: String,
    pub condition: String,
    pub sweep_index: usize,
    pub total_sweeps: usize,
    pub state_progress: f64,
    pub frame_delta_us: i64,
    pub elapsed_sec: f64,
    pub remaining_sec: f64,
}

#[derive(Clone, Serialize)]
pub struct StimulusCompletePayload {
    pub summary: AcquisitionSummary,
}

#[derive(Clone, Serialize)]
pub struct StimulusPreviewPayload {
    /// PNG-encoded RGBA image bytes.
    pub png_bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Serialize)]
pub struct ErrorPayload {
    pub source: String,
    pub message: String,
}

// ── Event forwarder loop ───────────────────────────────────────────────

pub fn run_event_forwarder(app: AppHandle, state: Arc<Mutex<AppState>>) {
    eprintln!("[events] event forwarder started");

    let mut last_camera_frame_emit = Instant::now() - CAMERA_FRAME_INTERVAL;

    loop {
        let mut did_work = false;

        // ── Drain camera events ────────────────────────────────────────
        {
            let app_state = match state.lock() {
                Ok(s) => s,
                Err(_) => { eprintln!("[events] state lock poisoned, exiting event forwarder"); return; }
            };
            let rx = app_state.threads.camera_rx.clone();
            drop(app_state);

            if let Some(rx) = rx {
                while let Ok(evt) = rx.try_recv() {
                    did_work = true;
                    match evt {
                        CameraEvt::Enumerated(devices) => {
                            eprintln!("[events] camera enumerated: {} device(s)", devices.len());
                            let _ = app.emit("camera:enumerated", devices);
                        }
                        CameraEvt::Connected(info) => {
                            eprintln!("[events] camera connected: {}x{}", info.width_px, info.height_px);
                            {
                                let mut s = match state.lock() {
                                    Ok(s) => s,
                                    Err(_) => { eprintln!("[events] state lock poisoned"); continue; }
                                };
                                // Read current exposure from rig config (the single source of truth).
                                let exposure_us = s.config.lock()
                                    .map(|cfg| cfg.rig.camera.exposure_us)
                                    .unwrap_or(0);
                                s.session.camera_connected = true;
                                s.session.camera = Some(crate::session::CameraInfo {
                                    model: info.model.clone(),
                                    width_px: info.width_px,
                                    height_px: info.height_px,
                                    bits_per_pixel: info.bits_per_pixel,
                                    exposure_us,
                                });
                            }
                            let _ = app.emit("camera:status", CameraStatusPayload {
                                connected: true,
                                model: Some(info.model),
                                width_px: Some(info.width_px),
                                height_px: Some(info.height_px),
                            });
                        }
                        CameraEvt::Disconnected => {
                            eprintln!("[events] camera disconnected");
                            let was_acquiring = {
                                let mut s = match state.lock() {
                                    Ok(s) => s,
                                    Err(_) => { eprintln!("[events] state lock poisoned"); continue; }
                                };
                                let was = s.session.is_acquiring;
                                s.session.camera_connected = false;
                                s.session.camera = None;
                                s.latest_camera_frame = None;
                                // If acquiring, stop the stimulus thread.
                                if was {
                                    if let Some(ref tx) = s.threads.stimulus_tx {
                                        let _ = tx.send(crate::messages::StimulusCmd::Stop);
                                    }
                                }
                                was
                            };
                            let _ = app.emit("camera:status", CameraStatusPayload {
                                connected: false,
                                model: None,
                                width_px: None,
                                height_px: None,
                            });
                            if was_acquiring {
                                let _ = app.emit("error", ErrorPayload {
                                    source: "camera".into(),
                                    message: "Camera disconnected during acquisition. Data saved as partial.".into(),
                                });
                            }
                        }
                        CameraEvt::Frame(frame) => {
                            // Always cache the latest frame for on-demand retrieval.
                            let width = frame.width;
                            let height = frame.height;
                            let seq = frame.sequence_number;
                            let hw_ts = frame.hardware_timestamp_us;
                            let _sys_ts = frame.system_timestamp_us;
                            let pixels = frame.pixels;
                            {
                                let mut s = match state.lock() {
                                    Ok(s) => s,
                                    Err(_) => { eprintln!("[events] state lock poisoned"); continue; }
                                };
                                s.latest_camera_frame = Some(CameraFrameCache {
                                    pixels: pixels.clone(),
                                    width,
                                    height,
                                    sequence_number: seq,
                                });
                                // Track recent timestamps for timing validation.
                                let cap = s.camera_ring_capacity;
                                s.camera_hw_timestamps_ring.push(hw_ts);
                                s.camera_sys_timestamps_ring.push(_sys_ts);
                                if s.camera_hw_timestamps_ring.len() > cap {
                                    let excess = s.camera_hw_timestamps_ring.len() - cap;
                                    s.camera_hw_timestamps_ring.drain(..excess);
                                    s.camera_sys_timestamps_ring.drain(..excess);
                                }
                                // Accumulate ALL camera frames during acquisition.
                                if let Some(ref mut acq) = s.acquisition {
                                    acq.accumulator.add_frame(pixels.clone(), hw_ts, _sys_ts, seq);
                                }
                            }

                            // Throttled PNG encode + emit for UI preview.
                            let now = Instant::now();
                            if now.duration_since(last_camera_frame_emit) >= CAMERA_FRAME_INTERVAL {
                                last_camera_frame_emit = now;
                                if let Some(png_bytes) = encode_16bit_to_png(&pixels, width, height) {
                                    let _ = app.emit("camera:frame", CameraFramePayload {
                                        png_bytes,
                                        width,
                                        height,
                                        sequence_number: seq,
                                    });
                                }
                            }
                        }
                        CameraEvt::Error(msg) => {
                            eprintln!("[events] camera error: {msg}");
                            let _ = app.emit("error", ErrorPayload {
                                source: "camera".into(),
                                message: msg,
                            });
                        }
                    }
                }
            }
        }

        // ── Drain stimulus events ──────────────────────────────────────
        {
            let app_state = match state.lock() {
                Ok(s) => s,
                Err(_) => { eprintln!("[events] state lock poisoned, exiting event forwarder"); return; }
            };
            let rx = app_state.threads.stimulus_rx.clone();
            drop(app_state);

            if let Some(rx) = rx {
                while let Ok(evt) = rx.try_recv() {
                    did_work = true;
                    match evt {
                        StimulusEvt::Ready => {
                            eprintln!("[events] stimulus thread ready");
                            let _ = app.emit("stimulus:ready", ());
                        }
                        StimulusEvt::Frame(f) => {
                            let _ = app.emit("stimulus:frame", StimulusFramePayload {
                                state: f.state,
                                condition: f.condition,
                                sweep_index: f.sweep_index,
                                total_sweeps: f.total_sweeps,
                                state_progress: f.state_progress,
                                frame_delta_us: f.frame_delta_us,
                                elapsed_sec: f.elapsed_sec,
                                remaining_sec: f.remaining_sec,
                            });
                        }
                        StimulusEvt::PreviewFrame(pf) => {
                            if let Some(png_bytes) = encode_rgba_to_png(&pf.rgba_pixels, pf.width, pf.height) {
                                let _ = app.emit("stimulus:preview", StimulusPreviewPayload {
                                    png_bytes,
                                    width: pf.width,
                                    height: pf.height,
                                });
                            }
                        }
                        StimulusEvt::Complete(result) => {
                            eprintln!("[events] acquisition complete");
                            let ds_cfg = result.dataset.config();
                            let total_sweeps = ds_cfg.conditions.len() * ds_cfg.repetitions as usize;
                            let total_frames = result.dataset.frame_count();
                            let dropped_frames = result.dataset.dropped_frame_indices.len();
                            let duration_sec = total_frames as f64
                                * (1.0 / ds_cfg.measured_refresh_hz.max(1.0));

                            // Finish accumulator and store as pending save.
                            {
                                let mut s = match state.lock() {
                                    Ok(s) => s,
                                    Err(_) => { eprintln!("[events] state lock poisoned"); continue; }
                                };

                                let (accumulated, acq_experiment, acq_rig_geometry,
                                     acq_camera_exposure_us, acq_camera_binning, acq_display_settings,
                                     acq_hardware_snapshot, acq_timing_characterization) =
                                    if let Some(acq) = s.end_acquisition() {
                                        eprintln!("[events] accumulator: {}", acq.accumulator.stats());
                                        let data = acq.accumulator.finish();
                                        (data, acq.experiment, acq.rig_geometry,
                                         acq.camera_exposure_us, acq.camera_binning, acq.display_settings,
                                         acq.hardware_snapshot, acq.timing_characterization)
                                    } else {
                                        eprintln!("[events] WARNING: no acquisition state at completion");
                                        (AccumulatedData {
                                            frames: Vec::new(),
                                            hardware_timestamps_us: Vec::new(),
                                            system_timestamps_us: Vec::new(),
                                            sequence_numbers: Vec::new(),
                                            width: 0,
                                            height: 0,
                                        },
                                        s.experiment.clone(),
                                        crate::config::RigGeometry { viewing_distance_cm: 0.0 },
                                        0, 1,
                                        crate::config::DisplaySettings { target_stimulus_fps: 0, monitor_rotation_deg: 0.0 },
                                        None, None)
                                    };

                                let schedule = crate::export::SweepSchedule {
                                    sweep_sequence: result.sweep_sequence,
                                    sweep_start_us: result.sweep_start_us,
                                    sweep_end_us: result.sweep_end_us,
                                };

                                s.pending_save = Some(crate::state::PendingSave {
                                    camera_data: accumulated,
                                    stimulus_dataset: result.dataset,
                                    schedule,
                                    completed_normally: result.completed_normally,
                                    experiment: acq_experiment,
                                    hardware_snapshot: acq_hardware_snapshot,
                                    timing_characterization: acq_timing_characterization,
                                    rig_geometry: acq_rig_geometry,
                                    camera_exposure_us: acq_camera_exposure_us,
                                    camera_binning: acq_camera_binning,
                                    display_settings: acq_display_settings,
                                });

                                s.session.is_acquiring = false;
                            }

                            // Emit to frontend — user decides whether to save.
                            let summary = AcquisitionSummary {
                                total_sweeps,
                                total_frames,
                                dropped_frames,
                                duration_sec,
                                file_path: None,
                            };
                            let _ = app.emit("stimulus:complete", StimulusCompletePayload {
                                summary,
                            });
                        }
                        StimulusEvt::Stopped => {
                            eprintln!("[events] acquisition stopped");
                            {
                                let mut s = match state.lock() {
                                    Ok(s) => s,
                                    Err(_) => { eprintln!("[events] state lock poisoned"); continue; }
                                };
                                s.session.is_acquiring = false;
                            }
                            let _ = app.emit("stimulus:stopped", ());
                        }
                        StimulusEvt::Error(msg) => {
                            eprintln!("[events] stimulus error: {msg}");
                            {
                                let mut s = match state.lock() {
                                    Ok(s) => s,
                                    Err(_) => { eprintln!("[events] state lock poisoned"); continue; }
                                };
                                s.session.is_acquiring = false;
                            }
                            let _ = app.emit("error", ErrorPayload {
                                source: "stimulus".into(),
                                message: msg,
                            });
                        }
                    }
                }
            }
        }

        // Sleep if no work done to avoid busy-spinning.
        if !did_work {
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

// ── PNG encoding ───────────────────────────────────────────────────────

/// Public wrapper for use by commands.rs.
pub fn encode_16bit_to_png_pub(pixels: &[u16], width: u32, height: u32) -> Option<Vec<u8>> {
    encode_16bit_to_png(pixels, width, height)
}

/// Build a hardware snapshot from the current app state.
pub(crate) fn build_hardware_snapshot(state: &crate::state::AppState) -> Option<crate::export::HardwareSnapshot> {
    let monitor = state.session.selected_display.as_ref()?;
    let measured_hz = state.session.display_measured_refresh_hz();
    let cam = state.session.camera.as_ref()
        .expect("Camera info must be available when building hardware snapshot");
    let (cam_model, cam_w, cam_h) = (cam.model.clone(), cam.width_px, cam.height_px);

    Some(crate::export::HardwareSnapshot {
        monitor_name: monitor.name.clone(),
        monitor_width_px: monitor.width_px,
        monitor_height_px: monitor.height_px,
        monitor_width_cm: monitor.width_cm,
        monitor_height_cm: monitor.height_cm,
        monitor_refresh_hz: monitor.refresh_hz as f64,
        measured_refresh_hz: measured_hz,
        gamma_corrected: false, // OpenISI does not currently apply gamma correction
        camera_model: cam_model,
        camera_width_px: cam_w,
        camera_height_px: cam_h,
    })
}

/// Encode 16-bit grayscale pixels to an 8-bit grayscale PNG.
/// This runs ~10 times per second for UI preview only — not in any hot path.
fn encode_16bit_to_png(pixels: &[u16], width: u32, height: u32) -> Option<Vec<u8>> {
    let expected = (width * height) as usize;
    if pixels.len() < expected {
        return None;
    }

    // Convert 16-bit to 8-bit by shifting right 8 bits.
    let pixels_8bit: Vec<u8> = pixels[..expected]
        .iter()
        .map(|&p| (p >> 8) as u8)
        .collect();

    let mut buf = Vec::with_capacity(expected + 1024);
    {
        let mut encoder = png::Encoder::new(&mut buf, width, height);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        // Fast compression for preview — speed over size.
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&pixels_8bit).ok()?;
    }
    Some(buf)
}

/// Encode RGBA pixels to an RGBA PNG.
/// Used for stimulus preview frames (~10 fps, small resolution).
fn encode_rgba_to_png(pixels: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let expected = (width * height * 4) as usize;
    if pixels.len() < expected {
        return None;
    }

    let mut buf = Vec::with_capacity(expected / 2);
    {
        let mut encoder = png::Encoder::new(&mut buf, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&pixels[..expected]).ok()?;
    }
    Some(buf)
}
