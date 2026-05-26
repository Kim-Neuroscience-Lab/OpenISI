//! Registry-snapshot → `AnalysisParams` bridge.
//!
//! This module is the ONLY place that knows both the `openisi-params`
//! marker types and the `crates/isi-analysis` method enums. Every
//! value entering an `AnalysisParams` flows through one of the typed
//! `snap.typed::<P>()` calls below, which structurally proves the
//! value came from the SSoT param registry (no inline literals).
//!
//! Adding a parameter:
//! 1. Add a `define_params!` entry in `definitions.rs`. The macro
//!    emits the marker type automatically.
//! 2. Add the field to the appropriate method enum variant.
//! 3. Thread the new marker into the variant's constructor signature
//!    (in `methods/<stage>.rs`).
//! 4. Add the corresponding `snap.typed::<NewMarker>()` argument here.
//!
//! Steps 3 and 4 are mechanically enforced — the constructor's
//! argument list and the bridge call must agree on the markers, or
//! the compiler rejects the build.

use openisi_params as p;
use openisi_params::{CortexSourceKind, PatchRefinementKind, PatchThresholdKind, RegistrySnapshot};

use crate::methods::{
    CortexSource, CycleCombineMethod, EccentricityMethod, PatchExtractionMethod,
    PatchRefinementMethod, PatchThresholdMethod, PhaseSmoothingMethod, QualityGateMethod,
    SignMapSmoothingMethod, VfsComputationMethod,
};
use crate::params::AnalysisParams;

/// Build a fully-populated `AnalysisParams` from a `RegistrySnapshot`.
///
/// The implementation is forced by the type system into the shape
/// `Method::variant(snap.typed::<Marker>())` — there is no other
/// expression the constructors accept. If you add a parameter or a
/// new method variant, the compiler will tell you exactly which line
/// to update.
pub fn analysis_params_from_snapshot(snap: &RegistrySnapshot) -> AnalysisParams {
    let cycle_combine = match snap.typed::<p::CycleCombineMethod>().into_inner() {
        p::CycleCombineKind::MarshelGarrett2011DelaySubtraction =>
            CycleCombineMethod::marshel_garrett2011_delay_subtraction(),
        p::CycleCombineKind::KalatskyStryker2003RawAverage =>
            CycleCombineMethod::kalatsky_stryker2003_raw_average(),
    };

    let phase_smoothing = match snap.typed::<p::PhaseSmoothingMethod>().into_inner() {
        p::PhaseSmoothingKind::OpenIsiAmpWeightedPhasor =>
            PhaseSmoothingMethod::open_isi_amp_weighted_phasor(
                snap.typed::<p::PhaseSmoothingOpenIsiAmpWeightedPhasorSigmaPx>(),
            ),
    };

    let vfs_computation = match snap.typed::<p::VfsComputationMethod>().into_inner() {
        p::VfsComputationKind::OpenIsiChainRulePhasorGradient =>
            VfsComputationMethod::open_isi_chain_rule_phasor_gradient(),
    };

    let sign_map_smoothing = match snap.typed::<p::SignMapSmoothingMethod>().into_inner() {
        p::SignMapSmoothingKind::Gaussian =>
            SignMapSmoothingMethod::gaussian(
                snap.typed::<p::SignMapSmoothingGaussianSigmaUm>(),
            ),
    };

    let cortex_source = match snap.typed::<p::CortexSourceMethod>().into_inner() {
        CortexSourceKind::Reliability =>
            CortexSource::reliability(snap.typed::<p::CortexSourceReliabilityThreshold>()),
        CortexSourceKind::UserPolygon =>
            CortexSource::user_polygon(),
        CortexSourceKind::SnlcGarrett2014ImBound =>
            CortexSource::snlc_garrett2014_im_bound(
                snap.typed::<p::CortexSourceSnlcK>(),
                snap.typed::<p::CortexSourceSnlcClose>(),
                snap.typed::<p::CortexSourceSnlcDilate>(),
            ),
        CortexSourceKind::AllenZhuang2017FullFrame =>
            CortexSource::allen_zhuang2017_full_frame(),
    };

    let patch_threshold = match snap.typed::<p::PatchThresholdMethod>().into_inner() {
        PatchThresholdKind::AllenZhuang2017FixedSignMapThr =>
            PatchThresholdMethod::allen_zhuang2017_fixed_sign_map_thr(
                snap.typed::<p::PatchThresholdAllenValue>(),
            ),
        PatchThresholdKind::Garrett2014SigmaScaled =>
            PatchThresholdMethod::garrett2014_sigma_scaled(
                snap.typed::<p::PatchThresholdGarrettK>(),
            ),
    };

    let patch_extraction = match snap.typed::<p::PatchExtractionMethod>().into_inner() {
        p::PatchExtractionKind::AllenZhuang2017LabelOpenCloseDilate =>
            PatchExtractionMethod::allen_zhuang2017_label_open_close_dilate(
                snap.typed::<p::PatchExtractionAllenOpenIter>(),
                snap.typed::<p::PatchExtractionAllenCloseIter>(),
                snap.typed::<p::PatchExtractionAllenDilationIter>(),
                snap.typed::<p::PatchExtractionAllenBorderWidth>(),
                snap.typed::<p::PatchExtractionAllenSmallPatchThr>(),
            ),
    };

    let patch_refinement = match snap.typed::<p::PatchRefinementMethod>().into_inner() {
        PatchRefinementKind::None => PatchRefinementMethod::none(),
        PatchRefinementKind::AllenZhuang2017SplitMerge =>
            PatchRefinementMethod::allen_zhuang2017_split_merge(
                snap.typed::<p::PatchRefinementAllenSplitOverlapThr>(),
                snap.typed::<p::PatchRefinementAllenSplitLocalMinCutStep>(),
                snap.typed::<p::PatchRefinementAllenMergeOverlapThr>(),
                snap.typed::<p::PatchRefinementAllenVisualSpacePixelSize>(),
                snap.typed::<p::PatchRefinementAllenVisualSpaceCloseIter>(),
                snap.typed::<p::PatchRefinementAllenEccMapFilterSigma>(),
                snap.typed::<p::PatchRefinementAllenBorderWidth>(),
                snap.typed::<p::PatchRefinementAllenSmallPatchThr>(),
            ),
    };

    let quality_gate = match snap.typed::<p::QualityGateMethod>().into_inner() {
        p::QualityGateKind::None => QualityGateMethod::none(),
    };

    let eccentricity = match snap.typed::<p::EccentricityMethod>().into_inner() {
        p::EccentricityKind::Garrett2014WholeCortexV1 =>
            EccentricityMethod::garrett2014_whole_cortex_v1(),
    };

    AnalysisParams::new(
        cycle_combine,
        phase_smoothing,
        vfs_computation,
        sign_map_smoothing,
        cortex_source,
        patch_threshold,
        patch_extraction,
        patch_refinement,
        quality_gate,
        eccentricity,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use openisi_params::Registry;

    /// Default registry → bridge → AnalysisParams produces a value with
    /// every stage populated from PARAM_DEFS defaults. Schema-coverage:
    /// if a new method variant exists in PARAM_DEFS but its constructor
    /// isn't wired into the bridge, this test fails to compile.
    #[test]
    fn default_registry_roundtrips() {
        let dir = std::path::Path::new("/tmp/openisi-bridge-test");
        let reg = Registry::new(dir, dir);
        let snap = reg.snapshot();
        let _ap = analysis_params_from_snapshot(&snap);
        // Constructor success is the assertion — if any stage failed to
        // type-check (e.g., a variant added without bridge wiring), we
        // wouldn't reach here.
    }
}
