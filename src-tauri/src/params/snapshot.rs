//! Registry snapshot — frozen parameter state for thread messages and .oisi provenance.
//!
//! A snapshot clones all values at a point in time. Thread-safe (Send + Sync via Clone).
//! Typed getters match Registry's interface so consumers can use either interchangeably.

use super::{ParamId, ParamValue, PARAM_DEFS};
use super::{Carrier, Envelope, Order, Projection, Structure};

/// Frozen snapshot of all parameter values at a point in time.
#[derive(Debug, Clone)]
pub struct RegistrySnapshot {
    pub(crate) values: Vec<ParamValue>,
}

impl RegistrySnapshot {
    /// Get a parameter value by ID.
    pub fn get(&self, id: ParamId) -> &ParamValue {
        &self.values[id as usize]
    }

    /// Get the ParamDef for a given ID.
    pub fn def(id: ParamId) -> &'static super::ParamDef {
        &PARAM_DEFS[id as usize]
    }
}

// ── Typed getters (mirror Registry's generated getters) ──────────────────────
//
// We generate these with a macro to stay in sync with the parameter definitions.
// Each getter matches the same snake_case name as Registry's getter.
macro_rules! snapshot_getter {
    ($variant:ident, Bool) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> bool {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::Bool(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    ($variant:ident, U16) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> u16 {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::U16(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    ($variant:ident, U32) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> u32 {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::U32(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    ($variant:ident, I32) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> i32 {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::I32(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    ($variant:ident, Usize) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> usize {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::Usize(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    ($variant:ident, F64) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> f64 {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::F64(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    ($variant:ident, String) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> &str {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::String(v) => v.as_str(),
                    _ => unreachable!(),
                }
            }
        }
    };
    ($variant:ident, StringVec) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> &[String] {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::StringVec(v) => v.as_slice(),
                    _ => unreachable!(),
                }
            }
        }
    };
    ($variant:ident, Envelope) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> Envelope {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::Envelope(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    ($variant:ident, Carrier) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> Carrier {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::Carrier(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    ($variant:ident, Projection) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> Projection {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::Projection(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    ($variant:ident, Structure) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> Structure {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::Structure(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
    ($variant:ident, Order) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> Order {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::Order(v) => *v,
                    _ => unreachable!(),
                }
            }
        }
    };
}

impl RegistrySnapshot {
    // Camera
    snapshot_getter!(CameraExposureUs, U32);
    snapshot_getter!(CameraBinning, U16);

    // Rig Geometry
    snapshot_getter!(ViewingDistanceCm, F64);

    // Ring Overlay
    snapshot_getter!(RingOverlayEnabled, Bool);
    snapshot_getter!(RingOverlayRadiusPx, U32);
    snapshot_getter!(RingOverlayCenterXPx, U32);
    snapshot_getter!(RingOverlayCenterYPx, U32);

    // Display
    snapshot_getter!(TargetStimulusFps, U32);
    snapshot_getter!(MonitorRotationDeg, F64);

    // Analysis
    snapshot_getter!(SmoothingSigma, F64);
    snapshot_getter!(RotationK, I32);
    snapshot_getter!(AziAngularRange, F64);
    snapshot_getter!(AltAngularRange, F64);
    snapshot_getter!(OffsetAzi, F64);
    snapshot_getter!(OffsetAlt, F64);
    snapshot_getter!(Epsilon, F64);

    // Analysis Segmentation
    snapshot_getter!(SignMapFilterSigma, F64);
    snapshot_getter!(SignMapThreshold, F64);
    snapshot_getter!(OpenRadius, Usize);
    snapshot_getter!(CloseRadius, Usize);
    snapshot_getter!(DilateRadius, Usize);
    snapshot_getter!(PadBorder, Usize);
    snapshot_getter!(SpurIterations, Usize);
    snapshot_getter!(SplitOverlapThreshold, F64);
    snapshot_getter!(MergeOverlapThreshold, F64);
    snapshot_getter!(MergeDilateRadius, Usize);
    snapshot_getter!(MergeCloseRadius, Usize);
    snapshot_getter!(EccentricityRadius, F64);

    // System Tuning
    snapshot_getter!(CameraFrameSendIntervalMs, U32);
    snapshot_getter!(CameraPollIntervalMs, U32);
    snapshot_getter!(CameraFirstFrameTimeoutMs, U32);
    snapshot_getter!(CameraFirstFramePollMs, U32);
    snapshot_getter!(DisplayValidationSampleCount, U32);
    snapshot_getter!(PreviewWidthPx, U32);
    snapshot_getter!(PreviewIntervalMs, U32);
    snapshot_getter!(PreviewCycleSec, F64);
    snapshot_getter!(IdleSleepMs, U32);
    snapshot_getter!(FpsWindowFrames, Usize);
    snapshot_getter!(DropDetectionWarmupFrames, Usize);
    snapshot_getter!(DropDetectionThreshold, F64);

    // Paths
    snapshot_getter!(DataDirectory, String);
    snapshot_getter!(ExperimentsDirectory, String);

    // Experiment Geometry
    snapshot_getter!(HorizontalOffsetDeg, F64);
    snapshot_getter!(VerticalOffsetDeg, F64);
    snapshot_getter!(ExperimentProjection, Projection);

    // Stimulus
    snapshot_getter!(StimulusEnvelope, Envelope);
    snapshot_getter!(StimulusCarrier, Carrier);

    // Stimulus Params
    snapshot_getter!(Contrast, F64);
    snapshot_getter!(MeanLuminance, F64);
    snapshot_getter!(BackgroundLuminance, F64);
    snapshot_getter!(CheckSizeDeg, F64);
    snapshot_getter!(CheckSizeCm, F64);
    snapshot_getter!(StrobeFrequencyHz, F64);
    snapshot_getter!(StimulusWidthDeg, F64);
    snapshot_getter!(SweepSpeedDegPerSec, F64);
    snapshot_getter!(RotationSpeedDegPerSec, F64);
    snapshot_getter!(ExpansionSpeedDegPerSec, F64);
    snapshot_getter!(RotationDeg, F64);

    // Presentation
    snapshot_getter!(Conditions, StringVec);
    snapshot_getter!(Repetitions, U32);
    snapshot_getter!(PresentationStructure, Structure);
    snapshot_getter!(PresentationOrder, Order);

    // Timing
    snapshot_getter!(BaselineStartSec, F64);
    snapshot_getter!(BaselineEndSec, F64);
    snapshot_getter!(InterStimulusSec, F64);
    snapshot_getter!(InterDirectionSec, F64);

    // ── Computed values (mirror Registry computed.rs) ───────────────────

    /// Luminance high = mean_luminance * (1 + contrast). Clamped to [0, 1].
    pub fn luminance_high(&self) -> f64 {
        let mean = self.mean_luminance();
        let contrast = self.contrast();
        (mean + contrast * mean).clamp(0.0, 1.0)
    }

    /// Luminance low = mean_luminance * (1 - contrast). Clamped to [0, 1].
    pub fn luminance_low(&self) -> f64 {
        let mean = self.mean_luminance();
        let contrast = self.contrast();
        (mean - contrast * mean).clamp(0.0, 1.0)
    }
}
