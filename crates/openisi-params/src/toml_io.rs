//! TOML serialization/deserialization for the parameter registry.
//!
//! Reads/writes rig.toml and experiment.toml in a format identical to the
//! existing config system. Parameters are addressed by their `toml_path`
//! (e.g., "camera.exposure_us" → `[camera]` section, key `exposure_us`).

use std::path::Path;

use super::{
    Carrier, CortexSourceKind, CycleCombineKind, EccentricityKind, Envelope, ExperimentMeta,
    Order, ParamId, ParamValue, PatchExtractionKind, PatchRefinementKind, PatchThresholdKind,
    PersistTarget, PhaseSmoothingKind, Projection, QualityGateKind, SignMapSmoothingKind,
    Structure, VfsComputationKind, PARAM_DEFS,
};
use super::registry::Registry;
use crate::error::{ParamsError, ParamsResult};

// ─── Loading ──────────────────────────────────────────────────────────────────

/// Which config layer a TOML file represents — drives whether reads
/// mark params as user overrides (so `save_user_*` later serializes
/// them) or apply them silently as the shipped baseline.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum LoadLayer {
    /// Shipped/baseline read. Values populate `values[]` via the
    /// internal `set_from_shipped`; `user_overrides` is NOT touched.
    Shipped,
    /// User-layer read (either from `user_dir/<target>.toml` or from a
    /// user-chosen template path). Values populate `values[]` via the
    /// public `set` which marks them in `user_overrides`.
    User,
}

/// Load a TOML file for one `PersistTarget` at the given layer. Unknown
/// keys (paths not in the registry for that target) are a hard error,
/// not silently ignored. Keys that fail to parse to the declared type
/// are also a hard error.
fn load_for_target(
    registry: &mut Registry,
    path: &Path,
    target: PersistTarget,
    label: &str,
    layer: LoadLayer,
) -> ParamsResult<()> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| ParamsError::Config(format!("Failed to read {label} ({}): {e}", path.display())))?;
    let table: toml::Value = contents.parse().map_err(|e| {
        ParamsError::Config(format!("Failed to parse {label} ({}): {e}", path.display()))
    })?;

    // 1. Set every registered param for this target whose key is present.
    //    Wrapped in `batch()` so the N individual set validations don't
    //    fire N change events — at most one event is emitted when the load
    //    completes.
    let mut known_paths: std::collections::HashSet<&'static str> =
        std::collections::HashSet::new();
    for def in PARAM_DEFS.iter() {
        if def.persist != target { continue; }
        known_paths.insert(def.toml_path);
    }
    registry.batch(|reg| -> ParamsResult<()> {
        for def in PARAM_DEFS.iter() {
            if def.persist != target { continue; }
            if let Some(val) = navigate_toml(&table, def.toml_path) {
                // toml_to_param_value returns ParamsError::Config with the
                // toml_path embedded; add the file label for full
                // context.
                let pv = toml_to_param_value(def.id, val).map_err(|e| {
                    ParamsError::Config(format!("{label}: {e}"))
                })?;
                match layer {
                    LoadLayer::Shipped => reg.set_from_shipped(def.id, pv)?,
                    LoadLayer::User => reg.set(def.id, pv)?,
                }
            }
        }
        Ok(())
    })?;

    // 2. Validate: every leaf in the TOML must be a known key for this
    //    target. Catches typos that would otherwise be silently ignored.
    let mut unknown: Vec<String> = Vec::new();
    collect_unknown_leaves(&table, "", &known_paths, &mut unknown);
    if !unknown.is_empty() {
        return Err(ParamsError::Config(format!(
            "{label}: unknown key(s) — not defined in the parameter registry: {}",
            unknown.join(", ")
        )));
    }

    Ok(())
}

// ── Shipped-layer loaders (baseline; do NOT mark user_overrides) ─────

/// Load the shipped rig.toml into the registry's baseline.
pub fn load_shipped_rig(registry: &mut Registry, path: &Path) -> ParamsResult<()> {
    load_for_target(registry, path, PersistTarget::Rig, "rig.toml (shipped)", LoadLayer::Shipped)
}

