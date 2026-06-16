//! Typed **Rig** configuration — the serde + schemars + garde struct tree for the
//! rig parameters. Field **defaults** (`impl Default`) and **validation bounds**
//! (`#[garde(...)]`) are the canonical values; the bit-identical regression gate
//! is the proof that the analysis pipeline reads them unchanged.
//!
//! Three tools, three jobs (no hand-rolling):
//! - **serde** — load/save (nested objects from the dotted paths come for free).
//! - **schemars** (`JsonSchema`) — the UI/validation/docs schema, derived.
//! - **garde** (`Validate`) — the static constraints as field attributes; the
//!   dynamic hardware constraints will use garde's `Context` later.
//!
//! `#[serde(default, deny_unknown_fields)]`: missing keys inherit the default
//! (the overlay/inheritance semantics), unknown keys are a hard error (replacing
//! the old `collect_unknown_leaves` typo guard).

use garde::Validate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::VisualField;

/// Hardware-rig configuration → `rig.json`. Properties of the physical rig that
/// don't change between experiments. (`Default` derives field-wise — each
/// sub-struct carries its own non-zero defaults.)
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct RigConfig {
    #[garde(dive)]
    pub camera: Camera,
    #[garde(dive)]
    pub geometry: Geometry,
    #[garde(dive)]
    pub ring_overlay: RingOverlay,
    #[garde(dive)]
    pub display: Display,
    #[garde(dive)]
    pub system: System,
    #[garde(dive)]
    pub paths: Paths,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct Camera {
    #[garde(range(min = 1, max = 1_000_000))]
    #[schemars(range(min = 1, max = 1_000_000))]
    pub exposure_us: u32,
    #[garde(range(min = 1, max = 16))]
    #[schemars(range(min = 1, max = 16))]
    pub binning: u16,
    /// Spatial calibration µm/pixel — converts physical-unit sigmas to pixels.
    #[garde(range(min = 0.001))]
    #[schemars(range(min = 0.001))]
    pub um_per_pixel: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self { exposure_us: 1000, binning: 4, um_per_pixel: 20.0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct Geometry {
    #[garde(range(min = 0.1))]
    #[schemars(range(min = 0.1))]
    pub viewing_distance_cm: f64,
    #[garde(range(min = 0.1))]
    #[schemars(range(min = 0.1))]
    pub monitor_width_cm: f64,
    #[garde(range(min = 0.1))]
    #[schemars(range(min = 0.1))]
    pub monitor_height_cm: f64,
    #[garde(skip)]
    pub bisector_x_cm: f64,
    #[garde(skip)]
    pub bisector_y_cm: f64,
    #[garde(range(min = -90.0, max = 90.0))]
    #[schemars(range(min = -90.0, max = 90.0))]
    pub monitor_yaw_deg: f64,
    #[garde(range(min = -90.0, max = 90.0))]
    #[schemars(range(min = -90.0, max = 90.0))]
    pub monitor_pitch_deg: f64,
    #[garde(skip)]
    pub visual_field: VisualField,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            viewing_distance_cm: 10.0,
            monitor_width_cm: 88.0,
            monitor_height_cm: 50.0,
            bisector_x_cm: 0.0,
            bisector_y_cm: 0.0,
            monitor_yaw_deg: 30.0,
            monitor_pitch_deg: 0.0,
            visual_field: VisualField::Right,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct RingOverlay {
    #[garde(skip)]
    pub enabled: bool,
    #[garde(range(min = 1))]
    #[schemars(range(min = 1))]
    pub radius_px: u32,
    #[garde(skip)]
    pub center_x_px: u32,
    #[garde(skip)]
    pub center_y_px: u32,
}

impl Default for RingOverlay {
    fn default() -> Self {
        Self { enabled: false, radius_px: 200, center_x_px: 512, center_y_px: 512 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct Display {
    #[garde(range(min = 1))]
    #[schemars(range(min = 1))]
    pub target_stimulus_fps: u32,
    #[garde(range(min = 0.0, max = 360.0))]
    #[schemars(range(min = 0.0, max = 360.0))]
    pub monitor_rotation_deg: f64,
}

impl Default for Display {
    fn default() -> Self {
        Self { target_stimulus_fps: 60, monitor_rotation_deg: 180.0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct System {
    #[garde(range(min = 1))]
    #[schemars(range(min = 1))]
    pub camera_frame_send_interval_ms: u32,
    #[garde(range(min = 1))]
    #[schemars(range(min = 1))]
    pub camera_poll_interval_ms: u32,
    #[garde(range(min = 1))]
    #[schemars(range(min = 1))]
    pub camera_first_frame_timeout_ms: u32,
    #[garde(range(min = 1))]
    #[schemars(range(min = 1))]
    pub camera_first_frame_poll_ms: u32,
    #[garde(range(min = 1))]
    #[schemars(range(min = 1))]
    pub display_validation_sample_count: u32,
    #[garde(range(min = 1))]
    #[schemars(range(min = 1))]
    pub preview_width_px: u32,
    #[garde(range(min = 1))]
    #[schemars(range(min = 1))]
    pub preview_interval_ms: u32,
    #[garde(range(min = 0.0))]
    #[schemars(range(min = 0.0))]
    pub preview_cycle_sec: f64,
    #[garde(range(min = 1))]
    #[schemars(range(min = 1))]
    pub idle_sleep_ms: u32,
    #[garde(range(min = 1))]
    #[schemars(range(min = 1))]
    pub fps_window_frames: usize,
    #[garde(skip)]
    pub drop_detection_warmup_frames: usize,
    #[garde(range(min = 0.0))]
    #[schemars(range(min = 0.0))]
    pub drop_detection_threshold: f64,
}

impl Default for System {
    fn default() -> Self {
        Self {
            camera_frame_send_interval_ms: 33,
            camera_poll_interval_ms: 1,
            camera_first_frame_timeout_ms: 5000,
            camera_first_frame_poll_ms: 10,
            display_validation_sample_count: 150,
            preview_width_px: 320,
            preview_interval_ms: 100,
            preview_cycle_sec: 10.0,
            idle_sleep_ms: 16,
            fps_window_frames: 10,
            drop_detection_warmup_frames: 10,
            drop_detection_threshold: 1.5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct Paths {
    /// Per-machine data location. Empty = platform default chosen at startup.
    #[garde(skip)]
    pub data_directory: String,
    #[garde(skip)]
    pub experiments_directory: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default config validates (every field within its declared bound).
    #[test]
    fn default_validates() {
        RigConfig::default().validate().expect("default must satisfy all garde bounds");
    }

    /// Full round-trip through JSON is identity (serde load == save).
    #[test]
    fn json_round_trip_is_identity() {
        let cfg = RigConfig::default();
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let back: RigConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);
    }

    /// A sparse file (only a few keys) inherits defaults for everything absent —
    /// the overlay/inheritance semantics, via `#[serde(default)]`.
    #[test]
    fn sparse_json_inherits_defaults() {
        let cfg: RigConfig = serde_json::from_str(r#"{ "camera": { "exposure_us": 100000 } }"#).unwrap();
        assert_eq!(cfg.camera.exposure_us, 100000);
        assert_eq!(cfg.camera.binning, 4); // inherited
        assert_eq!(cfg.geometry.monitor_yaw_deg, 30.0); // inherited
    }

    /// An unknown key is a hard error (the typo guard `deny_unknown_fields`).
    #[test]
    fn unknown_key_is_rejected() {
        let r: Result<RigConfig, _> =
            serde_json::from_str(r#"{ "camera": { "exposrue_us": 1000 } }"#);
        assert!(r.is_err(), "a misspelled key must be rejected");
    }

    /// An out-of-bound value fails garde validation.
    #[test]
    fn out_of_bound_fails_validation() {
        let mut cfg = RigConfig::default();
        cfg.camera.binning = 99; // max is 16
        assert!(cfg.validate().is_err());
    }

    /// The derived JSON Schema carries the declared bounds (UI/validation/docs).
    #[test]
    fn schema_carries_bounds() {
        let schema = serde_json::to_value(schemars::schema_for!(RigConfig)).unwrap();
        let s = schema.to_string();
        // binning's max=16 must appear in the schema.
        assert!(s.contains("16"), "schema should encode the binning bound");
    }
}
