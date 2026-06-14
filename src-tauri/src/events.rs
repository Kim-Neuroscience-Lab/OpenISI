//! Event forwarder — bridges crossbeam channels to Tauri events.
//!
//! Runs on its own thread. Drains events from stimulus and camera threads,
//! updates workspace state, and emits Tauri events to the frontend.
//! This is the only place where crossbeam Receiver channels are consumed.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::export::AccumulatedData;
use crate::messages::{AnalysisEvt, CameraEvt, StimulusEvt};
use crate::state::{AcquisitionSummary, AppState, CameraFrameCache};

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
    /// Stable machine-readable code (e.g. `E_DISCONNECTED`) for terminal,
    /// typed failures — mirrors the `AppErrorWire.code` that command returns
    /// carry, so the frontend can pattern-match push-event errors the same way.
    /// `None` for transient/string-only notifications.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<&'static str>,
    /// Top-level category (e.g. `Acquisition`) for terminal typed failures.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<&'static str>,
}

impl ErrorPayload {
    /// A transient/notification error with no stable code (string-only).
    fn transient(source: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            message: message.into(),
            code: None,
            category: None,
        }
    }

    /// A terminal, typed error — carries the same stable `code`/`category` the
    /// IPC command wire exposes, derived from the dedicated `AppError` system.
    fn typed(source: impl Into<String>, err: &crate::error::AppError) -> Self {
        let wire = err.to_wire();
        Self {
            source: source.into(),
            message: wire.message,
            code: Some(wire.code),
            category: Some(wire.category),
        }
    }
}

#[derive(Clone, Serialize)]
pub struct AnalysisLifecyclePayload {
    /// `.oisi` file path as string (JSON-friendly).
    pub path: String,
    /// Human-readable message; empty unless the event carries one.
    pub message: String,
}

// ── Event forwarder loop ───────────────────────────────────────────────

