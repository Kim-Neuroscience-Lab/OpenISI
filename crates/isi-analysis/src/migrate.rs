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

    // Known stage names. Root-level keys outside this set are either
    // intentionally dropped (LEGACY_ROOT_DROPS — moved to /rig_params or
    // /experiment_params at acquisition time, or no longer used) or are
    // truly unknown and surface as a typed error so a stray key in a
    // pre-2026 file isn't silently lost.
    const STAGES: &[&str] = &[
        "cycle_combine",
        "phase_smoothing",
        "vfs_computation",
        "sign_map_smoothing",
        "cortex_source",
        "patch_threshold",
        "patch_extraction",
        "patch_refinement",
        "eccentricity",
    ];

    // Pre-2026 root-level keys that no longer belong in /analysis_params.
    // Each carries a comment naming where (if anywhere) the value now lives.
    const LEGACY_ROOT_DROPS: &[&str] = &[
        // The pre-2026 fixed σ_px for sign-map smoothing. Re-derived from
        // the registry default `sign_map_smoothing.gaussian.sigma_um` at
        // runtime (μm × camera_um_per_pixel). Forward-migrating the px
        // value would require knowing the capture-time `um_per_pixel`,
        // which pre-2026 files don't carry — best-effort would silently
        // produce a different effective σ. We drop and let the new
        // default apply.
        "smoothing_sigma",
        // Stimulus geometry — now in `.oisi /experiment_params`, captured
        // at acquisition time from the live registry.
        "rotation_k",
        "azi_angular_range",
        "alt_angular_range",
        "offset_azi",
        "offset_alt",
        // Pre-2026 numerical floor — no current-code consumer.
        "epsilon",
        // The no-op quality-gate stage was removed (only a `None` variant,
        // never applied in the pipeline), so a pre-2026 file carrying
        // `quality_gate.*` keys has them dropped (not migrated, not errored).
        "quality_gate",
    ];

    // Fail loudly on unknown root-level keys so a stray field in a
    // pre-2026 file is investigated rather than absorbed.
    for key in old_obj.keys() {
        if STAGES.contains(&key.as_str()) {
            continue;
        }
        if LEGACY_ROOT_DROPS.contains(&key.as_str()) {
            continue;
        }
        return Err(AnalysisError::Validation(format!(
            "/analysis_params: unknown legacy root-level key {key:?} \
             (not a current-schema stage, not in the documented \
             LEGACY_ROOT_DROPS list). If this key is safe to drop, add \
             it to LEGACY_ROOT_DROPS in `crates/isi-analysis/src/migrate.rs` \
             with a comment naming where (if anywhere) the value now lives."
        )));
    }

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
        let Some(stage_obj) = stage_entry.as_object_mut() else {
            continue;
        };

        // Translate any old / renamed method names to current canon
        // before writing them in. Same map is also applied after, when
        // reading current-shape files that pre-date a rename.
        let method = rename_legacy_method(stage, method);

        stage_obj.insert(
            "method".into(),
            serde_json::Value::String(method.to_string()),
        );

        // Partition the old stage's non-`method` entries by shape:
        //   - SCALAR siblings are legacy FLAT tunables (`{method, sigma_px}`) →
        //     nest them under the active variant subtree (the flat→nested
        //     migration this function was written for).
        //   - OBJECT siblings are already variant subtrees, reached when a
        //     *current* multi-variant tree tripped `is_pre_2026` only via a
        //     legacy variant *name* (e.g. a file analyzed before the
        //     phase_smoothing→SNLC / eccentricity→OpenISI renames). They belong
        //     at the STAGE level as-is; the second pass renames any legacy-named
        //     ones. Nesting them under the active variant (the old behavior)
        //     double-nested every subtree — `patch_threshold.garrett….allen….value`
        //     — and `from_json_tree` then rejected the tree as unknown-key.
        let mut flat_tunables: Vec<(String, serde_json::Value)> = Vec::new();
        for (k, v) in old_stage.iter() {
            if k == "method" {
                continue;
            }
            if v.is_object() {
                stage_obj.insert(k.clone(), v.clone());
            } else {
                flat_tunables.push((k.clone(), v.clone()));
            }
        }

        let variant_entry = stage_obj
            .entry(method.to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        let Some(variant_obj) = variant_entry.as_object_mut() else {
            continue;
        };
        for (k, v) in flat_tunables {
            variant_obj.insert(k, v);
        }
    }

    // Second pass: walk the now-current-shape tree and rename any
    // variant-subtree keys that were renamed under the legacy → canon
    // mapping. (`rename_legacy_method` handles the `"method"` string;
    // the variant subtree under the OLD name needs its key rewritten
    // to the NEW name too, in case the old tree had nested data.)
    if let Some(obj) = new_tree.as_object_mut() {
        for (stage, stage_val) in obj.iter_mut() {
            let Some(stage_obj) = stage_val.as_object_mut() else {
                continue;
            };
            let keys: Vec<String> = stage_obj.keys().cloned().collect();
            for old_key in keys {
                if old_key == "method" {
                    continue;
                }
                let new_key = rename_legacy_method(stage, &old_key);
                if new_key != old_key {
                    if let Some(value) = stage_obj.remove(&old_key) {
                        stage_obj.insert(new_key.to_string(), value);
                    }
                }
            }
        }
    }

    Ok(new_tree)
}

