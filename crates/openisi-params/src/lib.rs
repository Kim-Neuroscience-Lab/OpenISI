//! Reactive parameter registry — single source of truth for all configuration.
//!
//! Every parameter is defined once in `definitions.rs` via the `define_params!`
//! macro. The registry owns all values, validates against static constraints,
//! and serializes to/from TOML files that are byte-compatible with the
//! existing config format.
//!
//! This crate is depended on by both `src-tauri` (Tauri-shell wiring,
//! IPC commands) and `crates/isi-analysis` (algorithm code that needs
//! `RegistryParam` marker types for the bridge). It owns no Tauri,
//! ndarray, hdf5, or tch dependencies — only serde + toml.

use serde::{Deserialize, Serialize};

pub mod error;
pub use error::{ParamsError, ParamsResult};

// Re-export stimulus crate enums so the rest of the app can use `params::Envelope` etc.
pub use openisi_stimulus::dataset::EnvelopeType as Envelope;
pub use openisi_stimulus::geometry::ProjectionType as Projection;
pub use openisi_stimulus::sequencer::Order;

// Carrier, Structure, and VisualField are defined locally (not in the stimulus crate).
pub use self::carrier_types::Carrier;
pub use self::carrier_types::Structure;
pub use self::carrier_types::VisualField;

/// Carrier, Structure, and VisualField enums — previously in config.rs, now canonical here.
///
/// Every enum here follows the same unified pattern as
/// [`analysis_kinds`]: serde provides the wire string, strum::Display
/// provides the human-facing label, strum::EnumIter lets the
/// descriptor layer enumerate variants. The wire string and the UI
/// label both come from this one declaration, so they can never drift.
mod carrier_types {
    use serde::{Deserialize, Serialize};

    #[derive(
        Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::Display, strum::EnumIter,
    )]
    #[serde(rename_all = "snake_case")]
    pub enum Carrier {
        #[strum(to_string = "Solid")]
        Solid,
        #[strum(to_string = "Checkerboard")]
        Checkerboard,
    }

    impl Carrier {
        pub fn to_shader_int(self) -> i32 {
            match self {
                Carrier::Solid => 0,
                Carrier::Checkerboard => 1,
            }
        }
    }

    #[derive(
        Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::Display, strum::EnumIter,
    )]
    #[serde(rename_all = "snake_case")]
    pub enum Structure {
        #[strum(to_string = "Blocked")]
        Blocked,
        #[strum(to_string = "Interleaved")]
        Interleaved,
    }

    /// Which hemifield the stimulus monitor occupies — matches the
    /// `visual_field` discriminator in Zhuang's `retinotopic_mapping`
    /// `MonitorSetup.py`. Determines the sign convention for azimuth
    /// in the pixel→(az, el) transform.
    #[derive(
        Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::Display, strum::EnumIter,
    )]
    #[serde(rename_all = "snake_case")]
    pub enum VisualField {
        #[strum(to_string = "Left hemifield")]
        Left,
        #[strum(to_string = "Right hemifield")]
        Right,
    }
}

// ─── Submodules ───────────────────────────────────────────────────────────────

#[macro_use]
mod macros;
mod definitions;
pub mod param_json;
pub mod registry;
pub mod toml_io;

pub mod analysis_kinds;
pub mod computed;
pub mod constraints;
pub mod hardware;
pub mod labels;
pub mod registry_param;
pub mod snapshot;
// `commands.rs` (Tauri IPC commands) lives in src-tauri, not here.

pub use registry_param::{RegistryParam, Tagged};

pub use definitions::ParamId;
// PARAM_DEFS is a LazyLock<Vec<ParamDef>>, re-exported for convenience.
pub use definitions::PARAM_DEFS;
// Re-export every marker type the macro emits (one per `define_params!`
// entry). Consumers — especially the bridge in `crates/isi-analysis` —
// reference them as `openisi_params::SignMapSmoothingGaussianSigmaUm`, etc.
pub use analysis_kinds::{
    BaselineKind, CortexSourceKind, CycleAverageKind, CycleCombineKind, EccentricityKind,
    PatchExtractionKind, PatchRefinementKind, PatchThresholdKind, PhaseSmoothingKind,
    SignMapSmoothingKind, VfsComputationKind,
};
pub use definitions::*;
pub use labels::{enum_options, EnumOption};
pub use registry::Registry;

