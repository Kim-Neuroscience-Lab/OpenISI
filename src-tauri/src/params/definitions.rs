//! Parameter definitions — THE single source of truth for all ~70 parameters.
//!
//! One `define_params!` invocation generates:
//! - `ParamId` enum
//! - `PARAM_DEFS` static table
//! - Typed getters/setters on `Registry`

use super::{
    Carrier, Envelope, GroupId, Order, ParamDef, ParamValue, PersistTarget, Projection,
    StaticConstraint, Structure,
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

    // ── Analysis ──────────────────────────────────────────────────────
    SmoothingSigma: F64 = 2.0,
        "analysis.smoothing_sigma", Rig, Retinotopy,
        "Smoothing Sigma", "\u{03c3}", StaticConstraint::MinF64(0.0);

    RotationK: I32 = 0,
        "analysis.rotation_k", Rig, Retinotopy,
        "Rotation K", "", StaticConstraint::RangeI32(-3, 3);

    AziAngularRange: F64 = 100.0,
        "analysis.azi_angular_range", Rig, Retinotopy,
        "Azimuth Angular Range", "\u{00b0}", StaticConstraint::RangeF64(0.0, 360.0);

    AltAngularRange: F64 = 100.0,
        "analysis.alt_angular_range", Rig, Retinotopy,
        "Altitude Angular Range", "\u{00b0}", StaticConstraint::RangeF64(0.0, 360.0);

    OffsetAzi: F64 = 0.0,
        "analysis.offset_azi", Rig, Retinotopy,
        "Azimuth Offset", "\u{00b0}", StaticConstraint::RangeF64(-180.0, 180.0);

    OffsetAlt: F64 = 0.0,
        "analysis.offset_alt", Rig, Retinotopy,
        "Altitude Offset", "\u{00b0}", StaticConstraint::RangeF64(-180.0, 180.0);

    Epsilon: F64 = 0.0000000001,
        "analysis.epsilon", Rig, Retinotopy,
        "Epsilon", "", StaticConstraint::MinF64(0.0);

    // ── Analysis Segmentation ─────────────────────────────────────────
    SignMapFilterSigma: F64 = 9.0,
        "analysis.segmentation.sign_map_filter_sigma", Rig, Segmentation,
        "Sign Map Filter Sigma", "\u{03c3}", StaticConstraint::MinF64(0.0);

    SignMapThreshold: F64 = 0.35,
        "analysis.segmentation.sign_map_threshold", Rig, Segmentation,
        "Sign Map Threshold", "", StaticConstraint::RangeF64(0.0, 1.0);

    OpenRadius: Usize = 2,
        "analysis.segmentation.open_radius", Rig, Segmentation,
        "Open Radius", "px", StaticConstraint::None;

    CloseRadius: Usize = 10,
        "analysis.segmentation.close_radius", Rig, Segmentation,
        "Close Radius", "px", StaticConstraint::None;

    DilateRadius: Usize = 3,
        "analysis.segmentation.dilate_radius", Rig, Segmentation,
        "Dilate Radius", "px", StaticConstraint::None;

    PadBorder: Usize = 30,
        "analysis.segmentation.pad_border", Rig, Segmentation,
        "Pad Border", "px", StaticConstraint::None;

    SpurIterations: Usize = 4,
        "analysis.segmentation.spur_iterations", Rig, Segmentation,
        "Spur Iterations", "", StaticConstraint::None;

    SplitOverlapThreshold: F64 = 1.1,
        "analysis.segmentation.split_overlap_threshold", Rig, Segmentation,
        "Split Overlap Threshold", "", StaticConstraint::MinF64(0.0);

    MergeOverlapThreshold: F64 = 0.1,
        "analysis.segmentation.merge_overlap_threshold", Rig, Segmentation,
        "Merge Overlap Threshold", "", StaticConstraint::MinF64(0.0);

    MergeDilateRadius: Usize = 3,
        "analysis.segmentation.merge_dilate_radius", Rig, Segmentation,
        "Merge Dilate Radius", "px", StaticConstraint::None;

    MergeCloseRadius: Usize = 5,
        "analysis.segmentation.merge_close_radius", Rig, Segmentation,
        "Merge Close Radius", "px", StaticConstraint::None;

    EccentricityRadius: F64 = 30.0,
        "analysis.segmentation.eccentricity_radius", Rig, Segmentation,
        "Eccentricity Radius", "px", StaticConstraint::MinF64(0.0);

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
