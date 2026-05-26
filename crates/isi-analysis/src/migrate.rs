//! Migration of pre-2026 `/analysis_params` trees to the current
//! registry-tree schema.
//!
//! Lifted out of the headless binary so the translation is reusable: by
//! the end-to-end migrate→reconstruct→bridge test, and by a future GUI
//! migrate command (the CLI is no longer the only caller). The detection
//! counterpart is [`crate::io::is_pre_2026_analysis_params`].

use openisi_params::{PersistTarget, Registry};

use crate::AnalysisError;

/// Translate a pre-2026 `/analysis_params` JSON tree into the current
/// registry-tree shape. Pure function: takes the old tree, returns the
/// new tree. Defaults for any tunable not present in the old tree come
/// from `PARAM_DEFS` via a fresh `Registry` snapshot.
///
/// Algorithm:
/// 1. Start from a full default registry tree (`to_json_for_target`), so
///    every variant subtree is pre-populated with canonical defaults.
/// 2. For each known stage in the OLD tree: copy `old[stage]["method"]`
///    to `new[stage]["method"]`, and move every other `(key, value)` in
///    `old[stage]` into `new[stage][<method>][key]` — the variant subtree.
/// 3. Root-level moved fields from the very-old schema (`azi_angular_range`,
///    `rotation_k`, …) are silently dropped — they now live in
///    `/experiment_params` and `/rig_params`, captured at acquisition time.
///
/// This is the ONLY place the old schema's field names appear post-refactor.
pub fn translate_pre_2026_analysis_params(
    old: &serde_json::Value,
) -> Result<serde_json::Value, AnalysisError> {
    // Base = registry defaults; overlay the old tree's methods + tunables.
    // The paths are unused — we only snapshot PARAM_DEFS defaults, no load.
    let here = std::path::Path::new(".");
    let default_registry = Registry::new(here, here);
    let mut new_tree = default_registry
        .snapshot()
        .to_json_for_target(PersistTarget::Analysis);

    let Some(old_obj) = old.as_object() else {
        return Err(AnalysisError::Validation(
            "/analysis_params is not a JSON object — cannot migrate".into(),
        ));
    };

    // Known stage names. Root-level keys that aren't stages (e.g. moved
    // fields like `azi_angular_range`) are silently dropped.
    const STAGES: &[&str] = &[
        "cycle_combine",
        "phase_smoothing",
        "vfs_computation",
        "sign_map_smoothing",
        "cortex_source",
        "patch_threshold",
        "patch_extraction",
        "patch_refinement",
        "quality_gate",
        "eccentricity",
    ];

    let new_obj = new_tree
        .as_object_mut()
        .expect("registry tree is always an object");

    for stage in STAGES {
        let Some(old_stage) = old_obj.get(*stage).and_then(|v| v.as_object()) else {
            continue; // stage absent → keep the default
        };
        let Some(method) = old_stage.get("method").and_then(|v| v.as_str()) else {
            continue; // malformed; keep the default
        };

        // Build or replace new[stage]; fields missing from old fall through
        // to the PARAM_DEFS defaults already present in new_tree.
        let stage_entry = new_obj
            .entry((*stage).to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        let Some(stage_obj) = stage_entry.as_object_mut() else { continue; };

        stage_obj.insert("method".into(), serde_json::Value::String(method.to_string()));

        let variant_entry = stage_obj
            .entry(method.to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        let Some(variant_obj) = variant_entry.as_object_mut() else { continue; };

        for (k, v) in old_stage.iter() {
            if k == "method" {
                continue;
            }
            variant_obj.insert(k.clone(), v.clone());
        }
    }

    Ok(new_tree)
}

#[cfg(test)]
mod tests {
    use super::translate_pre_2026_analysis_params;
    use serde_json::json;

    #[test]
    fn translates_tagged_enum_with_tunable_to_variant_subtree() {
        // Pre-2026 shape: per-stage tagged enum, tunable at stage level.
        let old = json!({
            "phase_smoothing": {
                "method": "open_isi_amp_weighted_phasor",
                "sigma_px": 2.5
            }
        });
        let new = translate_pre_2026_analysis_params(&old).unwrap();
        // New shape: tunable nested under variant subtree.
        assert_eq!(
            new["phase_smoothing"]["method"],
            json!("open_isi_amp_weighted_phasor")
        );
        assert_eq!(
            new["phase_smoothing"]["open_isi_amp_weighted_phasor"]["sigma_px"],
            json!(2.5)
        );
    }

    #[test]
    fn missing_tunable_falls_back_to_param_defs_default() {
        // Old shape with method present but tunable absent (was Option::None).
        let old = json!({
            "phase_smoothing": { "method": "open_isi_amp_weighted_phasor" }
        });
        let new = translate_pre_2026_analysis_params(&old).unwrap();
        // PARAM_DEFS default for sigma_px is 1.0 (Allen phaseMapFilterSigma).
        assert_eq!(
            new["phase_smoothing"]["open_isi_amp_weighted_phasor"]["sigma_px"],
            json!(1.0)
        );
    }

    #[test]
    fn root_level_moved_fields_are_dropped() {
        // Very-old shape: stimulus-geometry fields at root that have since
        // moved to /experiment_params.
        let old = json!({
            "azi_angular_range": 120.0,
            "rotation_k": 2,
            "cycle_combine": { "method": "marshel_garrett2011_delay_subtraction" }
        });
        let new = translate_pre_2026_analysis_params(&old).unwrap();
        assert!(new.get("azi_angular_range").is_none());
        assert!(new.get("rotation_k").is_none());
        assert_eq!(
            new["cycle_combine"]["method"],
            json!("marshel_garrett2011_delay_subtraction")
        );
    }

    #[test]
    fn stage_absent_from_old_keeps_param_defs_defaults() {
        // Old tree contains only one stage; others must come from PARAM_DEFS.
        let old = json!({
            "sign_map_smoothing": { "method": "gaussian", "sigma_um": 90.0 }
        });
        let new = translate_pre_2026_analysis_params(&old).unwrap();
        assert_eq!(new["sign_map_smoothing"]["gaussian"]["sigma_um"], json!(90.0));
        assert_eq!(
            new["patch_threshold"]["method"],
            json!("garrett2014_sigma_scaled")
        );
        assert_eq!(
            new["patch_threshold"]["garrett2014_sigma_scaled"]["k"],
            json!(1.5)
        );
    }

    #[test]
    fn multi_field_variant_migrates_all_tunables() {
        // patch_refinement.allen_zhuang2017_split_merge — 8 tunables.
        let old = json!({
            "patch_refinement": {
                "method": "allen_zhuang2017_split_merge",
                "split_overlap_thr": 1.5,
                "merge_overlap_thr": 0.05
            }
        });
        let new = translate_pre_2026_analysis_params(&old).unwrap();
        let subtree = &new["patch_refinement"]["allen_zhuang2017_split_merge"];
        assert_eq!(subtree["split_overlap_thr"], json!(1.5));
        assert_eq!(subtree["merge_overlap_thr"], json!(0.05));
        // Unset fields take PARAM_DEFS defaults.
        assert_eq!(subtree["split_local_min_cut_step"], json!(5.0));
        assert_eq!(subtree["visual_space_close_iter"], json!(15));
        assert_eq!(subtree["small_patch_thr"], json!(100));
    }
}
