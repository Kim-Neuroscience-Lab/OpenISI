//! Tauri commands for parameter registry — get descriptors and batch-set values.

use serde::Serialize;
use tauri::State;

use crate::commands::SharedState;
use crate::error::{AppError, AppResult};

use openisi_params::{EnumOption, enum_options};

use super::constraints::EffectiveConstraint;
use super::registry::param_value_to_json;
use super::{GroupId, PARAM_DEFS, ParamId, ParamValue, StaticConstraint};

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
/// wire string the registry persists (snake_case), `label` is the
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
    fn none() -> Self {
        Self {
            min: None,
            max: None,
            values: None,
        }
    }

    fn range(min: f64, max: f64) -> Self {
        Self {
            min: Some(min),
            max: Some(max),
            values: None,
        }
    }

    fn min_only(min: f64) -> Self {
        Self {
            min: Some(min),
            max: None,
            values: None,
        }
    }

    fn enum_values(vals: Vec<EnumOption>) -> Self {
        Self {
            min: None,
            max: None,
            values: Some(vals),
        }
    }

    fn from_effective(
        eff: &EffectiveConstraint,
        static_c: &StaticConstraint,
        value: &ParamValue,
    ) -> Self {
        // If this is an enum type, return its (wire, label) options
        // regardless of constraint — enum constraints aren't ranges.
        if let Some(vals) = super::commands::enum_options_for(value) {
            return Self::enum_values(vals);
        }
        match eff {
            EffectiveConstraint::Static => Self::from_static(static_c),
            EffectiveConstraint::RangeU16(min, max) => Self::range(*min as f64, *max as f64),
            EffectiveConstraint::RangeU32(min, max) => Self::range(*min as f64, *max as f64),
            EffectiveConstraint::RangeF64(min, max) => Self::range(*min, *max),
            EffectiveConstraint::MinF64(min) => Self::min_only(*min),
        }
    }

    fn from_static(c: &StaticConstraint) -> Self {
        match c {
            StaticConstraint::None => Self::none(),
            StaticConstraint::RangeU16(min, max) => Self::range(*min as f64, *max as f64),
            StaticConstraint::RangeU32(min, max) => Self::range(*min as f64, *max as f64),
            StaticConstraint::RangeI32(min, max) => Self::range(*min as f64, *max as f64),
            StaticConstraint::RangeUsize(min, max) => Self::range(*min as f64, *max as f64),
            StaticConstraint::RangeF64(min, max) => Self::range(*min, *max),
            StaticConstraint::MinF64(min) => Self::min_only(*min),
            StaticConstraint::MinU32(min) => Self::min_only(*min as f64),
            StaticConstraint::MinUsize(min) => Self::min_only(*min as f64),
        }
    }
}

/// Type discriminant string for the frontend.
fn param_type_str(value: &ParamValue) -> &'static str {
    match value {
        ParamValue::Bool(_) => "bool",
        ParamValue::U16(_) => "u16",
        ParamValue::U32(_) => "u32",
        ParamValue::I32(_) => "i32",
        ParamValue::Usize(_) => "usize",
        ParamValue::F64(_) => "f64",
        ParamValue::String(_) => "string",
        ParamValue::StringVec(_) => "string_vec",
        ParamValue::Envelope(_)
        | ParamValue::Carrier(_)
        | ParamValue::Projection(_)
        | ParamValue::Structure(_)
        | ParamValue::Order(_)
        | ParamValue::VisualField(_)
        | ParamValue::Baseline(_)
        | ParamValue::CycleAverage(_)
        | ParamValue::CycleCombine(_)
        | ParamValue::PhaseSmoothing(_)
        | ParamValue::VfsComputation(_)
        | ParamValue::SignMapSmoothing(_)
        | ParamValue::CortexSource(_)
        | ParamValue::PatchThreshold(_)
        | ParamValue::PatchExtraction(_)
        | ParamValue::PatchRefinement(_)
        | ParamValue::Eccentricity(_) => "enum",
    }
}

