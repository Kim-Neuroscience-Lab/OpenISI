//! Tauri commands for parameters — get descriptors (from the typed `ConfigStore`
//! via `config_descriptors`) and batch-set values (via `ConfigStore.merge`).

use serde::Serialize;
use tauri::State;

use crate::commands::SharedState;
use crate::error::{AppError, AppResult};

use openisi_params::EnumOption;

use super::GroupId;

// ─── Descriptor JSON ─────────────────────────────────────────────────────────

/// Parameter descriptor sent to the frontend. Contains everything needed
/// to render a form control: current value, effective constraint, metadata.
#[derive(Debug, Serialize)]
pub struct ParamDescriptorJson {
    /// Dotted ID (e.g., "camera.exposure_us")
    pub id: String,
    /// Human-readable label
    pub label: String,
    /// Unit string (e.g., "µs", "°", "")
    pub unit: String,
    /// Type discriminant for the frontend
    pub param_type: String,
    /// Current value
    pub value: serde_json::Value,
    /// Effective constraint (dynamic override or static)
    pub constraint: ConstraintJson,
    /// Whether this parameter is currently active (visible/editable)
    pub active: bool,
    /// Logical group
    pub group: GroupId,
}

/// Serializable constraint for the frontend.
/// Uses a flat format: { min?, max?, values? } — no tagged unions.
/// The frontend checks: if `values` exists → enum, if `min`/`max` exist → range, else → unconstrained.
///
/// `values` is a list of `{ value, label }` objects — `value` is the
/// wire string the config persists (snake_case), `label` is the
/// human-facing string the UI renders inside `<option>`. Both come
/// from the same enum declaration in `openisi-params` (serde for the
/// wire string, `strum::Display` for the label), so they cannot drift.
#[derive(Debug, Serialize)]
pub struct ConstraintJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<EnumOption>>,
}

impl ConstraintJson {
    pub(crate) fn none() -> Self {
        Self {
            min: None,
            max: None,
            values: None,
        }
    }

    pub(crate) fn range(min: f64, max: f64) -> Self {
        Self {
            min: Some(min),
            max: Some(max),
            values: None,
        }
    }

    pub(crate) fn min_only(min: f64) -> Self {
        Self {
            min: Some(min),
            max: None,
            values: None,
        }
    }

    pub(crate) fn enum_values(vals: Vec<EnumOption>) -> Self {
        Self {
            min: None,
            max: None,
            values: Some(vals),
        }
    }
}

// ─── Commands ────────────────────────────────────────────────────────────────

/// One analysis-view stage card: the snake_case group `key` (matches the
/// `group` arg of [`get_param_descriptors`]) and its human `title`.
#[derive(Debug, Serialize)]
pub struct AnalysisStageJson {
    pub key: String,
    pub title: String,
}

/// Return the analysis pipeline stages, in order, for the analysis-view UI to
/// render one card per stage. Single source of truth:
/// `openisi_params::analysis_stage_groups()` + `GroupId::card_title`, so the
/// frontend never hardcodes the stage list.
#[tauri::command]
pub fn get_analysis_stages() -> Vec<AnalysisStageJson> {
    openisi_params::analysis_stage_groups()
        .into_iter()
        .map(|g| AnalysisStageJson {
            key: g.to_string(),
            title: g.card_title().to_string(),
        })
        .collect()
}

/// Return descriptors for all parameters, optionally filtered by group/target.
/// Sourced from the typed [`ConfigStore`] via the descriptor generator (whose
/// output is locked to the frontend's contract by golden tests).
#[tauri::command]
pub fn get_param_descriptors(
    state: State<'_, SharedState>,
    group: Option<String>,
) -> AppResult<Vec<ParamDescriptorJson>> {
    let store = state.config.lock();
    Ok(super::config_descriptors::config_descriptors(
        &store,
        group.as_deref(),
    ))
}

/// Result of a batch set_params call.
#[derive(Debug, Serialize)]
pub struct SetParamsResult {
    /// Number of params successfully updated.
    pub updated: usize,
    /// Params that were clamped by constraint cascades (not directly set by the caller).
    pub cascaded: Vec<CascadedChange>,
}

/// A param whose value was changed by constraint cascade (not by the caller's input).
#[derive(Debug, Serialize)]
pub struct CascadedChange {
    pub id: String,
    pub value: serde_json::Value,
}

/// Batch-update parameters. Validates all values before applying any.
///
/// `updates` is a JSON object of `{ "toml.path": value, ... }`.
#[tauri::command]
pub fn set_params(
    state: State<'_, SharedState>,
    updates: serde_json::Value,
) -> AppResult<SetParamsResult> {
    use super::config_descriptors::{analysis_overlay, nest_overlay, target_for_path, Target};

    let updates = updates
        .as_object()
        .ok_or_else(|| AppError::Validation("updates must be a JSON object".into()))?;

    // Route each dotted update to its typed config and merge it (garde validates,
    // hardware bounds clamp/reject). An analysis method switch carries the new
    // variant's default tunables (see `analysis_overlay`).
    let mut store = state.config.lock();
    let mut updated = 0;
    let mut errors = Vec::new();

    for (path, value) in updates {
        let Some(target) = target_for_path(path) else {
            errors.push(format!("unknown parameter: {path}"));
            continue;
        };
        let result = match target {
            Target::Rig => store.merge_rig(&nest_overlay(path, value.clone())),
            Target::Experiment => store.merge_experiment(&nest_overlay(path, value.clone())),
            Target::UiState => store.merge_ui_state(&nest_overlay(path, value.clone())),
            Target::Analysis => store.merge_analysis(&analysis_overlay(path, value.clone())),
        };
        match result {
            Ok(()) => updated += 1,
            Err(e) => errors.push(format!("{path}: {e}")),
        }
    }

    if !errors.is_empty() {
        return Err(AppError::Validation(errors.join("; ")));
    }

    // The `params:changed` event (fired by the store's observer) carries the
    // delta for the frontend; no separate cascade list.
    Ok(SetParamsResult {
        updated,
        cascaded: Vec::new(),
    })
}

