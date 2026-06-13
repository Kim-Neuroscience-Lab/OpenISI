//! Registry snapshot — frozen parameter state for thread messages and .oisi provenance.
//!
//! A snapshot clones all values at a point in time. Thread-safe (Send + Sync via Clone).
//! Typed getters match Registry's interface so consumers can use either interchangeably.

use super::hardware::HardwareContext;
use super::{Carrier, Envelope, Order, Projection, Structure, VisualField};
use super::{ParamId, ParamValue, PARAM_DEFS};

/// Frozen snapshot of all parameter values at a point in time, plus the
/// hardware context they were captured against. The hardware fields are
/// the EDID/runtime facts the registry currently believes about the
/// connected hardware; snapshot consumers should prefer the
/// `effective_*` accessors which apply the user-override > hardware >
/// shipped-default precedence rather than reading the raw param.
#[derive(Debug, Clone)]
pub struct RegistrySnapshot {
    pub(crate) values: Vec<ParamValue>,
    pub(crate) hardware: HardwareContext,
    pub(crate) user_overrides: std::collections::HashSet<ParamId>,
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

    /// True iff the named param was set by the user (UI / overlay file)
    /// rather than coming from the shipped baseline.
    pub fn is_user_override(&self, id: ParamId) -> bool {
        self.user_overrides.contains(&id)
    }

    /// The hardware context this snapshot was captured against.
    pub fn hardware(&self) -> &HardwareContext {
        &self.hardware
    }

    /// Effective monitor panel width in cm — see
    /// [`crate::hardware::effective_hardware_value`] for precedence rules.
    pub fn effective_monitor_width_cm(&self) -> Option<f64> {
        crate::hardware::effective_hardware_value(
            self.user_overrides.contains(&ParamId::MonitorWidthCm),
            self.monitor_width_cm(),
            self.hardware.monitor_width_cm,
            |w| *w > 0.0,
        )
    }

    /// Effective monitor panel height in cm — same precedence as width.
    pub fn effective_monitor_height_cm(&self) -> Option<f64> {
        crate::hardware::effective_hardware_value(
            self.user_overrides.contains(&ParamId::MonitorHeightCm),
            self.monitor_height_cm(),
            self.hardware.monitor_height_cm,
            |h| *h > 0.0,
        )
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
            if def.persist != target {
                continue;
            }
            let value = &self.values[def.id as usize];
            insert_dotted(&mut root, def.toml_path, crate::param_json::to_json(value));
        }
        serde_json::Value::Object(root)
    }
}