// ─── Core types ───────────────────────────────────────────────────────────────

/// Type-erased parameter value.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    Bool(bool),
    U16(u16),
    U32(u32),
    I32(i32),
    Usize(usize),
    F64(f64),
    String(String),
    StringVec(Vec<String>),
    Envelope(Envelope),
    Carrier(Carrier),
    Projection(Projection),
    Structure(Structure),
    Order(Order),
    VisualField(VisualField),
    // Per-stage method choices for the analysis pipeline. Tag-only — the
    // per-variant tunables live as separate `Analysis` params gated by
    // `active_when` predicates.
    Baseline(BaselineKind),
    CycleAverage(CycleAverageKind),
    CycleCombine(CycleCombineKind),
    PhaseSmoothing(PhaseSmoothingKind),
    VfsComputation(VfsComputationKind),
    SignMapSmoothing(SignMapSmoothingKind),
    CortexSource(CortexSourceKind),
    PatchThreshold(PatchThresholdKind),
    PatchExtraction(PatchExtractionKind),
    PatchRefinement(PatchRefinementKind),
    Eccentricity(EccentricityKind),
}

/// Which TOML file a parameter persists to.
///
/// - `Rig`: hardware-specific config (camera, geometry, display, system) →
///   `config/rig.toml`. Properties of the physical rig that don't change
///   between experiments.
/// - `Experiment`: stimulus design and presentation order → loaded from the
///   per-experiment TOML.
/// - `Analysis`: data-processing parameters (phase/VFS smoothing,
///   segmentation thresholds, etc.) → `config/analysis.toml`. Independent
///   of hardware; can vary per dataset.
/// - `UiState`: per-user view/display preferences (e.g., SNR threshold
///   toggles for the figure renderer) — NOT analysis math, NOT persisted
///   into `.oisi` provenance. Lives in the registry for UI binding and
///   change events, but is treated separately by `AnalysisParams` (the
///   macro-generated bridge excludes UiState-target params).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistTarget {
    Rig,
    Experiment,
    Analysis,
    UiState,
}

/// Logical grouping for UI and descriptor queries.
/// Each variant maps 1:1 to a card/section in the frontend.
///
/// `strum::EnumString` with `serialize_all = "snake_case"` derives the
/// `&str → GroupId` parse used by the Tauri descriptor commands, so the
/// string↔variant mapping can never drift from this declaration (adding a
/// variant here automatically extends the parser).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    strum::EnumString,
    strum::Display,
)]
#[strum(serialize_all = "snake_case")]
pub enum GroupId {
    Stimulus,
    Geometry,
    Timing,
    Presentation,
    Retinotopy,
    Camera,
    Display,
    Ring,
    System,
    Paths,
    // Per-stage analysis groups — one card each in the analysis view.
    // Method-choice and tunable params for each stage carry the matching
    // GroupId so the UI groups them together.
    Baseline,
    CycleAverage,
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

impl GroupId {
    /// Human-facing card title for the analysis-view stage sections. The
    /// exhaustive match (no `_`) makes adding a `GroupId` a compile error until
    /// its title is defined — so the UI stage list (built from
    /// [`analysis_stage_groups`] via a Tauri command) can never lack a title.
    pub fn card_title(self) -> &'static str {
        match self {
            GroupId::Baseline => "ΔF/F Baseline",
            GroupId::CycleAverage => "Cycle Average",
            GroupId::CycleCombine => "Cycle Combine",
            GroupId::PhaseSmoothing => "Phase Smoothing",
            GroupId::VfsComputation => "VFS Computation",
            GroupId::SignMapSmoothing => "Sign Map Smoothing",
            GroupId::CortexSource => "Cortex Source",
            GroupId::PatchThreshold => "Patch Threshold",
            GroupId::PatchExtraction => "Patch Extraction",
            GroupId::PatchRefinement => "Patch Refinement",
            GroupId::Eccentricity => "Eccentricity",
            // Non-analysis groups (not rendered as analysis cards) — readable
            // fallback titles rather than a panic.
            GroupId::Stimulus => "Stimulus",
            GroupId::Geometry => "Geometry",
            GroupId::Timing => "Timing",
            GroupId::Presentation => "Presentation",
            GroupId::Retinotopy => "Retinotopy",
            GroupId::Camera => "Camera",
            GroupId::Display => "Display",
            GroupId::Ring => "Ring",
            GroupId::System => "System",
            GroupId::Paths => "Paths",
        }
    }
}

