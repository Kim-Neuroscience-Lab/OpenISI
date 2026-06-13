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
    BaselineMethod, CortexSourceMethod, CycleAverageMethod, CycleCombineMethod, EccentricityMethod,
    PatchExtractionMethod, PatchRefinementMethod, PatchThresholdMethod, PhaseSmoothingMethod,
    SignMapSmoothingMethod, SplitMergeParams, VfsComputationMethod,
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
    let baseline = match snap.typed::<p::BaselineMethod>().into_inner() {
        p::BaselineKind::AllenAllFrameMean => BaselineMethod::allen_all_frame_mean(),
        p::BaselineKind::AllenAllFrameMedian => BaselineMethod::allen_all_frame_median(),
        p::BaselineKind::OpenIsiInterSweepMean => BaselineMethod::open_isi_inter_sweep_mean(),
        p::BaselineKind::OpenIsiInterSweepMedian => BaselineMethod::open_isi_inter_sweep_median(),
    };

    let cycle_average = match snap.typed::<p::CycleAverageMethod>().into_inner() {
        p::CycleAverageKind::SimpleComplexAverage => CycleAverageMethod::simple_complex_average(),
        p::CycleAverageKind::PhaseLockedAverage => CycleAverageMethod::phase_locked_average(),
    };

    let cycle_combine = match snap.typed::<p::CycleCombineMethod>().into_inner() {
        p::CycleCombineKind::KalatskyStryker2003DelaySubtraction => {
            CycleCombineMethod::kalatsky_stryker2003_delay_subtraction()
        }
        p::CycleCombineKind::UnweightedCycleAverage => {
            CycleCombineMethod::unweighted_cycle_average()
        }
    };

    let phase_smoothing = match snap.typed::<p::PhaseSmoothingMethod>().into_inner() {
        p::PhaseSmoothingKind::SnlcAmpWeightedPhasor => {
            PhaseSmoothingMethod::snlc_amp_weighted_phasor(
                snap.typed::<p::PhaseSmoothingSnlcAmpWeightedPhasorSigmaPx>(),
            )
        }
        p::PhaseSmoothingKind::AllenZhuang2017PositionGaussian => {
            PhaseSmoothingMethod::allen_zhuang2017_position_gaussian(
                snap.typed::<p::PhaseSmoothingAllenZhuang2017PositionGaussianSigmaPx>(),
            )
        }
    };

    let vfs_computation = match snap.typed::<p::VfsComputationMethod>().into_inner() {
        p::VfsComputationKind::OpenIsiChainRulePhasorGradient => {
            VfsComputationMethod::open_isi_chain_rule_phasor_gradient()
        }
    };

    let sign_map_smoothing = match snap.typed::<p::SignMapSmoothingMethod>().into_inner() {
        p::SignMapSmoothingKind::Gaussian => {
            SignMapSmoothingMethod::gaussian(snap.typed::<p::SignMapSmoothingGaussianSigmaUm>())
        }
    };

    let cortex_source = match snap.typed::<p::CortexSourceMethod>().into_inner() {
        CortexSourceKind::Reliability => {
            CortexSourceMethod::reliability(snap.typed::<p::CortexSourceReliabilityThreshold>())
        }
        CortexSourceKind::UserPolygon => CortexSourceMethod::user_polygon(),
        CortexSourceKind::SnlcGarrett2014ImBound => CortexSourceMethod::snlc_garrett2014_im_bound(
            snap.typed::<p::CortexSourceSnlcK>(),
            snap.typed::<p::CortexSourceSnlcClose>(),
            snap.typed::<p::CortexSourceSnlcDilate>(),
        ),
        CortexSourceKind::NoRestriction => CortexSourceMethod::no_restriction(),
    };

    let patch_threshold = match snap.typed::<p::PatchThresholdMethod>().into_inner() {
        PatchThresholdKind::AllenZhuang2017FixedSignMapThr => {
            PatchThresholdMethod::allen_zhuang2017_fixed_sign_map_thr(
                snap.typed::<p::PatchThresholdAllenValue>(),
            )
        }
        PatchThresholdKind::Garrett2014SigmaScaled => {
            PatchThresholdMethod::garrett2014_sigma_scaled(
                snap.typed::<p::PatchThresholdGarrettK>(),
            )
        }
    };

    let patch_extraction = match snap.typed::<p::PatchExtractionMethod>().into_inner() {
        p::PatchExtractionKind::AllenZhuang2017LabelOpenCloseDilate => {
            PatchExtractionMethod::allen_zhuang2017_label_open_close_dilate(
                snap.typed::<p::PatchExtractionAllenOpenIter>(),
                snap.typed::<p::PatchExtractionAllenCloseIter>(),
                snap.typed::<p::PatchExtractionAllenDilationIter>(),
                snap.typed::<p::PatchExtractionAllenBorderWidth>(),
                snap.typed::<p::PatchExtractionAllenSmallPatchThr>(),
            )
        }
    };

    let patch_refinement = match snap.typed::<p::PatchRefinementMethod>().into_inner() {
        PatchRefinementKind::None => PatchRefinementMethod::none(),
        PatchRefinementKind::AllenZhuang2017SplitMerge => {
            PatchRefinementMethod::allen_zhuang2017_split_merge(SplitMergeParams {
                split_overlap_thr: snap.typed::<p::PatchRefinementAllenSplitOverlapThr>(),
                split_local_min_cut_step: snap.typed::<p::PatchRefinementAllenSplitLocalMinCutStep>(),
                merge_overlap_thr: snap.typed::<p::PatchRefinementAllenMergeOverlapThr>(),
                visual_space_pixel_size: snap.typed::<p::PatchRefinementAllenVisualSpacePixelSize>(),
                visual_space_close_iter: snap.typed::<p::PatchRefinementAllenVisualSpaceCloseIter>(),
                ecc_map_filter_sigma: snap.typed::<p::PatchRefinementAllenEccMapFilterSigma>(),
                border_width: snap.typed::<p::PatchRefinementAllenBorderWidth>(),
                small_patch_thr: snap.typed::<p::PatchRefinementAllenSmallPatchThr>(),
            })
        }
    };

    let eccentricity = match snap.typed::<p::EccentricityMethod>().into_inner() {
        p::EccentricityKind::OpenIsiWholeCortexV1 => EccentricityMethod::open_isi_whole_cortex_v1(),
        p::EccentricityKind::SnlcGetAreaBordersV1Center => {
            EccentricityMethod::snlc_get_area_borders_v1_center()
        }
    };

    AnalysisParams::new(
        baseline,
        cycle_average,
        cycle_combine,
        phase_smoothing,
        vfs_computation,
        sign_map_smoothing,
        cortex_source,
        patch_threshold,
        patch_extraction,
        patch_refinement,
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

    /// The drift guard for `config/analysis.toml`. The shipped config is this
    /// lab's deliberately-tuned working state (it OVERRIDES PARAM_DEFS, by
    /// design — so we do NOT assert it equals the defaults). What must hold is
    /// that it still loads and bridges to a valid `AnalysisParams` end-to-end:
    /// a method/param rename that left a stale key in the shipped config would
    /// fail here (fail-loud load / unknown-key), instead of silently degrading
    /// a real analysis run. This is the production load path.
    #[test]
    fn shipped_analysis_config_loads_and_bridges() {
        let config = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config");
        let mut reg = Registry::new(&config, &config);
        reg.load_analysis()
            .expect("shipped config/analysis.toml must load cleanly (no stale/unknown keys)");
        let _ap = analysis_params_from_snapshot(&reg.snapshot());
    }
}