impl RegistrySnapshot {
    /// Build a snapshot from a JSON tree produced by `to_json_for_target`
    /// (e.g. a `.oisi` file's `/analysis_params` attribute).
    ///
    /// **Strict, fail-loud schema — no silent fallbacks.** This
    /// reconstructs the exact parameters a recorded result was produced
    /// with; silently substituting a code default would corrupt
    /// provenance and break reproducibility. Therefore:
    /// - Every param with `persist == target` MUST be present in `root`.
    ///   A missing key is a hard `ParamsError::Config` naming what's
    ///   absent. Files predating a parameter are handled upstream by the
    ///   explicit migration gate (`isi_analysis::io::is_pre_2026_analysis_params`),
    ///   not by silently defaulting here.
    /// - Any leaf key in `root` that is NOT a registered `target` param
    ///   is a hard error (unknown / typo'd key) — mirrors the TOML
    ///   loader's `deny_unknown`-style strictness in `toml_io`.
    /// - Schema-mismatched values (string where number expected, integer
    ///   out of range) error out via `json_value_to_param`.
    ///
    /// Params of *other* targets keep their `PARAM_DEFS` defaults so the
    /// returned snapshot is fully populated.
    pub fn from_json_tree(
        target: super::PersistTarget,
        root: &serde_json::Value,
    ) -> crate::error::ParamsResult<Self> {
        use crate::error::ParamsError;

        // Start from defaults for every param (other targets stay default).
        let mut values: Vec<ParamValue> =
            PARAM_DEFS.iter().map(|def| def.default.clone()).collect();

        // Overlay values for params in this target; a missing key is fatal.
        let mut known_paths: std::collections::HashSet<&'static str> =
            std::collections::HashSet::new();
        let mut missing: Vec<&'static str> = Vec::new();
        for def in PARAM_DEFS.iter() {
            if def.persist != target {
                continue;
            }
            known_paths.insert(def.toml_path);
            match navigate_dotted(root, def.toml_path) {
                Some(json_value) => {
                    values[def.id as usize] =
                        crate::param_json::from_json(&def.default, json_value, def.toml_path)?;
                }
                None => missing.push(def.toml_path),
            }
        }
        if !missing.is_empty() {
            return Err(ParamsError::Config(format!(
                "registry tree for {target:?} is missing required key(s): {}. \
                 The recorded parameters are incomplete for the current schema — \
                 re-run analysis or migrate the file.",
                missing.join(", ")
            )));
        }

        // Reject any leaf key that is not a registered param for this target.
        let mut unknown: Vec<String> = Vec::new();
        collect_unknown_json_leaves(root, "", &known_paths, &mut unknown);
        if !unknown.is_empty() {
            return Err(ParamsError::Config(format!(
                "registry tree for {target:?} has unknown key(s) not defined in the \
                 parameter registry: {}",
                unknown.join(", ")
            )));
        }

        // Reconstructing from a .oisi file: no live hardware context and
        // no user_overrides info — the file already encodes the effective
        // value at acquisition time. Consumers should query
        // `monitor_width_cm()` directly when reading these snapshots.
        Ok(Self {
            values,
            hardware: HardwareContext::default(),
            user_overrides: std::collections::HashSet::new(),
        })
    }
}