/// Load the shipped analysis.toml into the registry's baseline.
pub fn load_shipped_analysis(registry: &mut Registry, path: &Path) -> ParamsResult<()> {
    load_for_target(registry, path, PersistTarget::Analysis, "analysis.toml (shipped)", LoadLayer::Shipped)
}

/// Load the shipped experiment.toml into the registry's baseline.
pub fn load_shipped_experiment(registry: &mut Registry, path: &Path) -> ParamsResult<()> {
    load_for_target(registry, path, PersistTarget::Experiment, "experiment.toml (shipped)", LoadLayer::Shipped)
}

// ── User-layer loaders (overlay; mark user_overrides) ────────────────

/// Overlay the user's rig.toml on top of the baseline. Params present
/// in the file are marked as user overrides so `save_user_rig` will
/// re-serialize them.
pub fn load_user_rig(registry: &mut Registry, path: &Path) -> ParamsResult<()> {
    load_for_target(registry, path, PersistTarget::Rig, "rig.toml (user)", LoadLayer::User)
}

/// Overlay the user's analysis.toml on top of the baseline.
pub fn load_user_analysis(registry: &mut Registry, path: &Path) -> ParamsResult<()> {
    load_for_target(registry, path, PersistTarget::Analysis, "analysis.toml (user)", LoadLayer::User)
}

/// Overlay the user's experiment.toml on top of the baseline.
pub fn load_user_experiment(registry: &mut Registry, path: &Path) -> ParamsResult<()> {
    load_for_target(registry, path, PersistTarget::Experiment, "experiment.toml (user)", LoadLayer::User)
}

/// Load a stand-alone experiment template from an arbitrary path
/// (typically chosen by the user via a file dialog). All params present
/// are applied as user overrides — opening a template is equivalent to
/// the user setting each value by hand. Distinct from the layered
/// `load_user_experiment`, which targets the canonical user-overrides
/// file at `user_dir/experiment.toml`.
pub fn load_experiment(registry: &mut Registry, path: &Path) -> ParamsResult<()> {
    load_for_target(registry, path, PersistTarget::Experiment, "experiment template", LoadLayer::User)
}

/// Walk a TOML table and append the dotted path of every leaf value that
/// is not a known `toml_path` for this target. Skips intermediate tables
/// whose every nested leaf is known (those are just structural).
fn collect_unknown_leaves(
    val: &toml::Value,
    prefix: &str,
    known: &std::collections::HashSet<&'static str>,
    out: &mut Vec<String>,
) {
    if let toml::Value::Table(t) = val {
        for (k, v) in t {
            let path = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
            if v.is_table() {
                collect_unknown_leaves(v, &path, known, out);
            } else if !known.contains(path.as_str()) {
                out.push(path);
            }
        }
    }
}

/// Load a saved .experiment.toml file, extracting metadata and experiment params.
pub fn load_experiment_file(
    registry: &mut Registry,
    path: &Path,
) -> ParamsResult<ExperimentMeta> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| ParamsError::Config(format!("Failed to read {}: {e}", path.display())))?;
    let table: toml::Value = contents
        .parse()
        .map_err(|e| ParamsError::Config(format!("Failed to parse {}: {e}", path.display())))?;

    // Extract metadata from top-level keys
    let meta = ExperimentMeta {
        name: table.get("name").and_then(|v| v.as_str()).map(String::from),
        description: table
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from),
        created: table
            .get("created")
            .and_then(|v| v.as_str())
            .map(String::from),
        modified: table
            .get("modified")
            .and_then(|v| v.as_str())
            .map(String::from),
    };

    // Load experiment params — wrapped in batch() for atomic change emission.
    registry.batch(|reg| -> ParamsResult<()> {
        for def in PARAM_DEFS.iter() {
            if def.persist != PersistTarget::Experiment {
                continue;
            }
            if let Some(val) = navigate_toml(&table, def.toml_path) {
                // toml_to_param_value already embeds the toml_path;
                // add the file path for full context.
                let pv = toml_to_param_value(def.id, val).map_err(|e| {
                    ParamsError::Config(format!("{}: {e}", path.display()))
                })?;
                reg.set(def.id, pv)?;
            }
        }
        Ok(())
    })?;

    Ok(meta)
}

