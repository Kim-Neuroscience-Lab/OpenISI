//! Acquisition workflow commands: session metadata, start/stop, save/discard, preview.

use serde::Serialize;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::events::build_hardware_snapshot;
use crate::export::AcquisitionAccumulator;
use crate::messages::{AcquisitionCommand, PreviewCommand, StimulusCmd};
use crate::params::RegistrySnapshot;
use crate::state::AcquisitionState;

use super::SharedState;

/// Validate experiment parameters before acquisition or preview.
fn validate_experiment(snap: &RegistrySnapshot) -> AppResult<()> {
    use crate::params::Envelope;
    match snap.stimulus_envelope() {
        Envelope::Bar => {
            if snap.sweep_speed_deg_per_sec() <= 0.0 {
                return Err(AppError::Validation(
                    "Sweep speed must be greater than zero".into(),
                ));
            }
        }
        Envelope::Wedge => {
            if snap.rotation_speed_deg_per_sec() <= 0.0 {
                return Err(AppError::Validation(
                    "Rotation speed must be greater than zero".into(),
                ));
            }
        }
        Envelope::Ring => {
            if snap.expansion_speed_deg_per_sec() <= 0.0 {
                return Err(AppError::Validation(
                    "Expansion speed must be greater than zero".into(),
                ));
            }
        }
        Envelope::Fullfield => {}
    }
    if snap.stimulus_width_deg() <= 0.0 {
        return Err(AppError::Validation(
            "Stimulus width must be greater than zero".into(),
        ));
    }
    if snap.repetitions() == 0 {
        return Err(AppError::Validation(
            "Repetitions must be at least 1".into(),
        ));
    }
    if snap.conditions().is_empty() {
        return Err(AppError::Validation("No conditions defined".into()));
    }
    Ok(())
}

/// Set session metadata (animal ID and notes).
#[tauri::command]
pub fn set_session_metadata(
    state: State<'_, SharedState>,
    animal_id: String,
    notes: String,
) -> AppResult<()> {
    let mut session = state.session.lock();
    session.animal_id = animal_id;
    session.notes = notes;
    Ok(())
}

/// Start acquisition — ties stimulus + camera together.
#[tauri::command]
pub fn start_acquisition(state: State<'_, SharedState>) -> AppResult<()> {
    // Take a snapshot for validation and acquisition.
    let snapshot = state.registry.lock().snapshot();

    validate_experiment(&snapshot)?;

    // Gather everything we need from the session in one critical section,
    // copy it out, then drop the guard before sending / building state.
    let (monitor, measured_refresh_hz, hardware_snapshot, timing_characterization, cam_w, cam_h) = {
        let session = state.session.lock();

        let monitor = session
            .selected_display
            .as_ref()
            .ok_or(AppError::Validation("No display selected".into()))?
            .clone();

        if !session.camera_connected {
            return Err(AppError::Validation("Camera not connected".into()));
        }

        let measured_refresh_hz = session
            .display_validation
            .as_ref()
            .ok_or(AppError::Validation(
                "Display not validated — validate display before acquiring".into(),
            ))?
            .measured_refresh_hz;

        // Timing validation is strongly recommended but not a hard block.
        if let Some(ref tc) = session.timing_characterization
            && tc.regime == crate::timing::TimingRegime::Systematic
        {
            tracing::warn!(
                beat_period_sec = tc.beat_period_sec,
                "systematic timing regime — every trial sees approximately the same sub-frame onset position",
            );
        }

        // Build hardware snapshot from current session state (valid at start).
        let hardware_snapshot = build_hardware_snapshot(&session);
        let timing_characterization = session.timing_characterization.clone();

        let (cam_w, cam_h) = {
            let cam = session.camera.as_ref().ok_or(AppError::NotAvailable(
                "Camera info not available during acquisition".into(),
            ))?;
            (cam.width_px, cam.height_px)
        };

        (
            monitor,
            measured_refresh_hz,
            hardware_snapshot,
            timing_characterization,
            cam_w,
            cam_h,
        )
    };

    let acq_cmd = AcquisitionCommand {
        snapshot: snapshot.clone(),
        monitor,
        measured_refresh_hz,
    };

    state
        .threads
        .stimulus_tx
        .send(StimulusCmd::StartAcquisition(acq_cmd))
        .map_err(|e| AppError::Hardware(format!("Failed to send acquisition command: {e}")))?;

    // Start camera frame accumulation with acquisition-time snapshot.
    let mut accumulator = AcquisitionAccumulator::new();
    accumulator.start(cam_w, cam_h);
    state.capture.lock().acquisition = Some(AcquisitionState {
        accumulator,
        snapshot,
        hardware_snapshot,
        timing_characterization,
    });
    state.session.lock().is_acquiring = true;

    Ok(())
}

/// Stop the current acquisition.
#[tauri::command]
pub fn stop_acquisition(state: State<'_, SharedState>) -> AppResult<()> {
    state
        .threads
        .stimulus_tx
        .send(StimulusCmd::Stop)
        .map_err(|e| AppError::Hardware(format!("Failed to send stop command: {e}")))?;
    state.session.lock().is_acquiring = false;
    Ok(())
}

