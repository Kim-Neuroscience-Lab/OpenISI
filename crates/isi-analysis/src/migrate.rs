//! Migration of pre-2026 `/analysis_params` trees to the current **tagged
//! `AnalysisConfig`** schema (`{"<stage>": {"method": "x", <x's tunables>}}`).
//!
//! Lifted out of the headless binary so the translation is reusable: by
//! the end-to-end migrate→reconstruct test, and by a future GUI
//! migrate command (the CLI is no longer the only caller). The detection
//! counterpart is [`crate::io::is_pre_2026_analysis_params`].

use crate::AnalysisError;

/// The canonical **intermediate** default tree: every analysis stage's default
/// method plus *every* variant's default tunables, nested under variant subtrees.
/// This is the historical "registry tree" shape the migration overlays onto and
/// then collapses to the tagged schema. It is hardcoded here (rather than derived
/// from the live config) because migration is a fixed historical transform and
/// this module is by design the one place legacy schema names + their fill-in
/// defaults live. Values mirror `config/analysis.json`'s defaults exactly; the
/// `*_falls_back_to_*_default` tests pin them.
fn default_intermediate_tree() -> serde_json::Value {
    serde_json::json!({
        "baseline": { "method": "open_isi_inter_sweep_mean" },
        "cycle_average": { "method": "simple_complex_average" },
        "cycle_combine": { "method": "kalatsky_stryker2003_delay_subtraction" },
        "phase_smoothing": {
            "method": "snlc_amp_weighted_phasor",
            "snlc_amp_weighted_phasor": { "sigma_px": 1.0 },
            "allen_zhuang2017_position_gaussian": { "sigma_px": 1.0 }
        },
        "vfs_computation": { "method": "open_isi_chain_rule_phasor_gradient" },
        "sign_map_smoothing": {
            "method": "gaussian",
            "gaussian": { "sigma_um": 60.0 }
        },
        "cortex_source": {
            "method": "snlc_garrett2014_im_bound",
            "reliability": { "threshold": 0.5 },
            "snlc_garrett2014_im_bound": { "k": 1.5, "close": 10, "dilate": 3 }
        },
        "patch_threshold": {
            "method": "garrett2014_sigma_scaled",
            "garrett2014_sigma_scaled": { "k": 1.5 },
            "allen_zhuang2017_fixed_sign_map_thr": { "value": 0.35 }
        },
        "patch_extraction": {
            "method": "allen_zhuang2017_label_open_close_dilate",
            "allen_zhuang2017_label_open_close_dilate": {
                "open_iter": 3, "close_iter": 3, "dilation_iter": 15,
                "border_width": 1, "small_patch_thr": 50
            }
        },
        "patch_refinement": {
            "method": "allen_zhuang2017_split_merge",
            "allen_zhuang2017_split_merge": {
                "split_overlap_thr": 1.1, "split_local_min_cut_step": 5.0,
                "merge_overlap_thr": 0.01, "visual_space_pixel_size": 0.5,
                "visual_space_close_iter": 15, "ecc_map_filter_sigma": 10,
                "border_width": 1, "small_patch_thr": 100
            }
        },
        "eccentricity": { "method": "open_isi_whole_cortex_v1" }
    })
}

