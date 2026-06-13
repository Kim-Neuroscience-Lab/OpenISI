//! Analysis parameters.
//!
//! Two distinct types live here:
//!
//! - `AnalysisParams` — algorithmic choices (per-stage method enums).
//!   Serialized into `.oisi /analysis_params` so every analyzed file
//!   records exactly which methods (and their parameters) produced its
//!   data.
//!
//! - `AcquisitionProperties` — stimulus geometry + camera calibration
//!   facts about how the data was captured. NOT algorithm choices.
//!   Read from `.oisi`'s `/rig_params` (camera calibration) and
//!   `/experiment_params` (stimulus geometry) JSON attributes at
//!   analyze time. A capture-time fact, not a knob.

use serde::{Deserialize, Serialize};

use crate::methods::{
    BaselineMethod, CortexSourceMethod, CycleAverageMethod, CycleCombineMethod, EccentricityMethod,
    PatchExtractionMethod, PatchRefinementMethod, PatchThresholdMethod, PhaseSmoothingMethod,
    SignMapSmoothingMethod, VfsComputationMethod,
};

// ---------------------------------------------------------------------------
// AcquisitionProperties — capture-time facts
// ---------------------------------------------------------------------------

/// Provenance state of an `AcquisitionProperties` constructed from a
/// `.oisi` file's attributes. The type itself forces every consumer to
/// decide what to do when the .oisi lacks full provenance — there's no
/// silent fallback: the user's "no fallbacks / provenance always"
/// principle is enforced by the compiler, not by a stderr warning that
/// might or might not fire at a particular caller.
///
/// - `Full`: every acquisition property came from `.oisi /rig_params` +
///   `/experiment_params`. Safe for any analysis or display.
/// - `Partial { missing }`: at least one attribute group was present
///   but some specific fields were absent or non-numeric and were
///   defaulted. The listed field names tell the caller which.
/// - `Defaulted`: both `/rig_params` and `/experiment_params` are
///   absent entirely (a pre-2026-05-23 .oisi or a hand-written test
///   file). All fields are pristine defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "level", rename_all = "snake_case")]
pub enum ProvenanceLevel {
    Full,
    Partial { missing: Vec<String> },
    Defaulted,
}

impl ProvenanceLevel {
    /// One-line summary suitable for stderr / UI badges. Returns `None`
    /// for `Full` (no warning needed); a structured string otherwise.
    /// The single source of truth for provenance-warning text — every
    /// caller routes through this to stay consistent.
    pub fn warning_summary(&self) -> Option<String> {
        match self {
            Self::Full => None,
            Self::Defaulted => Some(
                "acquisition properties: no /rig_params or /experiment_params \
                 in the .oisi file — analysis ran with pristine defaults \
                 (typical for pre-2026-05-23 captures or hand-built files)"
                    .into(),
            ),
            Self::Partial { missing } => Some(format!(
                "acquisition properties: {} field(s) defaulted because they were \
                 absent in the .oisi attrs: [{}]",
                missing.len(),
                missing.join(", "),
            )),
        }
    }
}

/// Capture-time facts about the acquisition: stimulus geometry +
/// camera calibration. These are NOT algorithm choices — they describe
/// how the data was acquired and are recorded with each `.oisi` at
/// capture time (`/rig_params` + `/experiment_params` JSON attrs).
///
/// `AcquisitionProperties` is constructed by the analysis orchestrator
/// from those attrs at the start of every run; `compute_retinotopy`
/// and `compute_analysis` receive it as a separate input from
/// `AnalysisParams`. This separation enforces the invariant that
/// re-running analysis on a file does not silently change which
/// stimulus/camera the file was captured against.
///
/// The `provenance` field records whether every field came from the
/// file's attrs (`Full`), some were defaulted (`Partial`), or all were
/// defaulted (`Defaulted`). Callers MUST `match` on it to decide
/// whether to proceed silently, warn, or refuse — the type forbids
/// silent fallbacks at every consumer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AcquisitionProperties {
    /// Total azimuth sweep extent (degrees).
    pub azi_angular_range: f64,
    /// Total altitude sweep extent (degrees).
    pub alt_angular_range: f64,
    /// Azimuth visual-field offset (degrees) — center of the swept range.
    pub offset_azi: f64,
    /// Altitude visual-field offset (degrees).
    pub offset_alt: f64,
    /// Number of 90° CCW rotations of position maps before analysis.
    /// Absorbs fixed rig-camera orientation offsets.
    pub rotation_k: i32,
    /// Spatial calibration of the cortex camera: micrometers per pixel.
    /// Used to convert physical-unit smoothing sigmas (e.g.
    /// `SignMapSmoothingMethod::Gaussian { sigma_um }`) into pixel
    /// counts at runtime. Per-rig hardware property.
    pub um_per_pixel: f64,
    /// Provenance state — set by `from_oisi_attrs`; consumers MUST
    /// match on this. See [`ProvenanceLevel`].
    #[serde(default = "ProvenanceLevel::defaulted_for_serde")]
    pub provenance: ProvenanceLevel,
}

