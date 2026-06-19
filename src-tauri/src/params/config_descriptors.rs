//! Descriptor generation from the typed [`ConfigStore`].
//!
//! Produces the `ParamDescriptorJson[]` the frontend consumes, sourced from the
//! typed configs. Three proper sources, one per kind of data:
//! - **values** ← the typed config (serialized, navigated by dotted path);
//! - **types / ranges / enum-options** ← `kind` + `constraint` here (which the
//!   garde attrs on the structs also enforce — these are the UI *hints*);
//! - **labels / units / groups** ← the [`UiMeta`] catalog below (genuine UI
//!   presentation copy — `label`/`unit`/`GroupId`, which schemars does not supply).
//!
//! Fidelity to the frontend's descriptor contract is locked by golden-snapshot
//! tests (`*_descriptors_golden`).

use openisi_params::config::{
    AnalysisConfig, ConfigStore, ExperimentConfig, RigConfig, UiStateConfig,
};
use openisi_params::{enum_options, EnumOption, GroupId};

use super::commands::{ConstraintJson, ParamDescriptorJson};

/// Numeric/value kind of a parameter (matches the frontend's `param_type`).
#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Bool,
    U16,
    U32,
    I32,
    Usize,
    F64,
    String,
    StringVec,
    /// An enum; the `EnumKind` names which enum's options to project.
    Enum(EnumKind),
}

impl Kind {
    fn type_str(self) -> &'static str {
        match self {
            Kind::Bool => "bool",
            Kind::U16 => "u16",
            Kind::U32 => "u32",
            Kind::I32 => "i32",
            Kind::Usize => "usize",
            Kind::F64 => "f64",
            Kind::String => "string",
            Kind::StringVec => "string_vec",
            Kind::Enum(_) => "enum",
        }
    }
}

/// Which enum a `Kind::Enum` param projects (for `(wire, label)` options).
#[derive(Clone, Copy)]
pub(crate) enum EnumKind {
    VisualField,
    Projection,
    Envelope,
    Carrier,
    Structure,
    Order,
    // Analysis per-stage method selectors.
    Baseline,
    ResponseNormalization,
    CycleAverage,
    Rectification,
    DirectionSmoothing,
    CycleCombine,
    PhaseSmoothing,
    VfsComputation,
    SignMapSmoothing,
    CortexSource,
    PatchThreshold,
    PatchExtraction,
    PatchRefinement,
    Eccentricity,
}

impl EnumKind {
    fn options(self) -> Vec<EnumOption> {
        use openisi_params::{
            BaselineKind, Carrier, CortexSourceKind, CycleAverageKind, CycleCombineKind,
            DirectionSmoothingKind, EccentricityKind, Envelope, Order, PatchExtractionKind,
            PatchRefinementKind, PatchThresholdKind, PhaseSmoothingKind, Projection,
            RectificationKind, ResponseNormalizationKind, SignMapSmoothingKind, Structure,
            VfsComputationKind, VisualField,
        };
        match self {
            EnumKind::VisualField => enum_options::<VisualField>(),
            EnumKind::Projection => enum_options::<Projection>(),
            EnumKind::Envelope => enum_options::<Envelope>(),
            EnumKind::Carrier => enum_options::<Carrier>(),
            EnumKind::Structure => enum_options::<Structure>(),
            EnumKind::Order => enum_options::<Order>(),
            EnumKind::Baseline => enum_options::<BaselineKind>(),
            EnumKind::ResponseNormalization => enum_options::<ResponseNormalizationKind>(),
            EnumKind::CycleAverage => enum_options::<CycleAverageKind>(),
            EnumKind::Rectification => enum_options::<RectificationKind>(),
            EnumKind::DirectionSmoothing => enum_options::<DirectionSmoothingKind>(),
            EnumKind::CycleCombine => enum_options::<CycleCombineKind>(),
            EnumKind::PhaseSmoothing => enum_options::<PhaseSmoothingKind>(),
            EnumKind::VfsComputation => enum_options::<VfsComputationKind>(),
            EnumKind::SignMapSmoothing => enum_options::<SignMapSmoothingKind>(),
            EnumKind::CortexSource => enum_options::<CortexSourceKind>(),
            EnumKind::PatchThreshold => enum_options::<PatchThresholdKind>(),
            EnumKind::PatchExtraction => enum_options::<PatchExtractionKind>(),
            EnumKind::PatchRefinement => enum_options::<PatchRefinementKind>(),
            EnumKind::Eccentricity => enum_options::<EccentricityKind>(),
        }
    }
}

/// UI presentation hint for one parameter.
#[derive(Clone, Copy)]
pub(crate) enum Bound {
    None,
    Range(f64, f64),
    Min(f64),
}

/// One descriptor's UI metadata. `id` is the dotted path *within its config*
/// (e.g. `"camera.exposure_us"` navigates the rig config).
pub(crate) struct UiMeta {
    pub id: &'static str,
    pub group: GroupId,
    pub label: &'static str,
    pub unit: &'static str,
    pub kind: Kind,
    pub bound: Bound,
}

