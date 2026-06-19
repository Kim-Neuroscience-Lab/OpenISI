//! Typed configuration — single source of truth for all configuration.
//!
//! Every parameter lives in a typed serde struct under [`config`]
//! (`RigConfig`/`ExperimentConfig`/`AnalysisConfig`/`UiStateConfig`): serde owns
//! (de)serialization to JSON, schemars derives the UI/validation schema, garde
//! validates the static bounds, and the [`config::ConfigStore`] owns the live
//! values + the dynamic hardware constraints. The analysis pipeline's per-stage
//! method enums (`config::analysis`) are the *canonical* method types, shared
//! directly with `crates/isi-analysis` (no bridge marker types).
//!
//! This crate is depended on by both `src-tauri` (Tauri-shell wiring, IPC
//! commands) and `crates/isi-analysis`. It owns no Tauri, ndarray, hdf5, or tch
//! dependencies — only serde + schemars + garde.

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
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        Serialize,
        Deserialize,
        schemars::JsonSchema,
        strum::Display,
        strum::EnumIter,
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
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        Serialize,
        Deserialize,
        schemars::JsonSchema,
        strum::Display,
        strum::EnumIter,
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
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        Serialize,
        Deserialize,
        schemars::JsonSchema,
        strum::Display,
        strum::EnumIter,
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

pub mod config;

pub mod analysis_kinds;
pub mod hardware;
pub mod labels;
pub mod observer;
// `commands.rs` (Tauri IPC commands) lives in src-tauri, not here.

// The tag-only per-stage method-choice enums (with their strum UI labels) drive
// the analysis-view dropdowns via the descriptor layer; consumers reference them
// as `openisi_params::CortexSourceKind`, etc.
pub use analysis_kinds::{
    BaselineKind, CortexSourceKind, CycleAverageKind, CycleCombineKind, DirectionSmoothingKind,
    EccentricityKind, PatchExtractionKind, PatchRefinementKind, PatchThresholdKind,
    PhaseSmoothingKind, RectificationKind, ResponseNormalizationKind, SignMapSmoothingKind,
    VfsComputationKind,
};
pub use labels::{enum_options, EnumOption};
pub use observer::ParamChangeObserver;

// ─── Core types ───────────────────────────────────────────────────────────────

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

/// The analysis-view pipeline stages, in pipeline order. This is the single
/// source of truth the UI's stage list is built from (via the
/// `get_analysis_stages` Tauri command); it mirrors the field order of
/// [`config::AnalysisConfig`]. The `analysis_stage_groups_*` test pins it to the
/// canonical 11 stages so a stage can't silently go missing from the UI.
pub fn analysis_stage_groups() -> Vec<GroupId> {
    vec![
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
    ]
}

pub use hardware::HardwareContext;

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

    /// Pin the stage list that the UI's analysis-view cards are built from (via
    /// the `get_analysis_stages` Tauri command). Order and membership must be
    /// exactly the 11 pipeline stages; every stage must have a non-empty title
    /// and a `from_str`-round-trippable snake_case key.
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