/// Translate a pre-2026 `/analysis_params` JSON tree into the current tagged
/// `AnalysisConfig` shape. Pure function: takes the old tree, returns the new
/// tree. Defaults for any tunable not present in the old tree come from
/// [`default_intermediate_tree`].
///
/// Algorithm:
/// 1. Start from the full default intermediate tree, so every variant subtree
///    is pre-populated with canonical defaults.
/// 2. For each known stage in the OLD tree: copy `old[stage]["method"]`
///    (renamed to canon), and overlay its tunables onto the active variant
///    subtree (a legacy FLAT tunable is nested; an already-nested subtree is
///    kept as-is).
/// 3. Root-level moved fields from the very-old schema (`azi_angular_range`,
///    `rotation_k`, …) are silently dropped — they now live in
///    `/experiment_params` and `/rig_params`, captured at acquisition time.
/// 4. Finally collapse to the tagged shape: the active variant's tunables are
///    flattened to the stage level and the inactive subtrees dropped, so the
///    result deserializes straight into `AnalysisConfig`.
///
/// This is the ONLY place the old schema's field names appear post-refactor.
pub fn translate_pre_2026_analysis_params(
    old: &serde_json::Value,
) -> Result<serde_json::Value, AnalysisError> {
    // Base = canonical defaults; overlay the old tree's methods + tunables.
    let mut new_tree = default_intermediate_tree();

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
        // the config default `sign_map_smoothing.gaussian.sigma_um` at
        // runtime (μm × camera_um_per_pixel). Forward-migrating the px
        // value would require knowing the capture-time `um_per_pixel`,
        // which pre-2026 files don't carry — best-effort would silently
        // produce a different effective σ. We drop and let the new
        // default apply.
        "smoothing_sigma",
        // Stimulus geometry — now in `.oisi /experiment_params`, captured
        // at acquisition time from the live config.
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
        // to the typed-config defaults already present in new_tree.
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

    // Final pass: collapse the intermediate registry shape (per-stage `method`
    // plus every variant's subtree) into the canonical **tagged
    // `AnalysisConfig`** shape — the ACTIVE variant's tunables flattened to the
    // stage level, the inactive variant subtrees dropped. The result is what the
    // current schema expects: `{"<stage>": {"method": "x", <x's tunables>}}`,
    // which deserializes straight into `AnalysisConfig`.
    if let Some(obj) = new_tree.as_object_mut() {
        for stage_val in obj.values_mut() {
            let Some(stage_obj) = stage_val.as_object_mut() else {
                continue;
            };
            let Some(method) = stage_obj
                .get("method")
                .and_then(|v| v.as_str())
                .map(String::from)
            else {
                continue;
            };
            // Pull the active variant's subtree out, drop ALL variant subtrees,
            // then flatten the active one to the stage level.
            let active = stage_obj
                .get(&method)
                .and_then(|v| v.as_object())
                .cloned();
            let subtree_keys: Vec<String> = stage_obj
                .iter()
                .filter(|(k, v)| k.as_str() != "method" && v.is_object())
                .map(|(k, _)| k.clone())
                .collect();
            for k in subtree_keys {
                stage_obj.remove(&k);
            }
            if let Some(active) = active {
                for (k, v) in active {
                    stage_obj.insert(k, v);
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
    use openisi_params::config::AnalysisConfig;
    use serde_json::json;

    /// Every migrated tree must deserialize straight into `AnalysisConfig` (the
    /// canonical current schema) — the property `is_pre_2026` keys off.
    fn assert_current_schema(tree: &serde_json::Value) {
        serde_json::from_value::<AnalysisConfig>(tree.clone())
            .expect("migrated tree must deserialize as AnalysisConfig");
    }

    #[test]
    fn translates_tagged_enum_with_tunable_to_tagged() {
        // Pre-2026 shape: per-stage tagged enum, tunable at stage level. The old
        // `open_isi_amp_weighted_phasor` name also exercises the SNLC rename.
        let old = json!({
            "phase_smoothing": {
                "method": "open_isi_amp_weighted_phasor",
                "sigma_px": 2.5
            }
        });
        let new = translate_pre_2026_analysis_params(&old).unwrap();
        // New shape: renamed variant, tunable FLAT at the stage level (tagged).
        assert_eq!(
            new["phase_smoothing"]["method"],
            json!("snlc_amp_weighted_phasor")
        );
        assert_eq!(new["phase_smoothing"]["sigma_px"], json!(2.5));
        assert_current_schema(&new);
    }

    #[test]
    fn missing_tunable_falls_back_to_param_defs_default() {
        // Old shape with method present but tunable absent (was Option::None);
        // also migrates the legacy `open_isi_amp_weighted_phasor` name.
        let old = json!({
            "phase_smoothing": { "method": "open_isi_amp_weighted_phasor" }
        });
        let new = translate_pre_2026_analysis_params(&old).unwrap();
        // The typed-config default for sigma_px is 1.0 (Allen phaseMapFilterSigma).
        assert_eq!(new["phase_smoothing"]["sigma_px"], json!(1.0));
        assert_current_schema(&new);
    }

    #[test]
    fn nested_legacy_name_flattens_and_drops_inactive_variant() {
        // A file analyzed before this session's renames carries the nested
        // multi-variant schema with a legacy variant NAME. Migration renames,
        // flattens the active variant to the stage level, and drops the inactive
        // sibling subtree — yielding the tagged schema.
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

        // phase_smoothing: renamed, tunable flat at stage level.
        assert_eq!(
            new["phase_smoothing"]["method"],
            json!("snlc_amp_weighted_phasor")
        );
        assert_eq!(new["phase_smoothing"]["sigma_px"], json!(2.0));

        // patch_threshold: active (garrett) flattened; inactive (allen) dropped.
        assert_eq!(new["patch_threshold"]["k"], json!(1.5));
        assert!(new["patch_threshold"].get("value").is_none());
        assert!(new["patch_threshold"]
            .get("allen_zhuang2017_fixed_sign_map_thr")
            .is_none());

        assert_current_schema(&new);
    }

    #[test]
    fn eccentricity_legacy_garrett_name_migrates_to_open_isi() {
        let old = json!({
            "eccentricity": { "method": "garrett2014_whole_cortex_v1" }
        });
        let new = translate_pre_2026_analysis_params(&old).unwrap();
        assert_eq!(
            new["eccentricity"]["method"],
            json!("open_isi_whole_cortex_v1")
        );
        assert_current_schema(&new);
    }

    #[test]
    fn root_level_moved_fields_are_dropped() {
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
            json!("kalatsky_stryker2003_delay_subtraction")
        );
        assert_current_schema(&new);
    }

    #[test]
    fn stage_absent_from_old_keeps_param_defs_defaults() {
        let old = json!({
            "sign_map_smoothing": { "method": "gaussian", "sigma_um": 90.0 }
        });
        let new = translate_pre_2026_analysis_params(&old).unwrap();
        assert_eq!(new["sign_map_smoothing"]["sigma_um"], json!(90.0));
        assert_eq!(
            new["patch_threshold"]["method"],
            json!("garrett2014_sigma_scaled")
        );
        assert_eq!(new["patch_threshold"]["k"], json!(1.5));
        assert_current_schema(&new);
    }

    #[test]
    fn multi_field_variant_migrates_all_tunables() {
        let old = json!({
            "patch_refinement": {
                "method": "allen_zhuang2017_split_merge",
                "split_overlap_thr": 1.5,
                "merge_overlap_thr": 0.05
            }
        });
        let new = translate_pre_2026_analysis_params(&old).unwrap();
        let stage = &new["patch_refinement"];
        assert_eq!(stage["split_overlap_thr"], json!(1.5));
        assert_eq!(stage["merge_overlap_thr"], json!(0.05));
        // Unset fields take the typed-config defaults.
        assert_eq!(stage["split_local_min_cut_step"], json!(5.0));
        assert_eq!(stage["visual_space_close_iter"], json!(15));
        assert_eq!(stage["small_patch_thr"], json!(100));
        assert_current_schema(&new);
    }
}
