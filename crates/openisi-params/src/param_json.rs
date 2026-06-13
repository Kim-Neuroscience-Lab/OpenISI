//! The single `ParamValue` ⇄ `serde_json::Value` converter.
//!
//! Every serialization boundary projects `ParamValue` through THIS module:
//! - Tauri IPC descriptors / change events (`registry::param_value_to_json`)
//! - `.oisi` provenance JSON (`snapshot::{to_json_for_target, from_json_tree}`)
//! - TOML config files (`toml_io`, which bridges TOML ⇄ JSON via serde —
//!   `toml::Value` and `serde_json::Value` are both serde formats)
//!
//! One definition-driven converter, not three hand-written ones. A fix or
//! a new `ParamValue` variant is edited here once. Enum values serialize
//! via serde (`#[serde(rename_all = "snake_case")]` on every param enum),
//! which is byte-identical to the strings already on disk and on the wire.

use serde_json::Value;

use crate::error::{ParamsError, ParamsResult};
use crate::{
    BaselineKind, Carrier, CortexSourceKind, CycleAverageKind, CycleCombineKind, EccentricityKind,
    Envelope, Order, ParamValue, PatchExtractionKind, PatchRefinementKind, PatchThresholdKind,
    PhaseSmoothingKind, Projection, SignMapSmoothingKind, Structure, VfsComputationKind,
    VisualField,
};

/// Convert a `ParamValue` to a `serde_json::Value`. Enums serialize via
/// serde to their `snake_case` string form.
pub fn to_json(v: &ParamValue) -> Value {
    fn ser<T: serde::Serialize>(k: &T) -> Value {
        // Unit-variant enums are infallible to serialize; Null is an
        // unreachable fallback that keeps this total.
        serde_json::to_value(k).unwrap_or(Value::Null)
    }
    match v {
        ParamValue::Bool(b) => Value::Bool(*b),
        ParamValue::U16(n) => Value::Number((*n as u64).into()),
        ParamValue::U32(n) => Value::Number((*n as u64).into()),
        ParamValue::I32(n) => Value::Number((*n as i64).into()),
        ParamValue::Usize(n) => Value::Number((*n as u64).into()),
        ParamValue::F64(f) => serde_json::Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        ParamValue::String(s) => Value::String(s.clone()),
        ParamValue::StringVec(v) => {
            Value::Array(v.iter().map(|s| Value::String(s.clone())).collect())
        }
        ParamValue::Envelope(e) => ser(e),
        ParamValue::Carrier(c) => ser(c),
        ParamValue::Projection(p) => ser(p),
        ParamValue::Structure(s) => ser(s),
        ParamValue::Order(o) => ser(o),
        ParamValue::VisualField(v) => ser(v),
        ParamValue::Baseline(k) => ser(k),
        ParamValue::CycleAverage(k) => ser(k),
        ParamValue::CycleCombine(k) => ser(k),
        ParamValue::PhaseSmoothing(k) => ser(k),
        ParamValue::VfsComputation(k) => ser(k),
        ParamValue::SignMapSmoothing(k) => ser(k),
        ParamValue::CortexSource(k) => ser(k),
        ParamValue::PatchThreshold(k) => ser(k),
        ParamValue::PatchExtraction(k) => ser(k),
        ParamValue::PatchRefinement(k) => ser(k),
        ParamValue::Eccentricity(k) => ser(k),
    }
}