impl ProvenanceLevel {
    // Serde default helper — values constructed via deserialize (e.g.
    // for test fixtures) get `Defaulted` unless explicitly set.
    fn defaulted_for_serde() -> Self {
        Self::Defaulted
    }
}

impl Default for AcquisitionProperties {
    fn default() -> Self {
        Self {
            azi_angular_range: 100.0,
            alt_angular_range: 100.0,
            offset_azi: 0.0,
            offset_alt: 0.0,
            rotation_k: 0,
            um_per_pixel: 20.0,
            provenance: ProvenanceLevel::Defaulted,
        }
    }
}

impl AcquisitionProperties {
    /// Build from the `/rig_params` + `/experiment_params` JSON
    /// attributes captured in a `.oisi` file.
    ///
    /// **No silent fallbacks.** Every defaulted field is tracked in
    /// `provenance`; consumers MUST match on it to decide what to do.
    /// If both attrs are absent the result is `ProvenanceLevel::Defaulted`;
    /// if any individual field is absent the result is
    /// `ProvenanceLevel::Partial { missing: [..] }`; only when every
    /// field came from the file's attrs is it `ProvenanceLevel::Full`.
    pub fn from_oisi_attrs(
        rig: Option<&serde_json::Value>,
        experiment: Option<&serde_json::Value>,
    ) -> Self {
        // Per-field tracking: each (value, was_present) pair.
        // Param paths come from `definitions.rs`:
        //   stimulus_geometry.azi_angular_range  (Experiment)
        //   stimulus_geometry.alt_angular_range  (Experiment)
        //   stimulus_geometry.offset_azi         (Experiment)
        //   stimulus_geometry.offset_alt         (Experiment)
        //   stimulus_geometry.rotation_k         (Experiment)
        //   camera.um_per_pixel                  (Rig)
        let d = Self::default();
        let mut missing: Vec<String> = Vec::new();
        let f64_at = |root: Option<&serde_json::Value>,
                      path: &[&str],
                      default: f64,
                      name: &str,
                      missing: &mut Vec<String>| {
            match root
                .and_then(|v| navigate(v, path))
                .and_then(|v| v.as_f64())
            {
                Some(v) => v,
                None => {
                    missing.push(name.to_string());
                    default
                }
            }
        };
        let i32_at = |root: Option<&serde_json::Value>,
                      path: &[&str],
                      default: i32,
                      name: &str,
                      missing: &mut Vec<String>| {
            match root
                .and_then(|v| navigate(v, path))
                .and_then(|v| v.as_i64())
                .map(|n| n as i32)
            {
                Some(v) => v,
                None => {
                    missing.push(name.to_string());
                    default
                }
            }
        };

        let azi_angular_range = f64_at(
            experiment,
            &["stimulus_geometry", "azi_angular_range"],
            d.azi_angular_range,
            "azi_angular_range",
            &mut missing,
        );
        let alt_angular_range = f64_at(
            experiment,
            &["stimulus_geometry", "alt_angular_range"],
            d.alt_angular_range,
            "alt_angular_range",
            &mut missing,
        );
        let offset_azi = f64_at(
            experiment,
            &["stimulus_geometry", "offset_azi"],
            d.offset_azi,
            "offset_azi",
            &mut missing,
        );
        let offset_alt = f64_at(
            experiment,
            &["stimulus_geometry", "offset_alt"],
            d.offset_alt,
            "offset_alt",
            &mut missing,
        );
        let rotation_k = i32_at(
            experiment,
            &["stimulus_geometry", "rotation_k"],
            d.rotation_k,
            "rotation_k",
            &mut missing,
        );
        let um_per_pixel = f64_at(
            rig,
            &["camera", "um_per_pixel"],
            d.um_per_pixel,
            "um_per_pixel",
            &mut missing,
        );

        // Classify provenance. If BOTH attrs are wholly absent, every
        // field went missing → Defaulted. If at least one attr was
        // present, some fields may have come from it → Partial (or
        // Full if none missing).
        let provenance = if rig.is_none() && experiment.is_none() {
            ProvenanceLevel::Defaulted
        } else if missing.is_empty() {
            ProvenanceLevel::Full
        } else {
            ProvenanceLevel::Partial { missing }
        };

        Self {
            azi_angular_range,
            alt_angular_range,
            offset_azi,
            offset_alt,
            rotation_k,
            um_per_pixel,
            provenance,
        }
    }
}