// ─── Saving ───────────────────────────────────────────────────────────────────

/// Write a TOML root to `path`, creating parent directories on demand.
/// Common tail for both the user-layer and full-snapshot savers.
fn write_toml(
    path: &Path,
    root: toml::map::Map<String, toml::Value>,
    label: &str,
) -> ParamsResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            ParamsError::Config(format!("Failed to create {} for {label}: {e}", parent.display()))
        })?;
    }
    let toml_str = toml::to_string_pretty(&toml::Value::Table(root))
        .map_err(|e| ParamsError::Config(format!("Failed to serialize {label}: {e}")))?;
    std::fs::write(path, toml_str)
        .map_err(|e| ParamsError::Config(format!("Failed to write {} ({label}): {e}", path.display())))
}

/// Serialize only the params currently in `registry.user_overrides`
/// whose persist target matches. The shipped baseline is never written
/// — this is the user layer.
fn save_user_for_target(
    registry: &Registry,
    path: &Path,
    target: PersistTarget,
    label: &str,
) -> ParamsResult<()> {
    let mut root = toml::map::Map::new();
    for def in PARAM_DEFS.iter() {
        if def.persist != target { continue; }
        if !registry.is_user_override(def.id) { continue; }
        let value = registry.get(def.id);
        let toml_val = param_value_to_toml(value);
        insert_at_path(&mut root, def.toml_path, toml_val)?;
    }
    write_toml(path, root, label)
}

/// Serialize every param with the given persist target — full snapshot,
/// independent of `user_overrides`. Used by the named-experiment-template
/// feature (`save_experiment_as`), not by the layered auto-persist path.
fn save_full_for_target(
    registry: &Registry,
    path: &Path,
    target: PersistTarget,
    label: &str,
) -> ParamsResult<()> {
    let mut root = toml::map::Map::new();
    for def in PARAM_DEFS.iter() {
        if def.persist != target { continue; }
        let value = registry.get(def.id);
        let toml_val = param_value_to_toml(value);
        insert_at_path(&mut root, def.toml_path, toml_val)?;
    }
    write_toml(path, root, label)
}

// ── User-layer savers (sparse; user_overrides only) ──────────────────

/// Write the user layer's rig.toml — only params currently marked as
/// user overrides for the Rig target. Creates `path`'s parent dir lazily.
pub fn save_user_rig(registry: &Registry, path: &Path) -> ParamsResult<()> {
    save_user_for_target(registry, path, PersistTarget::Rig, "rig.toml (user)")
}

/// Write the user layer's analysis.toml.
pub fn save_user_analysis(registry: &Registry, path: &Path) -> ParamsResult<()> {
    save_user_for_target(registry, path, PersistTarget::Analysis, "analysis.toml (user)")
}

/// Write the user layer's experiment.toml.
pub fn save_user_experiment(registry: &Registry, path: &Path) -> ParamsResult<()> {
    save_user_for_target(registry, path, PersistTarget::Experiment, "experiment.toml (user)")
}

/// Save a stand-alone experiment template — full snapshot of every
/// Experiment-target param to an arbitrary user-chosen path. Distinct
/// from `save_user_experiment`, which writes only user overrides to
/// the layered file. Used by the `save_experiment_as` Tauri command.
pub fn save_experiment(registry: &Registry, path: &Path) -> ParamsResult<()> {
    save_full_for_target(registry, path, PersistTarget::Experiment, "experiment template")
}