/// The analysis-view pipeline stages, in declaration order, derived from
/// `PARAM_DEFS` (the distinct `Analysis`-persisted [`GroupId`]s in first-
/// appearance order). This is the single source of truth the UI's stage list
/// is built from — adding analysis params under a new `GroupId` makes that
/// stage appear automatically; nothing in the frontend is hand-maintained.
pub fn analysis_stage_groups() -> Vec<GroupId> {
    let mut stages: Vec<GroupId> = Vec::new();
    for def in PARAM_DEFS.iter() {
        if def.persist == PersistTarget::Analysis && !stages.contains(&def.group) {
            stages.push(def.group);
        }
    }
    stages
}

/// Static constraint for validation (Phase 1 — no dynamic constraints yet).
#[derive(Debug, Clone)]
pub enum StaticConstraint {
    None,
    RangeU16(u16, u16),
    RangeU32(u32, u32),
    RangeI32(i32, i32),
    RangeUsize(usize, usize),
    RangeF64(f64, f64),
    MinF64(f64),
    MinU32(u32),
    MinUsize(usize),
}

impl StaticConstraint {
    /// Validate a ParamValue against this constraint. Returns the
    /// `ParamsError::Validation` variant directly — no `Result<_, String>`
    /// dressed-up "internal helper" exemption.
    pub fn validate(&self, value: &ParamValue) -> crate::error::ParamsResult<()> {
        match (self, value) {
            (StaticConstraint::None, _) => Ok(()),

            (StaticConstraint::RangeU16(min, max), ParamValue::U16(v)) => {
                if *v >= *min && *v <= *max {
                    Ok(())
                } else {
                    Err(crate::error::ParamsError::Validation(format!(
                        "value {v} out of range [{min}, {max}]"
                    )))
                }
            }
            (StaticConstraint::RangeU32(min, max), ParamValue::U32(v)) => {
                if *v >= *min && *v <= *max {
                    Ok(())
                } else {
                    Err(crate::error::ParamsError::Validation(format!(
                        "value {v} out of range [{min}, {max}]"
                    )))
                }
            }
            (StaticConstraint::RangeI32(min, max), ParamValue::I32(v)) => {
                if *v >= *min && *v <= *max {
                    Ok(())
                } else {
                    Err(crate::error::ParamsError::Validation(format!(
                        "value {v} out of range [{min}, {max}]"
                    )))
                }
            }
            (StaticConstraint::RangeUsize(min, max), ParamValue::Usize(v)) => {
                if *v >= *min && *v <= *max {
                    Ok(())
                } else {
                    Err(crate::error::ParamsError::Validation(format!(
                        "value {v} out of range [{min}, {max}]"
                    )))
                }
            }
            (StaticConstraint::RangeF64(min, max), ParamValue::F64(v)) => {
                if *v >= *min && *v <= *max {
                    Ok(())
                } else {
                    Err(crate::error::ParamsError::Validation(format!(
                        "value {v} out of range [{min}, {max}]"
                    )))
                }
            }
            (StaticConstraint::MinF64(min), ParamValue::F64(v)) => {
                if *v >= *min {
                    Ok(())
                } else {
                    Err(crate::error::ParamsError::Validation(format!(
                        "value {v} below minimum {min}"
                    )))
                }
            }
            (StaticConstraint::MinU32(min), ParamValue::U32(v)) => {
                if *v >= *min {
                    Ok(())
                } else {
                    Err(crate::error::ParamsError::Validation(format!(
                        "value {v} below minimum {min}"
                    )))
                }
            }
            (StaticConstraint::MinUsize(min), ParamValue::Usize(v)) => {
                if *v >= *min {
                    Ok(())
                } else {
                    Err(crate::error::ParamsError::Validation(format!(
                        "value {v} below minimum {min}"
                    )))
                }
            }

            _ => Ok(()), // type mismatch = no constraint (enum values, strings, etc.)
        }
    }
}

