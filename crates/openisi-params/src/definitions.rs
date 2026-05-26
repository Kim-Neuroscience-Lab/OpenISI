//! Parameter definitions — THE single source of truth for all ~70 parameters.
//!
//! One `define_params!` invocation generates:
//! - `ParamId` enum
//! - `PARAM_DEFS` static table
//! - Typed getters/setters on `Registry`

use super::{
    Carrier, CortexSourceKind, CycleCombineKind, EccentricityKind, Envelope, GroupId, Order,
    ParamDef, ParamValue, PatchExtractionKind, PatchRefinementKind, PatchThresholdKind,
    PersistTarget, PhaseSmoothingKind, Projection, QualityGateKind, SignMapSmoothingKind,
    StaticConstraint, Structure, VfsComputationKind,
};
use super::registry::Registry;

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
    ViewingDistanceCm: F64 = 10.0,
        "geometry.viewing_distance_cm", Rig, Display,
        "Viewing Distance", "cm", StaticConstraint::MinF64(0.1);

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
    // Stage 1 — Cycle combine.

    CycleCombineMethod: CycleCombineKind = CycleCombineKind::MarshelGarrett2011DelaySubtraction,
        "cycle_combine.method", Analysis, CycleCombine,
        "Method", "", StaticConstraint::None;

    // Stage 2 — Phase / position phasor smoothing.

    PhaseSmoothingMethod: PhaseSmoothingKind = PhaseSmoothingKind::OpenIsiAmpWeightedPhasor,
        "phase_smoothing.method", Analysis, PhaseSmoothing,
        "Method", "", StaticConstraint::None;

    // Allen `phaseMapFilterSigma` (Zhuang 2017, `RetinotopicMapping.py` L298).
    PhaseSmoothingOpenIsiAmpWeightedPhasorSigmaPx: F64 = 1.0,
        "phase_smoothing.open_isi_amp_weighted_phasor.sigma_px", Analysis, PhaseSmoothing,
        "Smoothing \u{03c3}", "px", StaticConstraint::RangeF64(0.0, 50.0),
        active_when = |reg: &Registry|
            reg.phase_smoothing_method() == PhaseSmoothingKind::OpenIsiAmpWeightedPhasor;

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

    // Stage 9 — Quality gate.

    QualityGateMethod: QualityGateKind = QualityGateKind::None,
        "quality_gate.method", Analysis, QualityGate,
        "Method", "", StaticConstraint::None;

    // Stage 10 — Eccentricity.

    EccentricityMethod: EccentricityKind = EccentricityKind::Garrett2014WholeCortexV1,
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

    AziAngularRange: F64 = 100.0,
        "stimulus_geometry.azi_angular_range", Experiment, Geometry,
        "Azimuth Angular Range", "\u{00b0}", StaticConstraint::RangeF64(0.0, 360.0);

    AltAngularRange: F64 = 100.0,
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

    SweepSpeedDegPerSec: F64 = 90.0,
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