/// Return the `(wire, label)` options for an enum parameter.
///
/// Each variant arm dispatches to [`openisi_params::enum_options`],
/// which projects the enum through serde (for the wire string) and
/// `strum::Display` (for the label). No parallel string registry —
/// the labels live next to the enum variant declarations in
/// `openisi-params/src/analysis_kinds.rs`,
/// `openisi-params/src/lib.rs` (carrier_types mod), and
/// `openisi-stimulus/src/{dataset,geometry,sequencer}.rs`.
fn enum_options_for(value: &ParamValue) -> Option<Vec<EnumOption>> {
    use super::{
        BaselineKind, Carrier, CortexSourceKind, CycleAverageKind, CycleCombineKind,
        EccentricityKind, Envelope, Order, PatchExtractionKind, PatchRefinementKind,
        PatchThresholdKind, PhaseSmoothingKind, Projection, SignMapSmoothingKind, Structure,
        VfsComputationKind, VisualField,
    };

    Some(match value {
        ParamValue::Envelope(_) => enum_options::<Envelope>(),
        ParamValue::Carrier(_) => enum_options::<Carrier>(),
        ParamValue::Projection(_) => enum_options::<Projection>(),
        ParamValue::Structure(_) => enum_options::<Structure>(),
        ParamValue::Order(_) => enum_options::<Order>(),
        ParamValue::VisualField(_) => enum_options::<VisualField>(),

        ParamValue::Baseline(_) => enum_options::<BaselineKind>(),
        ParamValue::CycleAverage(_) => enum_options::<CycleAverageKind>(),
        ParamValue::CycleCombine(_) => enum_options::<CycleCombineKind>(),
        ParamValue::PhaseSmoothing(_) => enum_options::<PhaseSmoothingKind>(),
        ParamValue::VfsComputation(_) => enum_options::<VfsComputationKind>(),
        ParamValue::SignMapSmoothing(_) => enum_options::<SignMapSmoothingKind>(),
        ParamValue::CortexSource(_) => enum_options::<CortexSourceKind>(),
        ParamValue::PatchThreshold(_) => enum_options::<PatchThresholdKind>(),
        ParamValue::PatchExtraction(_) => enum_options::<PatchExtractionKind>(),
        ParamValue::PatchRefinement(_) => enum_options::<PatchRefinementKind>(),
        ParamValue::Eccentricity(_) => enum_options::<EccentricityKind>(),

        _ => return None,
    })
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
/// render one card per stage. Single source of truth: derived from `PARAM_DEFS`
/// (`openisi_params::analysis_stage_groups`) + `GroupId::card_title`, so the
/// frontend never hardcodes the stage list (adding analysis params under a new
/// `GroupId` makes the stage appear automatically).
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

/// Return descriptors for all parameters, optionally filtered by group.
#[tauri::command]
pub fn get_param_descriptors(
    state: State<'_, SharedState>,
    group: Option<String>,
) -> AppResult<Vec<ParamDescriptorJson>> {
    let reg = state.registry.lock();

    // Two filter modes:
    //   * `target=…` (special-cased "analysis" / "rig" / "experiment" /
    //     "ui_state") returns every param of that persist target,
    //     regardless of `GroupId`. Used by the Analysis view, which
    //     spans many per-stage groups.
    //   * `<GroupId>` (the normal case) filters by group.
    // An unknown group string is now a fail-loud empty result — the
    // previous behavior silently bypassed the filter and returned every
    // descriptor in the registry.
    let group_str = group.as_deref();
    let target_filter: Option<crate::params::PersistTarget> =
        group_str.and_then(parse_target_filter);
    let group_filter: Option<GroupId> = match (group_str, target_filter) {
        (Some(_), Some(_)) => None, // target_filter wins
        (Some(s), None) => match parse_group_id(s) {
            Some(g) => Some(g),
            None => {
                tracing::warn!(group = ?s, "get_param_descriptors: unknown group — returning empty");
                return Ok(Vec::new());
            }
        },
        (None, _) => None,
    };

    let mut descriptors = Vec::new();
    for def in PARAM_DEFS.iter() {
        if let Some(t) = target_filter {
            if def.persist != t {
                continue;
            }
        } else if let Some(filter) = group_filter
            && def.group != filter
        {
            continue;
        }

        // Show the EFFECTIVE value (user-override > hardware > default)
        // for hardware-influenced params like MonitorWidthCm so the UI
        // reflects what the system is actually using, not the shipped
        // default. For everything else, this is identical to `reg.get`.
        let effective_value = reg.effective_value(def.id);
        let value = &effective_value;
        let effective = reg.effective_constraint(def.id);

        let active = reg.is_active(def.id);

        descriptors.push(ParamDescriptorJson {
            id: def.toml_path.to_string(),
            label: def.label.to_string(),
            unit: def.unit.to_string(),
            param_type: param_type_str(value).to_string(),
            value: param_value_to_json(value),
            constraint: ConstraintJson::from_effective(&effective, &def.constraint, value),
            active,
            group: def.group,
        });
    }

    tracing::debug!(
        group = ?group,
        count = descriptors.len(),
        descriptors = ?descriptors.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(),
        "get_param_descriptors",
    );

    Ok(descriptors)
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
    let updates = updates
        .as_object()
        .ok_or_else(|| AppError::Validation("updates must be a JSON object".into()))?;

    // Build a lookup from toml_path to ParamId.
    let path_to_id: std::collections::HashMap<&str, ParamId> = PARAM_DEFS
        .iter()
        .map(|def| (def.toml_path, def.id))
        .collect();

    // Phase 1: Parse and validate all updates.
    let mut parsed: Vec<(ParamId, ParamValue)> = Vec::with_capacity(updates.len());
    for (key, json_val) in updates {
        let id = path_to_id
            .get(key.as_str())
            .ok_or_else(|| AppError::Validation(format!("unknown parameter: {key}")))?;

        let param_val = json_to_param_value(*id, json_val)
            .map_err(|e| AppError::Validation(format!("{key}: {e}")))?;

        parsed.push((*id, param_val));
    }

    // Phase 2: Apply in batch mode.
    let mut reg = state.registry.lock();
    let mut updated = 0;
    let mut errors = Vec::new();

    reg.batch(|r: &mut super::Registry| {
        for (id, value) in parsed {
            match r.set(id, value) {
                Ok(()) => updated += 1,
                Err(e) => {
                    let def = &PARAM_DEFS[id as usize];
                    errors.push(format!("{}: {e}", def.toml_path));
                }
            }
        }
    });

    if !errors.is_empty() {
        return Err(AppError::Validation(errors.join("; ")));
    }

    // Detect cascaded changes (params that changed but were not in the input set).
    // After batch, pending_changes is empty (already emitted), so we track by
    // comparing. For now, return an empty cascade list — the params:changed event
    // carries the full delta for the frontend.
    let cascaded = Vec::new();

    Ok(SetParamsResult { updated, cascaded })
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Parse a JSON value into a ParamValue based on the parameter's known type.
///
/// Delegates to [`openisi_params::param_json::from_json`] — the single
/// canonical `Value` → `ParamValue` converter used everywhere a JSON
/// boundary crosses into the registry (Tauri IPC here, `.oisi`
/// provenance round-trip, TOML overlay parsing). One implementation;
/// every range check, every enum-string parse, every unit conversion
/// lives in one file.
///
/// Prior to this delegation, this function maintained a parallel
/// implementation that used lossy `as u16` / `as i32` casts instead of
/// range-checked `try_from`, silently truncating out-of-range input
/// before validation ever saw it. That bug is gone; range overflows now
/// surface as `AppError::Validation` with the exact value and bound.
fn json_to_param_value(id: ParamId, val: &serde_json::Value) -> AppResult<ParamValue> {
    let def = &PARAM_DEFS[id as usize];
    Ok(openisi_params::param_json::from_json(
        &def.default,
        val,
        def.toml_path,
    )?)
}

/// Parse a group name string into a GroupId.
/// Accept persist-target names ("analysis", "rig", "experiment",
/// "ui_state") as a separate filter axis from `GroupId`. The Analysis
/// view uses "analysis" to gather every analysis-pipeline param across
/// all per-stage groups (CycleCombine, PhaseSmoothing, …).
fn parse_target_filter(s: &str) -> Option<crate::params::PersistTarget> {
    match s {
        "analysis" => Some(crate::params::PersistTarget::Analysis),
        "rig" => Some(crate::params::PersistTarget::Rig),
        "experiment" => Some(crate::params::PersistTarget::Experiment),
        "ui_state" => Some(crate::params::PersistTarget::UiState),
        _ => None,
    }
}

/// Parse a snake_case group key into a `GroupId`. Delegates to the
/// `strum::EnumString` derive on `GroupId` (single source of truth — adding a
/// `GroupId` variant extends this automatically, so the parser can't drift).
fn parse_group_id(s: &str) -> Option<GroupId> {
    use std::str::FromStr;
    GroupId::from_str(s).ok()
}