fn navigate<'a>(root: &'a serde_json::Value, path: &[&str]) -> Option<&'a serde_json::Value> {
    let mut current = root;
    for segment in path {
        current = current.get(segment)?;
    }
    Some(current)
}

// ---------------------------------------------------------------------------
// AnalysisParams — algorithm choices
// ---------------------------------------------------------------------------

/// Analysis parameters: per-stage method choices. Every analyzed
/// `.oisi` file records the exact `AnalysisParams` that produced its
/// results, so re-analysis is bit-reproducible.
///
/// Acquisition properties (stimulus geometry, camera calibration) live
/// in [`AcquisitionProperties`] and are recorded via
/// `/rig_params` + `/experiment_params` at capture time.
///
/// **Strict schema, enforced at reconstruction (not via serde on this
/// struct).** The on-disk form is the Registry-tree JSON in the `.oisi`
/// `/analysis_params` attribute, reloaded through
/// `RegistrySnapshot::from_json_tree`, which is fail-loud: every analysis
/// param must be present and known, or it returns `ParamsError::Config` —
/// corrupted or incomplete files do NOT silently load with code-default
/// values. The orchestrator catches that error and surfaces a clean
/// "schema mismatch — re-run analysis" message; the pre-2026 migration
/// path (`is_pre_2026_analysis_params`) handles known schema drift
/// distinctly, upstream of reconstruction.
///
/// **No `Default` impl, no serde derives.** `AnalysisParams` is now a
/// runtime-only struct — its on-disk form lives in the `.oisi` HDF5
/// attr as the Registry-tree JSON (produced by
/// `RegistrySnapshot::to_json_for_target(PersistTarget::Analysis)`),
/// not as serde-derived JSON of this struct. The only construction
/// path is `Self::new(...)` below, called from the bridge in
/// `bridge::analysis_params_from_snapshot`. `#[non_exhaustive]`
/// prevents struct-literal construction from outside this crate.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct AnalysisParams {
    /// Stage 0: ΔF/F baseline (`F0` denominator for the bin-1 DFT).
    pub baseline: BaselineMethod,
    /// Projection: cycle averaging (combine the K per-cycle complex maps).
    pub cycle_average: CycleAverageMethod,
    /// Stage 1: cycle combination (fwd+rev → position phasor).
    pub cycle_combine: CycleCombineMethod,
    /// Stage 2: position phasor smoothing.
    pub phase_smoothing: PhaseSmoothingMethod,
    /// Stage 3: visual field sign computation.
    pub vfs_computation: VfsComputationMethod,
    /// Stage 4: sign map smoothing.
    pub sign_map_smoothing: SignMapSmoothingMethod,
    /// Stage 5: cortex / ROI source.
    pub cortex_source: CortexSourceMethod,
    /// Stage 6: patch threshold (which pixels become patch candidates).
    pub patch_threshold: PatchThresholdMethod,
    /// Stage 7: patch extraction (label → smooth → assign signs).
    pub patch_extraction: PatchExtractionMethod,
    /// Stage 8: patch refinement (split + merge).
    pub patch_refinement: PatchRefinementMethod,
    /// Stage 10: eccentricity map computation.
    pub eccentricity: EccentricityMethod,
}

