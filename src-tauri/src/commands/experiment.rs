//! Experiment management commands: load, save, update, list.

use serde::Serialize;
use tauri::State;

use crate::error::{lock_state, AppError, AppResult};
use crate::params::{ParamId, ParamValue, RegistrySnapshot};

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
    let reg = lock_state(&app.registry, "list_experiments registry")?;
    let exp_dir = reg.experiments_dir();
    drop(reg);

    let mut summaries = Vec::new();
    if !exp_dir.exists() {
        return Ok(summaries);
    }
    if let Ok(entries) = std::fs::read_dir(&exp_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "toml") {
                // Try to load via a temporary registry to read the experiment params.
                let mut tmp_reg = crate::params::Registry::new(path.parent().unwrap_or(std::path::Path::new(".")));
                if crate::params::toml_io::load_experiment(&mut tmp_reg, &path).is_ok() {
                    let snap = tmp_reg.snapshot();
                    summaries.push(ExperimentSummary {
                        path: path.to_string_lossy().to_string(),
                        name: String::new(), // metadata not in params
                        description: String::new(),
                        envelope: format!("{:?}", snap.stimulus_envelope()).to_lowercase(),
                        conditions: snap.conditions().to_vec(),
                        repetitions: snap.repetitions(),
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
    let app = lock_state(&state, "get_experiment")?;
    let reg = lock_state(&app.registry, "get_experiment registry")?;
    Ok(experiment_to_json(&reg.snapshot()))
}

/// Update experiment configuration. Persists to experiment.toml.
#[tauri::command]
pub fn update_experiment(state: State<'_, SharedState>, config: serde_json::Value) -> AppResult<()> {
    let app = lock_state(&state, "update_experiment")?;
    let mut reg = lock_state(&app.registry, "update_experiment registry")?;

    // Apply all experiment-related fields from the JSON.
    apply_experiment_json(&mut reg, &config)?;

    if let Err(e) = reg.save_experiment() {
        return Err(AppError::Config(format!("Failed to write experiment.toml: {e}")));
    }
    Ok(())
}

/// Load an experiment from a specific file path.
#[tauri::command]
pub fn load_experiment(state: State<'_, SharedState>, path: String) -> AppResult<serde_json::Value> {
    let app = lock_state(&state, "load_experiment")?;
    let mut reg = lock_state(&app.registry, "load_experiment registry")?;

    crate::params::toml_io::load_experiment(&mut reg, std::path::Path::new(&path))
        .map_err(|e| AppError::Config(format!("Failed to load experiment from {path}: {e}")))?;

    Ok(experiment_to_json(&reg.snapshot()))
}

/// Save the current experiment to a new file in the experiments directory.
#[tauri::command]
pub fn save_experiment_as(state: State<'_, SharedState>, name: String) -> AppResult<String> {
    let app = lock_state(&state, "save_experiment_as")?;
    let reg = lock_state(&app.registry, "save_experiment_as registry")?;
    let exp_dir = reg.experiments_dir();
    drop(reg);

    let _ = std::fs::create_dir_all(&exp_dir);

    // Sanitize filename.
    let safe_name: String = name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let path = exp_dir.join(format!("{safe_name}.toml"));

    // Save experiment params to the target path.
    let reg = lock_state(&app.registry, "save_experiment_as registry write")?;
    crate::params::toml_io::save_experiment(&reg, &path)
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
    let monitor = app.session.selected_display.as_ref()
        .ok_or(AppError::Validation(
            "No display selected — select a display to compute duration".into(),
        ))?;

    let reg = lock_state(&app.registry, "get_duration_summary registry")?;
    let snap = reg.snapshot();
    drop(reg);

    let n_conditions = snap.conditions().len();
    let reps = snap.repetitions() as usize;
    let sweep_count = n_conditions * reps;

    let geometry = DisplayGeometry::new(
        snap.experiment_projection(),
        snap.viewing_distance_cm(),
        snap.horizontal_offset_deg(),
        snap.vertical_offset_deg(),
        monitor.width_cm,
        monitor.height_cm,
        monitor.width_px,
        monitor.height_px,
    );

    let sweep_duration_sec = match snap.stimulus_envelope() {
        crate::params::Envelope::Bar => {
            let total_travel = geometry.visual_field_width_deg() + snap.stimulus_width_deg();
            total_travel / snap.sweep_speed_deg_per_sec()
        }
        crate::params::Envelope::Wedge => {
            360.0 / snap.rotation_speed_deg_per_sec()
        }
        crate::params::Envelope::Ring => {
            let total_travel = geometry.get_max_eccentricity_deg() + snap.stimulus_width_deg();
            total_travel / snap.expansion_speed_deg_per_sec()
        }
        crate::params::Envelope::Fullfield => 0.0,
    };

    let total_sweep_time = sweep_count as f64 * sweep_duration_sec;
    let total_baseline = snap.baseline_start_sec() + snap.baseline_end_sec();
    let total_inter = if sweep_count > 1 {
        (sweep_count - 1) as f64 * snap.inter_stimulus_sec()
    } else {
        0.0
    };
    let total_inter_dir = if n_conditions > 1 {
        (n_conditions - 1) as f64 * snap.inter_direction_sec() * reps as f64
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

// ── Helpers ──────────────────────────────────────────────────────────────

fn experiment_to_json(snap: &RegistrySnapshot) -> serde_json::Value {
    serde_json::json!({
        "geometry": {
            "horizontal_offset_deg": snap.horizontal_offset_deg(),
            "vertical_offset_deg": snap.vertical_offset_deg(),
            "projection": format!("{:?}", snap.experiment_projection()).to_lowercase(),
        },
        "stimulus": {
            "envelope": format!("{:?}", snap.stimulus_envelope()).to_lowercase(),
            "carrier": format!("{:?}", snap.stimulus_carrier()).to_lowercase(),
            "params": {
                "contrast": snap.contrast(),
                "mean_luminance": snap.mean_luminance(),
                "background_luminance": snap.background_luminance(),
                "check_size_deg": snap.check_size_deg(),
                "check_size_cm": snap.check_size_cm(),
                "strobe_frequency_hz": snap.strobe_frequency_hz(),
                "stimulus_width_deg": snap.stimulus_width_deg(),
                "sweep_speed_deg_per_sec": snap.sweep_speed_deg_per_sec(),
                "rotation_speed_deg_per_sec": snap.rotation_speed_deg_per_sec(),
                "expansion_speed_deg_per_sec": snap.expansion_speed_deg_per_sec(),
                "rotation_deg": snap.rotation_deg(),
            }
        },
        "presentation": {
            "conditions": snap.conditions(),
            "repetitions": snap.repetitions(),
            "structure": format!("{:?}", snap.presentation_structure()).to_lowercase(),
            "order": format!("{:?}", snap.presentation_order()).to_lowercase(),
        },
        "timing": {
            "baseline_start_sec": snap.baseline_start_sec(),
            "baseline_end_sec": snap.baseline_end_sec(),
            "inter_stimulus_sec": snap.inter_stimulus_sec(),
            "inter_direction_sec": snap.inter_direction_sec(),
        }
    })
}

/// Apply experiment fields from a JSON value to the registry.
fn apply_experiment_json(reg: &mut crate::params::Registry, json: &serde_json::Value) -> AppResult<()> {
    reg.batch(|r| -> Result<(), String> {
        // Geometry
        if let Some(g) = json.get("geometry") {
            if let Some(v) = g.get("horizontal_offset_deg").and_then(|v| v.as_f64()) {
                r.set(ParamId::HorizontalOffsetDeg, ParamValue::F64(v))?;
            }
            if let Some(v) = g.get("vertical_offset_deg").and_then(|v| v.as_f64()) {
                r.set(ParamId::VerticalOffsetDeg, ParamValue::F64(v))?;
            }
            if let Some(v) = g.get("projection").and_then(|v| v.as_str()) {
                if let Ok(p) = serde_json::from_value::<crate::params::Projection>(serde_json::json!(v)) {
                    r.set(ParamId::ExperimentProjection, ParamValue::Projection(p))?;
                }
            }
        }
        // Stimulus
        if let Some(s) = json.get("stimulus") {
            if let Some(v) = s.get("envelope").and_then(|v| v.as_str()) {
                if let Ok(e) = serde_json::from_value::<crate::params::Envelope>(serde_json::json!(v)) {
                    r.set(ParamId::StimulusEnvelope, ParamValue::Envelope(e))?;
                }
            }
            if let Some(v) = s.get("carrier").and_then(|v| v.as_str()) {
                if let Ok(c) = serde_json::from_value::<crate::params::Carrier>(serde_json::json!(v)) {
                    r.set(ParamId::StimulusCarrier, ParamValue::Carrier(c))?;
                }
            }
            if let Some(p) = s.get("params") {
                if let Some(v) = p.get("contrast").and_then(|v| v.as_f64()) { r.set(ParamId::Contrast, ParamValue::F64(v))?; }
                if let Some(v) = p.get("mean_luminance").and_then(|v| v.as_f64()) { r.set(ParamId::MeanLuminance, ParamValue::F64(v))?; }
                if let Some(v) = p.get("background_luminance").and_then(|v| v.as_f64()) { r.set(ParamId::BackgroundLuminance, ParamValue::F64(v))?; }
                if let Some(v) = p.get("check_size_deg").and_then(|v| v.as_f64()) { r.set(ParamId::CheckSizeDeg, ParamValue::F64(v))?; }
                if let Some(v) = p.get("check_size_cm").and_then(|v| v.as_f64()) { r.set(ParamId::CheckSizeCm, ParamValue::F64(v))?; }
                if let Some(v) = p.get("strobe_frequency_hz").and_then(|v| v.as_f64()) { r.set(ParamId::StrobeFrequencyHz, ParamValue::F64(v))?; }
                if let Some(v) = p.get("stimulus_width_deg").and_then(|v| v.as_f64()) { r.set(ParamId::StimulusWidthDeg, ParamValue::F64(v))?; }
                if let Some(v) = p.get("sweep_speed_deg_per_sec").and_then(|v| v.as_f64()) { r.set(ParamId::SweepSpeedDegPerSec, ParamValue::F64(v))?; }
                if let Some(v) = p.get("rotation_speed_deg_per_sec").and_then(|v| v.as_f64()) { r.set(ParamId::RotationSpeedDegPerSec, ParamValue::F64(v))?; }
                if let Some(v) = p.get("expansion_speed_deg_per_sec").and_then(|v| v.as_f64()) { r.set(ParamId::ExpansionSpeedDegPerSec, ParamValue::F64(v))?; }
                if let Some(v) = p.get("rotation_deg").and_then(|v| v.as_f64()) { r.set(ParamId::RotationDeg, ParamValue::F64(v))?; }
            }
        }
        // Presentation
        if let Some(p) = json.get("presentation") {
            if let Some(arr) = p.get("conditions").and_then(|v| v.as_array()) {
                let conds: Vec<String> = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
                r.set(ParamId::Conditions, ParamValue::StringVec(conds))?;
            }
            if let Some(v) = p.get("repetitions").and_then(|v| v.as_u64()) {
                r.set(ParamId::Repetitions, ParamValue::U32(v as u32))?;
            }
            if let Some(v) = p.get("structure").and_then(|v| v.as_str()) {
                if let Ok(s) = serde_json::from_value::<crate::params::Structure>(serde_json::json!(v)) {
                    r.set(ParamId::PresentationStructure, ParamValue::Structure(s))?;
                }
            }
            if let Some(v) = p.get("order").and_then(|v| v.as_str()) {
                if let Ok(o) = serde_json::from_value::<crate::params::Order>(serde_json::json!(v)) {
                    r.set(ParamId::PresentationOrder, ParamValue::Order(o))?;
                }
            }
        }
        // Timing
        if let Some(t) = json.get("timing") {
            if let Some(v) = t.get("baseline_start_sec").and_then(|v| v.as_f64()) { r.set(ParamId::BaselineStartSec, ParamValue::F64(v))?; }
            if let Some(v) = t.get("baseline_end_sec").and_then(|v| v.as_f64()) { r.set(ParamId::BaselineEndSec, ParamValue::F64(v))?; }
            if let Some(v) = t.get("inter_stimulus_sec").and_then(|v| v.as_f64()) { r.set(ParamId::InterStimulusSec, ParamValue::F64(v))?; }
            if let Some(v) = t.get("inter_direction_sec").and_then(|v| v.as_f64()) { r.set(ParamId::InterDirectionSec, ParamValue::F64(v))?; }
        }
        Ok(())
    }).map_err(|e| AppError::Validation(e))?;
    Ok(())
}
