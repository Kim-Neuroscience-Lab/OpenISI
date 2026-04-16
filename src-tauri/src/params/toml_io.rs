//! TOML serialization/deserialization for the parameter registry.
//!
//! Reads/writes rig.toml and experiment.toml in a format identical to the
//! existing config system. Parameters are addressed by their `toml_path`
//! (e.g., "camera.exposure_us" → `[camera]` section, key `exposure_us`).

use std::path::Path;

use super::{
    Carrier, Envelope, ExperimentMeta, Order, ParamId, ParamValue, PersistTarget, Projection,
    Structure, PARAM_DEFS,
};
use super::registry::Registry;

// ─── Loading ──────────────────────────────────────────────────────────────────

/// Load rig.toml into the registry, setting all Rig-target parameters.
pub fn load_rig(registry: &mut Registry, path: &Path) -> Result<(), String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    let table: toml::Value = contents
        .parse()
        .map_err(|e| format!("Failed to parse {}: {e}", path.display()))?;

    for def in PARAM_DEFS.iter() {
        if def.persist != PersistTarget::Rig {
            continue;
        }
        if let Some(val) = navigate_toml(&table, def.toml_path) {
            match toml_to_param_value(def.id, val) {
                Ok(pv) => registry.set_unchecked(def.id, pv),
                Err(e) => {
                    eprintln!("[params] warning: {}: {e}", def.toml_path);
                }
            }
        }
        // Missing keys use defaults (already set in Registry::new)
    }
    Ok(())
}

/// Load experiment.toml into the registry, setting all Experiment-target parameters.
pub fn load_experiment(registry: &mut Registry, path: &Path) -> Result<(), String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    let table: toml::Value = contents
        .parse()
        .map_err(|e| format!("Failed to parse {}: {e}", path.display()))?;

    for def in PARAM_DEFS.iter() {
        if def.persist != PersistTarget::Experiment {
            continue;
        }
        if let Some(val) = navigate_toml(&table, def.toml_path) {
            match toml_to_param_value(def.id, val) {
                Ok(pv) => registry.set_unchecked(def.id, pv),
                Err(e) => {
                    eprintln!("[params] warning: {}: {e}", def.toml_path);
                }
            }
        }
    }
    Ok(())
}

/// Load a saved .experiment.toml file, extracting metadata and experiment params.
pub fn load_experiment_file(
    registry: &mut Registry,
    path: &Path,
) -> Result<ExperimentMeta, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    let table: toml::Value = contents
        .parse()
        .map_err(|e| format!("Failed to parse {}: {e}", path.display()))?;

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

    // Load experiment params
    for def in PARAM_DEFS.iter() {
        if def.persist != PersistTarget::Experiment {
            continue;
        }
        if let Some(val) = navigate_toml(&table, def.toml_path) {
            match toml_to_param_value(def.id, val) {
                Ok(pv) => registry.set_unchecked(def.id, pv),
                Err(e) => {
                    eprintln!("[params] warning: {}: {e}", def.toml_path);
                }
            }
        }
    }

    Ok(meta)
}

// ─── Saving ───────────────────────────────────────────────────────────────────

/// Save all Rig-target parameters to a TOML file.
pub fn save_rig(registry: &Registry, path: &Path) -> Result<(), String> {
    let mut root = toml::map::Map::new();

    for def in PARAM_DEFS.iter() {
        if def.persist != PersistTarget::Rig {
            continue;
        }
        let value = registry.get(def.id);
        let toml_val = param_value_to_toml(value);
        insert_at_path(&mut root, def.toml_path, toml_val);
    }

    let toml_str = toml::to_string_pretty(&toml::Value::Table(root))
        .map_err(|e| format!("Failed to serialize rig config: {e}"))?;
    std::fs::write(path, toml_str)
        .map_err(|e| format!("Failed to write {}: {e}", path.display()))
}

/// Save all Experiment-target parameters to a TOML file.
pub fn save_experiment(registry: &Registry, path: &Path) -> Result<(), String> {
    let mut root = toml::map::Map::new();

    for def in PARAM_DEFS.iter() {
        if def.persist != PersistTarget::Experiment {
            continue;
        }
        let value = registry.get(def.id);
        let toml_val = param_value_to_toml(value);
        insert_at_path(&mut root, def.toml_path, toml_val);
    }

    let toml_str = toml::to_string_pretty(&toml::Value::Table(root))
        .map_err(|e| format!("Failed to serialize experiment: {e}"))?;
    std::fs::write(path, toml_str)
        .map_err(|e| format!("Failed to write {}: {e}", path.display()))
}

/// Save experiment params + metadata to a .experiment.toml file.
pub fn save_experiment_file(
    registry: &Registry,
    path: &Path,
    meta: &ExperimentMeta,
) -> Result<(), String> {
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
        insert_at_path(&mut root, def.toml_path, toml_val);
    }

    let toml_str = toml::to_string_pretty(&toml::Value::Table(root))
        .map_err(|e| format!("Failed to serialize experiment file: {e}"))?;
    std::fs::write(path, toml_str)
        .map_err(|e| format!("Failed to write {}: {e}", path.display()))
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

