//! Per-stage method strategies for the OpenISI retinotopy analysis pipeline.
//!
//! Each stage in the pipeline is represented as a tagged enum (`#[serde(tag = "method")]`)
//! whose variants are the published methods for that stage. The orchestrator
//! dispatches by variant; each variant carries its own parameters. New methods
//! are added as new variants without changing the orchestrator's call sites.
//!
//! **Attribution standard.** Every method variant whose name includes an
//! author / year / package attribution (e.g. `KalatskyStryker2003DelaySubtraction`,
//! `Garrett2014SigmaScaled`) must cite its source in the docstring: paper
//! reference (author year, journal vol:page) plus source-code file and line
//! range when applicable. Variants without an attribution (e.g.
//! `UserPolygon`, `NoRestriction`, `None`) must explain why no citation
//! applies — the variant is named for what it does (not what it cites).
//!
//! A citation-audit test (`tests/method_attribution_audit.rs`) enforces this.
//!
//! **What this module is NOT.** Stages whose math is universal across the
//! published literature (per-cycle DFT, cycle-internal phase-locked
//! averaging, connected-component labeling, Jacobian magnification) remain
//! as plain functions in `compute` / `math` / `segmentation::connectivity`.
//! They graduate to method enums only when a published alternative routinely
//! competes.
//!
//! **Where each stage is validated.** Several method modules carry no
//! in-module unit tests *by design* — they are thin dispatchers over a
//! golden-validated compute/math primitive, and re-testing the dispatch would
//! be redundant. The real coverage lives at the primitive or end-to-end level:
//! - `baseline` → `tests/regression_oisi.rs` (real-data from-raw) + `equivalence.rs`.
//! - `cycle_combine` → `golden_vfs::kalatsky_combine_matches_snlc_gprocesskret`.
//! - `cycle_average` → its own goldens (faithful default + phase-lock property).
//! - `vfs_computation` → `golden_vfs::vfs_matches_allen_visual_sign_map_*`.
//! - `eccentricity` → `golden_vfs::garrett_eccentricity_*`, `golden_cortex_morph::compute_eccentricity_snlc_*`, the `v1ecc_*` goldens.
//! - `patch_extraction` → `golden_cortex_morph::allen_raw_patch_map_matches_scipy` + `equivalence.rs`.
//!
//! Stages with their OWN bespoke logic (cortex_source, phase_smoothing,
//! patch_threshold, patch_refinement, sign_map_smoothing) keep in-module goldens.

pub mod baseline;
pub mod cortex_source;
pub mod cycle_average;
pub mod cycle_combine;
pub mod eccentricity;
pub mod patch_extraction;
pub mod patch_refinement;
pub mod patch_threshold;
pub mod phase_smoothing;
pub mod sign_map_smoothing;
pub mod vfs_computation;

pub use baseline::{BaselineExt, BaselineMethod, BaselineResult};
pub use cortex_source::{CortexResolveContext, CortexSourceExt, CortexSourceMethod};
pub use cycle_average::{CycleAverageExt, CycleAverageMethod};
pub use cycle_combine::{CycleCombineExt, CycleCombineMethod};
pub use eccentricity::{EccentricityExt, EccentricityMethod};
pub use patch_extraction::{PatchExtractionExt, PatchExtractionMethod};
pub use patch_refinement::{PatchRefinementExt, PatchRefinementMethod};
pub use patch_threshold::{PatchThresholdExt, PatchThresholdMethod};
pub use phase_smoothing::{PhaseSmoothingExt, PhaseSmoothingMethod};
pub use sign_map_smoothing::{SignMapSmoothingExt, SignMapSmoothingMethod};
pub use vfs_computation::{VfsComputationExt, VfsComputationMethod};
