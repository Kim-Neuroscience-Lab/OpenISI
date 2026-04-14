//! Camera thread — direct PCO SDK integration via Rust FFI.
//!
//! Calls sc2_cam.dll and pco_recorder.dll directly. Single process, no IPC.

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use pco_sdk::{Frame, Sdk};

use crate::config::SystemTuning;
use crate::messages::{CameraCmd, CameraConnectedInfo, CameraDeviceInfo, CameraEvt, CameraFrameData};

/// Run the camera thread. Blocks until Shutdown command received.
pub fn run(cmd_rx: Receiver<CameraCmd>, evt_tx: Sender<CameraEvt>, sys_cfg: SystemTuning) {
    // Try to load the SDK once at thread start.
    let sdk = match Sdk::load() {
        Ok(sdk) => {
            eprintln!("[camera] PCO SDK loaded successfully");
            sdk
        }
        Err(e) => {
            eprintln!("[camera] PCO SDK not available: {e}");
            // SDK not available — wait for commands, reject Connect.
            no_sdk_loop(&cmd_rx, &evt_tx);
            return;
        }
    };

    // Main loop: wait for commands, handle sessions.
    loop {
        match cmd_rx.recv() {
            Ok(CameraCmd::Enumerate) => {
                do_enumerate(&sdk, &evt_tx);
            }
            Ok(CameraCmd::Connect { index, exposure_us, binning }) => {
                do_connect(&sdk, index, exposure_us, binning, &sys_cfg, &cmd_rx, &evt_tx);
            }
            Ok(CameraCmd::Shutdown) => {
                eprintln!("[camera] shutdown");
                return;
            }
            Ok(_) => {} // Ignore other commands when disconnected.
            Err(_) => return,
        }
    }
}

/// Loop when SDK is not available — reject Connect/Enumerate, wait for Shutdown.
fn no_sdk_loop(cmd_rx: &Receiver<CameraCmd>, evt_tx: &Sender<CameraEvt>) {
    loop {
        match cmd_rx.recv() {
            Ok(CameraCmd::Enumerate) => {
                let _ = evt_tx.send(CameraEvt::Enumerated(vec![]));
            }
            Ok(CameraCmd::Connect { .. }) => {
                let _ = evt_tx.send(CameraEvt::Error(
                    "PCO SDK not loaded — sc2_cam.dll not found".into(),
                ));
            }
            Ok(CameraCmd::Shutdown) => return,
            Ok(_) => {}
            Err(_) => return,
        }
    }
}

/// Enumerate available cameras and send results.
fn do_enumerate(sdk: &Sdk, evt_tx: &Sender<CameraEvt>) {
    eprintln!("[camera] enumerating cameras...");
    let cameras = sdk.enumerate_cameras(4);
    let devices: Vec<CameraDeviceInfo> = cameras
        .iter()
        .map(|c| CameraDeviceInfo {
            index: c.index,
            name: format!("{} (SN: {}, {})", c.name, c.serial_number, c.interface_type),
            width: c.width,
            height: c.height,
            max_fps: c.max_fps,
        })
        .collect();
    eprintln!("[camera] found {} camera(s)", devices.len());
    for d in &devices {
        eprintln!("  [{}] {} {}x{} {:.1}fps", d.index, d.name, d.width, d.height, d.max_fps);
    }
    let _ = evt_tx.send(CameraEvt::Enumerated(devices));
}

