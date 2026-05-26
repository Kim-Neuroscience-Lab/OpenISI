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
    /// Get a parameter value by ID (untyped — `ParamValue` wrapper).
    pub fn get(&self, id: ParamId) -> &ParamValue {
        &self.values[id as usize]
    }

    /// Get a parameter as its typed `Value`, wrapped in `Tagged<P>`.
    /// The marker type `P` ties the value to one specific `ParamId`;
    /// downstream constructors (method-enum `gaussian(...)` etc.) only
    /// accept `Tagged<P>` for a specific `P`, so the type system
    /// prevents bare literals from sneaking into the pipeline.
    pub fn typed<P: crate::registry_param::RegistryParam>(
        &self,
    ) -> crate::registry_param::Tagged<P> {
        let value = &self.values[P::ID as usize];
        crate::registry_param::Tagged::new(P::extract(value))
    }

    /// Get the ParamDef for a given ID.
    pub fn def(id: ParamId) -> &'static super::ParamDef {
        &PARAM_DEFS[id as usize]
    }

    /// Serialize every param with `def.persist == target` into a nested
    /// JSON object, using each param's `toml_path` as the dotted key. The
    /// resulting tree mirrors the TOML layout — `[segmentation] open_radius`
    /// becomes `{"segmentation": {"open_radius": ...}}`.
    ///
    /// Used by `export::write_oisi` to snapshot Rig + Experiment params
    /// into the `.oisi` at acquisition time, and by analysis code to
    /// record `/analysis_params` at analyze time. Single source of truth:
    /// the macro defines the params; this function serializes whatever
    /// the macro defined, no manual mirroring.
    pub fn to_json_for_target(&self, target: super::PersistTarget) -> serde_json::Value {
        let mut root = serde_json::Map::new();
        for def in PARAM_DEFS.iter() {
            if def.persist != target { continue; }
            let value = &self.values[def.id as usize];
            insert_dotted(&mut root, def.toml_path, param_value_to_json(value));
        }
        serde_json::Value::Object(root)
    }
}

/// Convert a `ParamValue` to a `serde_json::Value`. Enums (Envelope,
/// Carrier, etc.) serialize as their lowercase debug repr to match the
/// existing TOML convention.
fn param_value_to_json(v: &ParamValue) -> serde_json::Value {
    use serde_json::Value;
    // Analysis-stage method-choice Kind enums use their serde impls
    // (snake_case) — multi-word variant names like `SnlcGarrett2014ImBound`
    // need the `_` separators serde produces, which the older
    // `{Debug}-lowercase` trick used by Envelope/Carrier/etc. would not.
    fn ser<T: serde::Serialize>(k: &T) -> Value {
        serde_json::to_value(k).expect("kind serde to_value cannot fail")
    }
    match v {
        ParamValue::Bool(b) => Value::Bool(*b),
        ParamValue::U16(n) => Value::Number((*n as u64).into()),
        ParamValue::U32(n) => Value::Number((*n as u64).into()),
        ParamValue::I32(n) => Value::Number((*n as i64).into()),
        ParamValue::Usize(n) => Value::Number((*n as u64).into()),
        ParamValue::F64(f) => serde_json::Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        ParamValue::String(s) => Value::String(s.clone()),
        ParamValue::StringVec(v) => Value::Array(
            v.iter().map(|s| Value::String(s.clone())).collect(),
        ),
        ParamValue::Envelope(e) => Value::String(format!("{e:?}").to_lowercase()),
        ParamValue::Carrier(c) => Value::String(format!("{c:?}").to_lowercase()),
        ParamValue::Projection(p) => Value::String(format!("{p:?}").to_lowercase()),
        ParamValue::Structure(s) => Value::String(format!("{s:?}").to_lowercase()),
        ParamValue::Order(o) => Value::String(format!("{o:?}").to_lowercase()),
        ParamValue::CycleCombine(k) => ser(k),
        ParamValue::PhaseSmoothing(k) => ser(k),
        ParamValue::VfsComputation(k) => ser(k),
        ParamValue::SignMapSmoothing(k) => ser(k),
        ParamValue::CortexSource(k) => ser(k),
        ParamValue::PatchThreshold(k) => ser(k),
        ParamValue::PatchExtraction(k) => ser(k),
        ParamValue::PatchRefinement(k) => ser(k),
        ParamValue::QualityGate(k) => ser(k),
        ParamValue::Eccentricity(k) => ser(k),
    }
}