/// Save experiment params + metadata to a .experiment.toml file.
pub fn save_experiment_file(
    registry: &Registry,
    path: &Path,
    meta: &ExperimentMeta,
) -> ParamsResult<()> {
    let mut root = toml::map::Map::new();

    // Write metadata at top level
    if let Some(ref name) = meta.name {
        root.insert("name".into(), toml::Value::String(name.clone()));
    }
    if let Some(ref desc) = meta.description {
        root.insert("description".into(), toml::Value::String(desc.clone()));
    }
    if let Some(ref created) = meta.created {
        root.insert("created".into(), toml::Value::String(created.clone()));
    }
    if let Some(ref modified) = meta.modified {
        root.insert("modified".into(), toml::Value::String(modified.clone()));
    }

    // Write experiment params
    for def in PARAM_DEFS.iter() {
        if def.persist != PersistTarget::Experiment {
            continue;
        }
        let value = registry.get(def.id);
        let toml_val = param_value_to_toml(value);
        insert_at_path(&mut root, def.toml_path, toml_val)?;
    }

    let toml_str = toml::to_string_pretty(&toml::Value::Table(root))
        .map_err(|e| ParamsError::Config(format!("Failed to serialize experiment file: {e}")))?;
    std::fs::write(path, toml_str)
        .map_err(|e| ParamsError::Config(format!("Failed to write {}: {e}", path.display())))
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Navigate a TOML value tree by a dotted path (e.g., "camera.exposure_us").
fn navigate_toml<'a>(root: &'a toml::Value, path: &str) -> Option<&'a toml::Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = root;
    for part in &parts {
        current = current.get(part)?;
    }
    Some(current)
}

/// Insert a value into a nested TOML map at a dotted path. Returns an
/// error if a prefix of the path already holds a non-table value (which
/// would only happen if two `define_params!` entries declared
/// conflicting paths — a programming error in `definitions.rs`).
///
/// Errors are `ParamsError::Config` — the canonical error type for
/// configuration-shape failures. No `Result<_, String>` "internal
/// helper" exemption.
fn insert_at_path(
    root: &mut toml::map::Map<String, toml::Value>,
    path: &str,
    value: toml::Value,
) -> ParamsResult<()> {
    let parts: Vec<&str> = path.split('.').collect();
    let Some((last, head)) = parts.split_last() else {
        return Err(ParamsError::Config("empty TOML path".into()));
    };
    if head.is_empty() {
        root.insert((*last).to_string(), value);
        return Ok(());
    }
    let mut current = root;
    for part in head {
        let entry = current
            .entry(part.to_string())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        current = entry.as_table_mut().ok_or_else(|| {
            ParamsError::Config(format!("TOML path conflict at '{part}' in '{path}': expected table"))
        })?;
    }
    current.insert((*last).to_string(), value);
    Ok(())
}

