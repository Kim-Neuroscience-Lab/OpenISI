//! Typed **Experiment** configuration — the serde + schemars + garde struct tree
//! for the experiment parameters.
//!
//! Same discipline as [`super::rig`]: defaults (`Default`) and bounds
//! (`#[garde(...)]`) are the canonical values; serde does I/O, schemars derives the
//! schema, garde validates. Note `geometry` here is *experiment* geometry (rendering offsets +
//! projection), distinct from the rig's `geometry`; `stimulus_geometry` is the
//! sweep extent that feeds `AcquisitionProperties` at analysis time.

use garde::Validate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Carrier, Envelope, Order, Projection, Structure};

/// Stimulus design + presentation → `experiment.json`. Persisted into the
/// `.oisi` `/experiment_params` at capture and read back during analysis.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct ExperimentConfig {
    #[garde(dive)]
    pub geometry: Geometry,
    #[garde(dive)]
    pub stimulus_geometry: StimulusGeometry,
    #[garde(dive)]
    pub stimulus: Stimulus,
    #[garde(dive)]
    pub presentation: Presentation,
    #[garde(dive)]
    pub timing: Timing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct Geometry {
    #[garde(range(min = -180.0, max = 180.0))]
    #[schemars(range(min = -180.0, max = 180.0))]
    pub horizontal_offset_deg: f64,
    #[garde(range(min = -90.0, max = 90.0))]
    #[schemars(range(min = -90.0, max = 90.0))]
    pub vertical_offset_deg: f64,
    #[garde(skip)]
    pub projection: Projection,
}

impl Default for Geometry {
    fn default() -> Self {
        Self { horizontal_offset_deg: 0.0, vertical_offset_deg: 0.0, projection: Projection::Spherical }
    }
}

/// Sweep envelope extent + camera-frame rotation — the facts that convert phase
/// maps to visual-field degrees at analysis time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct StimulusGeometry {
    #[garde(range(min = -3, max = 3))]
    #[schemars(range(min = -3, max = 3))]
    pub rotation_k: i32,
    #[garde(range(min = 0.0, max = 360.0))]
    #[schemars(range(min = 0.0, max = 360.0))]
    pub azi_angular_range: f64,
    #[garde(range(min = 0.0, max = 360.0))]
    #[schemars(range(min = 0.0, max = 360.0))]
    pub alt_angular_range: f64,
    #[garde(range(min = -180.0, max = 180.0))]
    #[schemars(range(min = -180.0, max = 180.0))]
    pub offset_azi: f64,
    #[garde(range(min = -180.0, max = 180.0))]
    #[schemars(range(min = -180.0, max = 180.0))]
    pub offset_alt: f64,
}

