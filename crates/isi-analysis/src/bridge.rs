//! `AnalysisConfig` → `AnalysisParams` bridge.
//!
//! After UNIFY the typed config's internally-tagged per-stage enums (in
//! `openisi_params::config::analysis`) **are** the method types this crate's
//! pipeline consumes (re-exported as `methods::*Method`). So the conversions here
//! are field-wise clones, kept as named adapters so call sites read intent:
//!
//! - [`From<&AnalysisConfig> for AnalysisParams`] — the live/load path
//!   (a `ConfigStore`'s `AnalysisConfig` → pipeline params).
//! - [`From<&AnalysisParams> for AnalysisConfig`] — the provenance path
//!   (pipeline params → the `.oisi` `/analysis_params` serde form).
//! - [`analysis_params_from_oisi_tree`] — reconstruct params from a stored
//!   `/analysis_params` tree (fail-loud on a legacy schema).

use crate::params::AnalysisParams;

/// Build `AnalysisParams` directly from the typed [`AnalysisConfig`].
///
/// The config's internally-tagged enums ARE the per-stage method types, so this
/// is a field-wise clone — kept as a named conversion so call sites read intent.
impl From<&openisi_params::config::AnalysisConfig> for AnalysisParams {
    fn from(c: &openisi_params::config::AnalysisConfig) -> Self {
        AnalysisParams::new(
            c.baseline.clone(),
            c.cycle_average.clone(),
            c.cycle_combine.clone(),
            c.phase_smoothing.clone(),
            c.vfs_computation.clone(),
            c.sign_map_smoothing.clone(),
            c.cortex_source.clone(),
            c.patch_threshold.clone(),
            c.patch_extraction.clone(),
            c.patch_refinement.clone(),
            c.eccentricity.clone(),
        )
    }
}

/// Build an [`AnalysisConfig`](openisi_params::config::AnalysisConfig) from an
/// `AnalysisParams` — a field-wise clone, since after UNIFY the params' per-stage
/// fields ARE the config's tagged enums. This is the canonical `.oisi`
/// `/analysis_params` provenance form (serialized via serde).
impl From<&AnalysisParams> for openisi_params::config::AnalysisConfig {
    fn from(p: &AnalysisParams) -> Self {
        openisi_params::config::AnalysisConfig {
            baseline: p.baseline.clone(),
            cycle_average: p.cycle_average.clone(),
            cycle_combine: p.cycle_combine.clone(),
            phase_smoothing: p.phase_smoothing.clone(),
            vfs_computation: p.vfs_computation.clone(),
            sign_map_smoothing: p.sign_map_smoothing.clone(),
            cortex_source: p.cortex_source.clone(),
            patch_threshold: p.patch_threshold.clone(),
            patch_extraction: p.patch_extraction.clone(),
            patch_refinement: p.patch_refinement.clone(),
            eccentricity: p.eccentricity.clone(),
        }
    }
}

/// Reconstruct `AnalysisParams` from a `.oisi` `/analysis_params` tree in the
/// current (tagged `AnalysisConfig`) schema. Registry-free — used for
/// re-analysis of an already-analyzed file. A tree in the legacy registry/flat
/// schema fails to deserialize here; callers gate on
/// [`crate::io::is_pre_2026_analysis_params`] and migrate first.
pub fn analysis_params_from_oisi_tree(
    tree: &serde_json::Value,
) -> Result<AnalysisParams, crate::AnalysisError> {
    let config: openisi_params::config::AnalysisConfig =
        serde_json::from_value(tree.clone()).map_err(|e| {
            crate::AnalysisError::Validation(format!(
                "/analysis_params does not match the current schema: {e}"
            ))
        })?;
    Ok(AnalysisParams::from(&config))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default typed config bridges to a fully-populated `AnalysisParams`.
    /// Field-wise clone, so this is mostly a smoke test that the unified enums
    /// line up; the fingerprint of the default params is the stable reference the
    /// `regression_oisi` gate ultimately backstops.
    #[test]
    fn default_config_bridges() {
        let _ap = AnalysisParams::from(&openisi_params::config::AnalysisConfig::default());
    }

    /// The drift guard for the shipped `config/analysis.json`. The shipped config
    /// is this lab's deliberately-tuned working state; what must hold is that it
    /// still loads and bridges to a valid `AnalysisParams` end-to-end. A
    /// method/param rename that left a stale key in the shipped config would fail
    /// here (fail-loud load / unknown-key), instead of silently degrading a real
    /// analysis run. This is the production load path.
    #[test]
    fn shipped_analysis_json_loads_and_bridges() {
        let config = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config");
        let cfg: openisi_params::config::AnalysisConfig =
            openisi_params::config::load_target_from_dir(&config, None, "analysis.json")
                .expect("shipped config/analysis.json must load cleanly (no stale/unknown keys)");
        let _ap = AnalysisParams::from(&cfg);
    }

    /// A round-trip through the provenance form: params → `AnalysisConfig` (serde
    /// form) → back to params must be identity (both adapters are field clones).
    #[test]
    fn provenance_round_trip_is_identity() {
        let params = AnalysisParams::from(&openisi_params::config::AnalysisConfig::default());
        let cfg = openisi_params::config::AnalysisConfig::from(&params);
        let back = AnalysisParams::from(&cfg);
        let acq = crate::AcquisitionProperties::default();
        assert_eq!(
            crate::pipeline::fingerprint::compute(&params, &acq, "rec", None),
            crate::pipeline::fingerprint::compute(&back, &acq, "rec", None),
        );
    }
}