/// Handle a camera connection session. Returns when disconnected.
fn do_connect(sdk: &Sdk, camera_index: u16, initial_exposure_us: u32, binning: u16, sys_cfg: &SystemTuning, cmd_rx: &Receiver<CameraCmd>, evt_tx: &Sender<CameraEvt>) {
    // Get QPC frequency for system timestamp conversion.
    #[cfg(windows)]
    let qpc_freq = {
        let mut freq = 0i64;
        unsafe { let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut freq); }
        if freq == 0 {
            let _ = evt_tx.send(CameraEvt::Error("QueryPerformanceFrequency returned 0".into()));
            return;
        }
        freq
    };
    #[cfg(not(windows))]
    let qpc_freq = 0i64; // unused on non-Windows (SDK won't load), but needed for compilation

    // Open camera.
    let mut camera = match sdk.open_camera(camera_index) {
        Ok(cam) => cam,
        Err(e) => {
            let msg = format!("Failed to open camera: {e}");
            eprintln!("[camera] {msg}");
            let _ = evt_tx.send(CameraEvt::Error(msg));
            return;
        }
    };

    let info = camera.info();
    eprintln!(
        "[camera] opened: {}x{}, pixel rates: {:?}",
        camera.width, camera.height, info.pixel_rates
    );

    // Configure camera with persisted exposure.
    if let Err(e) = configure_camera(&mut camera, initial_exposure_us, binning) {
        let msg = format!("Failed to configure camera: {e}");
        eprintln!("[camera] {msg}");
        let _ = evt_tx.send(CameraEvt::Error(msg));
        return;
    }

    // Arm camera.
    if let Err(e) = camera.arm() {
        let msg = format!("Failed to arm camera: {e}");
        eprintln!("[camera] {msg}");
        let _ = evt_tx.send(CameraEvt::Error(msg));
        return;
    }

    let max_fps = match camera.get_max_fps() {
        Ok(fps) => fps,
        Err(e) => {
            let msg = format!("Failed to read max FPS: {e}");
            eprintln!("[camera] {msg}");
            let _ = evt_tx.send(CameraEvt::Error(msg));
            return;
        }
    };
    eprintln!(
        "[camera] armed: {}x{}, max {:.1} fps",
        camera.width, camera.height, max_fps
    );

    // Create recorder (10-frame ring buffer).
    let mut recorder = match camera.create_recorder(10) {
        Ok(rec) => rec,
        Err(e) => {
            let msg = format!("Failed to create recorder: {e}");
            eprintln!("[camera] {msg}");
            let _ = evt_tx.send(CameraEvt::Error(msg));
            return;
        }
    };

    // Start recording.
    if let Err(e) = recorder.start() {
        let msg = format!("Failed to start recording: {e}");
        eprintln!("[camera] {msg}");
        let _ = evt_tx.send(CameraEvt::Error(msg));
        return;
    }

    eprintln!("[camera] recording started, waiting for first frame...");

    // Wait for first frame.
    let deadline = Instant::now() + Duration::from_millis(sys_cfg.camera_first_frame_timeout_ms as u64);
    let first_frame = loop {
        if Instant::now() > deadline {
            let msg = format!("Timed out waiting for first frame ({}ms)", sys_cfg.camera_first_frame_timeout_ms);
            eprintln!("[camera] {msg}");
            let _ = evt_tx.send(CameraEvt::Error(msg));
            return;
        }
        match recorder.get_latest_frame() {
            Ok(Some(frame)) => break frame,
            Ok(None) => std::thread::sleep(Duration::from_millis(sys_cfg.camera_first_frame_poll_ms as u64)),
            Err(e) => {
                let msg = format!("Error reading first frame: {e}");
                eprintln!("[camera] {msg}");
                let _ = evt_tx.send(CameraEvt::Error(msg));
                return;
            }
        }
    };

    let width = first_frame.width;
    let height = first_frame.height;

    // Send connected event.
    let _ = evt_tx.send(CameraEvt::Connected(CameraConnectedInfo {
        model: "PCO".into(),
        width_px: width,
        height_px: height,
        bits_per_pixel: 16,
    }));

    // Send first frame.
    let mut last_frame_sent = Instant::now();
    send_frame(evt_tx, &first_frame, &mut last_frame_sent, qpc_freq);

    eprintln!("[camera] connected, entering frame loop");

    // Drop the initial recorder so we can use a uniform create-in-loop pattern.
    // The first frame was already sent above.
    drop(recorder);

    // Outer loop: each iteration creates a new recorder. Breaks out for exposure
    // changes (which require dropping the recorder to release the camera borrow).
    let mut pending_exposure: Option<u32> = None;
    let frame_send_interval = Duration::from_millis(sys_cfg.camera_frame_send_interval_ms as u64);
    let poll_interval = Duration::from_millis(sys_cfg.camera_poll_interval_ms as u64);

    loop {
        // Apply pending exposure change before creating recorder.
        if let Some(us) = pending_exposure.take() {
            if let Err(e) = camera.set_exposure_us(us) {
                eprintln!("[camera] set_exposure_us failed: {e}");
                let _ = evt_tx.send(CameraEvt::Error(format!("Set exposure failed: {e}")));
            }
            if let Err(e) = camera.arm() {
                eprintln!("[camera] re-arm failed: {e}");
                let _ = evt_tx.send(CameraEvt::Error(format!("Re-arm failed: {e}")));
            }
        }

        let mut recorder = match camera.create_recorder(10) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[camera] create recorder failed: {e}");
                let _ = evt_tx.send(CameraEvt::Error(format!("Recorder failed: {e}")));
                break;
            }
        };

        if let Err(e) = recorder.start() {
            eprintln!("[camera] start recording failed: {e}");
            let _ = evt_tx.send(CameraEvt::Error(format!("Start failed: {e}")));
            break;
        }

        // Inner frame loop — runs until disconnect/shutdown or exposure change.
        let mut should_exit = false;
        loop {
            match cmd_rx.try_recv() {
                Ok(CameraCmd::Disconnect) | Ok(CameraCmd::Shutdown) => {
                    should_exit = true;
                    break;
                }
                Ok(CameraCmd::SetExposure(us)) => {
                    eprintln!("[camera] SetExposure({us}µs)");
                    let _ = recorder.stop();
                    pending_exposure = Some(us);
                    break; // break inner loop, recorder dropped, outer loop creates new one
                }
                Ok(CameraCmd::Connect { .. }) => {}
                Ok(CameraCmd::Enumerate) => {}
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    should_exit = true;
                    break;
                }
            }

            match recorder.get_latest_frame() {
                Ok(Some(frame)) => {
                    let now = Instant::now();
                    if now.duration_since(last_frame_sent) >= frame_send_interval {
                        send_frame(evt_tx, &frame, &mut last_frame_sent, qpc_freq);
                    }
                    std::thread::sleep(poll_interval);
                }
                Ok(None) => {
                    std::thread::sleep(poll_interval);
                }
                Err(e) => {
                    eprintln!("[camera] frame read error: {e}");
                    let _ = evt_tx.send(CameraEvt::Error(format!("Frame read error: {e}")));
                    should_exit = true;
                    break;
                }
            }
        }
        // recorder dropped here — camera borrow released

        if should_exit {
            break;
        }
    }

    eprintln!("[camera] disconnected");
    let _ = evt_tx.send(CameraEvt::Disconnected);
}