impl Default for StimulusGeometry {
    fn default() -> Self {
        Self {
            rotation_k: 0,
            azi_angular_range: 140.0,
            alt_angular_range: 110.0,
            offset_azi: 0.0,
            offset_alt: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct Stimulus {
    #[garde(skip)]
    pub envelope: Envelope,
    #[garde(skip)]
    pub carrier: Carrier,
    #[garde(dive)]
    pub params: StimulusParams,
}

impl Default for Stimulus {
    fn default() -> Self {
        Self { envelope: Envelope::Bar, carrier: Carrier::Checkerboard, params: StimulusParams::default() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct StimulusParams {
    #[garde(range(min = 0.0, max = 1.0))]
    #[schemars(range(min = 0.0, max = 1.0))]
    pub contrast: f64,
    #[garde(range(min = 0.0, max = 1.0))]
    #[schemars(range(min = 0.0, max = 1.0))]
    pub mean_luminance: f64,
    #[garde(range(min = 0.0, max = 1.0))]
    #[schemars(range(min = 0.0, max = 1.0))]
    pub background_luminance: f64,
    #[garde(range(min = 0.001))]
    #[schemars(range(min = 0.001))]
    pub check_size_deg: f64,
    #[garde(range(min = 0.001))]
    #[schemars(range(min = 0.001))]
    pub check_size_cm: f64,
    #[garde(range(min = 0.0))]
    #[schemars(range(min = 0.0))]
    pub strobe_frequency_hz: f64,
    #[garde(range(min = 0.001))]
    #[schemars(range(min = 0.001))]
    pub stimulus_width_deg: f64,
    #[garde(range(min = 0.001))]
    #[schemars(range(min = 0.001))]
    pub sweep_speed_deg_per_sec: f64,
    #[garde(range(min = 0.001))]
    #[schemars(range(min = 0.001))]
    pub rotation_speed_deg_per_sec: f64,
    #[garde(range(min = 0.001))]
    #[schemars(range(min = 0.001))]
    pub expansion_speed_deg_per_sec: f64,
    #[garde(range(min = -360.0, max = 360.0))]
    #[schemars(range(min = -360.0, max = 360.0))]
    pub rotation_deg: f64,
}

impl Default for StimulusParams {
    fn default() -> Self {
        Self {
            contrast: 1.0,
            mean_luminance: 0.5,
            background_luminance: 0.0,
            check_size_deg: 25.0,
            check_size_cm: 1.0,
            strobe_frequency_hz: 6.0,
            stimulus_width_deg: 20.0,
            sweep_speed_deg_per_sec: 9.0,
            rotation_speed_deg_per_sec: 15.0,
            expansion_speed_deg_per_sec: 5.0,
            rotation_deg: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct Presentation {
    #[garde(skip)]
    pub conditions: Vec<String>,
    #[garde(range(min = 1))]
    #[schemars(range(min = 1))]
    pub repetitions: u32,
    #[garde(skip)]
    pub structure: Structure,
    #[garde(skip)]
    pub order: Order,
}

impl Default for Presentation {
    fn default() -> Self {
        Self {
            conditions: vec!["LR".into(), "RL".into(), "TB".into(), "BT".into()],
            repetitions: 1,
            structure: Structure::Blocked,
            order: Order::Sequential,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct Timing {
    #[garde(range(min = 0.0))]
    #[schemars(range(min = 0.0))]
    pub baseline_start_sec: f64,
    #[garde(range(min = 0.0))]
    #[schemars(range(min = 0.0))]
    pub baseline_end_sec: f64,
    #[garde(range(min = 0.0))]
    #[schemars(range(min = 0.0))]
    pub inter_stimulus_sec: f64,
    #[garde(range(min = 0.0))]
    #[schemars(range(min = 0.0))]
    pub inter_direction_sec: f64,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            baseline_start_sec: 5.0,
            baseline_end_sec: 5.0,
            inter_stimulus_sec: 0.0,
            inter_direction_sec: 5.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_validates() {
        ExperimentConfig::default().validate().expect("default must satisfy garde bounds");
    }

    #[test]
    fn json_round_trip_is_identity() {
        let cfg = ExperimentConfig::default();
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let back: ExperimentConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn sparse_json_inherits_defaults() {
        let cfg: ExperimentConfig =
            serde_json::from_str(r#"{ "presentation": { "repetitions": 3 } }"#).unwrap();
        assert_eq!(cfg.presentation.repetitions, 3);
        assert_eq!(cfg.stimulus.params.sweep_speed_deg_per_sec, 9.0); // inherited
        assert_eq!(cfg.timing.baseline_start_sec, 5.0); // inherited
    }

    #[test]
    fn unknown_key_is_rejected() {
        let r: Result<ExperimentConfig, _> =
            serde_json::from_str(r#"{ "timing": { "baseline_strat_sec": 5.0 } }"#);
        assert!(r.is_err());
    }

    #[test]
    fn out_of_bound_fails_validation() {
        let mut cfg = ExperimentConfig::default();
        cfg.stimulus.params.contrast = 2.0; // max is 1.0
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn nested_enum_deserializes_snake_case() {
        let cfg: ExperimentConfig =
            serde_json::from_str(r#"{ "geometry": { "projection": "cartesian" } }"#).unwrap();
        assert_eq!(cfg.geometry.projection, Projection::Cartesian);
    }
}
