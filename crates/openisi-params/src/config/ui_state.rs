//! Typed **UI-state** configuration — the serde + schemars + garde home for the
//! macro registry's `PersistTarget::UiState` parameters (Phase 3).
//!
//! These are view-only display toggles (the retinotopy SNR-threshold overlay):
//! NOT analysis math and NOT persisted into the `.oisi`. They were the one
//! `PersistTarget` without a typed config; this gives the SSoT cut a typed home
//! for them alongside [`RigConfig`](super::RigConfig) /
//! [`ExperimentConfig`](super::ExperimentConfig) /
//! [`AnalysisConfig`](super::AnalysisConfig). Defaults/bounds mirror
//! `definitions.rs` exactly.

use garde::Validate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// View-only UI display state (retinotopy SNR overlay) → `ui_state.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct UiStateConfig {
    /// Whether the SNR threshold mask is applied in the retinotopy view.
    #[garde(skip)]
    pub snr_threshold_enabled: bool,
    /// SNR threshold value (≥ 0).
    #[garde(range(min = 0.0))]
    #[schemars(range(min = 0.0))]
    pub snr_threshold_value: f64,
    /// Prefer the spectral SNR estimate over the temporal one.
    #[garde(skip)]
    pub snr_prefer_spectral: bool,
    /// Render the SNR mask as transparency rather than a solid cutout.
    #[garde(skip)]
    pub snr_use_transparent_mask: bool,
}

impl Default for UiStateConfig {
    fn default() -> Self {
        Self {
            snr_threshold_enabled: false,
            snr_threshold_value: 2.0,
            snr_prefer_spectral: true,
            snr_use_transparent_mask: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_validates() {
        UiStateConfig::default().validate().expect("default must satisfy garde bounds");
    }

    #[test]
    fn json_round_trip_is_identity() {
        let cfg = UiStateConfig::default();
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let back: UiStateConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn unknown_key_is_rejected() {
        let r: Result<UiStateConfig, _> =
            serde_json::from_str(r#"{ "snr_threshold_enabeld": true }"#);
        assert!(r.is_err());
    }

    #[test]
    fn out_of_bound_fails_validation() {
        let cfg = UiStateConfig {
            snr_threshold_value: -1.0,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }
}
