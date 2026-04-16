//! Acquisition workflow commands: session metadata, start/stop, save/discard, preview.

use serde::Serialize;
use tauri::State;

use crate::error::{lock_state, AppError, AppResult};
use crate::events::build_hardware_snapshot;
use crate::messages::{AcquisitionCommand, PreviewCommand, StimulusCmd};
use crate::params::RegistrySnapshot;

use super::SharedState;

/// Validate experiment parameters before acquisition or preview.
fn validate_experiment(snap: &RegistrySnapshot) -> AppResult<()> {
    use crate::params::Envelope;
    match snap.stimulus_envelope() {
        Envelope::Bar => {
            if snap.sweep_speed_deg_per_sec() <= 0.0 {
                return Err(AppError::Validation("Sweep speed must be greater than zero".into()));
            }
        }
        Envelope::Wedge => {
            if snap.rotation_speed_deg_per_sec() <= 0.0 {
                return Err(AppError::Validation("Rotation speed must be greater than zero".into()));
            }
        }
        Envelope::Ring => {
            if snap.expansion_speed_deg_per_sec() <= 0.0 {
                return Err(AppError::Validation("Expansion speed must be greater than zero".into()));
            }
        }
        Envelope::Fullfield => {}
    }
    if snap.stimulus_width_deg() <= 0.0 {
        return Err(AppError::Validation("Stimulus width must be greater than zero".into()));
    }
    if snap.repetitions() == 0 {
        return Err(AppError::Validation("Repetitions must be at least 1".into()));
    }
    if snap.conditions().is_empty() {
        return Err(AppError::Validation("No conditions defined".into()));
    }
    Ok(())
}

/// Set session metadata (animal ID and notes).
#[tauri::command]
pub fn set_session_metadata(state: State<'_, SharedState>, animal_id: String, notes: String) -> AppResult<()> {
    let mut app = lock_state(&state, "set_session_metadata")?;
    app.session.animal_id = animal_id;
    app.session.notes = notes;
    Ok(())
}

/// Start acquisition — ties stimulus + camera together.
#[tauri::command]
pub fn start_acquisition(state: State<'_, SharedState>) -> AppResult<()> {
    let mut app = lock_state(&state, "start_acquisition")?;

    // Take a snapshot for validation and acquisition.
    let reg = lock_state(&app.registry, "start_acquisition registry")?;
    let snapshot = reg.snapshot();
    drop(reg);

    validate_experiment(&snapshot)?;

    // Check prerequisites.
    let monitor = app.session.selected_display.as_ref()
        .ok_or(AppError::Validation("No display selected".into()))?
        .clone();

    if !app.session.camera_connected {
        return Err(AppError::Validation("Camera not connected".into()));
    }

    if app.session.display_validation.is_none() {
        return Err(AppError::Validation(
            "Display not validated — validate display before acquiring".into(),
        ));
    }

    // Timing validation is strongly recommended but not a hard block.
    if let Some(ref tc) = app.session.timing_characterization {
        if tc.regime == crate::timing::TimingRegime::Systematic {
            eprintln!(
                "[acquire] WARNING: Systematic timing regime (beat period {:.1}s). \
                 Every trial sees approximately the same sub-frame onset position.",
                tc.beat_period_sec
            );
        }
    }

    let measured_refresh_hz = app.session.display_validation.as_ref()
        .ok_or(AppError::Validation("Display not validated".into()))?
        .measured_refresh_hz;

    let acq_cmd = AcquisitionCommand {
        snapshot: snapshot.clone(),
        monitor: monitor.clone(),
        measured_refresh_hz,
    };

    let tx = app.threads.stimulus_tx.as_ref()
        .ok_or(AppError::NotAvailable(
            "Stimulus thread not running — select a display first".into(),
        ))?;
    tx.send(StimulusCmd::StartAcquisition(acq_cmd))
        .map_err(|e| AppError::Hardware(format!("Failed to send acquisition command: {e}")))?;

    // Build hardware snapshot from current session state (valid at start).
    let hardware_snapshot = build_hardware_snapshot(&app);
    let timing_characterization = app.session.timing_characterization.clone();

    // Start camera frame accumulation with acquisition-time snapshot.
    let (cam_w, cam_h) = {
        let cam = app.session.camera.as_ref()
            .ok_or(AppError::NotAvailable("Camera info not available during acquisition".into()))?;
        (cam.width_px, cam.height_px)
    };
    app.start_acquisition(
        cam_w,
        cam_h,
        snapshot,
        hardware_snapshot,
        timing_characterization,
    );

    Ok(())
}

/// Stop the current acquisition.
#[tauri::command]
pub fn stop_acquisition(state: State<'_, SharedState>) -> AppResult<()> {
    let mut app = lock_state(&state, "stop_acquisition")?;
    let tx = app.threads.stimulus_tx.as_ref()
        .ok_or(AppError::NotAvailable("Stimulus thread not running".into()))?;
    tx.send(StimulusCmd::Stop)
        .map_err(|e| AppError::Hardware(format!("Failed to send stop command: {e}")))?;
    app.session.is_acquiring = false;
    Ok(())
}

