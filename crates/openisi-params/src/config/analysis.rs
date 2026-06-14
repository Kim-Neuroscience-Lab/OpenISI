//! Typed **Analysis** configuration — the serde + schemars + garde replacement
//! for the macro registry's `PersistTarget::Analysis` parameters (Phase 3).
//!
//! This is where the old `active_when` predicates **collapse into the type
//! system**: each pipeline stage is an internally-tagged enum
//! (`#[serde(tag = "method")]`), so a tunable *cannot exist* unless its method
//! variant is selected — stronger than a runtime "is this control visible"
//! check. The wire form is `{"method": "snlc_amp_weighted_phasor", "sigma_px":
//! 1.0}`; `rename_all = "snake_case"` reproduces the exact method strings the
//! registry used.
//!
//! These tagged enums are intended as the **canonical** method+tunable types,
//! shared with `isi-analysis` directly (the bridge is deleted when consumers
//! migrate) — config tunables and compute tunables are the same parameters.
//!
//! Behavior note vs the old registry: only the **active** variant's tunables are
//! stored (the config = exactly what produced the result, which is what the
//! `.oisi` provenance needs). Per-method "remembered" tunables, if wanted, are a
//! frontend-state concern, not persisted analysis config.
//!
//! `deny_unknown_fields` is on the outer `AnalysisConfig` (catches unknown stage
//! keys); serde's internally-tagged enums don't support it on the variants.

use garde::Validate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Per-stage analysis method + tunable choices → `analysis.json`. Mirrors the
/// pipeline stage order; defaults match `definitions.rs`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct AnalysisConfig {
    #[garde(dive)]
    pub baseline: Baseline,
    #[garde(dive)]
    pub cycle_average: CycleAverage,
    #[garde(dive)]
    pub cycle_combine: CycleCombine,
    #[garde(dive)]
    pub phase_smoothing: PhaseSmoothing,
    #[garde(dive)]
    pub vfs_computation: VfsComputation,
    #[garde(dive)]
    pub sign_map_smoothing: SignMapSmoothing,
    #[garde(dive)]
    pub cortex_source: CortexSource,
    #[garde(dive)]
    pub patch_threshold: PatchThreshold,
    #[garde(dive)]
    pub patch_extraction: PatchExtraction,
    #[garde(dive)]
    pub patch_refinement: PatchRefinement,
    #[garde(dive)]
    pub eccentricity: Eccentricity,
}

// ── Stage 0: ΔF/F baseline (no tunables) ────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum Baseline {
    AllenAllFrameMean,
    AllenAllFrameMedian,
    #[default]
    OpenIsiInterSweepMean,
    OpenIsiInterSweepMedian,
}

// ── Projection: cycle averaging (no tunables) ───────────────────────────────
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum CycleAverage {
    #[default]
    SimpleComplexAverage,
    PhaseLockedAverage,
}

// ── Stage 1: cycle combine (no tunables) ────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum CycleCombine {
    #[default]
    KalatskyStryker2003DelaySubtraction,
    UnweightedCycleAverage,
}

// ── Stage 2: phase/position phasor smoothing ────────────────────────────────
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum PhaseSmoothing {
    SnlcAmpWeightedPhasor {
        #[garde(range(min = 0.0, max = 50.0))]
        #[schemars(range(min = 0.0, max = 50.0))]
        sigma_px: f64,
    },
    AllenZhuang2017PositionGaussian {
        #[garde(range(min = 0.0, max = 50.0))]
        #[schemars(range(min = 0.0, max = 50.0))]
        sigma_px: f64,
    },
}
impl Default for PhaseSmoothing {
    fn default() -> Self {
        Self::SnlcAmpWeightedPhasor { sigma_px: 1.0 }
    }
}

// ── Stage 3: VFS computation (no tunables) ──────────────────────────────────
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum VfsComputation {
    #[default]
    OpenIsiChainRulePhasorGradient,
}

// ── Stage 4: sign map smoothing ─────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum SignMapSmoothing {
    Gaussian {
        #[garde(range(min = 0.0, max = 500.0))]
        #[schemars(range(min = 0.0, max = 500.0))]
        sigma_um: f64,
    },
}
impl Default for SignMapSmoothing {
    fn default() -> Self {
        Self::Gaussian { sigma_um: 60.0 }
    }
}

// ── Stage 5: cortex / ROI source ────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum CortexSource {
    Reliability {
        #[garde(range(min = 0.0, max = 1.0))]
        #[schemars(range(min = 0.0, max = 1.0))]
        threshold: f64,
    },
    UserPolygon,
    SnlcGarrett2014ImBound {
        #[garde(range(min = 0.0, max = 10.0))]
        #[schemars(range(min = 0.0, max = 10.0))]
        k: f64,
        #[garde(range(min = 0, max = 50))]
        #[schemars(range(min = 0, max = 50))]
        close: i32,
        #[garde(range(min = 0, max = 50))]
        #[schemars(range(min = 0, max = 50))]
        dilate: i32,
    },
    NoRestriction,
}
impl Default for CortexSource {
    fn default() -> Self {
        Self::SnlcGarrett2014ImBound { k: 1.5, close: 10, dilate: 3 }
    }
}

// ── Stage 6: patch threshold ────────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum PatchThreshold {
    AllenZhuang2017FixedSignMapThr {
        #[garde(range(min = 0.0, max = 1.0))]
        #[schemars(range(min = 0.0, max = 1.0))]
        value: f64,
    },
    Garrett2014SigmaScaled {
        #[garde(range(min = 0.0, max = 10.0))]
        #[schemars(range(min = 0.0, max = 10.0))]
        k: f64,
    },
}
impl Default for PatchThreshold {
    fn default() -> Self {
        Self::Garrett2014SigmaScaled { k: 1.5 }
    }
}