/// Walk a JSON object tree and append the dotted path of every leaf value
/// that is not a known `toml_path` for the target. Mirrors
/// `toml_io::collect_unknown_leaves` for the JSON provenance form, so the
/// `.oisi` reload path rejects unknown keys exactly as the TOML loader does.
fn collect_unknown_json_leaves(
    val: &serde_json::Value,
    prefix: &str,
    known: &std::collections::HashSet<&'static str>,
    out: &mut Vec<String>,
) {
    if let serde_json::Value::Object(map) = val {
        for (k, v) in map {
            let path = if prefix.is_empty() {
                k.clone()
            } else {
                format!("{prefix}.{k}")
            };
            if v.is_object() {
                collect_unknown_json_leaves(v, &path, known, out);
            } else if !known.contains(path.as_str()) {
                out.push(path);
            }
        }
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

/// Insert `value` at the dotted `path` (e.g., `"segmentation.open_radius"`)
/// into a JSON object tree, creating intermediate nested objects. If a
/// prefix segment already holds a non-object value (only possible if
/// two `define_params!` entries declared conflicting paths) the insert
/// is skipped — the conflict surfaces at TOML serialize time via the
/// stricter `toml_io::insert_at_path`.
fn insert_dotted(
    root: &mut serde_json::Map<String, serde_json::Value>,
    path: &str,
    value: serde_json::Value,
) {
    let parts: Vec<&str> = path.split('.').collect();
    let Some((last, head)) = parts.split_last() else {
        return;
    };
    if head.is_empty() {
        root.insert((*last).to_string(), value);
        return;
    }
    let mut current = root;
    for part in head {
        let entry = current
            .entry(part.to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        let Some(next) = entry.as_object_mut() else {
            return;
        };
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
    ($variant:ident, VisualField) => {
        paste::paste! {
            pub fn [<$variant:snake>](&self) -> VisualField {
                match &self.values[ParamId::$variant as usize] {
                    ParamValue::VisualField(v) => *v,
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
    snapshot_getter!(MonitorWidthCm, F64);
    snapshot_getter!(MonitorHeightCm, F64);
    snapshot_getter!(BisectorXCm, F64);
    snapshot_getter!(BisectorYCm, F64);
    snapshot_getter!(MonitorYawDeg, F64);
    snapshot_getter!(MonitorPitchDeg, F64);
    snapshot_getter!(StimulusVisualField, VisualField);

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

#[cfg(test)]
mod from_json_tree_tests {
    use super::*;
    use crate::{PersistTarget, Registry};
    use std::path::Path;

    fn default_snapshot() -> RegistrySnapshot {
        // Paths are unused — we only snapshot PARAM_DEFS defaults, no load.
        Registry::new(Path::new("."), Path::new(".")).snapshot()
    }

    fn default_analysis_tree() -> serde_json::Value {
        default_snapshot().to_json_for_target(PersistTarget::Analysis)
    }

    /// A tree written by `to_json_for_target` round-trips back to the same
    /// values — the writer and the strict reader agree on the schema.
    #[test]
    fn round_trips_complete_tree() {
        let tree = default_analysis_tree();
        let snap = RegistrySnapshot::from_json_tree(PersistTarget::Analysis, &tree)
            .expect("complete tree should load");
        let def_snap = default_snapshot();
        for def in PARAM_DEFS
            .iter()
            .filter(|d| d.persist == PersistTarget::Analysis)
        {
            assert_eq!(
                snap.get(def.id),
                def_snap.get(def.id),
                "mismatch at {}",
                def.toml_path
            );
        }
    }

    /// A missing analysis key is fatal — no silent default. Names the key.
    #[test]
    fn missing_key_is_fatal() {
        let mut tree = default_analysis_tree();
        let victim = PARAM_DEFS
            .iter()
            .find(|d| d.persist == PersistTarget::Analysis)
            .expect("at least one Analysis param");
        // Drop the whole top-level section so its leaf(s) go missing.
        let top = victim.toml_path.split('.').next().unwrap().to_string();
        tree.as_object_mut().unwrap().remove(&top);

        let err = RegistrySnapshot::from_json_tree(PersistTarget::Analysis, &tree)
            .expect_err("missing key must fail");
        assert!(
            format!("{err}").contains("missing required key"),
            "got: {err}"
        );
    }

    /// An unknown leaf key is fatal (deny-unknown parity with the TOML loader).
    #[test]
    fn unknown_key_is_fatal() {
        let mut tree = default_analysis_tree();
        tree.as_object_mut()
            .unwrap()
            .insert("bogus_stage".into(), serde_json::json!({ "nonsense": 1 }));
        let err = RegistrySnapshot::from_json_tree(PersistTarget::Analysis, &tree)
            .expect_err("unknown key must fail");
        assert!(format!("{err}").contains("unknown key"), "got: {err}");
    }

    /// An out-of-range integer is fatal — never silently truncated/wrapped.
    #[test]
    fn out_of_range_integer_is_fatal() {
        let mut tree = default_analysis_tree();
        let int_param = PARAM_DEFS.iter().find(|d| {
            d.persist == PersistTarget::Analysis
                && matches!(
                    d.default,
                    ParamValue::U16(_) | ParamValue::U32(_) | ParamValue::I32(_)
                )
        });
        let Some(p) = int_param else {
            return;
        }; // no integer analysis param to exercise

        // Set the leaf well beyond u32::MAX — out of range for u16/u32/i32 alike.
        let huge = serde_json::json!(u64::from(u32::MAX) + 1_000_000);
        let parts: Vec<&str> = p.toml_path.split('.').collect();
        let mut cur = &mut tree;
        for seg in &parts[..parts.len() - 1] {
            cur = cur.as_object_mut().unwrap().get_mut(*seg).unwrap();
        }
        cur.as_object_mut()
            .unwrap()
            .insert(parts[parts.len() - 1].to_string(), huge);

        let err = RegistrySnapshot::from_json_tree(PersistTarget::Analysis, &tree)
            .expect_err("out-of-range integer must fail");
        assert!(format!("{err}").contains("out of range"), "got: {err}");
    }
}