/// Build the descriptor list for a typed config value + its UI-meta catalog.
/// `cfg` is the config serialized to JSON; each `meta.id` is navigated within it
/// for the current value. (Rig/experiment/ui-state are plain trees → every param
/// active; analysis tagged enums add active-variant logic, handled separately.)
fn descriptors_for(cfg: &serde_json::Value, metas: &[UiMeta]) -> Vec<ParamDescriptorJson> {
    metas
        .iter()
        .map(|m| {
            let value = navigate(cfg, m.id).cloned().unwrap_or(serde_json::Value::Null);
            let constraint = match m.kind {
                Kind::Enum(ek) => ConstraintJson::enum_values(ek.options()),
                _ => match m.bound {
                    Bound::None => ConstraintJson::none(),
                    Bound::Range(lo, hi) => ConstraintJson::range(lo, hi),
                    Bound::Min(lo) => ConstraintJson::min_only(lo),
                },
            };
            ParamDescriptorJson {
                id: m.id.to_string(),
                label: m.label.to_string(),
                unit: m.unit.to_string(),
                param_type: m.kind.type_str().to_string(),
                value,
                constraint,
                active: true,
                group: m.group,
            }
        })
        .collect()
}

/// Follow a dotted path through a JSON object tree.
fn navigate<'a>(root: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut cur = root;
    for seg in path.split('.') {
        cur = cur.as_object()?.get(seg)?;
    }
    Some(cur)
}

/// Rig descriptors (every rig param is always active).
pub(crate) fn rig_descriptors(rig: &RigConfig) -> Vec<ParamDescriptorJson> {
    let json = serde_json::to_value(rig).expect("RigConfig serializes");
    descriptors_for(&json, RIG_META)
}

/// Experiment descriptors (plain tree → every param active).
pub(crate) fn experiment_descriptors(exp: &ExperimentConfig) -> Vec<ParamDescriptorJson> {
    let json = serde_json::to_value(exp).expect("ExperimentConfig serializes");
    descriptors_for(&json, EXP_META)
}

/// UI-state descriptors (plain tree → every param active).
pub(crate) fn ui_state_descriptors(ui: &UiStateConfig) -> Vec<ParamDescriptorJson> {
    let json = serde_json::to_value(ui).expect("UiStateConfig serializes");
    descriptors_for(&json, UI_STATE_META)
}

fn constraint_for(kind: Kind, bound: Bound) -> ConstraintJson {
    match kind {
        Kind::Enum(ek) => ConstraintJson::enum_values(ek.options()),
        _ => match bound {
            Bound::None => ConstraintJson::none(),
            Bound::Range(lo, hi) => ConstraintJson::range(lo, hi),
            Bound::Min(lo) => ConstraintJson::min_only(lo),
        },
    }
}