/// Static map from old variant strings to their renamed canonical
/// equivalents. Returns the input unchanged if no rename applies.
///
/// Renames so far:
/// - `cycle_combine`: `marshel_garrett2011_delay_subtraction` → `kalatsky_stryker2003_delay_subtraction`
///   (the delay-subtraction technique is Kalatsky 2003's; Marshel/Garrett inherit it)
/// - `cycle_combine`: `kalatsky_stryker2003_raw_average` → `unweighted_cycle_average`
///   (Kalatsky never proposed raw averaging; that's the absence of their delay correction)
/// - `cortex_source`: `allen_zhuang2017_full_frame` → `no_restriction`
///   (Allen/Zhuang didn't introduce a "full-frame" cortex source; they simply omitted the restriction)
/// - `phase_smoothing`: `open_isi_amp_weighted_phasor` → `snlc_amp_weighted_phasor`
///   (the amplitude-weighted complex smoothing is SNLC `Gprocesskret.m`'s; our
///   smoothed phase is identical — only the magnitude normalization is OpenISI)
/// - `eccentricity`: `garrett2014_whole_cortex_v1` → `open_isi_whole_cortex_v1`
///   (our recipe pairs an Allen cos-on-altitude formula with a mean-over-pixels
///   center — neither Garrett's nor SNLC's exact selection; the faithful SNLC
///   reference point is the separate `snlc_get_area_borders_v1_center` variant)
pub(crate) fn rename_legacy_method<'a>(stage: &str, method: &'a str) -> &'a str {
    match (stage, method) {
        ("cycle_combine", "marshel_garrett2011_delay_subtraction") => {
            "kalatsky_stryker2003_delay_subtraction"
        }
        ("cycle_combine", "kalatsky_stryker2003_raw_average") => "unweighted_cycle_average",
        ("cortex_source", "allen_zhuang2017_full_frame") => "no_restriction",
        ("phase_smoothing", "open_isi_amp_weighted_phasor") => "snlc_amp_weighted_phasor",
        ("eccentricity", "garrett2014_whole_cortex_v1") => "open_isi_whole_cortex_v1",
        _ => method,
    }
}

#[cfg(test)]
mod tests {
    use super::translate_pre_2026_analysis_params;
    use serde_json::json;

    #[test]
    fn translates_tagged_enum_with_tunable_to_variant_subtree() {
        // Pre-2026 shape: per-stage tagged enum, tunable at stage level. The old
        // `open_isi_amp_weighted_phasor` name also exercises the SNLC rename:
        // both the `method` string and the subtree key migrate to the new name.
        let old = json!({
            "phase_smoothing": {
                "method": "open_isi_amp_weighted_phasor",
                "sigma_px": 2.5
            }
        });
        let new = translate_pre_2026_analysis_params(&old).unwrap();
        // New shape: renamed variant, tunable nested under the renamed subtree.
        assert_eq!(
            new["phase_smoothing"]["method"],
            json!("snlc_amp_weighted_phasor")
        );
        assert_eq!(
            new["phase_smoothing"]["snlc_amp_weighted_phasor"]["sigma_px"],
            json!(2.5)
        );
    }