impl RegistrySnapshot {
    /// Build a snapshot from a JSON tree produced by `to_json_for_target`.
    /// Walks `PARAM_DEFS` filtered to `target`; for each param, follows
    /// its `toml_path` through `root`. Missing paths fall through to
    /// the param's `default` — necessary for forward compatibility
    /// when older `.oisi` files predate a parameter's introduction.
    /// Schema-mismatched values (e.g. string where number expected)
    /// error out via `ParamsError::Config`.
    ///
    /// Constructs the snapshot directly from `PARAM_DEFS` defaults
    /// initially, then overlays values from the JSON. Other targets
    /// (params not in `target`) keep their defaults — the snapshot
    /// is fully populated.
    pub fn from_json_tree(
        target: super::PersistTarget,
        root: &serde_json::Value,
    ) -> crate::error::ParamsResult<Self> {
        // Start from defaults for every param.
        let mut values: Vec<ParamValue> =
            PARAM_DEFS.iter().map(|def| def.default.clone()).collect();

        // Overlay values from the JSON tree for params in this target.
        for def in PARAM_DEFS.iter() {
            if def.persist != target {
                continue;
            }
            let Some(json_value) = navigate_dotted(root, def.toml_path) else {
                continue; // missing → leave default
            };
            values[def.id as usize] = json_value_to_param(&def.default, json_value, def.toml_path)?;
        }

        Ok(Self { values })
    }
}

/// Follow a dotted path through a JSON tree. Returns `None` if any
/// segment is absent or a non-object intermediate node.
fn navigate_dotted<'a>(root: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut current = root;
    for part in path.split('.') {
        current = current.as_object()?.get(part)?;
    }
    Some(current)
}

