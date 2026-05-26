//! Per-stage method strategies for the OpenISI retinotopy analysis pipeline.
//!
//! Each stage in the pipeline is represented as a tagged enum (`#[serde(tag = "method")]`)
//! whose variants are the published methods for that stage. The orchestrator
//! dispatches by variant; each variant carries its own parameters. New methods
//! are added as new variants without changing the orchestrator's call sites.
//!
//! **Attribution standard.** Every method variant whose name includes an
//! author / year / package attribution (e.g. `AllenZhuang2017FullFrame`,
//! `Garrett2014SigmaScaled`) must cite its source in the docstring: paper
//! reference (author year, journal vol:page) plus source-code file and line
//! range when applicable. Variants without an attribution (e.g.
//! `UserPolygon`, `FullFrame`, `None`) must explain why no citation applies.
//!
//! A citation-audit test (`tests/method_attribution_audit.rs`) enforces this.
//!
//! **What this module is NOT.** Stages whose math is universal across the
//! published literature (per-cycle DFT, cycle-internal phase-locked
//! averaging, connected-component labeling, Jacobian magnification) remain
//! as plain functions in `compute` / `math` / `segmentation::connectivity`.
//! They graduate to method enums only when a published alternative routinely
//! competes.

pub mod cortex_source;
pub mod cycle_combine;
pub mod eccentricity;
pub mod patch_extraction;
pub mod patch_refinement;
pub mod patch_threshold;
pub mod phase_smoothing;
pub mod quality_gate;
pub mod sign_map_smoothing;
pub mod vfs_computation;

pub use cortex_source::CortexSource;
pub use cycle_combine::CycleCombineMethod;
pub use eccentricity::EccentricityMethod;
pub use patch_extraction::PatchExtractionMethod;
pub use patch_refinement::PatchRefinementMethod;
pub use patch_threshold::PatchThresholdMethod;
pub use phase_smoothing::PhaseSmoothingMethod;
pub use quality_gate::QualityGateMethod;
pub use sign_map_smoothing::SignMapSmoothingMethod;
pub use vfs_computation::VfsComputationMethod;