/// Analysis descriptors. Unlike the plain configs, the tagged `AnalysisConfig`
/// stores only the *active* variant's tunables, so activation follows the
/// selected method: a stage's method param is always active; a tunable is active
/// iff its variant is the selected method (its value then comes from the config),
/// otherwise it's inactive and carries its canonical default from the
/// [`ANALYSIS_TUNABLE_META`] catalog (so every variant's tunable is still listed).
pub(crate) fn analysis_descriptors(analysis: &AnalysisConfig) -> Vec<ParamDescriptorJson> {
    let json = serde_json::to_value(analysis).expect("AnalysisConfig serializes");
    let mut out = Vec::new();

    // Method selectors — always active.
    for m in ANALYSIS_METHOD_META {
        let stage = m.id.strip_suffix(".method").expect("method id");
        out.push(ParamDescriptorJson {
            id: m.id.to_string(),
            label: m.label.to_string(),
            unit: m.unit.to_string(),
            param_type: m.kind.type_str().to_string(),
            value: json
                .get(stage)
                .and_then(|s| s.get("method"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            constraint: constraint_for(m.kind, m.bound),
            active: true,
            group: m.group,
        });
    }

    // Per-variant tunables — active iff the stage's selected method is this variant.
    for t in ANALYSIS_TUNABLE_META {
        let selected = json
            .get(t.stage)
            .and_then(|s| s.get("method"))
            .and_then(|m| m.as_str())
            .unwrap_or("");
        let active = selected == t.variant;
        let value = if active {
            json.get(t.stage)
                .and_then(|s| s.get(t.field))
                .cloned()
                .unwrap_or(serde_json::Value::Null)
        } else {
            t.default_json()
        };
        out.push(ParamDescriptorJson {
            id: t.id.to_string(),
            label: t.label.to_string(),
            unit: t.unit.to_string(),
            param_type: t.kind.type_str().to_string(),
            value,
            constraint: constraint_for(t.kind, t.bound),
            active,
            group: t.group,
        });
    }
    out
}

/// Every descriptor the store can serve (rig + experiment + analysis + ui-state).
fn all_descriptors(store: &ConfigStore) -> Vec<ParamDescriptorJson> {
    let mut out = rig_descriptors(store.rig());
    out.extend(experiment_descriptors(store.experiment()));
    out.extend(analysis_descriptors(store.analysis()));
    out.extend(ui_state_descriptors(&UiStateConfig::default()));
    out
}

/// Descriptors served by `get_param_descriptors`. Filter semantics: a target
/// keyword (`"rig"`/`"experiment"`/
/// `"analysis"`/`"ui_state"`) returns that persist target's params; otherwise the
/// arg is a `GroupId` and filters by group; an unknown arg returns empty; `None`
/// returns everything.
pub fn config_descriptors(store: &ConfigStore, group: Option<&str>) -> Vec<ParamDescriptorJson> {
    use std::str::FromStr;
    let Some(g) = group else {
        return all_descriptors(store);
    };
    match g {
        "rig" => return rig_descriptors(store.rig()),
        "experiment" => return experiment_descriptors(store.experiment()),
        "analysis" => return analysis_descriptors(store.analysis()),
        "ui_state" => return ui_state_descriptors(&UiStateConfig::default()),
        _ => {}
    }
    match GroupId::from_str(g) {
        Ok(gid) => all_descriptors(store)
            .into_iter()
            .filter(|d| d.group == gid)
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// One analysis per-variant tunable.
pub(crate) struct ATun {
    pub id: &'static str,
    pub stage: &'static str,
    pub variant: &'static str,
    pub field: &'static str,
    pub group: GroupId,
    pub label: &'static str,
    pub unit: &'static str,
    pub kind: Kind,
    pub bound: Bound,
    pub default_f: f64,
}

impl ATun {
    pub(crate) fn default_json(&self) -> serde_json::Value {
        match self.kind {
            Kind::I32 => serde_json::json!(self.default_f as i64),
            Kind::U16 | Kind::U32 | Kind::Usize => serde_json::json!(self.default_f as u64),
            _ => serde_json::json!(self.default_f),
        }
    }
}

/// Which typed config a dotted param path belongs to.
pub(crate) enum Target {
    Rig,
    Experiment,
    Analysis,
    UiState,
}

/// Classify a dotted param path (from `set_params`) to its config, via the
/// UI-meta catalogs (the single source of which param lives where).
pub(crate) fn target_for_path(path: &str) -> Option<Target> {
    if RIG_META.iter().any(|m| m.id == path) {
        return Some(Target::Rig);
    }
    if EXP_META.iter().any(|m| m.id == path) {
        return Some(Target::Experiment);
    }
    if UI_STATE_META.iter().any(|m| m.id == path) {
        return Some(Target::UiState);
    }
    if ANALYSIS_METHOD_META.iter().any(|m| m.id == path)
        || ANALYSIS_TUNABLE_META.iter().any(|t| t.id == path)
    {
        return Some(Target::Analysis);
    }
    None
}

/// Nest a flat dotted path + value into a JSON object overlay
/// (`"camera.exposure_us"` + v → `{"camera": {"exposure_us": v}}`).
pub(crate) fn nest_overlay(path: &str, value: serde_json::Value) -> serde_json::Value {
    let mut v = value;
    for seg in path.split('.').collect::<Vec<_>>().iter().rev() {
        let mut m = serde_json::Map::new();
        m.insert((*seg).to_string(), v);
        v = serde_json::Value::Object(m);
    }
    v
}

/// Build the analysis-config overlay for setting one analysis param. A method
/// change (`"<stage>.method"`) switches the variant, so the overlay carries the
/// new method PLUS that variant's default tunables (the tagged enum needs every
/// active field). A tunable change (`"<stage>.<variant>.<field>"`) sets the field
/// flat on the stage (the active variant stores tunables flat).
pub(crate) fn analysis_overlay(path: &str, value: serde_json::Value) -> serde_json::Value {
    if let Some(stage) = path.strip_suffix(".method") {
        let method = value.as_str().unwrap_or("").to_string();
        let mut stage_obj = serde_json::Map::new();
        stage_obj.insert("method".to_string(), value);
        for t in ANALYSIS_TUNABLE_META
            .iter()
            .filter(|t| t.stage == stage && t.variant == method)
        {
            stage_obj.insert(t.field.to_string(), t.default_json());
        }
        let mut top = serde_json::Map::new();
        top.insert(stage.to_string(), serde_json::Value::Object(stage_obj));
        return serde_json::Value::Object(top);
    }
    // Tunable "<stage>.<variant>.<field>" → {stage: {field: value}}.
    let parts: Vec<&str> = path.split('.').collect();
    if parts.len() == 3 {
        let mut inner = serde_json::Map::new();
        inner.insert(parts[2].to_string(), value);
        let mut top = serde_json::Map::new();
        top.insert(parts[0].to_string(), serde_json::Value::Object(inner));
        return serde_json::Value::Object(top);
    }
    nest_overlay(path, value)
}

use Bound::{Min, None as NoBound, Range};
use EnumKind as Ek;
use GroupId::{
    Camera, CortexSource as GCortex, Display, Eccentricity as GEcc, Geometry, Paths,
    PatchExtraction as GPExtract, PatchRefinement as GPRefine, PatchThreshold as GPThresh,
    PhaseSmoothing as GPhase, Presentation, Retinotopy, Ring, SignMapSmoothing as GSign, Stimulus,
    System, Timing, Baseline as GBaseline, CycleAverage as GCycAvg, CycleCombine as GCycComb,
    VfsComputation as GVfs,
};
use Kind::{Bool, Enum, F64, String as KString, StringVec, Usize, I32, U16, U32};

/// Rig param UI catalog (labels/units/groups for the `RigConfig` fields).
/// Fidelity is locked by the `rig_descriptors_golden` test.
pub(crate) static RIG_META: &[UiMeta] = &[
    // ── Camera ──
    UiMeta { id: "camera.exposure_us", group: Camera, label: "Exposure", unit: "\u{00b5}s", kind: U32, bound: Range(1.0, 1_000_000.0) },
    UiMeta { id: "camera.binning", group: Camera, label: "Binning", unit: "x", kind: U16, bound: Range(1.0, 16.0) },
    UiMeta { id: "camera.um_per_pixel", group: Camera, label: "Camera Pixel Size", unit: "\u{00b5}m/px", kind: F64, bound: Min(0.001) },
    // ── Display / rig geometry ──
    UiMeta { id: "geometry.viewing_distance_cm", group: Display, label: "Viewing Distance", unit: "cm", kind: F64, bound: Min(0.1) },
    UiMeta { id: "geometry.monitor_width_cm", group: Display, label: "Monitor Width", unit: "cm", kind: F64, bound: Min(0.1) },
    UiMeta { id: "geometry.monitor_height_cm", group: Display, label: "Monitor Height", unit: "cm", kind: F64, bound: Min(0.1) },
    UiMeta { id: "geometry.bisector_x_cm", group: Display, label: "Bisector X Intercept", unit: "cm", kind: F64, bound: NoBound },
    UiMeta { id: "geometry.bisector_y_cm", group: Display, label: "Bisector Y Intercept", unit: "cm", kind: F64, bound: NoBound },
    UiMeta { id: "geometry.monitor_yaw_deg", group: Display, label: "Monitor Yaw", unit: "\u{00b0}", kind: F64, bound: Range(-90.0, 90.0) },
    UiMeta { id: "geometry.monitor_pitch_deg", group: Display, label: "Monitor Pitch", unit: "\u{00b0}", kind: F64, bound: Range(-90.0, 90.0) },
    UiMeta { id: "geometry.visual_field", group: Display, label: "Visual Field", unit: "", kind: Enum(EnumKind::VisualField), bound: NoBound },
    // ── Ring overlay ──
    UiMeta { id: "ring_overlay.enabled", group: Ring, label: "Enabled", unit: "", kind: Bool, bound: NoBound },
    UiMeta { id: "ring_overlay.radius_px", group: Ring, label: "Radius", unit: "px", kind: U32, bound: Min(1.0) },
    UiMeta { id: "ring_overlay.center_x_px", group: Ring, label: "Center X", unit: "px", kind: U32, bound: NoBound },
    UiMeta { id: "ring_overlay.center_y_px", group: Ring, label: "Center Y", unit: "px", kind: U32, bound: NoBound },
    // ── Display ──
    UiMeta { id: "display.target_stimulus_fps", group: Display, label: "Target Stimulus FPS", unit: "Hz", kind: U32, bound: Min(1.0) },
    UiMeta { id: "display.monitor_rotation_deg", group: Display, label: "Monitor Rotation", unit: "\u{00b0}", kind: F64, bound: Range(0.0, 360.0) },
    // ── System tuning ──
    UiMeta { id: "system.camera_frame_send_interval_ms", group: System, label: "Camera Frame Send Interval", unit: "ms", kind: U32, bound: Min(1.0) },
    UiMeta { id: "system.camera_poll_interval_ms", group: System, label: "Camera Poll Interval", unit: "ms", kind: U32, bound: Min(1.0) },
    UiMeta { id: "system.camera_first_frame_timeout_ms", group: System, label: "Camera First Frame Timeout", unit: "ms", kind: U32, bound: Min(1.0) },
    UiMeta { id: "system.camera_first_frame_poll_ms", group: System, label: "Camera First Frame Poll", unit: "ms", kind: U32, bound: Min(1.0) },
    UiMeta { id: "system.display_validation_sample_count", group: System, label: "Display Validation Sample Count", unit: "", kind: U32, bound: Min(1.0) },
    UiMeta { id: "system.preview_width_px", group: System, label: "Preview Width", unit: "px", kind: U32, bound: Min(1.0) },
    UiMeta { id: "system.preview_interval_ms", group: System, label: "Preview Interval", unit: "ms", kind: U32, bound: Min(1.0) },
    UiMeta { id: "system.preview_cycle_sec", group: System, label: "Preview Cycle", unit: "s", kind: F64, bound: Min(0.0) },
    UiMeta { id: "system.idle_sleep_ms", group: System, label: "Idle Sleep", unit: "ms", kind: U32, bound: Min(1.0) },
    UiMeta { id: "system.fps_window_frames", group: System, label: "FPS Window Frames", unit: "", kind: Usize, bound: Min(1.0) },
    UiMeta { id: "system.drop_detection_warmup_frames", group: System, label: "Drop Detection Warmup", unit: "frames", kind: Usize, bound: NoBound },
    UiMeta { id: "system.drop_detection_threshold", group: System, label: "Drop Detection Threshold", unit: "", kind: F64, bound: Min(0.0) },
    // ── Paths ──
    UiMeta { id: "paths.data_directory", group: Paths, label: "Data Directory", unit: "", kind: KString, bound: NoBound },
    UiMeta { id: "paths.experiments_directory", group: Paths, label: "Experiments Directory", unit: "", kind: KString, bound: NoBound },
];

/// Experiment param UI catalog (labels/units/groups for the `ExperimentConfig` fields).
pub(crate) static EXP_META: &[UiMeta] = &[
    // ── Stimulus geometry ──
    UiMeta { id: "stimulus_geometry.rotation_k", group: Geometry, label: "Rotation K", unit: "", kind: I32, bound: Range(-3.0, 3.0) },
    UiMeta { id: "stimulus_geometry.azi_angular_range", group: Geometry, label: "Azimuth Angular Range", unit: "\u{00b0}", kind: F64, bound: Range(0.0, 360.0) },
    UiMeta { id: "stimulus_geometry.alt_angular_range", group: Geometry, label: "Altitude Angular Range", unit: "\u{00b0}", kind: F64, bound: Range(0.0, 360.0) },
    UiMeta { id: "stimulus_geometry.offset_azi", group: Geometry, label: "Azimuth Offset", unit: "\u{00b0}", kind: F64, bound: Range(-180.0, 180.0) },
    UiMeta { id: "stimulus_geometry.offset_alt", group: Geometry, label: "Altitude Offset", unit: "\u{00b0}", kind: F64, bound: Range(-180.0, 180.0) },
    // ── Experiment geometry ──
    UiMeta { id: "geometry.horizontal_offset_deg", group: Geometry, label: "Horizontal Offset", unit: "\u{00b0}", kind: F64, bound: Range(-180.0, 180.0) },
    UiMeta { id: "geometry.vertical_offset_deg", group: Geometry, label: "Vertical Offset", unit: "\u{00b0}", kind: F64, bound: Range(-90.0, 90.0) },
    UiMeta { id: "geometry.projection", group: Geometry, label: "Projection", unit: "", kind: Enum(EnumKind::Projection), bound: NoBound },
    // ── Stimulus ──
    UiMeta { id: "stimulus.envelope", group: Stimulus, label: "Envelope", unit: "", kind: Enum(EnumKind::Envelope), bound: NoBound },
    UiMeta { id: "stimulus.carrier", group: Stimulus, label: "Carrier", unit: "", kind: Enum(EnumKind::Carrier), bound: NoBound },
    UiMeta { id: "stimulus.params.contrast", group: Stimulus, label: "Contrast", unit: "", kind: F64, bound: Range(0.0, 1.0) },
    UiMeta { id: "stimulus.params.mean_luminance", group: Stimulus, label: "Mean Luminance", unit: "", kind: F64, bound: Range(0.0, 1.0) },
    UiMeta { id: "stimulus.params.background_luminance", group: Stimulus, label: "Background Luminance", unit: "", kind: F64, bound: Range(0.0, 1.0) },
    UiMeta { id: "stimulus.params.check_size_deg", group: Stimulus, label: "Check Size", unit: "\u{00b0}", kind: F64, bound: Min(0.001) },
    UiMeta { id: "stimulus.params.check_size_cm", group: Stimulus, label: "Check Size", unit: "cm", kind: F64, bound: Min(0.001) },
    UiMeta { id: "stimulus.params.strobe_frequency_hz", group: Stimulus, label: "Strobe Frequency", unit: "Hz", kind: F64, bound: Min(0.0) },
    UiMeta { id: "stimulus.params.stimulus_width_deg", group: Stimulus, label: "Stimulus Width", unit: "\u{00b0}", kind: F64, bound: Min(0.001) },
    UiMeta { id: "stimulus.params.sweep_speed_deg_per_sec", group: Stimulus, label: "Sweep Speed", unit: "\u{00b0}/s", kind: F64, bound: Min(0.001) },
    UiMeta { id: "stimulus.params.rotation_speed_deg_per_sec", group: Stimulus, label: "Rotation Speed", unit: "\u{00b0}/s", kind: F64, bound: Min(0.001) },
    UiMeta { id: "stimulus.params.expansion_speed_deg_per_sec", group: Stimulus, label: "Expansion Speed", unit: "\u{00b0}/s", kind: F64, bound: Min(0.001) },
    UiMeta { id: "stimulus.params.rotation_deg", group: Stimulus, label: "Rotation", unit: "\u{00b0}", kind: F64, bound: Range(-360.0, 360.0) },
    // ── Presentation ──
    UiMeta { id: "presentation.conditions", group: Presentation, label: "Conditions", unit: "", kind: StringVec, bound: NoBound },
    UiMeta { id: "presentation.repetitions", group: Presentation, label: "Repetitions", unit: "", kind: U32, bound: Min(1.0) },
    UiMeta { id: "presentation.structure", group: Presentation, label: "Structure", unit: "", kind: Enum(EnumKind::Structure), bound: NoBound },
    UiMeta { id: "presentation.order", group: Presentation, label: "Order", unit: "", kind: Enum(EnumKind::Order), bound: NoBound },
    // ── Timing ──
    UiMeta { id: "timing.baseline_start_sec", group: Timing, label: "Baseline Start", unit: "s", kind: F64, bound: Min(0.0) },
    UiMeta { id: "timing.baseline_end_sec", group: Timing, label: "Baseline End", unit: "s", kind: F64, bound: Min(0.0) },
    UiMeta { id: "timing.inter_stimulus_sec", group: Timing, label: "Inter-Stimulus", unit: "s", kind: F64, bound: Min(0.0) },
    UiMeta { id: "timing.inter_direction_sec", group: Timing, label: "Inter-Direction", unit: "s", kind: F64, bound: Min(0.0) },
];

/// UI-state param UI catalog (labels/units/groups for the `UiStateConfig` fields).
pub(crate) static UI_STATE_META: &[UiMeta] = &[
    UiMeta { id: "snr_threshold_enabled", group: Retinotopy, label: "SNR Threshold Enabled", unit: "", kind: Bool, bound: NoBound },
    UiMeta { id: "snr_threshold_value", group: Retinotopy, label: "SNR Threshold", unit: "", kind: F64, bound: Min(0.0) },
    UiMeta { id: "snr_prefer_spectral", group: Retinotopy, label: "Prefer Spectral SNR", unit: "", kind: Bool, bound: NoBound },
    UiMeta { id: "snr_use_transparent_mask", group: Retinotopy, label: "Use Transparent SNR Mask", unit: "", kind: Bool, bound: NoBound },
];

/// Analysis per-stage method selectors — always active (label "Method").
pub(crate) static ANALYSIS_METHOD_META: &[UiMeta] = &[
    UiMeta { id: "baseline.method", group: GBaseline, label: "Method", unit: "", kind: Enum(Ek::Baseline), bound: NoBound },
    UiMeta { id: "response_normalization.method", group: GBaseline, label: "Normalization", unit: "", kind: Enum(Ek::ResponseNormalization), bound: NoBound },
    UiMeta { id: "cycle_average.method", group: GCycAvg, label: "Method", unit: "", kind: Enum(Ek::CycleAverage), bound: NoBound },
    UiMeta { id: "rectification.method", group: GCycAvg, label: "Rectification", unit: "", kind: Enum(Ek::Rectification), bound: NoBound },
    UiMeta { id: "direction_smoothing.method", group: GCycComb, label: "Pre-combine smoothing", unit: "", kind: Enum(Ek::DirectionSmoothing), bound: NoBound },
    UiMeta { id: "cycle_combine.method", group: GCycComb, label: "Method", unit: "", kind: Enum(Ek::CycleCombine), bound: NoBound },
    UiMeta { id: "phase_smoothing.method", group: GPhase, label: "Method", unit: "", kind: Enum(Ek::PhaseSmoothing), bound: NoBound },
    UiMeta { id: "vfs_computation.method", group: GVfs, label: "Method", unit: "", kind: Enum(Ek::VfsComputation), bound: NoBound },
    UiMeta { id: "sign_map_smoothing.method", group: GSign, label: "Method", unit: "", kind: Enum(Ek::SignMapSmoothing), bound: NoBound },
    UiMeta { id: "cortex_source.method", group: GCortex, label: "Method", unit: "", kind: Enum(Ek::CortexSource), bound: NoBound },
    UiMeta { id: "patch_threshold.method", group: GPThresh, label: "Method", unit: "", kind: Enum(Ek::PatchThreshold), bound: NoBound },
    UiMeta { id: "patch_extraction.method", group: GPExtract, label: "Method", unit: "", kind: Enum(Ek::PatchExtraction), bound: NoBound },
    UiMeta { id: "patch_refinement.method", group: GPRefine, label: "Method", unit: "", kind: Enum(Ek::PatchRefinement), bound: NoBound },
    UiMeta { id: "eccentricity.method", group: GEcc, label: "Method", unit: "", kind: Enum(Ek::Eccentricity), bound: NoBound },
];

/// Analysis per-variant tunables (labels/units/groups + canonical defaults),
/// mirroring the tagged-enum variant fields in `config::analysis`.
pub(crate) static ANALYSIS_TUNABLE_META: &[ATun] = &[
    ATun { id: "direction_smoothing.snlc_adaptive_smoother.sigma_px", stage: "direction_smoothing", variant: "snlc_adaptive_smoother", field: "sigma_px", group: GCycComb, label: "Adaptive \u{03c3}", unit: "px", kind: F64, bound: Range(0.1, 50.0), default_f: 2.0 },
    ATun { id: "phase_smoothing.snlc_amp_weighted_phasor.sigma_px", stage: "phase_smoothing", variant: "snlc_amp_weighted_phasor", field: "sigma_px", group: GPhase, label: "Smoothing \u{03c3}", unit: "px", kind: F64, bound: Range(0.0, 50.0), default_f: 1.0 },
    ATun { id: "phase_smoothing.allen_zhuang2017_position_gaussian.sigma_px", stage: "phase_smoothing", variant: "allen_zhuang2017_position_gaussian", field: "sigma_px", group: GPhase, label: "Smoothing \u{03c3}", unit: "px", kind: F64, bound: Range(0.0, 50.0), default_f: 1.0 },
    ATun { id: "sign_map_smoothing.gaussian.sigma_um", stage: "sign_map_smoothing", variant: "gaussian", field: "sigma_um", group: GSign, label: "Smoothing \u{03c3}", unit: "\u{00b5}m", kind: F64, bound: Range(0.0, 500.0), default_f: 60.0 },
    ATun { id: "cortex_source.reliability.threshold", stage: "cortex_source", variant: "reliability", field: "threshold", group: GCortex, label: "Reliability threshold", unit: "", kind: F64, bound: Range(0.0, 1.0), default_f: 0.5 },
    ATun { id: "cortex_source.snlc_garrett2014_im_bound.k", stage: "cortex_source", variant: "snlc_garrett2014_im_bound", field: "k", group: GCortex, label: "\u{03c3} multiplier", unit: "", kind: F64, bound: Range(0.0, 10.0), default_f: 1.5 },
    ATun { id: "cortex_source.snlc_garrett2014_im_bound.close", stage: "cortex_source", variant: "snlc_garrett2014_im_bound", field: "close", group: GCortex, label: "Closing radius", unit: "px", kind: I32, bound: Range(0.0, 50.0), default_f: 10.0 },
    ATun { id: "cortex_source.snlc_garrett2014_im_bound.dilate", stage: "cortex_source", variant: "snlc_garrett2014_im_bound", field: "dilate", group: GCortex, label: "Dilation radius", unit: "px", kind: I32, bound: Range(0.0, 50.0), default_f: 3.0 },
    ATun { id: "cortex_source.snlc_mag_threshold.exponent", stage: "cortex_source", variant: "snlc_mag_threshold", field: "exponent", group: GCortex, label: "Magnitude exponent", unit: "", kind: F64, bound: Range(0.0, 10.0), default_f: 1.1 },
    ATun { id: "cortex_source.snlc_mag_threshold.threshold", stage: "cortex_source", variant: "snlc_mag_threshold", field: "threshold", group: GCortex, label: "ROI threshold", unit: "", kind: F64, bound: Range(0.0, 1.0), default_f: 0.12 },
    ATun { id: "patch_threshold.allen_zhuang2017_fixed_sign_map_thr.value", stage: "patch_threshold", variant: "allen_zhuang2017_fixed_sign_map_thr", field: "value", group: GPThresh, label: "Threshold", unit: "", kind: F64, bound: Range(0.0, 1.0), default_f: 0.35 },
    ATun { id: "patch_threshold.garrett2014_sigma_scaled.k", stage: "patch_threshold", variant: "garrett2014_sigma_scaled", field: "k", group: GPThresh, label: "\u{03c3} multiplier", unit: "", kind: F64, bound: Range(0.0, 10.0), default_f: 1.5 },
    ATun { id: "patch_extraction.allen_zhuang2017_label_open_close_dilate.open_iter", stage: "patch_extraction", variant: "allen_zhuang2017_label_open_close_dilate", field: "open_iter", group: GPExtract, label: "Opening iterations", unit: "", kind: I32, bound: Range(0.0, 50.0), default_f: 3.0 },
    ATun { id: "patch_extraction.allen_zhuang2017_label_open_close_dilate.close_iter", stage: "patch_extraction", variant: "allen_zhuang2017_label_open_close_dilate", field: "close_iter", group: GPExtract, label: "Closing iterations", unit: "", kind: I32, bound: Range(0.0, 50.0), default_f: 3.0 },
    ATun { id: "patch_extraction.allen_zhuang2017_label_open_close_dilate.dilation_iter", stage: "patch_extraction", variant: "allen_zhuang2017_label_open_close_dilate", field: "dilation_iter", group: GPExtract, label: "Dilation iterations", unit: "", kind: I32, bound: Range(0.0, 50.0), default_f: 15.0 },
    ATun { id: "patch_extraction.allen_zhuang2017_label_open_close_dilate.border_width", stage: "patch_extraction", variant: "allen_zhuang2017_label_open_close_dilate", field: "border_width", group: GPExtract, label: "Border width", unit: "px", kind: I32, bound: Range(1.0, 20.0), default_f: 1.0 },
    ATun { id: "patch_extraction.allen_zhuang2017_label_open_close_dilate.small_patch_thr", stage: "patch_extraction", variant: "allen_zhuang2017_label_open_close_dilate", field: "small_patch_thr", group: GPExtract, label: "Drop patches smaller than", unit: "px", kind: Usize, bound: Range(0.0, 10_000.0), default_f: 50.0 },
    ATun { id: "patch_refinement.allen_zhuang2017_split_merge.split_overlap_thr", stage: "patch_refinement", variant: "allen_zhuang2017_split_merge", field: "split_overlap_thr", group: GPRefine, label: "Split overlap threshold", unit: "", kind: F64, bound: Range(0.0, 10.0), default_f: 1.1 },
    ATun { id: "patch_refinement.allen_zhuang2017_split_merge.split_local_min_cut_step", stage: "patch_refinement", variant: "allen_zhuang2017_split_merge", field: "split_local_min_cut_step", group: GPRefine, label: "Split min cut step", unit: "\u{00b0}", kind: F64, bound: Range(0.0, 50.0), default_f: 5.0 },
    ATun { id: "patch_refinement.allen_zhuang2017_split_merge.merge_overlap_thr", stage: "patch_refinement", variant: "allen_zhuang2017_split_merge", field: "merge_overlap_thr", group: GPRefine, label: "Merge overlap threshold", unit: "", kind: F64, bound: Range(0.0, 1.0), default_f: 0.01 },
    ATun { id: "patch_refinement.allen_zhuang2017_split_merge.visual_space_pixel_size", stage: "patch_refinement", variant: "allen_zhuang2017_split_merge", field: "visual_space_pixel_size", group: GPRefine, label: "Visual-space pixel size", unit: "\u{00b0}", kind: F64, bound: Range(0.001, 10.0), default_f: 0.5 },
    ATun { id: "patch_refinement.allen_zhuang2017_split_merge.visual_space_close_iter", stage: "patch_refinement", variant: "allen_zhuang2017_split_merge", field: "visual_space_close_iter", group: GPRefine, label: "Visual-space close iterations", unit: "", kind: I32, bound: Range(0.0, 50.0), default_f: 15.0 },
    ATun { id: "patch_refinement.allen_zhuang2017_split_merge.ecc_map_filter_sigma", stage: "patch_refinement", variant: "allen_zhuang2017_split_merge", field: "ecc_map_filter_sigma", group: GPRefine, label: "Eccentricity filter \u{03c3}", unit: "px", kind: I32, bound: Range(0.0, 50.0), default_f: 10.0 },
    ATun { id: "patch_refinement.allen_zhuang2017_split_merge.border_width", stage: "patch_refinement", variant: "allen_zhuang2017_split_merge", field: "border_width", group: GPRefine, label: "Border width", unit: "px", kind: I32, bound: Range(1.0, 20.0), default_f: 1.0 },
    ATun { id: "patch_refinement.allen_zhuang2017_split_merge.small_patch_thr", stage: "patch_refinement", variant: "allen_zhuang2017_split_merge", field: "small_patch_thr", group: GPRefine, label: "Drop patches smaller than", unit: "px", kind: Usize, bound: Range(0.0, 10_000.0), default_f: 100.0 },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn config_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../config")
    }

    /// A `ConfigStore` loaded from the shipped `config/*.json` (rig + experiment +
    /// analysis), mirroring what `get_param_descriptors` serves.
    fn loaded_store() -> ConfigStore {
        let dir = config_dir();
        let mut s = ConfigStore::new(&dir, &dir);
        s.load_rig().expect("load rig.json");
        s.load_experiment().expect("load experiment.json");
        s.load_analysis().expect("load analysis.json");
        s
    }

    fn golden_path(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/params/golden")
            .join(format!("descriptors_{name}.json"))
    }

    /// Serialize descriptors to a stable, id-sorted pretty-JSON array.
    fn to_golden_json(descs: &[ParamDescriptorJson]) -> String {
        let mut vals: Vec<serde_json::Value> =
            descs.iter().map(|d| serde_json::to_value(d).unwrap()).collect();
        vals.sort_by(|a, b| a["id"].as_str().unwrap_or("").cmp(b["id"].as_str().unwrap_or("")));
        serde_json::to_string_pretty(&vals).unwrap()
    }

    /// Compare the generated descriptors to the committed golden snapshot — the
    /// frontend's descriptor contract, and the durable guard against drift. Set
    /// `OISI_REGEN_DESCRIPTOR_GOLDEN=1` to rewrite them after an intentional change.
    fn assert_matches_golden(name: &str, descs: &[ParamDescriptorJson]) {
        let got = to_golden_json(descs);
        let path = golden_path(name);
        if std::env::var("OISI_REGEN_DESCRIPTOR_GOLDEN").is_ok() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, format!("{got}\n")).unwrap();
            return;
        }
        let want = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read golden {}: {e}", path.display()));
        assert_eq!(
            got.trim(),
            want.trim(),
            "descriptor output for {name} drifted from the golden contract; if \
             intentional, regen with OISI_REGEN_DESCRIPTOR_GOLDEN=1"
        );
    }

    #[test]
    fn rig_descriptors_golden() {
        let s = loaded_store();
        assert_matches_golden("rig", &rig_descriptors(s.rig()));
    }

    #[test]
    fn experiment_descriptors_golden() {
        let s = loaded_store();
        assert_matches_golden("experiment", &experiment_descriptors(s.experiment()));
    }

    #[test]
    fn ui_state_descriptors_golden() {
        // UI-state is runtime-only; defaults are the served values.
        assert_matches_golden("ui_state", &ui_state_descriptors(&UiStateConfig::default()));
    }

    /// The hard case: analysis tagged enums. Method selectors + per-variant
    /// tunables (active iff selected; inactive carry their defaults), at the
    /// shipped tuned values.
    #[test]
    fn analysis_descriptors_golden() {
        let s = loaded_store();
        assert_matches_golden("analysis", &analysis_descriptors(s.analysis()));
    }
}
