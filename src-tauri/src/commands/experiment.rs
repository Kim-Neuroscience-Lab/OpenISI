//! Experiment management commands: load, save, update, list.

use serde::Serialize;
use tauri::State;

use openisi_params::config::ExperimentConfig;

use crate::error::{AppError, AppResult};

use super::SharedState;

/// Experiment summary for listing saved experiments.
#[derive(Serialize)]
pub struct ExperimentSummary {
    pub path: String,
    pub name: String,
    pub description: String,
    pub envelope: String,
    pub conditions: Vec<String>,
    pub repetitions: u32,
}

/// List available saved experiment files.
#[tauri::command]
pub fn list_experiments(state: State<'_, SharedState>) -> AppResult<Vec<ExperimentSummary>> {
    let exp_dir = state.config.lock().experiments_dir();

    let mut summaries = Vec::new();
    if !exp_dir.exists() {
        return Ok(summaries);
    }
    if let Ok(entries) = std::fs::read_dir(&exp_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                // Deserialize the template to read its experiment params.
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                if let Ok(exp) =
                    openisi_params::config::load_merged::<ExperimentConfig>(&text, None)
                {
                    summaries.push(ExperimentSummary {
                        path: path.to_string_lossy().to_string(),
                        name: String::new(), // metadata not in params
                        description: String::new(),
                        envelope: format!("{:?}", exp.stimulus.envelope).to_lowercase(),
                        conditions: exp.presentation.conditions.clone(),
                        repetitions: exp.presentation.repetitions,
                    });
                }
            }
        }
    }
    Ok(summaries)
}

/// Get the current experiment configuration as JSON.
#[tauri::command]
pub fn get_experiment(state: State<'_, SharedState>) -> AppResult<serde_json::Value> {
    let exp = state.config.lock().experiment().clone();
    Ok(experiment_to_json(&exp))
}

/// Update experiment configuration. Persists to experiment.json.
#[tauri::command]
pub fn update_experiment(
    state: State<'_, SharedState>,
    config: serde_json::Value,
) -> AppResult<()> {
    // Config-scoped, brief: merge the sparse overlay + save while holding the lock.
    let mut cfg = state.config.lock();
    cfg.merge_experiment(&config)?;
    if let Err(e) = cfg.save_all() {
        return Err(AppError::Config(format!(
            "Failed to write experiment.json: {e}"
        )));
    }
    Ok(())
}

/// Load an experiment from a specific file path.
#[tauri::command]
pub fn load_experiment(
    state: State<'_, SharedState>,
    path: String,
) -> AppResult<serde_json::Value> {
    let mut cfg = state.config.lock();
    cfg.load_experiment_template(std::path::Path::new(&path))
        .map_err(|e| AppError::Config(format!("Failed to load experiment from {path}: {e}")))?;
    Ok(experiment_to_json(cfg.experiment()))
}

/// Save the current experiment to a new file in the experiments directory.
#[tauri::command]
pub fn save_experiment_as(state: State<'_, SharedState>, name: String) -> AppResult<String> {
    let cfg = state.config.lock();
    let exp_dir = cfg.experiments_dir();

    // Sanitize filename.
    let safe_name: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = exp_dir.join(format!("{safe_name}.json"));

    cfg.save_experiment_template(&path)
        .map_err(|e| AppError::Config(format!("Failed to save experiment: {e}")))?;

    let path_str = path.to_string_lossy().to_string();
    tracing::info!(path = %path_str, "experiment saved");
    Ok(path_str)
}

/// Compute duration summary from current experiment config.
#[derive(Serialize)]
pub struct DurationSummary {
    pub total_sec: f64,
    pub sweep_count: usize,
    pub sweep_duration_sec: f64,
    pub formatted: String,
}