/// Parse a `serde_json::Value` into a `ParamValue` matching the variant
/// of `template` (the param's default — identifies the expected type).
fn json_value_to_param(
    template: &ParamValue,
    json: &serde_json::Value,
    path: &str,
) -> crate::error::ParamsResult<ParamValue> {
    use crate::error::ParamsError;
    let cfg = |msg: String| ParamsError::Config(msg);
    let str_enum = |s: &str| format!("\"{}\"", s);
    match template {
        ParamValue::Bool(_) => json.as_bool().map(ParamValue::Bool)
            .ok_or_else(|| cfg(format!("expected bool at {path}"))),
        ParamValue::U16(_) => json.as_u64().map(|v| ParamValue::U16(v as u16))
            .ok_or_else(|| cfg(format!("expected integer at {path}"))),
        ParamValue::U32(_) => json.as_u64().map(|v| ParamValue::U32(v as u32))
            .ok_or_else(|| cfg(format!("expected integer at {path}"))),
        ParamValue::I32(_) => json.as_i64().map(|v| ParamValue::I32(v as i32))
            .ok_or_else(|| cfg(format!("expected integer at {path}"))),
        ParamValue::Usize(_) => json.as_u64().map(|v| ParamValue::Usize(v as usize))
            .ok_or_else(|| cfg(format!("expected non-negative integer at {path}"))),
        ParamValue::F64(_) => json.as_f64()
            .or_else(|| json.as_i64().map(|i| i as f64))
            .map(ParamValue::F64)
            .ok_or_else(|| cfg(format!("expected number at {path}"))),
        ParamValue::String(_) => json.as_str().map(|s| ParamValue::String(s.to_string()))
            .ok_or_else(|| cfg(format!("expected string at {path}"))),
        ParamValue::StringVec(_) => json.as_array().map(|arr| {
            ParamValue::StringVec(arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        }).ok_or_else(|| cfg(format!("expected array of strings at {path}"))),
        ParamValue::Envelope(_) => json.as_str()
            .and_then(|s| serde_json::from_str::<super::Envelope>(&str_enum(s)).ok())
            .map(ParamValue::Envelope)
            .ok_or_else(|| cfg(format!("expected envelope string at {path}"))),
        ParamValue::Carrier(_) => json.as_str()
            .and_then(|s| serde_json::from_str::<super::Carrier>(&str_enum(s)).ok())
            .map(ParamValue::Carrier)
            .ok_or_else(|| cfg(format!("expected carrier string at {path}"))),
        ParamValue::Projection(_) => json.as_str()
            .and_then(|s| serde_json::from_str::<super::Projection>(&str_enum(s)).ok())
            .map(ParamValue::Projection)
            .ok_or_else(|| cfg(format!("expected projection string at {path}"))),
        ParamValue::Structure(_) => json.as_str()
            .and_then(|s| serde_json::from_str::<super::Structure>(&str_enum(s)).ok())
            .map(ParamValue::Structure)
            .ok_or_else(|| cfg(format!("expected structure string at {path}"))),
        ParamValue::Order(_) => json.as_str()
            .and_then(|s| serde_json::from_str::<super::Order>(&str_enum(s)).ok())
            .map(ParamValue::Order)
            .ok_or_else(|| cfg(format!("expected order string at {path}"))),
        ParamValue::CycleCombine(_) => json.as_str()
            .and_then(|s| serde_json::from_str::<super::CycleCombineKind>(&str_enum(s)).ok())
            .map(ParamValue::CycleCombine)
            .ok_or_else(|| cfg(format!("expected cycle_combine method string at {path}"))),
        ParamValue::PhaseSmoothing(_) => json.as_str()
            .and_then(|s| serde_json::from_str::<super::PhaseSmoothingKind>(&str_enum(s)).ok())
            .map(ParamValue::PhaseSmoothing)
            .ok_or_else(|| cfg(format!("expected phase_smoothing method string at {path}"))),
        ParamValue::VfsComputation(_) => json.as_str()
            .and_then(|s| serde_json::from_str::<super::VfsComputationKind>(&str_enum(s)).ok())
            .map(ParamValue::VfsComputation)
            .ok_or_else(|| cfg(format!("expected vfs_computation method string at {path}"))),
        ParamValue::SignMapSmoothing(_) => json.as_str()
            .and_then(|s| serde_json::from_str::<super::SignMapSmoothingKind>(&str_enum(s)).ok())
            .map(ParamValue::SignMapSmoothing)
            .ok_or_else(|| cfg(format!("expected sign_map_smoothing method string at {path}"))),
        ParamValue::CortexSource(_) => json.as_str()
            .and_then(|s| serde_json::from_str::<super::CortexSourceKind>(&str_enum(s)).ok())
            .map(ParamValue::CortexSource)
            .ok_or_else(|| cfg(format!("expected cortex_source method string at {path}"))),
        ParamValue::PatchThreshold(_) => json.as_str()
            .and_then(|s| serde_json::from_str::<super::PatchThresholdKind>(&str_enum(s)).ok())
            .map(ParamValue::PatchThreshold)
            .ok_or_else(|| cfg(format!("expected patch_threshold method string at {path}"))),
        ParamValue::PatchExtraction(_) => json.as_str()
            .and_then(|s| serde_json::from_str::<super::PatchExtractionKind>(&str_enum(s)).ok())
            .map(ParamValue::PatchExtraction)
            .ok_or_else(|| cfg(format!("expected patch_extraction method string at {path}"))),
        ParamValue::PatchRefinement(_) => json.as_str()
            .and_then(|s| serde_json::from_str::<super::PatchRefinementKind>(&str_enum(s)).ok())
            .map(ParamValue::PatchRefinement)
            .ok_or_else(|| cfg(format!("expected patch_refinement method string at {path}"))),
        ParamValue::QualityGate(_) => json.as_str()
            .and_then(|s| serde_json::from_str::<super::QualityGateKind>(&str_enum(s)).ok())
            .map(ParamValue::QualityGate)
            .ok_or_else(|| cfg(format!("expected quality_gate method string at {path}"))),
        ParamValue::Eccentricity(_) => json.as_str()
            .and_then(|s| serde_json::from_str::<super::EccentricityKind>(&str_enum(s)).ok())
            .map(ParamValue::Eccentricity)
            .ok_or_else(|| cfg(format!("expected eccentricity method string at {path}"))),
    }
}

/// Insert `value` at the dotted `path` (e.g., `"segmentation.open_radius"`)
/// into a JSON object tree, creating intermediate nested objects. If a
/// prefix segment already holds a non-object value (only possible if
/// two `define_params!` entries declared conflicting paths) the insert
/// is skipped — the conflict surfaces at TOML serialize time via the
/// stricter `toml_io::insert_at_path`.
fn insert_dotted(root: &mut serde_json::Map<String, serde_json::Value>, path: &str, value: serde_json::Value) {
    let parts: Vec<&str> = path.split('.').collect();
    let Some((last, head)) = parts.split_last() else { return };
    if head.is_empty() {
        root.insert((*last).to_string(), value);
        return;
    }
    let mut current = root;
    for part in head {
        let entry = current
            .entry(part.to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        let Some(next) = entry.as_object_mut() else { return };
        current = next;
    }
    current.insert((*last).to_string(), value);
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
    snapshot_getter!(RotationK, I32);
    snapshot_getter!(AziAngularRange, F64);
    snapshot_getter!(AltAngularRange, F64);
    snapshot_getter!(OffsetAzi, F64);
    snapshot_getter!(OffsetAlt, F64);
    snapshot_getter!(SnrThresholdEnabled, Bool);
    snapshot_getter!(SnrThresholdValue, F64);
    snapshot_getter!(SnrPreferSpectral, Bool);
    snapshot_getter!(SnrUseTransparentMask, Bool);

    // Analysis Segmentation (Allen retinotopic_mapping Python)
    // Segmentation method params live in AnalysisParams enum variants now —
    // no primitive registry getters.

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
