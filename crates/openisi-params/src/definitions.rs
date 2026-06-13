//! Parameter definitions — THE single source of truth for all ~70 parameters.
//!
//! One `define_params!` invocation generates:
//! - `ParamId` enum
//! - `PARAM_DEFS` static table
//! - Typed getters/setters on `Registry`

use super::registry::Registry;
use super::{
    BaselineKind, Carrier, CortexSourceKind, CycleAverageKind, CycleCombineKind, EccentricityKind,
    Envelope, GroupId, Order, ParamDef, ParamValue, PatchExtractionKind, PatchRefinementKind,
    PatchThresholdKind, PersistTarget, PhaseSmoothingKind, Projection, SignMapSmoothingKind,
    StaticConstraint, Structure, VfsComputationKind, VisualField,
};

define_params! {
    // ═══════════════════════════════════════════════════════════════════
    // Rig parameters (rig.toml)
    // ═══════════════════════════════════════════════════════════════════

    // ── Camera ────────────────────────────────────────────────────────
    CameraExposureUs: U32 = 1000,
        "camera.exposure_us", Rig, Camera,
        "Exposure", "\u{00b5}s", StaticConstraint::RangeU32(1, 1_000_000);

    CameraBinning: U16 = 4,
        "camera.binning", Rig, Camera,
        "Binning", "x", StaticConstraint::RangeU16(1, 16);

    // Spatial calibration of the cortex camera (µm/pixel). Hardware fact;
    // recorded into `.oisi /rig_params` at capture so re-analysis on a
    // different machine uses the original rig's calibration. Default
    // 20 µm/px matches typical mouse-cortex imaging (Allen Brain
    // Observatory calibration).
    CameraUmPerPixel: F64 = 20.0,
        "camera.um_per_pixel", Rig, Camera,
        "Camera Pixel Size", "\u{00b5}m/px", StaticConstraint::MinF64(0.001);

    // ── Display / Rig Geometry ──────────────────────────────────────────
    //
    // Rig geometry is calibrated per physical setup. Defaults reflect the
    // Zhuang et al. 2017 (eLife) / `retinotopic_mapping` reference rig: a
    // 40-inch LED TV ~88×50 cm, 13.5 cm viewing distance, monitor yawed
    // 30° inward (mouse midline at 30° to monitor plane), bisector intercept
    // at the geometric center until measured otherwise, right hemifield.
    // These are starting values — actual rigs MUST override in rig.toml.
    //
    // Marshel & Garrett 2011 (Neuron) uses 10 cm distance and 20° yaw on a
    // portrait 68×121 cm panel; either set of defaults is canonical. We
    // ship Zhuang because the analysis pipeline already cites Zhuang.

    ViewingDistanceCm: F64 = 10.0,
        "geometry.viewing_distance_cm", Rig, Display,
        "Viewing Distance", "cm", StaticConstraint::MinF64(0.1);

    // Calibrated panel size (cm). EDID values from the OS are unreliable
    // and not used as the ground truth — the rig measurement is.
    MonitorWidthCm: F64 = 88.0,
        "geometry.monitor_width_cm", Rig, Display,
        "Monitor Width", "cm", StaticConstraint::MinF64(0.1);

    MonitorHeightCm: F64 = 50.0,
        "geometry.monitor_height_cm", Rig, Display,
        "Monitor Height", "cm", StaticConstraint::MinF64(0.1);

    // Bisector intercept on the monitor face — where the eye's perpendicular
    // ray to the monitor lands, measured in cm from the monitor geometric
    // center. + X = toward anterior (nose), + Y = toward top. Marshel
    // places the intercept ~28 cm up from a 121-cm portrait monitor's
    // bottom (i.e. ~32 cm below center); Zhuang's `MonitorSetup.Monitor`
    // models the same via asymmetric `C2T_cm`/`C2A_cm`. Default 0 cm
    // means "eye looks at monitor geometric center" — only correct for
    // bench-test setups; calibrate per rig.
    BisectorXCm: F64 = 0.0,
        "geometry.bisector_x_cm", Rig, Display,
        "Bisector X Intercept", "cm", StaticConstraint::None;

    BisectorYCm: F64 = 0.0,
        "geometry.bisector_y_cm", Rig, Display,
        "Bisector Y Intercept", "cm", StaticConstraint::None;

    // Physical yaw of the monitor around its vertical axis, in degrees
    // inward toward the mouse nose. Marshel 2011 = 20°, Juavinett 2017 /
    // Zhuang 2017 = 30°. Compensates for the lateral position of the
    // mouse eye so the monitor plane is approximately parallel to the
    // retina.
    MonitorYawDeg: F64 = 30.0,
        "geometry.monitor_yaw_deg", Rig, Display,
        "Monitor Yaw", "\u{00b0}", StaticConstraint::RangeF64(-90.0, 90.0);

    // Physical pitch of the monitor around its horizontal axis, in degrees
    // tipping the top edge toward the mouse. Juavinett 2017 uses ~20° pitch
    // when the headframe is rotated to access lateral cortex; otherwise 0.
    MonitorPitchDeg: F64 = 0.0,
        "geometry.monitor_pitch_deg", Rig, Display,
        "Monitor Pitch", "\u{00b0}", StaticConstraint::RangeF64(-90.0, 90.0);

    // Which hemifield the stimulus monitor occupies. Marshel/Garrett/Zhuang
    // all stimulate the right hemifield by convention (mouse left eye
    // occluded for contralateral cortex imaging). Mirrors the
    // `visual_field` discriminator in Zhuang's `MonitorSetup.Monitor`.
    StimulusVisualField: VisualField = VisualField::Right,
        "geometry.visual_field", Rig, Display,
        "Visual Field", "", StaticConstraint::None;

    // ── Ring Overlay ──────────────────────────────────────────────────
    RingOverlayEnabled: Bool = false,
        "ring_overlay.enabled", Rig, Ring,
        "Enabled", "", StaticConstraint::None;

    RingOverlayRadiusPx: U32 = 200,
        "ring_overlay.radius_px", Rig, Ring,
        "Radius", "px", StaticConstraint::MinU32(1);

    RingOverlayCenterXPx: U32 = 512,
        "ring_overlay.center_x_px", Rig, Ring,
        "Center X", "px", StaticConstraint::None;

    RingOverlayCenterYPx: U32 = 512,
        "ring_overlay.center_y_px", Rig, Ring,
        "Center Y", "px", StaticConstraint::None;

    // ── Display ───────────────────────────────────────────────────────
    TargetStimulusFps: U32 = 60,
        "display.target_stimulus_fps", Rig, Display,
        "Target Stimulus FPS", "Hz", StaticConstraint::MinU32(1);

    MonitorRotationDeg: F64 = 180.0,
        "display.monitor_rotation_deg", Rig, Display,
        "Monitor Rotation", "\u{00b0}", StaticConstraint::RangeF64(0.0, 360.0);

    // ═══════════════════════════════════════════════════════════════════
    // Analysis pipeline (analysis.toml)
    // ═══════════════════════════════════════════════════════════════════
    //
    // Every per-stage analysis parameter — method choice and per-variant
    // tunable — is declared here. There are NO `const FOO_DEFAULT` or
    // `Default` impls in `crates/isi-analysis/src/methods/*.rs`; those
    // method enums are pure runtime data containers populated by the
    // src-tauri → analysis bridge (`params/analysis_bridge.rs`) from
    // a `RegistrySnapshot`.
    //
    // Each stage has:
    //   1. One method-choice param (`<stage>.method`) of the Kind enum
    //      type (`SignMapSmoothingKind`, etc.). Always required.
    //   2. Zero or more tunable params, one per field of one variant,
    //      with `active_when` predicates so the UI shows only the
    //      tunables of the currently-selected method.
    //
    // Stage 0 — ΔF/F baseline (F0 denominator for the bin-1 DFT). The
    // inter-sweep variants take F0 from the rest periods only; the all-frame
    // variants are the faithful Allen `normalizeMovie` baseline. No tunables.
    BaselineMethod: BaselineKind = BaselineKind::OpenIsiInterSweepMean,
        "baseline.method", Analysis, Baseline,
        "Method", "", StaticConstraint::None;

    // Projection — cycle averaging (combine the K per-cycle complex maps of a
    // direction). Default = plain complex average (faithful Allen/SNLC); no tunables.
    CycleAverageMethod: CycleAverageKind = CycleAverageKind::SimpleComplexAverage,
        "cycle_average.method", Analysis, CycleAverage,
        "Method", "", StaticConstraint::None;

    // Stage 1 — Cycle combine.

    CycleCombineMethod: CycleCombineKind = CycleCombineKind::KalatskyStryker2003DelaySubtraction,
        "cycle_combine.method", Analysis, CycleCombine,
        "Method", "", StaticConstraint::None;

    // Stage 2 — Phase / position phasor smoothing.

    PhaseSmoothingMethod: PhaseSmoothingKind = PhaseSmoothingKind::SnlcAmpWeightedPhasor,
        "phase_smoothing.method", Analysis, PhaseSmoothing,
        "Method", "", StaticConstraint::None;

    // SNLC amplitude-weighted complex-phasor smoothing σ. Mirrors Allen
    // `phaseMapFilterSigma` (Zhuang 2017, `RetinotopicMapping.py` L1258, default 1).
    PhaseSmoothingSnlcAmpWeightedPhasorSigmaPx: F64 = 1.0,
        "phase_smoothing.snlc_amp_weighted_phasor.sigma_px", Analysis, PhaseSmoothing,
        "Smoothing \u{03c3}", "px", StaticConstraint::RangeF64(0.0, 50.0),
        active_when = |reg: &Registry|
            reg.phase_smoothing_method() == PhaseSmoothingKind::SnlcAmpWeightedPhasor;

    // Allen `_getSignMap` `phaseMapFilterSigma` (Zhuang 2017, default 1) — the
    // scalar Gaussian on the position/phase map.
    PhaseSmoothingAllenZhuang2017PositionGaussianSigmaPx: F64 = 1.0,
        "phase_smoothing.allen_zhuang2017_position_gaussian.sigma_px", Analysis, PhaseSmoothing,
        "Smoothing \u{03c3}", "px", StaticConstraint::RangeF64(0.0, 50.0),
        active_when = |reg: &Registry|
            reg.phase_smoothing_method() == PhaseSmoothingKind::AllenZhuang2017PositionGaussian;

    // Stage 3 — VFS computation.

    VfsComputationMethod: VfsComputationKind = VfsComputationKind::OpenIsiChainRulePhasorGradient,
        "vfs_computation.method", Analysis, VfsComputation,
        "Method", "", StaticConstraint::None;

    // Stage 4 — Sign map smoothing.

    SignMapSmoothingMethod: SignMapSmoothingKind = SignMapSmoothingKind::Gaussian,
        "sign_map_smoothing.method", Analysis, SignMapSmoothing,
        "Method", "", StaticConstraint::None;

    // OpenISI 60 \u{00b5}m = \u{03c3}_px=3 at 20 \u{00b5}m/px (Allen \u{03c3}=180 \u{00b5}m
    // requires split/merge and over-merges on R43 without them).
    SignMapSmoothingGaussianSigmaUm: F64 = 60.0,
        "sign_map_smoothing.gaussian.sigma_um", Analysis, SignMapSmoothing,
        "Smoothing \u{03c3}", "\u{00b5}m", StaticConstraint::RangeF64(0.0, 500.0),
        active_when = |reg: &Registry|
            reg.sign_map_smoothing_method() == SignMapSmoothingKind::Gaussian;

    // Stage 5 — Cortex / ROI source.

    CortexSourceMethod: CortexSourceKind = CortexSourceKind::SnlcGarrett2014ImBound,
        "cortex_source.method", Analysis, CortexSource,
        "Method", "", StaticConstraint::None;

    CortexSourceReliabilityThreshold: F64 = 0.5,
        "cortex_source.reliability.threshold", Analysis, CortexSource,
        "Reliability threshold", "", StaticConstraint::RangeF64(0.0, 1.0),
        active_when = |reg: &Registry|
            reg.cortex_source_method() == CortexSourceKind::Reliability;

    // Garrett 2014 SNLC `getMouseAreasX.m` L61: `threshSeg = k * std(VFS)`.
    CortexSourceSnlcK: F64 = 1.5,
        "cortex_source.snlc_garrett2014_im_bound.k", Analysis, CortexSource,
        "\u{03c3} multiplier", "", StaticConstraint::RangeF64(0.0, 10.0),
        active_when = |reg: &Registry|
            reg.cortex_source_method() == CortexSourceKind::SnlcGarrett2014ImBound;

    CortexSourceSnlcClose: I32 = 10,
        "cortex_source.snlc_garrett2014_im_bound.close", Analysis, CortexSource,
        "Closing radius", "px", StaticConstraint::RangeI32(0, 50),
        active_when = |reg: &Registry|
            reg.cortex_source_method() == CortexSourceKind::SnlcGarrett2014ImBound;

    CortexSourceSnlcDilate: I32 = 3,
        "cortex_source.snlc_garrett2014_im_bound.dilate", Analysis, CortexSource,
        "Dilation radius", "px", StaticConstraint::RangeI32(0, 50),
        active_when = |reg: &Registry|
            reg.cortex_source_method() == CortexSourceKind::SnlcGarrett2014ImBound;

    // Stage 6 — Patch threshold.

    PatchThresholdMethod: PatchThresholdKind = PatchThresholdKind::Garrett2014SigmaScaled,
        "patch_threshold.method", Analysis, PatchThreshold,
        "Method", "", StaticConstraint::None;

    // Allen `signMapThr` (Zhuang 2017, `RetinotopicMapping.py` L1002).
    PatchThresholdAllenValue: F64 = 0.35,
        "patch_threshold.allen_zhuang2017_fixed_sign_map_thr.value", Analysis, PatchThreshold,
        "Threshold", "", StaticConstraint::RangeF64(0.0, 1.0),
        active_when = |reg: &Registry|
            reg.patch_threshold_method() == PatchThresholdKind::AllenZhuang2017FixedSignMapThr;

    // Juavinett 2017 / SNLC `getMouseAreasX.m` L61.
    PatchThresholdGarrettK: F64 = 1.5,
        "patch_threshold.garrett2014_sigma_scaled.k", Analysis, PatchThreshold,
        "\u{03c3} multiplier", "", StaticConstraint::RangeF64(0.0, 10.0),
        active_when = |reg: &Registry|
            reg.patch_threshold_method() == PatchThresholdKind::Garrett2014SigmaScaled;

    // Stage 7 — Patch extraction.

    PatchExtractionMethod: PatchExtractionKind = PatchExtractionKind::AllenZhuang2017LabelOpenCloseDilate,
        "patch_extraction.method", Analysis, PatchExtraction,
        "Method", "", StaticConstraint::None;

    // Allen `_getRawPatchMap` defaults (`RetinotopicMapping.py`).
    PatchExtractionAllenOpenIter: I32 = 3,
        "patch_extraction.allen_zhuang2017_label_open_close_dilate.open_iter", Analysis, PatchExtraction,
        "Opening iterations", "", StaticConstraint::RangeI32(0, 50),
        active_when = |reg: &Registry|
            reg.patch_extraction_method() == PatchExtractionKind::AllenZhuang2017LabelOpenCloseDilate;

    PatchExtractionAllenCloseIter: I32 = 3,
        "patch_extraction.allen_zhuang2017_label_open_close_dilate.close_iter", Analysis, PatchExtraction,
        "Closing iterations", "", StaticConstraint::RangeI32(0, 50),
        active_when = |reg: &Registry|
            reg.patch_extraction_method() == PatchExtractionKind::AllenZhuang2017LabelOpenCloseDilate;

    PatchExtractionAllenDilationIter: I32 = 15,
        "patch_extraction.allen_zhuang2017_label_open_close_dilate.dilation_iter", Analysis, PatchExtraction,
        "Dilation iterations", "", StaticConstraint::RangeI32(0, 50),
        active_when = |reg: &Registry|
            reg.patch_extraction_method() == PatchExtractionKind::AllenZhuang2017LabelOpenCloseDilate;

    PatchExtractionAllenBorderWidth: I32 = 1,
        "patch_extraction.allen_zhuang2017_label_open_close_dilate.border_width", Analysis, PatchExtraction,
        "Border width", "px", StaticConstraint::RangeI32(1, 20),
        active_when = |reg: &Registry|
            reg.patch_extraction_method() == PatchExtractionKind::AllenZhuang2017LabelOpenCloseDilate;

    // OpenISI deviation from Allen 100 — preserves edge areas P/POR
    // whose cortex representation is naturally smaller than V1.
    PatchExtractionAllenSmallPatchThr: Usize = 50,
        "patch_extraction.allen_zhuang2017_label_open_close_dilate.small_patch_thr", Analysis, PatchExtraction,
        "Drop patches smaller than", "px", StaticConstraint::RangeUsize(0, 10_000),
        active_when = |reg: &Registry|
            reg.patch_extraction_method() == PatchExtractionKind::AllenZhuang2017LabelOpenCloseDilate;

    // Stage 8 — Patch refinement.

    PatchRefinementMethod: PatchRefinementKind = PatchRefinementKind::AllenZhuang2017SplitMerge,
        "patch_refinement.method", Analysis, PatchRefinement,
        "Method", "", StaticConstraint::None;

    PatchRefinementAllenSplitOverlapThr: F64 = 1.1,
        "patch_refinement.allen_zhuang2017_split_merge.split_overlap_thr", Analysis, PatchRefinement,
        "Split overlap threshold", "", StaticConstraint::RangeF64(0.0, 10.0),
        active_when = |reg: &Registry|
            reg.patch_refinement_method() == PatchRefinementKind::AllenZhuang2017SplitMerge;

    PatchRefinementAllenSplitLocalMinCutStep: F64 = 5.0,
        "patch_refinement.allen_zhuang2017_split_merge.split_local_min_cut_step", Analysis, PatchRefinement,
        "Split min cut step", "\u{00b0}", StaticConstraint::RangeF64(0.0, 50.0),
        active_when = |reg: &Registry|
            reg.patch_refinement_method() == PatchRefinementKind::AllenZhuang2017SplitMerge;

    // OpenISI deviation from Allen 0.1 — our watershed cleanly partitions
    // patches; Allen's 0.1 would undo every legitimate split.
    PatchRefinementAllenMergeOverlapThr: F64 = 0.01,
        "patch_refinement.allen_zhuang2017_split_merge.merge_overlap_thr", Analysis, PatchRefinement,
        "Merge overlap threshold", "", StaticConstraint::RangeF64(0.0, 1.0),
        active_when = |reg: &Registry|
            reg.patch_refinement_method() == PatchRefinementKind::AllenZhuang2017SplitMerge;

    PatchRefinementAllenVisualSpacePixelSize: F64 = 0.5,
        "patch_refinement.allen_zhuang2017_split_merge.visual_space_pixel_size", Analysis, PatchRefinement,
        "Visual-space pixel size", "\u{00b0}", StaticConstraint::RangeF64(0.001, 10.0),
        active_when = |reg: &Registry|
            reg.patch_refinement_method() == PatchRefinementKind::AllenZhuang2017SplitMerge;

    PatchRefinementAllenVisualSpaceCloseIter: I32 = 15,
        "patch_refinement.allen_zhuang2017_split_merge.visual_space_close_iter", Analysis, PatchRefinement,
        "Visual-space close iterations", "", StaticConstraint::RangeI32(0, 50),
        active_when = |reg: &Registry|
            reg.patch_refinement_method() == PatchRefinementKind::AllenZhuang2017SplitMerge;

    PatchRefinementAllenEccMapFilterSigma: I32 = 10,
        "patch_refinement.allen_zhuang2017_split_merge.ecc_map_filter_sigma", Analysis, PatchRefinement,
        "Eccentricity filter \u{03c3}", "px", StaticConstraint::RangeI32(0, 50),
        active_when = |reg: &Registry|
            reg.patch_refinement_method() == PatchRefinementKind::AllenZhuang2017SplitMerge;

    PatchRefinementAllenBorderWidth: I32 = 1,
        "patch_refinement.allen_zhuang2017_split_merge.border_width", Analysis, PatchRefinement,
        "Border width", "px", StaticConstraint::RangeI32(1, 20),
        active_when = |reg: &Registry|
            reg.patch_refinement_method() == PatchRefinementKind::AllenZhuang2017SplitMerge;

    PatchRefinementAllenSmallPatchThr: Usize = 100,
        "patch_refinement.allen_zhuang2017_split_merge.small_patch_thr", Analysis, PatchRefinement,
        "Drop patches smaller than", "px", StaticConstraint::RangeUsize(0, 10_000),
        active_when = |reg: &Registry|
            reg.patch_refinement_method() == PatchRefinementKind::AllenZhuang2017SplitMerge;

    // Stage 10 — Eccentricity.

    EccentricityMethod: EccentricityKind = EccentricityKind::OpenIsiWholeCortexV1,
        "eccentricity.method", Analysis, Eccentricity,
        "Method", "", StaticConstraint::None;

    // ═══════════════════════════════════════════════════════════════════
    // Acquisition-time facts (recorded into .oisi /rig_params + /experiment_params)
    // ═══════════════════════════════════════════════════════════════════
    //
    // Stimulus-geometry facts (sweep ranges, offsets, rotation_k) are
    // Experiment-target — they describe how the stimulus was presented,
    // not algorithmic choices. They feed `AcquisitionProperties` at
    // analyze time, alongside `camera.um_per_pixel` from the rig.

    // ── Stimulus geometry (Experiment) ─────────────────────────────
    RotationK: I32 = 0,
        "stimulus_geometry.rotation_k", Experiment, Geometry,
        "Rotation K", "", StaticConstraint::RangeI32(-3, 3);

    // Declared sweep envelope of the drifting bar — Zhuang 2017 canonical
    // is 140° azimuth (-10° to 130°) × 110° altitude (-50° to 60°).
    // Marshel 2011 uses 147° × 153°. This is the EXTENT actually presented,
    // not the monitor's full angular subtense; the shader masks pixels
    // outside this envelope to background luminance so the recorded
    // `azi_angular_range`/`alt_angular_range` truly describes what the
    // mouse was shown.
    AziAngularRange: F64 = 140.0,
        "stimulus_geometry.azi_angular_range", Experiment, Geometry,
        "Azimuth Angular Range", "\u{00b0}", StaticConstraint::RangeF64(0.0, 360.0);

    AltAngularRange: F64 = 110.0,
        "stimulus_geometry.alt_angular_range", Experiment, Geometry,
        "Altitude Angular Range", "\u{00b0}", StaticConstraint::RangeF64(0.0, 360.0);

    OffsetAzi: F64 = 0.0,
        "stimulus_geometry.offset_azi", Experiment, Geometry,
        "Azimuth Offset", "\u{00b0}", StaticConstraint::RangeF64(-180.0, 180.0);

    OffsetAlt: F64 = 0.0,
        "stimulus_geometry.offset_alt", Experiment, Geometry,
        "Altitude Offset", "\u{00b0}", StaticConstraint::RangeF64(-180.0, 180.0);

    // ── UI display state (NOT analysis math, NOT persisted to .oisi) ─
    SnrThresholdEnabled: Bool = false,
        "snr_threshold_enabled", UiState, Retinotopy,
        "SNR Threshold Enabled", "", StaticConstraint::None;

    SnrThresholdValue: F64 = 2.0,
        "snr_threshold_value", UiState, Retinotopy,
        "SNR Threshold", "", StaticConstraint::MinF64(0.0);

    SnrPreferSpectral: Bool = true,
        "snr_prefer_spectral", UiState, Retinotopy,
        "Prefer Spectral SNR", "", StaticConstraint::None;

    SnrUseTransparentMask: Bool = true,
        "snr_use_transparent_mask", UiState, Retinotopy,
        "Use Transparent SNR Mask", "", StaticConstraint::None;

    // ── System Tuning ─────────────────────────────────────────────────
    CameraFrameSendIntervalMs: U32 = 33,
        "system.camera_frame_send_interval_ms", Rig, System,
        "Camera Frame Send Interval", "ms", StaticConstraint::MinU32(1);

    CameraPollIntervalMs: U32 = 1,
        "system.camera_poll_interval_ms", Rig, System,
        "Camera Poll Interval", "ms", StaticConstraint::MinU32(1);

    CameraFirstFrameTimeoutMs: U32 = 5000,
        "system.camera_first_frame_timeout_ms", Rig, System,
        "Camera First Frame Timeout", "ms", StaticConstraint::MinU32(1);

    CameraFirstFramePollMs: U32 = 10,
        "system.camera_first_frame_poll_ms", Rig, System,
        "Camera First Frame Poll", "ms", StaticConstraint::MinU32(1);

    DisplayValidationSampleCount: U32 = 150,
        "system.display_validation_sample_count", Rig, System,
        "Display Validation Sample Count", "", StaticConstraint::MinU32(1);

    PreviewWidthPx: U32 = 320,
        "system.preview_width_px", Rig, System,
        "Preview Width", "px", StaticConstraint::MinU32(1);

    PreviewIntervalMs: U32 = 100,
        "system.preview_interval_ms", Rig, System,
        "Preview Interval", "ms", StaticConstraint::MinU32(1);

    PreviewCycleSec: F64 = 10.0,
        "system.preview_cycle_sec", Rig, System,
        "Preview Cycle", "s", StaticConstraint::MinF64(0.0);

    IdleSleepMs: U32 = 16,
        "system.idle_sleep_ms", Rig, System,
        "Idle Sleep", "ms", StaticConstraint::MinU32(1);

    FpsWindowFrames: Usize = 10,
        "system.fps_window_frames", Rig, System,
        "FPS Window Frames", "", StaticConstraint::MinUsize(1);

    DropDetectionWarmupFrames: Usize = 10,
        "system.drop_detection_warmup_frames", Rig, System,
        "Drop Detection Warmup", "frames", StaticConstraint::None;

    DropDetectionThreshold: F64 = 1.5,
        "system.drop_detection_threshold", Rig, System,
        "Drop Detection Threshold", "", StaticConstraint::MinF64(0.0);

    // ── Paths ─────────────────────────────────────────────────────────
    DataDirectory: String = "",
        "paths.data_directory", Rig, Paths,
        "Data Directory", "", StaticConstraint::None;

    ExperimentsDirectory: String = "",
        "paths.experiments_directory", Rig, Paths,
        "Experiments Directory", "", StaticConstraint::None;

    // ═══════════════════════════════════════════════════════════════════
    // Experiment parameters (experiment.toml)
    // ═══════════════════════════════════════════════════════════════════

    // ── Experiment Geometry ───────────────────────────────────────────
    HorizontalOffsetDeg: F64 = 0.0,
        "geometry.horizontal_offset_deg", Experiment, Geometry,
        "Horizontal Offset", "\u{00b0}", StaticConstraint::RangeF64(-180.0, 180.0);

    VerticalOffsetDeg: F64 = 0.0,
        "geometry.vertical_offset_deg", Experiment, Geometry,
        "Vertical Offset", "\u{00b0}", StaticConstraint::RangeF64(-90.0, 90.0);

    ExperimentProjection: Projection = Projection::Spherical,
        "geometry.projection", Experiment, Geometry,
        "Projection", "", StaticConstraint::None;

    // ── Stimulus ──────────────────────────────────────────────────────
    StimulusEnvelope: Envelope = Envelope::Bar,
        "stimulus.envelope", Experiment, Stimulus,
        "Envelope", "", StaticConstraint::None;

    StimulusCarrier: Carrier = Carrier::Checkerboard,
        "stimulus.carrier", Experiment, Stimulus,
        "Carrier", "", StaticConstraint::None;

    // ── Stimulus Params ───────────────────────────────────────────────
    Contrast: F64 = 1.0,
        "stimulus.params.contrast", Experiment, Stimulus,
        "Contrast", "", StaticConstraint::RangeF64(0.0, 1.0);

    MeanLuminance: F64 = 0.5,
        "stimulus.params.mean_luminance", Experiment, Stimulus,
        "Mean Luminance", "", StaticConstraint::RangeF64(0.0, 1.0);

    // Black outside the stimulus envelope (deliberate rig choice). This
    // DIVERGES from the Marshel 2011 / Zhuang 2017 canon, which uses mean gray
    // (= MeanLuminance) outside the bar to keep bar-onset luminance transients
    // small. Black is used on this rig for a clean, unambiguous stimulus/FOV
    // boundary; the larger onset step is an accepted tradeoff.
    BackgroundLuminance: F64 = 0.0,
        "stimulus.params.background_luminance", Experiment, Stimulus,
        "Background Luminance", "", StaticConstraint::RangeF64(0.0, 1.0);

    CheckSizeDeg: F64 = 25.0,
        "stimulus.params.check_size_deg", Experiment, Stimulus,
        "Check Size", "\u{00b0}", StaticConstraint::MinF64(0.001);

    CheckSizeCm: F64 = 1.0,
        "stimulus.params.check_size_cm", Experiment, Stimulus,
        "Check Size", "cm", StaticConstraint::MinF64(0.001);

    StrobeFrequencyHz: F64 = 6.0,
        "stimulus.params.strobe_frequency_hz", Experiment, Stimulus,
        "Strobe Frequency", "Hz", StaticConstraint::MinF64(0.0);

    StimulusWidthDeg: F64 = 20.0,
        "stimulus.params.stimulus_width_deg", Experiment, Stimulus,
        "Stimulus Width", "\u{00b0}", StaticConstraint::MinF64(0.001);

    // Canonical Marshel 2011 (8.5–9.5°/s for intrinsic imaging) / Zhuang 2017
    // (9°/s). Previous default 90.0 was a 10× Godot-port carryover that ran
    // the bar across a typical 140° envelope in ~1.5 s instead of ~17 s.
    SweepSpeedDegPerSec: F64 = 9.0,
        "stimulus.params.sweep_speed_deg_per_sec", Experiment, Stimulus,
        "Sweep Speed", "\u{00b0}/s", StaticConstraint::MinF64(0.001);

    RotationSpeedDegPerSec: F64 = 15.0,
        "stimulus.params.rotation_speed_deg_per_sec", Experiment, Stimulus,
        "Rotation Speed", "\u{00b0}/s", StaticConstraint::MinF64(0.001);

    ExpansionSpeedDegPerSec: F64 = 5.0,
        "stimulus.params.expansion_speed_deg_per_sec", Experiment, Stimulus,
        "Expansion Speed", "\u{00b0}/s", StaticConstraint::MinF64(0.001);

    RotationDeg: F64 = 0.0,
        "stimulus.params.rotation_deg", Experiment, Stimulus,
        "Rotation", "\u{00b0}", StaticConstraint::RangeF64(-360.0, 360.0);

    // ── Presentation ──────────────────────────────────────────────────
    Conditions: StringVec = vec!["LR".into(), "RL".into(), "TB".into(), "BT".into()],
        "presentation.conditions", Experiment, Presentation,
        "Conditions", "", StaticConstraint::None;

    Repetitions: U32 = 1,
        "presentation.repetitions", Experiment, Presentation,
        "Repetitions", "", StaticConstraint::MinU32(1);

    PresentationStructure: Structure = Structure::Blocked,
        "presentation.structure", Experiment, Presentation,
        "Structure", "", StaticConstraint::None;

    PresentationOrder: Order = Order::Sequential,
        "presentation.order", Experiment, Presentation,
        "Order", "", StaticConstraint::None;

    // ── Timing ────────────────────────────────────────────────────────
    BaselineStartSec: F64 = 5.0,
        "timing.baseline_start_sec", Experiment, Timing,
        "Baseline Start", "s", StaticConstraint::MinF64(0.0);

    BaselineEndSec: F64 = 5.0,
        "timing.baseline_end_sec", Experiment, Timing,
        "Baseline End", "s", StaticConstraint::MinF64(0.0);

    InterStimulusSec: F64 = 0.0,
        "timing.inter_stimulus_sec", Experiment, Timing,
        "Inter-Stimulus", "s", StaticConstraint::MinF64(0.0);

    InterDirectionSec: F64 = 5.0,
        "timing.inter_direction_sec", Experiment, Timing,
        "Inter-Direction", "s", StaticConstraint::MinF64(0.0);
}