// ── Stage 7: patch extraction ───────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum PatchExtraction {
    AllenZhuang2017LabelOpenCloseDilate {
        #[garde(range(min = 0, max = 50))]
        #[schemars(range(min = 0, max = 50))]
        open_iter: i32,
        #[garde(range(min = 0, max = 50))]
        #[schemars(range(min = 0, max = 50))]
        close_iter: i32,
        #[garde(range(min = 0, max = 50))]
        #[schemars(range(min = 0, max = 50))]
        dilation_iter: i32,
        #[garde(range(min = 1, max = 20))]
        #[schemars(range(min = 1, max = 20))]
        border_width: i32,
        #[garde(range(min = 0, max = 10_000))]
        #[schemars(range(min = 0, max = 10_000))]
        small_patch_thr: usize,
    },
}
impl Default for PatchExtraction {
    fn default() -> Self {
        Self::AllenZhuang2017LabelOpenCloseDilate {
            open_iter: 3,
            close_iter: 3,
            dilation_iter: 15,
            border_width: 1,
            small_patch_thr: 50,
        }
    }
}

// ── Stage 8: patch refinement ───────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum PatchRefinement {
    None,
    AllenZhuang2017SplitMerge {
        #[garde(range(min = 0.0, max = 10.0))]
        #[schemars(range(min = 0.0, max = 10.0))]
        split_overlap_thr: f64,
        #[garde(range(min = 0.0, max = 50.0))]
        #[schemars(range(min = 0.0, max = 50.0))]
        split_local_min_cut_step: f64,
        #[garde(range(min = 0.0, max = 1.0))]
        #[schemars(range(min = 0.0, max = 1.0))]
        merge_overlap_thr: f64,
        #[garde(range(min = 0.001, max = 10.0))]
        #[schemars(range(min = 0.001, max = 10.0))]
        visual_space_pixel_size: f64,
        #[garde(range(min = 0, max = 50))]
        #[schemars(range(min = 0, max = 50))]
        visual_space_close_iter: i32,
        #[garde(range(min = 0, max = 50))]
        #[schemars(range(min = 0, max = 50))]
        ecc_map_filter_sigma: i32,
        #[garde(range(min = 1, max = 20))]
        #[schemars(range(min = 1, max = 20))]
        border_width: i32,
        #[garde(range(min = 0, max = 10_000))]
        #[schemars(range(min = 0, max = 10_000))]
        small_patch_thr: usize,
    },
}
impl Default for PatchRefinement {
    fn default() -> Self {
        Self::AllenZhuang2017SplitMerge {
            split_overlap_thr: 1.1,
            split_local_min_cut_step: 5.0,
            merge_overlap_thr: 0.01,
            visual_space_pixel_size: 0.5,
            visual_space_close_iter: 15,
            ecc_map_filter_sigma: 10,
            border_width: 1,
            small_patch_thr: 100,
        }
    }
}

// ── Stage 10: eccentricity (no tunables) ────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum Eccentricity {
    #[default]
    OpenIsiWholeCortexV1,
    SnlcGetAreaBordersV1Center,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_validates() {
        AnalysisConfig::default().validate().expect("default must satisfy garde bounds");
    }

    #[test]
    fn json_round_trip_is_identity() {
        let cfg = AnalysisConfig::default();
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let back: AnalysisConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);
    }

    /// The method strings on the wire match the registry's snake_case names.
    #[test]
    fn tagged_wire_format_uses_registry_method_strings() {
        let json = serde_json::to_value(PhaseSmoothing::default()).unwrap();
        assert_eq!(json["method"], "snlc_amp_weighted_phasor");
        assert_eq!(json["sigma_px"], 1.0);
        let cs = serde_json::to_value(CortexSource::default()).unwrap();
        assert_eq!(cs["method"], "snlc_garrett2014_im_bound");
        assert_eq!(cs["close"], 10);
    }

    /// active_when is now a TYPE guarantee: a tunable only exists in its variant.
    /// Selecting a variant carries exactly that variant's tunables — no others.
    #[test]
    fn tunables_only_exist_in_their_variant() {
        let cfg: AnalysisConfig =
            serde_json::from_str(r#"{ "cortex_source": { "method": "reliability", "threshold": 0.85 } }"#)
                .unwrap();
        assert_eq!(cfg.cortex_source, CortexSource::Reliability { threshold: 0.85 });
        // A different stage inherits its default (sparse overlay).
        assert_eq!(cfg.eccentricity, Eccentricity::OpenIsiWholeCortexV1);
    }

    #[test]
    fn out_of_bound_fails_validation() {
        let cfg = AnalysisConfig {
            phase_smoothing: PhaseSmoothing::SnlcAmpWeightedPhasor { sigma_px: 999.0 },
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn unknown_stage_key_is_rejected() {
        let r: Result<AnalysisConfig, _> =
            serde_json::from_str(r#"{ "phase_smooting": { "method": "gaussian" } }"#);
        assert!(r.is_err());
    }

    /// The derived schema for a tagged enum is a oneOf over the variants.
    #[test]
    fn schema_is_oneof_over_variants() {
        let schema = serde_json::to_value(schemars::schema_for!(CortexSource)).unwrap();
        let s = schema.to_string();
        assert!(s.contains("oneOf"), "internally-tagged enum should derive a oneOf schema");
        assert!(s.contains("no_restriction"));
    }
}