/// Convert a TOML value to a ParamValue based on the parameter's known type.
///
/// Errors are `ParamsError::Config` (the value's shape doesn't match
/// what the parameter expects) — no `Result<_, String>` exemption.
fn toml_to_param_value(id: ParamId, val: &toml::Value) -> ParamsResult<ParamValue> {
    let def = &PARAM_DEFS[id as usize];
    let cfg = |msg: String| ParamsError::Config(msg);
    match &def.default {
        ParamValue::Bool(_) => val.as_bool().map(ParamValue::Bool)
            .ok_or_else(|| cfg(format!("expected bool for {}", def.toml_path))),
        // Integer types: range-checked, never silently truncating. TOML
        // integers are i64; a value outside the target type's range is a
        // hard error rather than a wrapped number.
        ParamValue::U16(_) => {
            let n = val.as_integer()
                .ok_or_else(|| cfg(format!("expected integer for {}", def.toml_path)))?;
            u16::try_from(n).map(ParamValue::U16)
                .map_err(|_| cfg(format!("value {n} for {} out of range for u16 (0..={})", def.toml_path, u16::MAX)))
        }
        ParamValue::U32(_) => {
            let n = val.as_integer()
                .ok_or_else(|| cfg(format!("expected integer for {}", def.toml_path)))?;
            u32::try_from(n).map(ParamValue::U32)
                .map_err(|_| cfg(format!("value {n} for {} out of range for u32 (0..={})", def.toml_path, u32::MAX)))
        }
        ParamValue::I32(_) => {
            let n = val.as_integer()
                .ok_or_else(|| cfg(format!("expected integer for {}", def.toml_path)))?;
            i32::try_from(n).map(ParamValue::I32)
                .map_err(|_| cfg(format!("value {n} for {} out of range for i32 ({}..={})", def.toml_path, i32::MIN, i32::MAX)))
        }
        ParamValue::Usize(_) => {
            let n = val.as_integer()
                .ok_or_else(|| cfg(format!("expected integer for {}", def.toml_path)))?;
            usize::try_from(n).map(ParamValue::Usize)
                .map_err(|_| cfg(format!("value {n} for {} out of range for usize", def.toml_path)))
        }
        ParamValue::F64(_) => {
            if let Some(f) = val.as_float() {
                Ok(ParamValue::F64(f))
            } else if let Some(i) = val.as_integer() {
                Ok(ParamValue::F64(i as f64))
            } else {
                Err(cfg(format!("expected number for {}", def.toml_path)))
            }
        }
        ParamValue::String(_) => val.as_str().map(|s| ParamValue::String(s.to_string()))
            .ok_or_else(|| cfg(format!("expected string for {}", def.toml_path))),
        ParamValue::StringVec(_) => val.as_array().map(|arr| {
            ParamValue::StringVec(arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        }).ok_or_else(|| cfg(format!("expected array for {}", def.toml_path))),
        ParamValue::Envelope(_) => val.as_str()
            .and_then(|s| serde_json::from_str::<Envelope>(&format!("\"{s}\"")).ok())
            .map(ParamValue::Envelope)
            .ok_or_else(|| cfg(format!("expected envelope string for {}", def.toml_path))),
        ParamValue::Carrier(_) => val.as_str()
            .and_then(|s| serde_json::from_str::<Carrier>(&format!("\"{s}\"")).ok())
            .map(ParamValue::Carrier)
            .ok_or_else(|| cfg(format!("expected carrier string for {}", def.toml_path))),
        ParamValue::Projection(_) => val.as_str()
            .and_then(|s| serde_json::from_str::<Projection>(&format!("\"{s}\"")).ok())
            .map(ParamValue::Projection)
            .ok_or_else(|| cfg(format!("expected projection string for {}", def.toml_path))),
        ParamValue::Structure(_) => val.as_str()
            .and_then(|s| serde_json::from_str::<Structure>(&format!("\"{s}\"")).ok())
            .map(ParamValue::Structure)
            .ok_or_else(|| cfg(format!("expected structure string for {}", def.toml_path))),
        ParamValue::Order(_) => val.as_str()
            .and_then(|s| serde_json::from_str::<Order>(&format!("\"{s}\"")).ok())
            .map(ParamValue::Order)
            .ok_or_else(|| cfg(format!("expected order string for {}", def.toml_path))),
        ParamValue::CycleCombine(_) => val.as_str()
            .and_then(|s| serde_json::from_str::<CycleCombineKind>(&format!("\"{s}\"")).ok())
            .map(ParamValue::CycleCombine)
            .ok_or_else(|| cfg(format!("expected cycle_combine method string for {}", def.toml_path))),
        ParamValue::PhaseSmoothing(_) => val.as_str()
            .and_then(|s| serde_json::from_str::<PhaseSmoothingKind>(&format!("\"{s}\"")).ok())
            .map(ParamValue::PhaseSmoothing)
            .ok_or_else(|| cfg(format!("expected phase_smoothing method string for {}", def.toml_path))),
        ParamValue::VfsComputation(_) => val.as_str()
            .and_then(|s| serde_json::from_str::<VfsComputationKind>(&format!("\"{s}\"")).ok())
            .map(ParamValue::VfsComputation)
            .ok_or_else(|| cfg(format!("expected vfs_computation method string for {}", def.toml_path))),
        ParamValue::SignMapSmoothing(_) => val.as_str()
            .and_then(|s| serde_json::from_str::<SignMapSmoothingKind>(&format!("\"{s}\"")).ok())
            .map(ParamValue::SignMapSmoothing)
            .ok_or_else(|| cfg(format!("expected sign_map_smoothing method string for {}", def.toml_path))),
        ParamValue::CortexSource(_) => val.as_str()
            .and_then(|s| serde_json::from_str::<CortexSourceKind>(&format!("\"{s}\"")).ok())
            .map(ParamValue::CortexSource)
            .ok_or_else(|| cfg(format!("expected cortex_source method string for {}", def.toml_path))),
        ParamValue::PatchThreshold(_) => val.as_str()
            .and_then(|s| serde_json::from_str::<PatchThresholdKind>(&format!("\"{s}\"")).ok())
            .map(ParamValue::PatchThreshold)
            .ok_or_else(|| cfg(format!("expected patch_threshold method string for {}", def.toml_path))),
        ParamValue::PatchExtraction(_) => val.as_str()
            .and_then(|s| serde_json::from_str::<PatchExtractionKind>(&format!("\"{s}\"")).ok())
            .map(ParamValue::PatchExtraction)
            .ok_or_else(|| cfg(format!("expected patch_extraction method string for {}", def.toml_path))),
        ParamValue::PatchRefinement(_) => val.as_str()
            .and_then(|s| serde_json::from_str::<PatchRefinementKind>(&format!("\"{s}\"")).ok())
            .map(ParamValue::PatchRefinement)
            .ok_or_else(|| cfg(format!("expected patch_refinement method string for {}", def.toml_path))),
        ParamValue::QualityGate(_) => val.as_str()
            .and_then(|s| serde_json::from_str::<QualityGateKind>(&format!("\"{s}\"")).ok())
            .map(ParamValue::QualityGate)
            .ok_or_else(|| cfg(format!("expected quality_gate method string for {}", def.toml_path))),
        ParamValue::Eccentricity(_) => val.as_str()
            .and_then(|s| serde_json::from_str::<EccentricityKind>(&format!("\"{s}\"")).ok())
            .map(ParamValue::Eccentricity)
            .ok_or_else(|| cfg(format!("expected eccentricity method string for {}", def.toml_path))),
    }
}

/// Convert a ParamValue to a TOML value.
fn param_value_to_toml(value: &ParamValue) -> toml::Value {
    fn enum_str<T: serde::Serialize>(v: &T) -> toml::Value {
        // Unit-variant enums serialize to a quoted string like "\"bar\""
        // via serde_json. The serialize call is infallible for these types;
        // on the theoretical failure path we fall through to an empty
        // string rather than panic.
        let s = serde_json::to_string(v).unwrap_or_default();
        toml::Value::String(s.trim_matches('"').to_string())
    }
    match value {
        ParamValue::Bool(v) => toml::Value::Boolean(*v),
        ParamValue::U16(v) => toml::Value::Integer(*v as i64),
        ParamValue::U32(v) => toml::Value::Integer(*v as i64),
        ParamValue::I32(v) => toml::Value::Integer(*v as i64),
        ParamValue::Usize(v) => toml::Value::Integer(*v as i64),
        ParamValue::F64(v) => toml::Value::Float(*v),
        ParamValue::String(v) => toml::Value::String(v.clone()),
        ParamValue::StringVec(v) => {
            toml::Value::Array(v.iter().map(|s| toml::Value::String(s.clone())).collect())
        }
        ParamValue::Envelope(v) => enum_str(v),
        ParamValue::Carrier(v) => enum_str(v),
        ParamValue::Projection(v) => enum_str(v),
        ParamValue::Structure(v) => enum_str(v),
        ParamValue::Order(v) => enum_str(v),
        ParamValue::CycleCombine(v) => enum_str(v),
        ParamValue::PhaseSmoothing(v) => enum_str(v),
        ParamValue::VfsComputation(v) => enum_str(v),
        ParamValue::SignMapSmoothing(v) => enum_str(v),
        ParamValue::CortexSource(v) => enum_str(v),
        ParamValue::PatchThreshold(v) => enum_str(v),
        ParamValue::PatchExtraction(v) => enum_str(v),
        ParamValue::PatchRefinement(v) => enum_str(v),
        ParamValue::QualityGate(v) => enum_str(v),
        ParamValue::Eccentricity(v) => enum_str(v),
    }
}