/// Save the pending acquisition to a .oisi file. Called after user confirms.
#[tauri::command]
pub fn save_acquisition(state: State<'_, SharedState>, path: Option<String>) -> AppResult<String> {
    let mut app = lock_state(&state, "save_acquisition")?;

    let pending = app.pending_save.take()
        .ok_or(AppError::NotAvailable("No pending acquisition to save".into()))?;

    // Read metadata from live state at save time.
    let animal_id = app.session.animal_id.clone();
    let notes = app.session.notes.clone();
    let anatomical = app.anatomical_image.clone();

    // Determine output path.
    let output_path = if let Some(p) = path {
        std::path::PathBuf::from(p)
    } else {
        let data_dir = pending.snapshot.data_directory().to_string();
        let dir = if data_dir.is_empty() {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")))
        } else {
            std::path::PathBuf::from(data_dir)
        };
        let _ = std::fs::create_dir_all(&dir);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let safe_id: String = animal_id.trim().chars()
            .map(|c: char| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        let filename = if safe_id.is_empty() {
            format!("acquisition_{ts}.oisi")
        } else {
            format!("{safe_id}_{ts}.oisi")
        };
        dir.join(filename)
    };

    let session_meta = crate::export::SessionMetadata {
        animal_id,
        notes,
    };

    drop(app); // Release lock during file write.

    crate::export::write_oisi(
        &output_path,
        &pending.stimulus_dataset,
        pending.camera_data,
        &pending.snapshot,
        pending.hardware_snapshot.as_ref(),
        &pending.schedule,
        pending.timing_characterization.as_ref(),
        Some(&session_meta),
        anatomical.as_ref(),
        pending.completed_normally,
    ).map_err(|e| AppError::Io(std::io::Error::new(
        std::io::ErrorKind::Other,
        format!("Failed to write .oisi: {e}"),
    )))?;

    // Update summary with file path.
    let mut app = lock_state(&state, "save_acquisition update summary")?;
    if let Some(ref mut summary) = app.last_acquisition_summary {
        summary.file_path = Some(output_path.to_string_lossy().to_string());
    }

    Ok(output_path.to_string_lossy().to_string())
}

/// Discard the pending acquisition without saving.
#[tauri::command]
pub fn discard_acquisition(state: State<'_, SharedState>) -> AppResult<()> {
    let mut app = lock_state(&state, "discard_acquisition")?;
    let had_pending = app.pending_save.take().is_some();
    if had_pending {
        eprintln!("[commands] acquisition discarded by user");
    }
    Ok(())
}

/// Start stimulus preview on the stimulus monitor (no recording).
#[tauri::command]
pub fn start_preview(state: State<'_, SharedState>) -> AppResult<()> {
    let app = lock_state(&state, "start_preview")?;

    let reg = lock_state(&app.registry, "start_preview registry")?;
    let snapshot = reg.snapshot();
    drop(reg);

    validate_experiment(&snapshot)?;

    let monitor = app.session.selected_display.clone()
        .ok_or(AppError::Validation(
            "No display selected — select a display before previewing".into(),
        ))?;

    let tx = app.threads.stimulus_tx.as_ref()
        .ok_or(AppError::NotAvailable(
            "Stimulus thread not running — select a display first".into(),
        ))?;

    tx.send(StimulusCmd::Preview(PreviewCommand {
        snapshot,
        monitor,
    })).map_err(|e| AppError::Hardware(format!("Failed to send preview command: {e}")))?;
    Ok(())
}

/// Stop stimulus preview.
#[tauri::command]
pub fn stop_preview(state: State<'_, SharedState>) -> AppResult<()> {
    let app = lock_state(&state, "stop_preview")?;
    let tx = app.threads.stimulus_tx.as_ref()
        .ok_or(AppError::NotAvailable("Stimulus thread not running".into()))?;
    tx.send(StimulusCmd::StopPreview)
        .map_err(|e| AppError::Hardware(format!("Failed to send stop preview command: {e}")))?;
    Ok(())
}

/// Get full session state for UI hydration on screen mount.
#[tauri::command]
pub fn get_session(state: State<'_, SharedState>) -> AppResult<serde_json::Value> {
    let app = lock_state(&state, "get_session")?;
    let reg = lock_state(&app.registry, "get_session registry")?;
    let exposure_us = reg.camera_exposure_us();
    let monitor_rotation_deg = reg.monitor_rotation_deg();
    drop(reg);
    Ok(serde_json::json!({
        "selected_display": app.session.selected_display,
        "display_validation": app.session.display_validation,
        "timing_characterization": app.session.timing_characterization,
        "camera_connected": app.session.camera_connected,
        "camera": app.session.camera,
        "is_acquiring": app.session.is_acquiring,
        "stimulus_thread_ready": app.threads.stimulus_thread_spawned,
        "last_acquisition": app.last_acquisition_summary,
        "save_path": app.session.save_path,
        "monitor_rotation_deg": monitor_rotation_deg,
        "exposure_us": exposure_us,
        "anatomical_captured": app.anatomical_image.is_some(),
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
    let app = lock_state(&state, "get_workspace_status")?;

    let display = if let Some(ref v) = app.session.display_validation {
        if let Some(ref d) = app.session.selected_display {
            format!("{} {:.1}Hz", d.name, v.measured_refresh_hz)
        } else {
            "Validated".into()
        }
    } else if let Some(ref d) = app.session.selected_display {
        format!("{} (not validated)", d.name)
    } else {
        "None".into()
    };

    let camera = if let Some(ref c) = app.session.camera {
        format!("{} {}x{}", c.model, c.width_px, c.height_px)
    } else if app.session.camera_connected {
        "Connected".into()
    } else {
        "Disconnected".into()
    };

    let activity = if app.session.is_acquiring {
        "Acquiring".into()
    } else {
        "Idle".into()
    };

    Ok(WorkspaceStatus { display, camera, activity })
}
