//! Experiment management commands: load, save, update, list.

use serde::Serialize;
use tauri::State;

use crate::config::Experiment;
use crate::error::{lock_state, AppError, AppResult};

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
    let app = lock_state(&state, "list_experiments")?;
    let paths = lock_state(&app.config, "list_experiments config")?.list_experiments();

    let mut summaries = Vec::new();
    for path in paths {
        if let Ok(exp) = Experiment::load(&path) {
            summaries.push(ExperimentSummary {
                path: path.to_string_lossy().to_string(),
                name: exp.name.clone().unwrap_or_default(),
                description: exp.description.clone().unwrap_or_default(),
                envelope: format!("{:?}", exp.stimulus.envelope).to_lowercase(),
                conditions: exp.presentation.conditions.clone(),
                repetitions: exp.presentation.repetitions,
            });
        }
    }
    Ok(summaries)
}

/// Get the current experiment configuration.
#[tauri::command]
pub fn get_experiment(state: State<'_, SharedState>) -> AppResult<Experiment> {
    let app = lock_state(&state, "get_experiment")?;
    Ok(app.experiment.clone())
}

/// Update experiment configuration. Stores in memory (effective config) and persists to disk.
#[tauri::command]
pub fn update_experiment(state: State<'_, SharedState>, config: Experiment) -> AppResult<()> {
    let mut app = lock_state(&state, "update_experiment")?;
    // Store as the effective config — this is what acquisition will use.
    app.experiment = config.clone();
    // Also persist to disk.
    {
        let cfg = lock_state(&app.config, "update_experiment config")?;
        let exp_path = cfg.experiment_path();
        config.save(&exp_path)
            .map_err(|e| AppError::Config(format!("Failed to write experiment.toml: {e}")))?;
    }
    Ok(())
}

/// Load an experiment from a specific file path.
#[tauri::command]
pub fn load_experiment(state: State<'_, SharedState>, path: String) -> AppResult<Experiment> {
    let exp = Experiment::load(std::path::Path::new(&path))
        .map_err(|e| AppError::Config(format!("Failed to load experiment from {path}: {e}")))?;

    let mut app = lock_state(&state, "load_experiment")?;
    app.experiment = exp.clone();

    Ok(exp)
}

/// Save the current experiment to a new file in the experiments directory.
#[tauri::command]
pub fn save_experiment_as(state: State<'_, SharedState>, name: String) -> AppResult<String> {
    let mut app = lock_state(&state, "save_experiment_as")?;

    // Set name and timestamps.
    app.experiment.name = Some(name.clone());
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let now_str = format!("{now_secs}");
    if app.experiment.created.is_none() {
        app.experiment.created = Some(now_str.clone());
    }
    app.experiment.modified = Some(now_str);

    let exp_dir = lock_state(&app.config, "save_experiment_as config")?.experiments_dir();
    let _ = std::fs::create_dir_all(&exp_dir);

    // Sanitize filename.
    let safe_name: String = name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let path = exp_dir.join(format!("{safe_name}.toml"));

    app.experiment.save(&path)
        .map_err(|e| AppError::Config(format!("Failed to save experiment: {e}")))?;

    let path_str = path.to_string_lossy().to_string();
    eprintln!("[commands] experiment saved as: {path_str}");
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

    let app = lock_state(&state, "get_duration_summary")?;
    let exp = &app.experiment;
    let monitor = app.session.selected_display.as_ref()
        .ok_or(AppError::Validation(
            "No display selected — select a display to compute duration".into(),
        ))?;

    let rig_geometry = lock_state(&app.config, "get_duration_summary config")?.rig.geometry.clone();

    let n_conditions = exp.presentation.conditions.len();
    let reps = exp.presentation.repetitions as usize;
    let sweep_count = n_conditions * reps;

    let params = &exp.stimulus.params;

    let geometry = DisplayGeometry::new(
        exp.geometry.projection,
        rig_geometry.viewing_distance_cm,
        exp.geometry.horizontal_offset_deg,
        exp.geometry.vertical_offset_deg,
        monitor.width_cm,
        monitor.height_cm,
        monitor.width_px,
        monitor.height_px,
    );

    let sweep_duration_sec = match exp.stimulus.envelope {
        crate::config::Envelope::Bar => {
            // Bar: (VF width + bar width) / speed
            let total_travel = geometry.visual_field_width_deg() + params.stimulus_width_deg;
            total_travel / params.sweep_speed_deg_per_sec
        }
        crate::config::Envelope::Wedge => {
            // Wedge: 360deg / rotation speed
            360.0 / params.rotation_speed_deg_per_sec
        }
        crate::config::Envelope::Ring => {
            // Ring: (max eccentricity + ring width) / expansion speed
            let total_travel = geometry.get_max_eccentricity_deg() + params.stimulus_width_deg;
            total_travel / params.expansion_speed_deg_per_sec
        }
        crate::config::Envelope::Fullfield => {
            // Fullfield: no sweep, duration determined by timing
            0.0
        }
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
    let formatted = format!("{}:{:02} ({} sweeps x {:.1}s)", mins, secs, sweep_count, sweep_duration_sec);

    Ok(DurationSummary {
        total_sec,
        sweep_count,
        sweep_duration_sec,
        formatted,
    })
}