#[tauri::command]
pub fn get_duration_summary(state: State<'_, SharedState>) -> AppResult<DurationSummary> {
    use openisi_stimulus::geometry::DisplayGeometry;

    let monitor = state
        .session
        .lock()
        .selected_display
        .clone()
        .ok_or(AppError::Validation(
            "No display selected — select a display to compute duration".into(),
        ))?;

    let cfg = state.config.lock().snapshot();
    let exp = &cfg.experiment;
    let p = &exp.stimulus.params;

    let n_conditions = exp.presentation.conditions.len();
    let reps = exp.presentation.repetitions as usize;
    let sweep_count = n_conditions * reps;

    let geometry = DisplayGeometry::new(
        exp.geometry.projection,
        cfg.rig.geometry.viewing_distance_cm,
        exp.geometry.horizontal_offset_deg,
        exp.geometry.vertical_offset_deg,
        cfg.rig.geometry.bisector_x_cm,
        cfg.rig.geometry.bisector_y_cm,
        cfg.rig.geometry.monitor_width_cm,
        cfg.rig.geometry.monitor_height_cm,
        monitor.width_px,
        monitor.height_px,
    );

    let sweep_duration_sec = match exp.stimulus.envelope {
        crate::params::Envelope::Bar => {
            let total_travel = geometry.visual_field_width_deg() + p.stimulus_width_deg;
            total_travel / p.sweep_speed_deg_per_sec
        }
        crate::params::Envelope::Wedge => 360.0 / p.rotation_speed_deg_per_sec,
        crate::params::Envelope::Ring => {
            let total_travel = geometry.get_max_eccentricity_deg() + p.stimulus_width_deg;
            total_travel / p.expansion_speed_deg_per_sec
        }
        crate::params::Envelope::Fullfield => 0.0,
    };

    let total_sweep_time = sweep_count as f64 * sweep_duration_sec;
    let total_baseline = exp.timing.baseline_start_sec + exp.timing.baseline_end_sec;
    let total_inter = if sweep_count > 1 {
        (sweep_count - 1) as f64 * exp.timing.inter_stimulus_sec
    } else {
        0.0
    };
    let total_inter_dir = if n_conditions > 1 {
        (n_conditions - 1) as f64 * exp.timing.inter_direction_sec * reps as f64
    } else {
        0.0
    };

    let total_sec = total_baseline + total_sweep_time + total_inter + total_inter_dir;
    let mins = (total_sec / 60.0).floor() as u32;
    let secs = (total_sec % 60.0).round() as u32;
    let formatted = format!(
        "{}:{:02} ({} sweeps x {:.1}s)",
        mins, secs, sweep_count, sweep_duration_sec
    );

    Ok(DurationSummary {
        total_sec,
        sweep_count,
        sweep_duration_sec,
        formatted,
    })
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Project the typed `ExperimentConfig` to the flat JSON shape the frontend
/// consumes. Deliberately a hand-written subset (not a blanket serde dump): it
/// omits `stimulus_geometry` (an analysis-time fact the UI doesn't edit) and
/// renders enums as lowercase strings, exactly as the old descriptor did.
fn experiment_to_json(exp: &ExperimentConfig) -> serde_json::Value {
    let g = &exp.geometry;
    let s = &exp.stimulus;
    let p = &s.params;
    let pr = &exp.presentation;
    let t = &exp.timing;
    serde_json::json!({
        "geometry": {
            "horizontal_offset_deg": g.horizontal_offset_deg,
            "vertical_offset_deg": g.vertical_offset_deg,
            "projection": format!("{:?}", g.projection).to_lowercase(),
        },
        "stimulus": {
            "envelope": format!("{:?}", s.envelope).to_lowercase(),
            "carrier": format!("{:?}", s.carrier).to_lowercase(),
            "params": {
                "contrast": p.contrast,
                "mean_luminance": p.mean_luminance,
                "background_luminance": p.background_luminance,
                "check_size_deg": p.check_size_deg,
                "check_size_cm": p.check_size_cm,
                "strobe_frequency_hz": p.strobe_frequency_hz,
                "stimulus_width_deg": p.stimulus_width_deg,
                "sweep_speed_deg_per_sec": p.sweep_speed_deg_per_sec,
                "rotation_speed_deg_per_sec": p.rotation_speed_deg_per_sec,
                "expansion_speed_deg_per_sec": p.expansion_speed_deg_per_sec,
                "rotation_deg": p.rotation_deg,
            }
        },
        "presentation": {
            "conditions": pr.conditions,
            "repetitions": pr.repetitions,
            "structure": format!("{:?}", pr.structure).to_lowercase(),
            "order": format!("{:?}", pr.order).to_lowercase(),
        },
        "timing": {
            "baseline_start_sec": t.baseline_start_sec,
            "baseline_end_sec": t.baseline_end_sec,
            "inter_stimulus_sec": t.inter_stimulus_sec,
            "inter_direction_sec": t.inter_direction_sec,
        }
    })
}
