//! Config persistence — load a typed config from the shipped baseline plus an
//! optional sparse overlay (the dev or user layer), and serialize one back.
//!
//! Tools, not hand-rolling: **serde** does (de)serialization, **`json-patch`**
//! does the RFC 7386 merge of the overlay onto the baseline, **garde** validates
//! the result. Generic over the config type, so `RigConfig`/`ExperimentConfig`/
//! `AnalysisConfig` share one path. (Replaces `toml_io`'s hand-walked tree
//! merge + `collect_unknown_leaves` typo guard — `deny_unknown_fields` on the
//! structs catches typos now.)

use std::path::Path;

use garde::Validate;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::{ParamsError, ParamsResult};

/// Load a typed config from files: `<shipped_dir>/<filename>` (the baseline,
/// required) merged with `<user_dir>/<filename>` (a sparse overlay, optional).
/// Thin file wrapper over [`load_merged`] — the two-layer shipped+user load path,
/// typed.
pub fn load_target_from_dir<T>(
    shipped_dir: &Path,
    user_dir: Option<&Path>,
    filename: &str,
) -> ParamsResult<T>
where
    T: DeserializeOwned + Validate,
    T::Context: Default,
{
    let shipped = std::fs::read_to_string(shipped_dir.join(filename))
        .map_err(|e| ParamsError::Config(format!("reading {filename}: {e}")))?;
    let overlay = user_dir
        .map(|d| d.join(filename))
        .filter(|p| p.exists())
        .map(|p| std::fs::read_to_string(&p))
        .transpose()
        .map_err(|e| ParamsError::Config(format!("reading user {filename}: {e}")))?;
    load_merged(&shipped, overlay.as_deref())
}

/// Load a typed config: shipped baseline + optional sparse overlay, merged
/// (RFC 7386 JSON Merge Patch), deserialized, then validated. The overlay's keys
/// win; everything absent inherits the baseline; unknown keys (typos) are a hard
/// error; out-of-bound values fail garde validation.
pub fn load_merged<T>(shipped_json: &str, overlay_json: Option<&str>) -> ParamsResult<T>
where
    T: DeserializeOwned + Validate,
    T::Context: Default,
{
    let mut base: serde_json::Value = serde_json::from_str(shipped_json)
        .map_err(|e| ParamsError::Config(format!("parsing shipped config: {e}")))?;
    if let Some(overlay) = overlay_json {
        let patch: serde_json::Value = serde_json::from_str(overlay)
            .map_err(|e| ParamsError::Config(format!("parsing overlay config: {e}")))?;
        json_patch::merge(&mut base, &patch);
    }
    let cfg: T = serde_json::from_value(base)
        .map_err(|e| ParamsError::Config(format!("deserializing config: {e}")))?;
    cfg.validate_with(&Default::default())
        .map_err(|e| ParamsError::Config(format!("config validation failed: {e}")))?;
    Ok(cfg)
}

/// Serialize a typed config to pretty JSON for persistence (the user layer is
/// written as a full document; the shipped/dev layers are hand-maintained and
/// may be sparse — `load_merged` merges them).
pub fn to_json<T: Serialize>(config: &T) -> ParamsResult<String> {
    serde_json::to_string_pretty(config)
        .map_err(|e| ParamsError::Config(format!("serializing config: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{analysis::CortexSource, AnalysisConfig, RigConfig};

    #[test]
    fn load_shipped_only_is_defaults() {
        let shipped = to_json(&RigConfig::default()).unwrap();
        let cfg: RigConfig = load_merged(&shipped, None).unwrap();
        assert_eq!(cfg, RigConfig::default());
    }

    #[test]
    fn sparse_overlay_wins_over_baseline() {
        let shipped = to_json(&RigConfig::default()).unwrap();
        let overlay = r#"{ "camera": { "exposure_us": 100000 } }"#;
        let cfg: RigConfig = load_merged(&shipped, Some(overlay)).unwrap();
        assert_eq!(cfg.camera.exposure_us, 100000); // overlay wins
        assert_eq!(cfg.camera.binning, 4); // baseline inherited
        assert_eq!(cfg.geometry.monitor_yaw_deg, 30.0); // baseline inherited
    }

    #[test]
    fn overlay_can_switch_analysis_method() {
        let shipped = to_json(&AnalysisConfig::default()).unwrap();
        let overlay = r#"{ "cortex_source": { "method": "reliability", "threshold": 0.9 } }"#;
        let cfg: AnalysisConfig = load_merged(&shipped, Some(overlay)).unwrap();
        assert_eq!(cfg.cortex_source, CortexSource::Reliability { threshold: 0.9 });
    }

    #[test]
    fn overlay_unknown_key_is_rejected() {
        let shipped = to_json(&RigConfig::default()).unwrap();
        let overlay = r#"{ "camera": { "exposrue_us": 1 } }"#;
        let r: ParamsResult<RigConfig> = load_merged(&shipped, Some(overlay));
        assert!(r.is_err());
    }

    #[test]
    fn invalid_overlay_value_fails_load() {
        let shipped = to_json(&RigConfig::default()).unwrap();
        let overlay = r#"{ "camera": { "binning": 99 } }"#; // max 16
        let r: ParamsResult<RigConfig> = load_merged(&shipped, Some(overlay));
        assert!(r.is_err());
    }

    #[test]
    fn save_then_load_round_trips() {
        let cfg = AnalysisConfig::default();
        let json = to_json(&cfg).unwrap();
        let back: AnalysisConfig = load_merged(&json, None).unwrap();
        assert_eq!(cfg, back);
    }
}
