//! Analysis parameters.
//!
//! `AnalysisParams` — algorithmic choices (per-stage method enums).
//! Serialized into `.oisi /analysis_params` so every analyzed file
//! records exactly which methods (and their parameters) produced its data.
//!
//! The capture-time facts (`AcquisitionProperties` + `ProvenanceLevel`,
//! read from `.oisi`'s `/rig_params` + `/experiment_params`) are NOT
//! algorithm choices — they describe the *format*, so they live in the
//! `oisi` crate (re-exported from the crate root).

use crate::methods::{
    BaselineMethod, CortexSourceMethod, CycleAverageMethod, CycleCombineMethod,
    DirectionSmoothingMethod, EccentricityMethod, PatchExtractionMethod, PatchRefinementMethod,
    PatchThresholdMethod, PhaseSmoothingMethod, RectificationMethod, ResponseNormalizationMethod,
    SignMapSmoothingMethod, VfsComputationMethod,
};

// ---------------------------------------------------------------------------
// AnalysisParams — algorithm choices
// ---------------------------------------------------------------------------


/// Analysis parameters: per-stage method choices. Every analyzed
/// `.oisi` file records the exact `AnalysisParams` that produced its
/// results, so re-analysis is bit-reproducible.
///
/// Acquisition properties (stimulus geometry, camera calibration) live
/// in [`AcquisitionProperties`] and are recorded via
/// `/rig_params` + `/experiment_params` at capture time.
///
/// **Strict schema, enforced at reconstruction (not via serde on this
/// struct).** The on-disk form is the tagged-`AnalysisConfig` JSON in the
/// `.oisi` `/analysis_params` attribute, reloaded through
/// [`crate::bridge::analysis_params_from_oisi_tree`], which is fail-loud:
/// a tree that doesn't deserialize as the current `AnalysisConfig` returns
/// an error — corrupted or legacy files do NOT silently load with code-default
/// values. The orchestrator catches that error and surfaces a clean
/// "schema mismatch — re-run analysis" message; the pre-2026 migration
/// path (`is_pre_2026_analysis_params`) handles known schema drift
/// distinctly, upstream of reconstruction.
///
/// **No `Default` impl, no serde derives.** `AnalysisParams` is a
/// runtime-only struct — its on-disk form lives in the `.oisi` HDF5
/// attr as the tagged-`AnalysisConfig` JSON (produced by
/// `serde_json::to_value(&AnalysisConfig)`), not as serde-derived JSON of this
/// struct. The only construction path is `Self::new(...)` below, called from the
/// `bridge` adapters. `#[non_exhaustive]` prevents struct-literal construction
/// from outside this crate.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct AnalysisParams {
    /// Stage 0: ΔF/F baseline (`F0` denominator for the bin-1 DFT).
    pub baseline: BaselineMethod,
    /// Stage 0b: response normalization (fractional ΔF/F vs absolute ΔF).
    pub response_normalization: ResponseNormalizationMethod,
    /// Projection: cycle averaging (combine the K per-cycle complex maps).
    pub cycle_average: CycleAverageMethod,
    /// Projection: optional pre-DFT half-wave rectification (Allen isRectify).
    pub rectification: RectificationMethod,
    /// Stage 1a: optional pre-combine per-direction smoothing (SNLC adaptive).
    pub direction_smoothing: DirectionSmoothingMethod,
    /// Stage 1: cycle combination (fwd+rev → position phasor).
    pub cycle_combine: CycleCombineMethod,
    /// Stage 2: position phasor smoothing.
    pub phase_smoothing: PhaseSmoothingMethod,
    /// Stage 3: visual field sign computation.
    pub vfs_computation: VfsComputationMethod,
    /// Stage 4: sign map smoothing.
    pub sign_map_smoothing: SignMapSmoothingMethod,
    /// Stage 5: cortex / ROI source.
    pub cortex_source: CortexSourceMethod,
    /// Stage 6: patch threshold (which pixels become patch candidates).
    pub patch_threshold: PatchThresholdMethod,
    /// Stage 7: patch extraction (label → smooth → assign signs).
    pub patch_extraction: PatchExtractionMethod,
    /// Stage 8: patch refinement (split + merge).
    pub patch_refinement: PatchRefinementMethod,
    /// Stage 10: eccentricity map computation.
    pub eccentricity: EccentricityMethod,
}

impl AnalysisParams {
    /// Construct an `AnalysisParams` from already-built method enums.
    /// The `bridge` adapters (`From<&AnalysisConfig>`) are the only production
    /// callers; the method enums ARE the typed config's tagged enums, so every
    /// value in the result provably came from the canonical SSoT config.
    // Justified `#[allow]`, not a parameter object: the 11 arguments ARE the
    // struct's fields (no smaller cohesive concept to extract), and each is a
    // distinct method-enum type, so a positional swap is a compile error — the
    // exact mistake the lint guards against can't occur here.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        baseline: BaselineMethod,
        response_normalization: ResponseNormalizationMethod,
        cycle_average: CycleAverageMethod,
        rectification: RectificationMethod,
        direction_smoothing: DirectionSmoothingMethod,
        cycle_combine: CycleCombineMethod,
        phase_smoothing: PhaseSmoothingMethod,
        vfs_computation: VfsComputationMethod,
        sign_map_smoothing: SignMapSmoothingMethod,
        cortex_source: CortexSourceMethod,
        patch_threshold: PatchThresholdMethod,
        patch_extraction: PatchExtractionMethod,
        patch_refinement: PatchRefinementMethod,
        eccentricity: EccentricityMethod,
    ) -> Self {
        Self {
            baseline,
            response_normalization,
            cycle_average,
            rectification,
            direction_smoothing,
            cycle_combine,
            phase_smoothing,
            vfs_computation,
            sign_map_smoothing,
            cortex_source,
            patch_threshold,
            patch_extraction,
            patch_refinement,
            eccentricity,
        }
    }
}