    #[test]
    fn missing_tunable_falls_back_to_param_defs_default() {
        // Old shape with method present but tunable absent (was Option::None);
        // also migrates the legacy `open_isi_amp_weighted_phasor` name.
        let old = json!({
            "phase_smoothing": { "method": "open_isi_amp_weighted_phasor" }
        });
        let new = translate_pre_2026_analysis_params(&old).unwrap();
        // PARAM_DEFS default for sigma_px is 1.0 (Allen phaseMapFilterSigma).
        assert_eq!(
            new["phase_smoothing"]["snlc_amp_weighted_phasor"]["sigma_px"],
            json!(1.0)
        );
    }

    #[test]
    fn nested_current_tree_with_legacy_name_is_not_double_nested() {
        // Regression: a file analyzed before this session's renames carries the
        // CURRENT nested multi-variant schema but with a legacy variant NAME
        // (here `open_isi_amp_weighted_phasor` as both the method and its subtree
        // key), and a sibling stage with multiple variant subtrees. The detector
        // flags it (legacy name), and migration must rename in place WITHOUT
        // nesting the existing subtrees one level deeper — otherwise the reload
        // (`from_json_tree`) rejects keys like
        // `phase_smoothing.snlc….open_isi….sigma_px`.
        let old = json!({
            "phase_smoothing": {
                "method": "open_isi_amp_weighted_phasor",
                "open_isi_amp_weighted_phasor": { "sigma_px": 2.0 }
            },
            "patch_threshold": {
                "method": "garrett2014_sigma_scaled",
                "garrett2014_sigma_scaled": { "k": 1.5 },
                "allen_zhuang2017_fixed_sign_map_thr": { "value": 0.35 }
            }
        });
        let new = translate_pre_2026_analysis_params(&old).unwrap();

        // phase_smoothing: renamed, single (not double) nesting under SNLC name.
        assert_eq!(
            new["phase_smoothing"]["method"],
            json!("snlc_amp_weighted_phasor")
        );
        assert_eq!(
            new["phase_smoothing"]["snlc_amp_weighted_phasor"]["sigma_px"],
            json!(2.0)
        );
        assert!(
            new["phase_smoothing"]["snlc_amp_weighted_phasor"]
                .get("open_isi_amp_weighted_phasor")
                .is_none(),
            "must not double-nest the legacy-named subtree"
        );

        // patch_threshold (no rename): both variant subtrees preserved flat at
        // the stage level, NOT nested under the active variant.
        assert_eq!(
            new["patch_threshold"]["garrett2014_sigma_scaled"]["k"],
            json!(1.5)
        );
        assert_eq!(
            new["patch_threshold"]["allen_zhuang2017_fixed_sign_map_thr"]["value"],
            json!(0.35)
        );
        assert!(
            new["patch_threshold"]["garrett2014_sigma_scaled"]
                .get("allen_zhuang2017_fixed_sign_map_thr")
                .is_none(),
            "sibling variant subtree must stay at stage level"
        );

        // The migrated tree must reconstruct into an AnalysisParams without
        // unknown-key errors — the failure mode this bug produced.
        use openisi_params::{PersistTarget, RegistrySnapshot};
        RegistrySnapshot::from_json_tree(PersistTarget::Analysis, &new)
            .expect("migrated tree must reload cleanly");
    }

    #[test]
    fn eccentricity_legacy_garrett_name_migrates_to_open_isi() {
        // The old `garrett2014_whole_cortex_v1` name overclaimed faithfulness
        // (our recipe is an OpenISI composition). The rename map relabels it.
        let old = json!({
            "eccentricity": { "method": "garrett2014_whole_cortex_v1" }
        });
        let new = translate_pre_2026_analysis_params(&old).unwrap();
        assert_eq!(
            new["eccentricity"]["method"],
            json!("open_isi_whole_cortex_v1")
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
        // Legacy variant string is renamed to its canonical equivalent
        // (delay subtraction is Kalatsky 2003's contribution).
        assert_eq!(
            new["cycle_combine"]["method"],
            json!("kalatsky_stryker2003_delay_subtraction")
        );
    }

    #[test]
    fn stage_absent_from_old_keeps_param_defs_defaults() {
        // Old tree contains only one stage; others must come from PARAM_DEFS.
        let old = json!({
            "sign_map_smoothing": { "method": "gaussian", "sigma_um": 90.0 }
        });
        let new = translate_pre_2026_analysis_params(&old).unwrap();
        assert_eq!(
            new["sign_map_smoothing"]["gaussian"]["sigma_um"],
            json!(90.0)
        );
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