pub fn run_event_forwarder(app: AppHandle, state: Arc<AppState>) {
    tracing::debug!("event forwarder started");

    let mut last_camera_frame_emit = Instant::now() - CAMERA_FRAME_INTERVAL;

    // Cache the receivers once — they're `Clone` and immutable after startup,
    // so the drain loop never touches state to receive.
    let analysis_rx = state.threads.analysis_rx.clone();
    let camera_rx = state.threads.camera_rx.clone();
    let stimulus_rx = state.threads.stimulus_rx.clone();

    // The analysis channel can disconnect (the worker exits) WITHOUT ending
    // event forwarding for camera/stimulus, so track its liveness and drop it
    // from the blocking wait below once dead — otherwise a disconnected
    // receiver reports "ready" forever and the wait would spin.
    let mut analysis_live = true;

    loop {
        // ── Drain analysis worker events ───────────────────────────────
        {
            let rx = &analysis_rx;
            {
                use crossbeam_channel::TryRecvError;
                loop {
                    let evt = match rx.try_recv() {
                        Ok(e) => e,
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            analysis_live = false;
                            break;
                        }
                    };
                    let (event_name, payload) = match evt {
                        AnalysisEvt::Started { path } => (
                            "analysis:started",
                            AnalysisLifecyclePayload {
                                path: path.to_string_lossy().into_owned(),
                                message: String::new(),
                            },
                        ),
                        AnalysisEvt::Complete { path, message } => (
                            "analysis:complete",
                            AnalysisLifecyclePayload {
                                path: path.to_string_lossy().into_owned(),
                                message,
                            },
                        ),
                        AnalysisEvt::Failed { path, error } => {
                            tracing::error!(%error, "analysis failed");
                            (
                                "analysis:failed",
                                AnalysisLifecyclePayload {
                                    path: path.to_string_lossy().into_owned(),
                                    message: error,
                                },
                            )
                        }
                        AnalysisEvt::Cancelled { path } => (
                            "analysis:cancelled",
                            AnalysisLifecyclePayload {
                                path: path.to_string_lossy().into_owned(),
                                message: String::new(),
                            },
                        ),
                    };
                    let _ = app.emit(event_name, payload);
                }
            }
        }

        // ── Drain camera events ────────────────────────────────────────
        {
            let rx = &camera_rx;
            {
                use crossbeam_channel::TryRecvError;
                loop {
                    let evt = match rx.try_recv() {
                        Ok(e) => e,
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            // Camera worker thread is gone. Stop forwarding;
                            // no more events will ever arrive on this channel.
                            tracing::warn!("camera channel disconnected, exiting forwarder");
                            return;
                        }
                    };
                    match evt {
                        CameraEvt::Enumerated(devices) => {
                            tracing::info!(devices = devices.len(), "camera enumerated");
                            let _ = app.emit("camera:enumerated", devices);
                        }
                        CameraEvt::Connected(info) => {
                            tracing::info!(
                                width = info.width_px,
                                height = info.height_px,
                                "camera connected"
                            );
                            // Read current exposure from the config store (SSoT).
                            let exposure_us = state.config.lock().rig().camera.exposure_us;
                            {
                                let mut session = state.session.lock();
                                session.camera_connected = true;
                                session.camera = Some(crate::session::CameraInfo {
                                    model: info.model.clone(),
                                    width_px: info.width_px,
                                    height_px: info.height_px,
                                    bits_per_pixel: info.bits_per_pixel,
                                    exposure_us,
                                });
                            }
                            let _ = app.emit(
                                "camera:status",
                                CameraStatusPayload {
                                    connected: true,
                                    model: Some(info.model),
                                    width_px: Some(info.width_px),
                                    height_px: Some(info.height_px),
                                },
                            );
                        }
                        CameraEvt::Disconnected => {
                            tracing::info!("camera disconnected");
                            // Order: session → capture → send. One group at a time.
                            let was_acquiring = {
                                let mut session = state.session.lock();
                                let was = session.is_acquiring;
                                session.camera_connected = false;
                                session.camera = None;
                                was
                            };
                            state.capture.lock().latest_frame = None;
                            // If acquiring, stop the stimulus thread.
                            if was_acquiring {
                                let _ = state
                                    .threads
                                    .stimulus_tx
                                    .send(crate::messages::StimulusCmd::Stop);
                            }
                            let _ = app.emit(
                                "camera:status",
                                CameraStatusPayload {
                                    connected: false,
                                    model: None,
                                    width_px: None,
                                    height_px: None,
                                },
                            );
                            if was_acquiring {
                                let _ = app.emit("error", ErrorPayload::transient(
                                    "camera",
                                    "Camera disconnected during acquisition. Data saved as partial.",
                                ));
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
                            // Hot path: one `capture` critical section for the
                            // frame cache, timing ring push, and accumulator.
                            {
                                let mut capture = state.capture.lock();
                                capture.latest_frame = Some(CameraFrameCache {
                                    pixels: pixels.clone(),
                                    width,
                                    height,
                                    sequence_number: seq,
                                });
                                // Track recent timestamps for timing validation.
                                capture.timing.push(hw_ts, _sys_ts);
                                // Accumulate ALL camera frames during acquisition.
                                if let Some(ref mut acq) = capture.acquisition {
                                    acq.accumulator
                                        .add_frame(pixels.clone(), hw_ts, _sys_ts, seq);
                                }
                            }

                            // Throttled PNG encode + emit for UI preview.
                            let now = Instant::now();
                            if now.duration_since(last_camera_frame_emit) >= CAMERA_FRAME_INTERVAL {
                                last_camera_frame_emit = now;
                                if let Some(png_bytes) = encode_16bit_to_png(&pixels, width, height)
                                {
                                    let _ = app.emit(
                                        "camera:frame",
                                        CameraFramePayload {
                                            png_bytes,
                                            width,
                                            height,
                                            sequence_number: seq,
                                        },
                                    );
                                }
                            }
                        }
                        CameraEvt::Error(msg) => {
                            tracing::warn!(%msg, "camera error (transient)");
                            let _ = app.emit("error", ErrorPayload::transient("camera", msg));
                        }
                        CameraEvt::Fatal(err) => {
                            // Terminal camera failure — stop the run and notify
                            // the UI with the same typed code/category the IPC
                            // command wire carries.
                            tracing::error!(%err, "camera fatal");
                            state.session.lock().is_acquiring = false;
                            let app_err = crate::error::AppError::Acquisition(err);
                            let _ = app.emit("error", ErrorPayload::typed("camera", &app_err));
                        }
                    }
                }
            }
        }

        // ── Drain stimulus events ──────────────────────────────────────
        {
            let rx = &stimulus_rx;
            {
                use crossbeam_channel::TryRecvError;
                loop {
                    let evt = match rx.try_recv() {
                        Ok(e) => e,
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            // Stimulus worker thread is gone. Stop forwarding.
                            tracing::warn!("stimulus channel disconnected, exiting forwarder");
                            return;
                        }
                    };
                    match evt {
                        StimulusEvt::Ready => {
                            tracing::info!("stimulus thread ready");
                            let _ = app.emit("stimulus:ready", ());
                        }
                        StimulusEvt::Frame(f) => {
                            let _ = app.emit(
                                "stimulus:frame",
                                StimulusFramePayload {
                                    state: f.state,
                                    condition: f.condition,
                                    sweep_index: f.sweep_index,
                                    total_sweeps: f.total_sweeps,
                                    state_progress: f.state_progress,
                                    frame_delta_us: f.frame_delta_us,
                                    elapsed_sec: f.elapsed_sec,
                                    remaining_sec: f.remaining_sec,
                                },
                            );
                        }
                        StimulusEvt::PreviewFrame(pf) => {
                            if let Some(png_bytes) =
                                encode_rgba_to_png(&pf.rgba_pixels, pf.width, pf.height)
                            {
                                let _ = app.emit(
                                    "stimulus:preview",
                                    StimulusPreviewPayload {
                                        png_bytes,
                                        width: pf.width,
                                        height: pf.height,
                                    },
                                );
                            }
                        }
                        StimulusEvt::Complete(result) => {
                            tracing::info!("acquisition complete");
                            let result = *result;
                            let ds_cfg = result.dataset.config();
                            let total_sweeps =
                                ds_cfg.conditions.len() * ds_cfg.repetitions as usize;
                            let total_frames = result.dataset.frame_count();
                            let dropped_frames = result.dataset.dropped_frame_indices.len();
                            let duration_sec =
                                total_frames as f64 * (1.0 / ds_cfg.measured_refresh_hz.max(1.0));

                            // Finish accumulator and store as pending save.
                            // Multi-group site — strict order: capture → handoff → session.
                            // Take the acquisition out under the capture lock, then
                            // drop the guard before `finish()` (which moves all frames).
                            let acquisition = state.capture.lock().acquisition.take();

                            let (
                                accumulated,
                                acq_snapshot,
                                acq_hardware_snapshot,
                                acq_timing_characterization,
                            ) = if let Some(acq) = acquisition {
                                tracing::info!(stats = %acq.accumulator.stats(), "accumulator");
                                let data = acq.accumulator.finish();
                                (
                                    data,
                                    acq.snapshot,
                                    acq.hardware_snapshot,
                                    acq.timing_characterization,
                                )
                            } else {
                                tracing::warn!("no acquisition state at completion");
                                // Fallback: a live typed config snapshot.
                                let fallback_cfg = state.config.lock().snapshot();
                                (
                                    AccumulatedData {
                                        frames: Vec::new(),
                                        hardware_timestamps_us: Vec::new(),
                                        system_timestamps_us: Vec::new(),
                                        sequence_numbers: Vec::new(),
                                        width: 0,
                                        height: 0,
                                    },
                                    fallback_cfg,
                                    None,
                                    None,
                                )
                            };

                            let schedule = crate::export::SweepSchedule {
                                sweep_sequence: result.sweep_sequence,
                                sweep_start_us: result.sweep_start_us,
                                sweep_end_us: result.sweep_end_us,
                            };

                            state.handoff.lock().pending_save = Some(crate::state::PendingSave {
                                camera_data: accumulated,
                                stimulus_dataset: result.dataset,
                                schedule,
                                completed_normally: result.completed_normally,
                                snapshot: acq_snapshot,
                                hardware_snapshot: acq_hardware_snapshot,
                                timing_characterization: acq_timing_characterization,
                            });

                            state.session.lock().is_acquiring = false;

                            // Emit to frontend — user decides whether to save.
                            let summary = AcquisitionSummary {
                                total_sweeps,
                                total_frames,
                                dropped_frames,
                                duration_sec,
                                file_path: None,
                            };
                            let _ =
                                app.emit("stimulus:complete", StimulusCompletePayload { summary });
                        }
                        StimulusEvt::Stopped => {
                            tracing::info!("acquisition stopped");
                            state.session.lock().is_acquiring = false;
                            let _ = app.emit("stimulus:stopped", ());
                        }
                        StimulusEvt::Error(msg) => {
                            // Transient — log + show in UI, acquisition
                            // continues.
                            tracing::warn!(%msg, "stimulus error (transient)");
                            let _ = app.emit("error", ErrorPayload::transient("stimulus", msg));
                        }
                        StimulusEvt::Fatal(err) => {
                            // Terminal stimulus failure — stop the run, with the
                            // same typed code/category as the IPC command wire.
                            tracing::error!(%err, "stimulus fatal");
                            state.session.lock().is_acquiring = false;
                            let app_err = crate::error::AppError::Acquisition(err);
                            let _ = app.emit("error", ErrorPayload::typed("stimulus", &app_err));
                        }
                    }
                }
            }
        }

        // Block until at least one channel has a message — replaces the former
        // 10 ms poll-sleep, so there is no busy-idle and no poll latency: a
        // queued event wakes us immediately. A disconnected channel reports
        // ready, so the next drain pass observes it (camera/stimulus → exit;
        // analysis → mark dead and stop registering it, avoiding a spin).
        let mut sel = crossbeam_channel::Select::new();
        if analysis_live {
            sel.recv(&analysis_rx);
        }
        sel.recv(&camera_rx);
        sel.recv(&stimulus_rx);
        let _ = sel.ready();
    }
}

// ── PNG encoding ───────────────────────────────────────────────────────

/// Public wrapper for use by commands.rs.
pub fn encode_16bit_to_png_pub(pixels: &[u16], width: u32, height: u32) -> Option<Vec<u8>> {
    encode_16bit_to_png(pixels, width, height)
}

/// Build a hardware snapshot from a session snapshot. The caller holds the
/// `session` lock (or passes a guard) — this does no locking itself.
pub(crate) fn build_hardware_snapshot(
    session: &crate::session::Session,
) -> Option<crate::export::HardwareSnapshot> {
    let monitor = session.selected_display.as_ref()?;
    let measured_hz = session.display_measured_refresh_hz()?;
    let cam = session.camera.as_ref()?;
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
    let pixels_8bit: Vec<u8> = pixels[..expected].iter().map(|&p| (p >> 8) as u8).collect();

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