/// Insert a value into a nested TOML map at a dotted path.
fn insert_at_path(root: &mut toml::map::Map<String, toml::Value>, path: &str, value: toml::Value) {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.len() == 1 {
        root.insert(parts[0].to_string(), value);
        return;
    }

    // Navigate/create nested tables
    let mut current = root;
    for part in &parts[..parts.len() - 1] {
        current = current
            .entry(part.to_string())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
            .as_table_mut()
            .expect("TOML path conflict: expected table");
    }
    current.insert(parts.last().unwrap().to_string(), value);
}

/// Convert a TOML value to a ParamValue based on the parameter's known type.
fn toml_to_param_value(id: ParamId, val: &toml::Value) -> Result<ParamValue, String> {
    let def = &PARAM_DEFS[id as usize];
    match &def.default {
        ParamValue::Bool(_) => val
            .as_bool()
            .map(ParamValue::Bool)
            .ok_or_else(|| format!("expected bool for {}", def.toml_path)),

        ParamValue::U16(_) => val
            .as_integer()
            .map(|v| ParamValue::U16(v as u16))
            .ok_or_else(|| format!("expected integer for {}", def.toml_path)),

        ParamValue::U32(_) => val
            .as_integer()
            .map(|v| ParamValue::U32(v as u32))
            .ok_or_else(|| format!("expected integer for {}", def.toml_path)),

        ParamValue::I32(_) => val
            .as_integer()
            .map(|v| ParamValue::I32(v as i32))
            .ok_or_else(|| format!("expected integer for {}", def.toml_path)),

        ParamValue::Usize(_) => val
            .as_integer()
            .map(|v| ParamValue::Usize(v as usize))
            .ok_or_else(|| format!("expected integer for {}", def.toml_path)),

        ParamValue::F64(_) => {
            // TOML may represent "10.0" as float or "10" as integer
            if let Some(f) = val.as_float() {
                Ok(ParamValue::F64(f))
            } else if let Some(i) = val.as_integer() {
                Ok(ParamValue::F64(i as f64))
            } else {
                Err(format!("expected number for {}", def.toml_path))
            }
        }

        ParamValue::String(_) => val
            .as_str()
            .map(|s| ParamValue::String(s.to_string()))
            .ok_or_else(|| format!("expected string for {}", def.toml_path)),

        ParamValue::StringVec(_) => val
            .as_array()
            .map(|arr| {
                ParamValue::StringVec(
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect(),
                )
            })
            .ok_or_else(|| format!("expected array for {}", def.toml_path)),

        ParamValue::Envelope(_) => val
            .as_str()
            .and_then(|s| serde_json::from_str::<Envelope>(&format!("\"{s}\"")).ok())
            .map(ParamValue::Envelope)
            .ok_or_else(|| format!("expected envelope string for {}", def.toml_path)),

        ParamValue::Carrier(_) => val
            .as_str()
            .and_then(|s| serde_json::from_str::<Carrier>(&format!("\"{s}\"")).ok())
            .map(ParamValue::Carrier)
            .ok_or_else(|| format!("expected carrier string for {}", def.toml_path)),

        ParamValue::Projection(_) => val
            .as_str()
            .and_then(|s| serde_json::from_str::<Projection>(&format!("\"{s}\"")).ok())
            .map(ParamValue::Projection)
            .ok_or_else(|| format!("expected projection string for {}", def.toml_path)),

        ParamValue::Structure(_) => val
            .as_str()
            .and_then(|s| serde_json::from_str::<Structure>(&format!("\"{s}\"")).ok())
            .map(ParamValue::Structure)
            .ok_or_else(|| format!("expected structure string for {}", def.toml_path)),

        ParamValue::Order(_) => val
            .as_str()
            .and_then(|s| serde_json::from_str::<Order>(&format!("\"{s}\"")).ok())
            .map(ParamValue::Order)
            .ok_or_else(|| format!("expected order string for {}", def.toml_path)),
    }
}

/// Convert a ParamValue to a TOML value.
fn param_value_to_toml(value: &ParamValue) -> toml::Value {
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
        ParamValue::Envelope(v) => {
            // serde_json gives us "\"bar\"", strip the quotes
            let s = serde_json::to_string(v).unwrap();
            toml::Value::String(s.trim_matches('"').to_string())
        }
        ParamValue::Carrier(v) => {
            let s = serde_json::to_string(v).unwrap();
            toml::Value::String(s.trim_matches('"').to_string())
        }
        ParamValue::Projection(v) => {
            let s = serde_json::to_string(v).unwrap();
            toml::Value::String(s.trim_matches('"').to_string())
        }
        ParamValue::Structure(v) => {
            let s = serde_json::to_string(v).unwrap();
            toml::Value::String(s.trim_matches('"').to_string())
        }
        ParamValue::Order(v) => {
            let s = serde_json::to_string(v).unwrap();
            toml::Value::String(s.trim_matches('"').to_string())
        }
    }
}