/// Static definition of a parameter (one per parameter, lives in PARAM_DEFS).
pub struct ParamDef {
    pub id: ParamId,
    pub label: &'static str,
    pub unit: &'static str,
    pub group: GroupId,
    pub toml_path: &'static str,
    pub persist: PersistTarget,
    pub default: ParamValue,
    pub constraint: StaticConstraint,
    /// If Some, this parameter is only active when the function returns true.
    /// Inactive parameters are hidden in the UI. None = always active.
    pub active_when: Option<fn(&Registry) -> bool>,
}

// Re-export Phase 2 types for convenience.
pub use hardware::HardwareContext;
pub use snapshot::RegistrySnapshot;

/// Metadata for a saved experiment file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExperimentMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
}

#[cfg(test)]
mod group_id_tests {
    use super::GroupId;
    use std::str::FromStr;

    /// Pin the `strum` snake_case derive that the Tauri descriptor commands rely
    /// on to route param-group queries — especially the multi-word variants,
    /// where a wrong conversion would silently make a stage's UI card empty.
    #[test]
    fn group_id_from_str_snake_case() {
        assert_eq!(GroupId::from_str("baseline"), Ok(GroupId::Baseline));
        assert_eq!(GroupId::from_str("cycle_average"), Ok(GroupId::CycleAverage));
        assert_eq!(
            GroupId::from_str("vfs_computation"),
            Ok(GroupId::VfsComputation)
        );
        assert_eq!(
            GroupId::from_str("sign_map_smoothing"),
            Ok(GroupId::SignMapSmoothing)
        );
        assert_eq!(
            GroupId::from_str("patch_threshold"),
            Ok(GroupId::PatchThreshold)
        );
        assert_eq!(GroupId::from_str("cortex_source"), Ok(GroupId::CortexSource));
        // PascalCase and unknown keys must NOT parse.
        assert!(GroupId::from_str("CycleAverage").is_err());
        assert!(GroupId::from_str("not_a_group").is_err());
    }

    /// Pin the PARAM_DEFS-derived stage list that the UI's analysis-view cards
    /// are built from (via the `get_analysis_stages` Tauri command). Order and
    /// membership must be exactly the 11 pipeline stages; every stage must have
    /// a non-empty title and a `from_str`-round-trippable snake_case key.
    #[test]
    fn analysis_stage_groups_are_the_eleven_pipeline_stages_in_order() {
        use super::analysis_stage_groups;
        let expected = [
            GroupId::Baseline,
            GroupId::CycleAverage,
            GroupId::CycleCombine,
            GroupId::PhaseSmoothing,
            GroupId::VfsComputation,
            GroupId::SignMapSmoothing,
            GroupId::CortexSource,
            GroupId::PatchThreshold,
            GroupId::PatchExtraction,
            GroupId::PatchRefinement,
            GroupId::Eccentricity,
        ];
        assert_eq!(analysis_stage_groups(), expected);
        for g in analysis_stage_groups() {
            assert!(!g.card_title().is_empty(), "{g:?} has no card title");
            assert_eq!(
                GroupId::from_str(&g.to_string()),
                Ok(g),
                "stage key must round-trip: {g:?}"
            );
        }
    }
}