/// Parse a `serde_json::Value` into a `ParamValue` matching the variant of
/// `template` (the param's `default`, which identifies the expected type).
///
/// Integer types are **range-checked** — a value outside the target type's
/// range is a hard error, never a silent truncation/wrap. Enum strings are
/// validated against the enum's serde representation. `path` names the
/// parameter in error messages.
pub fn from_json(template: &ParamValue, json: &Value, path: &str) -> ParamsResult<ParamValue> {
    let cfg = |msg: String| ParamsError::Config(msg);
    let str_enum = |s: &str| format!("\"{s}\"");
    match template {
        ParamValue::Bool(_) => json
            .as_bool()
            .map(ParamValue::Bool)
            .ok_or_else(|| cfg(format!("expected bool at {path}"))),
        ParamValue::U16(_) => {
            let n = json
                .as_u64()
                .ok_or_else(|| cfg(format!("expected non-negative integer at {path}")))?;
            u16::try_from(n).map(ParamValue::U16).map_err(|_| {
                cfg(format!(
                    "value {n} at {path} out of range for u16 (0..={})",
                    u16::MAX
                ))
            })
        }
        ParamValue::U32(_) => {
            let n = json
                .as_u64()
                .ok_or_else(|| cfg(format!("expected non-negative integer at {path}")))?;
            u32::try_from(n).map(ParamValue::U32).map_err(|_| {
                cfg(format!(
                    "value {n} at {path} out of range for u32 (0..={})",
                    u32::MAX
                ))
            })
        }
        ParamValue::I32(_) => {
            let n = json
                .as_i64()
                .ok_or_else(|| cfg(format!("expected integer at {path}")))?;
            i32::try_from(n).map(ParamValue::I32).map_err(|_| {
                cfg(format!(
                    "value {n} at {path} out of range for i32 ({}..={})",
                    i32::MIN,
                    i32::MAX
                ))
            })
        }
        ParamValue::Usize(_) => {
            let n = json
                .as_u64()
                .ok_or_else(|| cfg(format!("expected non-negative integer at {path}")))?;
            usize::try_from(n)
                .map(ParamValue::Usize)
                .map_err(|_| cfg(format!("value {n} at {path} out of range for usize")))
        }
        ParamValue::F64(_) => json
            .as_f64()
            .or_else(|| json.as_i64().map(|i| i as f64))
            .map(ParamValue::F64)
            .ok_or_else(|| cfg(format!("expected number at {path}"))),
        ParamValue::String(_) => json
            .as_str()
            .map(|s| ParamValue::String(s.to_string()))
            .ok_or_else(|| cfg(format!("expected string at {path}"))),
        ParamValue::StringVec(_) => json
            .as_array()
            .map(|arr| {
                ParamValue::StringVec(
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect(),
                )
            })
            .ok_or_else(|| cfg(format!("expected array of strings at {path}"))),
        ParamValue::Envelope(_) => json
            .as_str()
            .and_then(|s| serde_json::from_str::<Envelope>(&str_enum(s)).ok())
            .map(ParamValue::Envelope)
            .ok_or_else(|| cfg(format!("expected envelope string at {path}"))),
        ParamValue::Carrier(_) => json
            .as_str()
            .and_then(|s| serde_json::from_str::<Carrier>(&str_enum(s)).ok())
            .map(ParamValue::Carrier)
            .ok_or_else(|| cfg(format!("expected carrier string at {path}"))),
        ParamValue::Projection(_) => json
            .as_str()
            .and_then(|s| serde_json::from_str::<Projection>(&str_enum(s)).ok())
            .map(ParamValue::Projection)
            .ok_or_else(|| cfg(format!("expected projection string at {path}"))),
        ParamValue::Structure(_) => json
            .as_str()
            .and_then(|s| serde_json::from_str::<Structure>(&str_enum(s)).ok())
            .map(ParamValue::Structure)
            .ok_or_else(|| cfg(format!("expected structure string at {path}"))),
        ParamValue::Order(_) => json
            .as_str()
            .and_then(|s| serde_json::from_str::<Order>(&str_enum(s)).ok())
            .map(ParamValue::Order)
            .ok_or_else(|| cfg(format!("expected order string at {path}"))),
        ParamValue::VisualField(_) => json
            .as_str()
            .and_then(|s| serde_json::from_str::<VisualField>(&str_enum(s)).ok())
            .map(ParamValue::VisualField)
            .ok_or_else(|| {
                cfg(format!(
                    "expected visual_field string (\"left\" or \"right\") at {path}"
                ))
            }),
        ParamValue::Baseline(_) => json
            .as_str()
            .and_then(|s| serde_json::from_str::<BaselineKind>(&str_enum(s)).ok())
            .map(ParamValue::Baseline)
            .ok_or_else(|| cfg(format!("expected baseline method string at {path}"))),
        ParamValue::CycleAverage(_) => json
            .as_str()
            .and_then(|s| serde_json::from_str::<CycleAverageKind>(&str_enum(s)).ok())
            .map(ParamValue::CycleAverage)
            .ok_or_else(|| cfg(format!("expected cycle_average method string at {path}"))),
        ParamValue::CycleCombine(_) => json
            .as_str()
            .and_then(|s| serde_json::from_str::<CycleCombineKind>(&str_enum(s)).ok())
            .map(ParamValue::CycleCombine)
            .ok_or_else(|| cfg(format!("expected cycle_combine method string at {path}"))),
        ParamValue::PhaseSmoothing(_) => json
            .as_str()
            .and_then(|s| serde_json::from_str::<PhaseSmoothingKind>(&str_enum(s)).ok())
            .map(ParamValue::PhaseSmoothing)
            .ok_or_else(|| cfg(format!("expected phase_smoothing method string at {path}"))),
        ParamValue::VfsComputation(_) => json
            .as_str()
            .and_then(|s| serde_json::from_str::<VfsComputationKind>(&str_enum(s)).ok())
            .map(ParamValue::VfsComputation)
            .ok_or_else(|| cfg(format!("expected vfs_computation method string at {path}"))),
        ParamValue::SignMapSmoothing(_) => json
            .as_str()
            .and_then(|s| serde_json::from_str::<SignMapSmoothingKind>(&str_enum(s)).ok())
            .map(ParamValue::SignMapSmoothing)
            .ok_or_else(|| {
                cfg(format!(
                    "expected sign_map_smoothing method string at {path}"
                ))
            }),
        ParamValue::CortexSource(_) => json
            .as_str()
            .and_then(|s| serde_json::from_str::<CortexSourceKind>(&str_enum(s)).ok())
            .map(ParamValue::CortexSource)
            .ok_or_else(|| cfg(format!("expected cortex_source method string at {path}"))),
        ParamValue::PatchThreshold(_) => json
            .as_str()
            .and_then(|s| serde_json::from_str::<PatchThresholdKind>(&str_enum(s)).ok())
            .map(ParamValue::PatchThreshold)
            .ok_or_else(|| cfg(format!("expected patch_threshold method string at {path}"))),
        ParamValue::PatchExtraction(_) => json
            .as_str()
            .and_then(|s| serde_json::from_str::<PatchExtractionKind>(&str_enum(s)).ok())
            .map(ParamValue::PatchExtraction)
            .ok_or_else(|| cfg(format!("expected patch_extraction method string at {path}"))),
        ParamValue::PatchRefinement(_) => json
            .as_str()
            .and_then(|s| serde_json::from_str::<PatchRefinementKind>(&str_enum(s)).ok())
            .map(ParamValue::PatchRefinement)
            .ok_or_else(|| cfg(format!("expected patch_refinement method string at {path}"))),
        ParamValue::Eccentricity(_) => json
            .as_str()
            .and_then(|s| serde_json::from_str::<EccentricityKind>(&str_enum(s)).ok())
            .map(ParamValue::Eccentricity)
            .ok_or_else(|| cfg(format!("expected eccentricity method string at {path}"))),
    }
}