impl AnalysisParams {
    /// Construct an `AnalysisParams` from already-built method enums.
    /// The bridge in `bridge::analysis_params_from_snapshot` is the
    /// only production caller; it constructs each method enum via
    /// its registry-typed constructor, so every value in the result
    /// provably came from the canonical SSoT.
    // Justified `#[allow]`, not a parameter object: the 11 arguments ARE the
    // struct's fields (no smaller cohesive concept to extract), and each is a
    // distinct method-enum type, so a positional swap is a compile error — the
    // exact mistake the lint guards against can't occur here.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        baseline: BaselineMethod,
        cycle_average: CycleAverageMethod,
        cycle_combine: CycleCombineMethod,
        phase_smoothing: PhaseSmoothingMethod,
        vfs_computation: VfsComputationMethod,
        sign_map_smoothing: SignMapSmoothingMethod,
        cortex_source: CortexSourceMethod,
        patch_threshold: PatchThresholdMethod,
        patch_extraction: PatchExtractionMethod,
        patch_refinement: PatchRefinementMethod,
        eccentricity: EccentricityMethod,
    ) -> Self {
        Self {
            baseline,
            cycle_average,
            cycle_combine,
            phase_smoothing,
            vfs_computation,
            sign_map_smoothing,
            cortex_source,
            patch_threshold,
            patch_extraction,
            patch_refinement,
            eccentricity,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn full_rig() -> serde_json::Value {
        json!({ "camera": { "um_per_pixel": 12.5 } })
    }

    fn full_experiment() -> serde_json::Value {
        json!({
            "stimulus_geometry": {
                "azi_angular_range": 120.0,
                "alt_angular_range": 60.0,
                "offset_azi": 5.0,
                "offset_alt": -3.0,
                "rotation_k": 2,
            }
        })
    }

    #[test]
    fn from_oisi_attrs_full_provenance_when_both_attrs_complete() {
        let rig = full_rig();
        let exp = full_experiment();
        let p = AcquisitionProperties::from_oisi_attrs(Some(&rig), Some(&exp));
        assert_eq!(p.provenance, ProvenanceLevel::Full);
        assert_eq!(p.um_per_pixel, 12.5);
        assert_eq!(p.azi_angular_range, 120.0);
        assert_eq!(p.alt_angular_range, 60.0);
        assert_eq!(p.offset_azi, 5.0);
        assert_eq!(p.offset_alt, -3.0);
        assert_eq!(p.rotation_k, 2);
        // No warning text for Full.
        assert!(p.provenance.warning_summary().is_none());
    }

    #[test]
    fn from_oisi_attrs_defaulted_when_both_attrs_absent() {
        let p = AcquisitionProperties::from_oisi_attrs(None, None);
        assert_eq!(p.provenance, ProvenanceLevel::Defaulted);
        // All fields are defaults.
        let d = AcquisitionProperties::default();
        assert_eq!(p.um_per_pixel, d.um_per_pixel);
        assert_eq!(p.azi_angular_range, d.azi_angular_range);
        assert_eq!(p.rotation_k, d.rotation_k);
        // Warning summary present.
        assert!(p.provenance.warning_summary().is_some());
    }

    #[test]
    fn from_oisi_attrs_partial_when_rig_missing() {
        // Experiment present; rig absent → um_per_pixel defaulted.
        let exp = full_experiment();
        let p = AcquisitionProperties::from_oisi_attrs(None, Some(&exp));
        match &p.provenance {
            ProvenanceLevel::Partial { missing } => {
                assert!(
                    missing.iter().any(|f| f == "um_per_pixel"),
                    "expected um_per_pixel in missing, got: {missing:?}"
                );
                // Experiment-side fields should NOT be missing.
                assert!(!missing.iter().any(|f| f == "azi_angular_range"));
            }
            other => panic!("expected Partial, got {other:?}"),
        }
        // um_per_pixel is at its default; experiment fields from JSON.
        assert_eq!(
            p.um_per_pixel,
            AcquisitionProperties::default().um_per_pixel
        );
        assert_eq!(p.azi_angular_range, 120.0);
    }

    #[test]
    fn from_oisi_attrs_partial_when_experiment_missing() {
        let rig = full_rig();
        let p = AcquisitionProperties::from_oisi_attrs(Some(&rig), None);
        match &p.provenance {
            ProvenanceLevel::Partial { missing } => {
                // All 5 stimulus-geometry fields should be missing.
                for f in [
                    "azi_angular_range",
                    "alt_angular_range",
                    "offset_azi",
                    "offset_alt",
                    "rotation_k",
                ] {
                    assert!(
                        missing.iter().any(|m| m == f),
                        "expected {f} in missing, got: {missing:?}"
                    );
                }
                assert!(!missing.iter().any(|f| f == "um_per_pixel"));
            }
            other => panic!("expected Partial, got {other:?}"),
        }
        assert_eq!(p.um_per_pixel, 12.5);
        let d = AcquisitionProperties::default();
        assert_eq!(p.azi_angular_range, d.azi_angular_range);
    }

    #[test]
    fn from_oisi_attrs_partial_when_individual_field_missing() {
        // Experiment present, but offset_azi missing specifically.
        let rig = full_rig();
        let exp = json!({
            "stimulus_geometry": {
                "azi_angular_range": 120.0,
                "alt_angular_range": 60.0,
                // offset_azi intentionally absent
                "offset_alt": -3.0,
                "rotation_k": 2,
            }
        });
        let p = AcquisitionProperties::from_oisi_attrs(Some(&rig), Some(&exp));
        match &p.provenance {
            ProvenanceLevel::Partial { missing } => {
                assert_eq!(missing.len(), 1);
                assert_eq!(missing[0], "offset_azi");
            }
            other => panic!("expected Partial with offset_azi, got {other:?}"),
        }
        assert_eq!(p.offset_azi, AcquisitionProperties::default().offset_azi);
    }

    #[test]
    fn from_oisi_attrs_non_numeric_field_treated_as_missing() {
        // azi_angular_range present but a string, not a number → treat as missing.
        let rig = full_rig();
        let exp = json!({
            "stimulus_geometry": {
                "azi_angular_range": "not a number",
                "alt_angular_range": 60.0,
                "offset_azi": 5.0,
                "offset_alt": -3.0,
                "rotation_k": 2,
            }
        });
        let p = AcquisitionProperties::from_oisi_attrs(Some(&rig), Some(&exp));
        match &p.provenance {
            ProvenanceLevel::Partial { missing } => {
                assert!(missing.iter().any(|m| m == "azi_angular_range"));
            }
            other => panic!("expected Partial, got {other:?}"),
        }
    }

    #[test]
    fn from_oisi_attrs_extra_unknown_fields_ignored() {
        // Extra fields in JSON should be silently ignored (not break parsing).
        let rig = json!({
            "camera": { "um_per_pixel": 12.5, "unrelated_field": "junk" },
            "extra_top_level": 42,
        });
        let exp = full_experiment();
        let p = AcquisitionProperties::from_oisi_attrs(Some(&rig), Some(&exp));
        assert_eq!(p.provenance, ProvenanceLevel::Full);
        assert_eq!(p.um_per_pixel, 12.5);
    }

    #[test]
    fn provenance_warning_summary_format() {
        assert!(ProvenanceLevel::Full.warning_summary().is_none());
        let defaulted = ProvenanceLevel::Defaulted.warning_summary().unwrap();
        assert!(defaulted.contains("no /rig_params or /experiment_params"));
        let partial = ProvenanceLevel::Partial {
            missing: vec!["foo".into(), "bar".into()],
        }
        .warning_summary()
        .unwrap();
        assert!(partial.contains("2 field"));
        assert!(partial.contains("foo"));
        assert!(partial.contains("bar"));
    }
}