/// Save the pending acquisition to a .oisi file. Called after user confirms.
#[tauri::command]
pub fn save_acquisition(state: State<'_, SharedState>, path: Option<String>) -> AppResult<String> {
    // Take the pending save + anatomical from handoff, then drop the guard.
    let (pending, anatomical) = {
        let mut handoff = state.handoff.lock();
        let pending = handoff.pending_save.take().ok_or(AppError::NotAvailable(
            "No pending acquisition to save".into(),
        ))?;
        let anatomical = handoff.anatomical.clone();
        (pending, anatomical)
    };

    // Read metadata from live state at save time.
    let (animal_id, notes) = {
        let session = state.session.lock();
        (session.animal_id.clone(), session.notes.clone())
    };

    // Determine output path.
    let output_path = if let Some(p) = path {
        std::path::PathBuf::from(p)
    } else {
        let data_dir = pending.snapshot.data_directory().to_string();
        let dir = if data_dir.is_empty() {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
                })
        } else {
            std::path::PathBuf::from(data_dir)
        };
        let _ = std::fs::create_dir_all(&dir);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let safe_id: String = animal_id
            .trim()
            .chars()
            .map(|c: char| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let filename = if safe_id.is_empty() {
            format!("acquisition_{ts}.oisi")
        } else {
            format!("{safe_id}_{ts}.oisi")
        };
        dir.join(filename)
    };

    let session_meta = crate::export::SessionMetadata { animal_id, notes };

    crate::export::write_oisi(
        &output_path,
        crate::export::OisiBundle {
            stimulus_dataset: &pending.stimulus_dataset,
            camera_data: pending.camera_data,
            snapshot: &pending.snapshot,
            hardware: pending.hardware_snapshot.as_ref(),
            schedule: &pending.schedule,
            timing: pending.timing_characterization.as_ref(),
            session_meta: Some(&session_meta),
            anatomical: anatomical.as_ref(),
            acquisition_complete: pending.completed_normally,
            // Stimulus present-timing is only physically measurable on a real
            // hardware scanout; over RDP (virtual display) it is not. Recorded as
            // provenance so the stimulus-drop count is never mistaken for a real
            // defect on a remote-validated run.
            stimulus_timing_validatable: !crate::monitor::is_remote_session(),
        },
    )?;

    // Update summary with file path.
    if let Some(ref mut summary) = state.handoff.lock().last_summary {
        summary.file_path = Some(output_path.to_string_lossy().to_string());
    }

    Ok(output_path.to_string_lossy().to_string())
}

/// Discard the pending acquisition without saving.
#[tauri::command]
pub fn discard_acquisition(state: State<'_, SharedState>) -> AppResult<()> {
    let had_pending = state.handoff.lock().pending_save.take().is_some();
    if had_pending {
        tracing::info!("acquisition discarded by user");
    }
    Ok(())
}

/// Start stimulus preview on the stimulus monitor (no recording).
#[tauri::command]
pub fn start_preview(state: State<'_, SharedState>) -> AppResult<()> {
    let snapshot = state.registry.lock().snapshot();

    validate_experiment(&snapshot)?;

    let monitor = state
        .session
        .lock()
        .selected_display
        .clone()
        .ok_or(AppError::Validation(
            "No display selected — select a display before previewing".into(),
        ))?;

    state
        .threads
        .stimulus_tx
        .send(StimulusCmd::Preview(PreviewCommand { snapshot, monitor }))
        .map_err(|e| AppError::Hardware(format!("Failed to send preview command: {e}")))?;
    Ok(())
}

/// Stop stimulus preview.
#[tauri::command]
pub fn stop_preview(state: State<'_, SharedState>) -> AppResult<()> {
    state
        .threads
        .stimulus_tx
        .send(StimulusCmd::StopPreview)
        .map_err(|e| AppError::Hardware(format!("Failed to send stop preview command: {e}")))?;
    Ok(())
}

/// Get full session state for UI hydration on screen mount.
#[tauri::command]
pub fn get_session(state: State<'_, SharedState>) -> AppResult<serde_json::Value> {
    let (exposure_us, monitor_rotation_deg) = {
        let reg = state.registry.lock();
        (reg.camera_exposure_us(), reg.monitor_rotation_deg())
    };
    let stimulus_thread_ready = state.threads.stimulus_spawn.lock().spawned;
    let (last_acquisition, anatomical_captured) = {
        let handoff = state.handoff.lock();
        (handoff.last_summary.clone(), handoff.anatomical.is_some())
    };
    let session = state.session.lock();
    Ok(serde_json::json!({
        "selected_display": session.selected_display,
        "display_validation": session.display_validation,
        "timing_characterization": session.timing_characterization,
        "camera_connected": session.camera_connected,
        "camera": session.camera,
        "is_acquiring": session.is_acquiring,
        "stimulus_thread_ready": stimulus_thread_ready,
        "last_acquisition": last_acquisition,
        "save_path": session.save_path,
        "monitor_rotation_deg": monitor_rotation_deg,
        "exposure_us": exposure_us,
        "anatomical_captured": anatomical_captured,
    }))
}

/// Get workspace status summary (for status bar).
#[derive(Serialize)]
pub struct WorkspaceStatus {
    pub display: String,
    pub camera: String,
    pub activity: String,
}

#[tauri::command]
pub fn get_workspace_status(state: State<'_, SharedState>) -> AppResult<WorkspaceStatus> {
    let session = state.session.lock();

    let display = if let Some(ref v) = session.display_validation {
        if let Some(ref d) = session.selected_display {
            format!("{} {:.1}Hz", d.name, v.measured_refresh_hz)
        } else {
            "Validated".into()
        }
    } else if let Some(ref d) = session.selected_display {
        format!("{} (not validated)", d.name)
    } else {
        "None".into()
    };

    let camera = if let Some(ref c) = session.camera {
        format!("{} {}x{}", c.model, c.width_px, c.height_px)
    } else if session.camera_connected {
        "Connected".into()
    } else {
        "Disconnected".into()
    };

    let activity = if session.is_acquiring {
        "Acquiring".into()
    } else {
        "Idle".into()
    };

    Ok(WorkspaceStatus {
        display,
        camera,
        activity,
    })
}