fn configure_camera(camera: &mut pco_sdk::Camera<'_>, exposure_us: u32, binning: u16) -> pco_sdk::Result<()> {
    let rate = camera.set_max_pixel_rate()?;
    eprintln!("[camera] pixel rate set to {} MHz", rate / 1_000_000);
    if binning > 1 {
        camera.set_binning(binning, binning)?;
    }
    camera.set_timestamp_binary()?;
    camera.set_exposure_us(exposure_us)?;
    eprintln!("[camera] exposure set to {}µs", exposure_us);
    Ok(())
}

fn send_frame(evt_tx: &Sender<CameraEvt>, frame: &Frame, last_sent: &mut Instant, _qpc_freq: i64) {
    *last_sent = Instant::now();
    // Read system QPC at the moment we receive this frame — for clock sync with stimulus.
    #[cfg(windows)]
    let system_us = {
        let mut qpc = 0i64;
        unsafe { let _ = windows::Win32::System::Performance::QueryPerformanceCounter(&mut qpc); }
        ((qpc as i128 * 1_000_000) / _qpc_freq as i128) as i64
    };
    #[cfg(not(windows))]
    let system_us = {
        // Use SystemTime as a fallback (won't actually execute since SDK won't load on non-Windows).
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_micros() as i64)
            .unwrap_or(0)
    };
    let _ = evt_tx.send(CameraEvt::Frame(CameraFrameData {
        pixels: frame.pixels.clone(),
        width: frame.width,
        height: frame.height,
        sequence_number: frame.image_number as u64,
        hardware_timestamp_us: frame.timestamp.to_us_since_midnight(),
        system_timestamp_us: system_us,
    }));
}
