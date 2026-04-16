//! Tauri commands for parameter registry — get descriptors and batch-set values.

use serde::Serialize;
use tauri::State;

use crate::commands::SharedState;
use crate::error::{lock_state, AppError, AppResult};

use super::constraints::EffectiveConstraint;
use super::registry::param_value_to_json;
use super::{GroupId, ParamId, ParamValue, StaticConstraint, PARAM_DEFS};

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
#[derive(Debug, Serialize)]
pub struct ConstraintJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,
}

impl ConstraintJson {
    fn none() -> Self {
        Self { min: None, max: None, values: None }
    }

    fn range(min: f64, max: f64) -> Self {
        Self { min: Some(min), max: Some(max), values: None }
    }

    fn min_only(min: f64) -> Self {
        Self { min: Some(min), max: None, values: None }
    }

    fn enum_values(vals: Vec<&str>) -> Self {
        Self { min: None, max: None, values: Some(vals.into_iter().map(String::from).collect()) }
    }

    fn from_effective(eff: &EffectiveConstraint, static_c: &StaticConstraint, value: &ParamValue) -> Self {
        // If this is an enum type, return enum values regardless of constraint
        if let Some(vals) = super::commands::enum_values(value) {
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
        | ParamValue::Order(_) => "enum",
    }
}

/// Get the list of valid enum values for an enum parameter.
fn enum_values(value: &ParamValue) -> Option<Vec<&'static str>> {
    match value {
        ParamValue::Envelope(_) => Some(vec!["bar", "wedge", "ring", "fullfield"]),
        ParamValue::Carrier(_) => Some(vec!["solid", "checkerboard"]),
        ParamValue::Projection(_) => Some(vec!["cartesian", "spherical", "cylindrical"]),
        ParamValue::Structure(_) => Some(vec!["blocked", "interleaved"]),
        ParamValue::Order(_) => Some(vec!["sequential", "interleaved", "randomized"]),
        _ => None,
    }
}

// ─── Commands ────────────────────────────────────────────────────────────────

/// Return descriptors for all parameters, optionally filtered by group.
#[tauri::command]
pub fn get_param_descriptors(
    state: State<'_, SharedState>,
    group: Option<String>,
) -> AppResult<Vec<ParamDescriptorJson>> {
    let s = lock_state(&state, "get_param_descriptors")?;
    let reg = lock_state(&s.registry, "get_param_descriptors registry")?;

    // Parse group filter if provided.
    let group_filter: Option<GroupId> = group.as_deref().and_then(parse_group_id);

    let mut descriptors = Vec::new();
    for def in PARAM_DEFS.iter() {
        if let Some(filter) = group_filter {
            if def.group != filter {
                continue;
            }
        }

        let value = reg.get(def.id);
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

    eprintln!("[params] get_param_descriptors(group={:?}) returning {} descriptors: {:?}",
        group, descriptors.len(), descriptors.iter().map(|d| d.id.as_str()).collect::<Vec<_>>());

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

    let s = lock_state(&state, "set_params")?;

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
    let mut reg = lock_state(&s.registry, "set_params registry")?;
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
fn json_to_param_value(id: ParamId, val: &serde_json::Value) -> Result<ParamValue, String> {
    let def = &PARAM_DEFS[id as usize];
    match &def.default {
        ParamValue::Bool(_) => val
            .as_bool()
            .map(ParamValue::Bool)
            .ok_or_else(|| "expected boolean".into()),

        ParamValue::U16(_) => val
            .as_u64()
            .map(|v| ParamValue::U16(v as u16))
            .ok_or_else(|| "expected integer".into()),

        ParamValue::U32(_) => val
            .as_u64()
            .map(|v| ParamValue::U32(v as u32))
            .ok_or_else(|| "expected integer".into()),

        ParamValue::I32(_) => val
            .as_i64()
            .map(|v| ParamValue::I32(v as i32))
            .ok_or_else(|| "expected integer".into()),

        ParamValue::Usize(_) => val
            .as_u64()
            .map(|v| ParamValue::Usize(v as usize))
            .ok_or_else(|| "expected integer".into()),

        ParamValue::F64(_) => val
            .as_f64()
            .map(ParamValue::F64)
            .ok_or_else(|| "expected number".into()),

        ParamValue::String(_) => val
            .as_str()
            .map(|s| ParamValue::String(s.to_string()))
            .ok_or_else(|| "expected string".into()),

        ParamValue::StringVec(_) => val
            .as_array()
            .map(|arr| {
                ParamValue::StringVec(
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect(),
                )
            })
            .ok_or_else(|| "expected array of strings".into()),

        ParamValue::Envelope(_) => val
            .as_str()
            .and_then(|s| serde_json::from_str::<super::Envelope>(&format!("\"{s}\"")).ok())
            .map(ParamValue::Envelope)
            .ok_or_else(|| "expected envelope string".into()),

        ParamValue::Carrier(_) => val
            .as_str()
            .and_then(|s| serde_json::from_str::<super::Carrier>(&format!("\"{s}\"")).ok())
            .map(ParamValue::Carrier)
            .ok_or_else(|| "expected carrier string".into()),

        ParamValue::Projection(_) => val
            .as_str()
            .and_then(|s| serde_json::from_str::<super::Projection>(&format!("\"{s}\"")).ok())
            .map(ParamValue::Projection)
            .ok_or_else(|| "expected projection string".into()),

        ParamValue::Structure(_) => val
            .as_str()
            .and_then(|s| serde_json::from_str::<super::Structure>(&format!("\"{s}\"")).ok())
            .map(ParamValue::Structure)
            .ok_or_else(|| "expected structure string".into()),

        ParamValue::Order(_) => val
            .as_str()
            .and_then(|s| serde_json::from_str::<super::Order>(&format!("\"{s}\"")).ok())
            .map(ParamValue::Order)
            .ok_or_else(|| "expected order string".into()),
    }
}

/// Parse a group name string into a GroupId.
fn parse_group_id(s: &str) -> Option<GroupId> {
    match s {
        "stimulus" => Some(GroupId::Stimulus),
        "geometry" => Some(GroupId::Geometry),
        "timing" => Some(GroupId::Timing),
        "presentation" => Some(GroupId::Presentation),
        "retinotopy" => Some(GroupId::Retinotopy),
        "segmentation" => Some(GroupId::Segmentation),
        "camera" => Some(GroupId::Camera),
        "display" => Some(GroupId::Display),
        "ring" => Some(GroupId::Ring),
        "system" => Some(GroupId::System),
        "paths" => Some(GroupId::Paths),
        _ => None,
    }
}
