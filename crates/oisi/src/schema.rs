//! Single source of truth for the `.oisi` (HDF5) on-disk layout.
//!
//! Every group, dataset, and attribute an `.oisi` file may contain is declared
//! ONCE here, as Rust data ([`SCHEMA`]). From this one declaration:
//!
//! * the writers (`oisi::io` and the Tauri `export`) reference the entity
//!   names via [`name`] consts (no stringly-typed HDF5 keys that can drift);
//! * `docs/oisi.schema.json` is **generated** by [`to_json_schema`] (a golden
//!   test asserts the committed doc equals the generated one — the doc can no
//!   longer drift from this source by hand-editing);
//! * the contract test introspects a real `.oisi` and checks it against
//!   [`SCHEMA`] in both directions.
//!
//! To change the layout: edit [`SCHEMA`] (+ a `name` const if adding a name),
//! update the writer, run `OISI_REGEN_SCHEMA=1 cargo test -p isi-analysis
//! --test oisi_schema_contract` to regenerate the JSON, and commit both.

use std::collections::BTreeSet;

use serde_json::{json, Map, Value};

/// Whether an entity is always present (given its parent is) or conditional.
#[derive(Clone, Copy)]
pub enum Presence {
    /// Always written when the parent group is written.
    Always,
    /// Written only under a stated condition (the `present_when` clause).
    When(&'static str),
}

/// An HDF5 attribute spec.
#[derive(Clone, Copy)]
pub struct Attr {
    pub name: &'static str,
    pub dtype: &'static str,
    pub presence: Presence,
    pub doc: &'static str,
}

/// An HDF5 dataset spec (may itself carry attributes — e.g. `/results` MapMeta).
#[derive(Clone, Copy)]
pub struct Dataset {
    pub name: &'static str,
    pub dtype: &'static str,
    pub shape: &'static str,
    pub presence: Presence,
    pub doc: &'static str,
    pub attrs: &'static [Attr],
}

/// An HDF5 group spec (recursive: may contain subgroups).
#[derive(Clone, Copy)]
pub struct Group {
    pub path: &'static str,
    pub presence: Presence,
    pub doc: &'static str,
    pub attrs: &'static [Attr],
    /// When `Some`, the group carries dynamically-named attributes (the string
    /// documents the naming pattern); the contract test allows any attr name.
    pub dynamic_attrs: Option<&'static str>,
    pub datasets: &'static [Dataset],
    pub subgroups: &'static [Group],
}

/// The whole `.oisi` layout.
pub struct Schema {
    pub format_version: &'static str,
    pub root_attrs: &'static [Attr],
    /// Datasets that live at the file root (currently just `/anatomical`).
    pub root_datasets: &'static [Dataset],
    pub groups: &'static [Group],
}

/// Canonical HDF5 entity names — the ONE place each on-disk key string lives.
/// Both [`SCHEMA`] and the writers reference these, so a name cannot drift
/// between what is written and what is documented.
pub mod name {
    // Root attributes.
    pub const VERSION: &str = "version";
    pub const SOURCE_TYPE: &str = "source_type";
    pub const CREATED_AT: &str = "created_at";
    pub const SOFTWARE_VERSION: &str = "software_version";
    pub const STIMULUS_METADATA: &str = "stimulus_metadata";
    pub const RIG_PARAMS: &str = "rig_params";
    pub const EXPERIMENT_PARAMS: &str = "experiment_params";
    pub const ANALYSIS_PARAMS: &str = "analysis_params";
    pub const ANIMAL_ID: &str = "animal_id";
    pub const NOTES: &str = "notes";

    // Top-level groups / root datasets.
    pub const ANATOMICAL: &str = "anatomical";
    pub const HARDWARE: &str = "hardware";
    pub const ACQUISITION: &str = "acquisition";
    pub const COMPLEX_MAPS: &str = "complex_maps";
    pub const RESULTS: &str = "results";
    pub const ANALYSIS_STATE: &str = "analysis_state";
    pub const CACHE: &str = "cache";

    // /hardware attributes.
    pub const MONITOR_NAME: &str = "monitor_name";
    pub const MONITOR_WIDTH_PX: &str = "monitor_width_px";
    pub const MONITOR_HEIGHT_PX: &str = "monitor_height_px";
    pub const MONITOR_WIDTH_CM: &str = "monitor_width_cm";
    pub const MONITOR_HEIGHT_CM: &str = "monitor_height_cm";
    pub const MONITOR_REFRESH_HZ: &str = "monitor_refresh_hz";
    pub const MEASURED_REFRESH_HZ: &str = "measured_refresh_hz";
    pub const GAMMA_CORRECTED: &str = "gamma_corrected";
    pub const CAMERA_MODEL: &str = "camera_model";
    pub const CAMERA_WIDTH_PX: &str = "camera_width_px";
    pub const CAMERA_HEIGHT_PX: &str = "camera_height_px";
    pub const VIEWING_DISTANCE_CM: &str = "viewing_distance_cm";
    pub const CAMERA_EXPOSURE_US: &str = "camera_exposure_us";
    pub const CAMERA_BINNING: &str = "camera_binning";
    pub const MONITOR_ROTATION_DEG: &str = "monitor_rotation_deg";
    pub const TARGET_STIMULUS_FPS: &str = "target_stimulus_fps";

    // Subgroups of /acquisition.
    pub const CAMERA: &str = "camera";
    pub const STIMULUS: &str = "stimulus";
    pub const SCHEDULE: &str = "schedule";
    pub const CLOCK_SYNC: &str = "clock_sync";
    pub const TIMING: &str = "timing";
    pub const QUALITY: &str = "quality";

    // /acquisition/camera.
    pub const FRAMES: &str = "frames";
    pub const TIMESTAMPS_SEC: &str = "timestamps_sec";
    pub const HARDWARE_TIMESTAMPS_US: &str = "hardware_timestamps_us";
    pub const SYSTEM_TIMESTAMPS_US: &str = "system_timestamps_us";
    pub const SEQUENCE_NUMBERS: &str = "sequence_numbers";

    // /acquisition/stimulus.
    pub const TIMESTAMPS_US: &str = "timestamps_us";
    pub const STATE_IDS: &str = "state_ids";
    pub const CONDITION_INDICES: &str = "condition_indices";
    pub const SWEEP_INDICES: &str = "sweep_indices";
    pub const PROGRESS: &str = "progress";
    pub const FRAME_DELTAS_US: &str = "frame_deltas_us";
    pub const DROPPED_FRAME_INDICES: &str = "dropped_frame_indices";

    // /acquisition/schedule.
    pub const SWEEP_SEQUENCE: &str = "sweep_sequence";
    pub const SWEEP_START_US: &str = "sweep_start_us";
    pub const SWEEP_END_US: &str = "sweep_end_us";
    pub const SWEEP_START_SEC: &str = "sweep_start_sec";
    pub const SWEEP_END_SEC: &str = "sweep_end_sec";

    // /acquisition/clock_sync.
    pub const T0_SYSTEM_US: &str = "t0_system_us";
    pub const START_OFFSET_US: &str = "start_offset_us";
    pub const END_OFFSET_US: &str = "end_offset_us";
    pub const DRIFT_US: &str = "drift_us";

    // /acquisition/timing.
    pub const F_CAM_HZ: &str = "f_cam_hz";
    pub const F_STIM_HZ: &str = "f_stim_hz";
    pub const T_CAM_SEC: &str = "t_cam_sec";
    pub const T_STIM_SEC: &str = "t_stim_sec";
    pub const RATE_RATIO: &str = "rate_ratio";
    pub const BEAT_PERIOD_SEC: &str = "beat_period_sec";
    pub const PHASE_INCREMENT: &str = "phase_increment";
    pub const REGIME: &str = "regime";
    pub const EXPECTED_PHASE_SAMPLES: &str = "expected_phase_samples";
    pub const PHASE_COVERAGE: &str = "phase_coverage";
    pub const ONSET_UNCERTAINTY_SEC: &str = "onset_uncertainty_sec";
    pub const ONSET_UNCERTAINTY_FRACTION: &str = "onset_uncertainty_fraction";
    pub const CAM_SAMPLE_COUNT: &str = "cam_sample_count";
    pub const STIM_SAMPLE_COUNT: &str = "stim_sample_count";
    pub const CAM_JITTER_SEC: &str = "cam_jitter_sec";
    pub const STIM_JITTER_SEC: &str = "stim_jitter_sec";
    pub const WARNINGS: &str = "warnings";

    // /acquisition/quality.
    pub const CAMERA_FRAME_DELTAS_US: &str = "camera_frame_deltas_us";
    pub const CAMERA_SEQUENCE_GAPS: &str = "camera_sequence_gaps";
    pub const STIMULUS_FRAME_DELTAS_US: &str = "stimulus_frame_deltas_us";
    pub const STIMULUS_DROPPED_INDICES: &str = "stimulus_dropped_indices";
    pub const MEAN_FRAME_INTENSITY: &str = "mean_frame_intensity";
    pub const CAMERA_DROPS_TOTAL: &str = "camera_drops_total";
    pub const STIMULUS_DROPS_TOTAL: &str = "stimulus_drops_total";
    pub const ACQUISITION_COMPLETE: &str = "acquisition_complete";
    pub const STIMULUS_TIMING_VALIDATABLE: &str = "stimulus_timing_validatable";
    pub const DISPLAY_SCANOUT: &str = "display_scanout";

    // /complex_maps.
    pub const AZI_FWD: &str = "azi_fwd";
    pub const AZI_REV: &str = "azi_rev";
    pub const ALT_FWD: &str = "alt_fwd";
    pub const ALT_REV: &str = "alt_rev";

    // /results datasets.
    pub const AZI_PHASE: &str = "azi_phase";
    pub const ALT_PHASE: &str = "alt_phase";
    pub const AZI_PHASE_DEGREES: &str = "azi_phase_degrees";
    pub const ALT_PHASE_DEGREES: &str = "alt_phase_degrees";
    pub const AZI_AMPLITUDE: &str = "azi_amplitude";
    pub const ALT_AMPLITUDE: &str = "alt_amplitude";
    pub const VFS: &str = "vfs";
    pub const VFS_SMOOTHED: &str = "vfs_smoothed";
    pub const VFS_SMOOTHED_THRESHOLDED: &str = "vfs_smoothed_thresholded";
    pub const CORTEX_MASK: &str = "cortex_mask";
    pub const AREA_LABELS: &str = "area_labels";
    pub const AREA_SIGNS: &str = "area_signs";
    pub const AREA_BORDERS: &str = "area_borders";
    pub const ECCENTRICITY: &str = "eccentricity";
    pub const POLAR_ANGLE: &str = "polar_angle";
    pub const MAGNIFICATION: &str = "magnification";
    pub const MAGNIFICATION_RAW: &str = "magnification_raw";
    pub const MAGNIFICATION_AXIS: &str = "magnification_axis";
    pub const MAGNIFICATION_DISTORTION: &str = "magnification_distortion";
    pub const CONTOURS_AZI: &str = "contours_azi";
    pub const CONTOURS_ALT: &str = "contours_alt";
    pub const SPECTRAL_SNR_AZI: &str = "spectral_snr_azi";
    pub const SPECTRAL_SNR_ALT: &str = "spectral_snr_alt";
    pub const ALLEN_POWER_SNR_AZI: &str = "allen_power_snr_azi";
    pub const ALLEN_POWER_SNR_ALT: &str = "allen_power_snr_alt";
    pub const RELIABILITY_AZI_FWD: &str = "reliability_azi_fwd";
    pub const RELIABILITY_AZI_REV: &str = "reliability_azi_rev";
    pub const RELIABILITY_ALT_FWD: &str = "reliability_alt_fwd";
    pub const RELIABILITY_ALT_REV: &str = "reliability_alt_rev";
    pub const AZI_DELAY: &str = "azi_delay";
    pub const ALT_DELAY: &str = "alt_delay";

    // /results MapMeta per-dataset attributes.
    pub const PALETTE: &str = "palette";
    pub const UNITS: &str = "units";
    pub const DISPLAY_MIN: &str = "display_min";
    pub const DISPLAY_MAX: &str = "display_max";
    pub const WRAP_PERIOD: &str = "wrap_period";
    pub const NAN_MEANS: &str = "nan_means";
    pub const ZERO_MEANS: &str = "zero_means";

    // /cache.
    pub const IMSEG: &str = "imseg";
    pub const THRESHOLD_APPLIED: &str = "threshold_applied";
}

use name as n;
use Presence::{Always, When};

/// MapMeta attributes attached to every `/results` dataset except `area_signs`
/// (the renderer reads these and does zero inference).
const MAP_META: &[Attr] = &[
    Attr { name: n::PALETTE, dtype: "VarLenUnicode", presence: Always, doc: "Colormap key (hsv_circular | jet | hot | binary | categorical)." },
    Attr { name: n::UNITS, dtype: "VarLenUnicode", presence: Always, doc: "Value units (rad | deg | unitless | bool | label)." },
    Attr { name: n::DISPLAY_MIN, dtype: "f64", presence: Always, doc: "Lower bound of the display range." },
    Attr { name: n::DISPLAY_MAX, dtype: "f64", presence: Always, doc: "Upper bound of the display range." },
    Attr { name: n::WRAP_PERIOD, dtype: "f64", presence: Always, doc: "Circular wrap period (0 = non-circular)." },
    Attr { name: n::NAN_MEANS, dtype: "VarLenUnicode", presence: Always, doc: "Semantic of NaN (empty if no NaN)." },
    Attr { name: n::ZERO_MEANS, dtype: "VarLenUnicode", presence: Always, doc: "Sentinel meaning of literal 0 (empty if 0 is a value)." },
];

/// A `/results` f64 map with its MapMeta attributes.
const fn result_f64(name: &'static str, presence: Presence, doc: &'static str) -> Dataset {
    Dataset { name, dtype: "f64", shape: "(H, W)", presence, doc, attrs: MAP_META }
}

const CAMERA_DATASETS: &[Dataset] = &[
    Dataset { name: n::FRAMES, dtype: "u16", shape: "(T, H, W)", presence: Always, doc: "Raw sensor frames; gzip+fletcher32 chunked (1, H, W).", attrs: &[] },
    Dataset { name: n::TIMESTAMPS_SEC, dtype: "f64", shape: "(T,)", presence: Always, doc: "Unified seconds from t0 (first frame system timestamp).", attrs: &[] },
    Dataset { name: n::HARDWARE_TIMESTAMPS_US, dtype: "i64", shape: "(T,)", presence: Always, doc: "Camera internal-clock timestamps.", attrs: &[] },
    Dataset { name: n::SYSTEM_TIMESTAMPS_US, dtype: "i64", shape: "(T,)", presence: Always, doc: "QPC timestamps at frame read.", attrs: &[] },
    Dataset { name: n::SEQUENCE_NUMBERS, dtype: "i64", shape: "(T,)", presence: Always, doc: "Camera sequence numbers.", attrs: &[] },
];

const STIMULUS_DATASETS: &[Dataset] = &[
    Dataset { name: n::TIMESTAMPS_US, dtype: "i64", shape: "(N,)", presence: Always, doc: "Stimulus QPC timestamps.", attrs: &[] },
    Dataset { name: n::TIMESTAMPS_SEC, dtype: "f64", shape: "(N,)", presence: Always, doc: "Unified seconds from t0.", attrs: &[] },
    Dataset { name: n::STATE_IDS, dtype: "u8", shape: "(N,)", presence: Always, doc: "Per-frame stimulus state id.", attrs: &[] },
    Dataset { name: n::CONDITION_INDICES, dtype: "u8", shape: "(N,)", presence: Always, doc: "Per-frame condition index.", attrs: &[] },
    Dataset { name: n::SWEEP_INDICES, dtype: "u32", shape: "(N,)", presence: Always, doc: "Per-frame sweep index.", attrs: &[] },
    Dataset { name: n::PROGRESS, dtype: "f32", shape: "(N,)", presence: Always, doc: "Per-frame sweep progress.", attrs: &[] },
    Dataset { name: n::FRAME_DELTAS_US, dtype: "i64", shape: "(N,)", presence: Always, doc: "Inter-frame deltas.", attrs: &[] },
    Dataset { name: n::DROPPED_FRAME_INDICES, dtype: "u32", shape: "(D,)", presence: Always, doc: "Indices of frames flagged as drops at capture.", attrs: &[] },
];

const SCHEDULE_DATASETS: &[Dataset] = &[
    Dataset { name: n::SWEEP_START_US, dtype: "i64", shape: "(K,)", presence: Always, doc: "Sweep start (raw µs).", attrs: &[] },
    Dataset { name: n::SWEEP_END_US, dtype: "i64", shape: "(K,)", presence: Always, doc: "Sweep end (raw µs).", attrs: &[] },
    Dataset { name: n::SWEEP_START_SEC, dtype: "f64", shape: "(K,)", presence: Always, doc: "Sweep start (unified s from t0).", attrs: &[] },
    Dataset { name: n::SWEEP_END_SEC, dtype: "f64", shape: "(K,)", presence: Always, doc: "Sweep end (unified s from t0).", attrs: &[] },
];

const QUALITY_DATASETS: &[Dataset] = &[
    Dataset { name: n::CAMERA_FRAME_DELTAS_US, dtype: "i64", shape: "(T-1,)", presence: Always, doc: "Camera inter-frame deltas.", attrs: &[] },
    Dataset { name: n::CAMERA_SEQUENCE_GAPS, dtype: "u32", shape: "(G,)", presence: Always, doc: "Indices of camera sequence gaps.", attrs: &[] },
    Dataset { name: n::STIMULUS_FRAME_DELTAS_US, dtype: "i64", shape: "(N-1,)", presence: Always, doc: "Stimulus inter-frame deltas.", attrs: &[] },
    Dataset { name: n::STIMULUS_DROPPED_INDICES, dtype: "u32", shape: "(D,)", presence: Always, doc: "Stimulus dropped-frame indices.", attrs: &[] },
    Dataset { name: n::MEAN_FRAME_INTENSITY, dtype: "f32", shape: "(T,)", presence: Always, doc: "Mean pixel intensity per camera frame.", attrs: &[] },
];

const ACQUISITION_SUBGROUPS: &[Group] = &[
    Group {
        path: "/acquisition/camera", presence: When("≥1 camera frame captured"),
        doc: "Raw camera frames + per-frame timestamps.", attrs: &[], dynamic_attrs: None,
        datasets: CAMERA_DATASETS, subgroups: &[],
    },
    Group {
        path: "/acquisition/stimulus", presence: Always,
        doc: "Per-frame stimulus state arrays.", attrs: &[], dynamic_attrs: None,
        datasets: STIMULUS_DATASETS, subgroups: &[],
    },
    Group {
        path: "/acquisition/schedule", presence: Always,
        doc: "Realized sweep schedule.", dynamic_attrs: None,
        attrs: &[Attr { name: n::SWEEP_SEQUENCE, dtype: "VarLenUnicode (JSON array)", presence: Always, doc: "Sweep direction names, JSON array." }],
        datasets: SCHEDULE_DATASETS, subgroups: &[],
    },
    Group {
        path: "/acquisition/clock_sync", presence: Always,
        doc: "Camera↔QPC clock offset at start/end (drift detection).", dynamic_attrs: None,
        attrs: &[
            Attr { name: n::T0_SYSTEM_US, dtype: "f64", presence: Always, doc: "System timestamp of the first camera frame." },
            Attr { name: n::START_OFFSET_US, dtype: "f64", presence: When("≥2 frames carrying both hardware and system timestamps"), doc: "system − hardware at first frame." },
            Attr { name: n::END_OFFSET_US, dtype: "f64", presence: When("≥2 frames carrying both hardware and system timestamps"), doc: "system − hardware at last frame." },
            Attr { name: n::DRIFT_US, dtype: "f64", presence: When("≥2 frames carrying both hardware and system timestamps"), doc: "end_offset − start_offset." },
        ],
        datasets: &[], subgroups: &[],
    },
    Group {
        path: "/acquisition/timing", presence: When("a TimingCharacterization was attached to the acquisition"),
        doc: "Timing characterization (frame-rate ratios, jitter, phase coverage).", dynamic_attrs: None,
        attrs: &[
            Attr { name: n::F_CAM_HZ, dtype: "f64", presence: Always, doc: "Camera frame rate." },
            Attr { name: n::F_STIM_HZ, dtype: "f64", presence: Always, doc: "Stimulus frame rate." },
            Attr { name: n::T_CAM_SEC, dtype: "f64", presence: Always, doc: "Camera period." },
            Attr { name: n::T_STIM_SEC, dtype: "f64", presence: Always, doc: "Stimulus period." },
            Attr { name: n::RATE_RATIO, dtype: "f64", presence: Always, doc: "Camera/stimulus rate ratio." },
            Attr { name: n::BEAT_PERIOD_SEC, dtype: "f64", presence: Always, doc: "Beat period." },
            Attr { name: n::PHASE_INCREMENT, dtype: "f64", presence: Always, doc: "Per-frame phase increment." },
            Attr { name: n::REGIME, dtype: "VarLenUnicode", presence: Always, doc: "Timing regime." },
            Attr { name: n::EXPECTED_PHASE_SAMPLES, dtype: "f64", presence: Always, doc: "Expected phase samples." },
            Attr { name: n::PHASE_COVERAGE, dtype: "f64", presence: Always, doc: "Phase coverage." },
            Attr { name: n::ONSET_UNCERTAINTY_SEC, dtype: "f64", presence: Always, doc: "Onset uncertainty (s)." },
            Attr { name: n::ONSET_UNCERTAINTY_FRACTION, dtype: "f64", presence: Always, doc: "Onset uncertainty (fraction)." },
            Attr { name: n::CAM_SAMPLE_COUNT, dtype: "u32", presence: Always, doc: "Camera sample count." },
            Attr { name: n::STIM_SAMPLE_COUNT, dtype: "u32", presence: Always, doc: "Stimulus sample count." },
            Attr { name: n::CAM_JITTER_SEC, dtype: "f64", presence: Always, doc: "Camera jitter (s)." },
            Attr { name: n::STIM_JITTER_SEC, dtype: "f64", presence: Always, doc: "Stimulus jitter (s)." },
            Attr { name: n::WARNINGS, dtype: "VarLenUnicode (JSON array)", presence: When("the TimingCharacterization carried ≥1 warning"), doc: "Timing warnings, JSON array." },
        ],
        datasets: &[], subgroups: &[],
    },
    Group {
        path: "/acquisition/quality", presence: Always,
        doc: "Per-acquisition timing/drop quality metrics computed at export.", dynamic_attrs: None,
        attrs: &[
            Attr { name: n::CAMERA_DROPS_TOTAL, dtype: "u32", presence: Always, doc: "Total camera drops." },
            Attr { name: n::STIMULUS_DROPS_TOTAL, dtype: "u32", presence: Always, doc: "Total stimulus drops." },
            Attr { name: n::ACQUISITION_COMPLETE, dtype: "u8 (0/1)", presence: Always, doc: "Whether the acquisition ran to completion." },
            Attr { name: n::STIMULUS_TIMING_VALIDATABLE, dtype: "u32 (0/1)", presence: Always, doc: "1 iff presented on a physical hardware scanout (vblank); else stimulus_drops_total is not physically meaningful." },
            Attr { name: n::DISPLAY_SCANOUT, dtype: "VarLenUnicode", presence: Always, doc: "\"physical\" | \"remote_virtual\" — paired with stimulus_timing_validatable." },
        ],
        datasets: QUALITY_DATASETS, subgroups: &[],
    },
];

const HARDWARE_ATTRS: &[Attr] = &[
    Attr { name: n::MONITOR_NAME, dtype: "VarLenUnicode", presence: Always, doc: "Monitor name." },
    Attr { name: n::MONITOR_WIDTH_PX, dtype: "u32", presence: Always, doc: "Monitor width (px)." },
    Attr { name: n::MONITOR_HEIGHT_PX, dtype: "u32", presence: Always, doc: "Monitor height (px)." },
    Attr { name: n::MONITOR_WIDTH_CM, dtype: "f64", presence: Always, doc: "Monitor width (cm)." },
    Attr { name: n::MONITOR_HEIGHT_CM, dtype: "f64", presence: Always, doc: "Monitor height (cm)." },
    Attr { name: n::MONITOR_REFRESH_HZ, dtype: "f64", presence: Always, doc: "Nominal refresh (Hz)." },
    Attr { name: n::MEASURED_REFRESH_HZ, dtype: "f64", presence: Always, doc: "Measured refresh (Hz)." },
    Attr { name: n::GAMMA_CORRECTED, dtype: "u8 (0/1)", presence: Always, doc: "Gamma-correction flag." },
    Attr { name: n::CAMERA_MODEL, dtype: "VarLenUnicode", presence: Always, doc: "Camera model." },
    Attr { name: n::CAMERA_WIDTH_PX, dtype: "u32", presence: Always, doc: "Camera width (px)." },
    Attr { name: n::CAMERA_HEIGHT_PX, dtype: "u32", presence: Always, doc: "Camera height (px)." },
    Attr { name: n::VIEWING_DISTANCE_CM, dtype: "f64", presence: Always, doc: "Viewing distance (cm)." },
    Attr { name: n::CAMERA_EXPOSURE_US, dtype: "u32", presence: Always, doc: "Camera exposure (µs)." },
    Attr { name: n::CAMERA_BINNING, dtype: "u16", presence: Always, doc: "Camera binning factor." },
    Attr { name: n::MONITOR_ROTATION_DEG, dtype: "f64", presence: Always, doc: "Monitor rotation (deg)." },
    Attr { name: n::TARGET_STIMULUS_FPS, dtype: "u32", presence: Always, doc: "Target stimulus FPS." },
];

const COMPLEX_MAPS_DATASETS: &[Dataset] = &[
    Dataset { name: n::AZI_FWD, dtype: "f64", shape: "(H, W, 2)", presence: Always, doc: "Real/imag split; [:,:,0]=real, [:,:,1]=imag.", attrs: &[] },
    Dataset { name: n::AZI_REV, dtype: "f64", shape: "(H, W, 2)", presence: Always, doc: "Real/imag split.", attrs: &[] },
    Dataset { name: n::ALT_FWD, dtype: "f64", shape: "(H, W, 2)", presence: Always, doc: "Real/imag split.", attrs: &[] },
    Dataset { name: n::ALT_REV, dtype: "f64", shape: "(H, W, 2)", presence: Always, doc: "Real/imag split.", attrs: &[] },
];

const RESULTS_DATASETS: &[Dataset] = &[
    result_f64(n::AZI_PHASE, Always, "Azimuth phase (rad)."),
    result_f64(n::ALT_PHASE, Always, "Altitude phase (rad)."),
    result_f64(n::AZI_PHASE_DEGREES, Always, "Azimuth phase (deg)."),
    result_f64(n::ALT_PHASE_DEGREES, Always, "Altitude phase (deg)."),
    result_f64(n::AZI_AMPLITUDE, Always, "Azimuth response amplitude."),
    result_f64(n::ALT_AMPLITUDE, Always, "Altitude response amplitude."),
    result_f64(n::VFS, Always, "Raw visual field sign."),
    result_f64(n::VFS_SMOOTHED, Always, "Smoothed VFS (segmentation input)."),
    result_f64(n::VFS_SMOOTHED_THRESHOLDED, Always, "Thresholded smoothed VFS."),
    Dataset { name: n::CORTEX_MASK, dtype: "u8 (bool)", shape: "(H, W)", presence: Always, doc: "Cortex ROI mask.", attrs: MAP_META },
    Dataset { name: n::AREA_LABELS, dtype: "i32", shape: "(H, W)", presence: Always, doc: "Per-area integer labels (0 = none).", attrs: MAP_META },
    Dataset { name: n::AREA_SIGNS, dtype: "i8", shape: "(N,)", presence: Always, doc: "Per-patch sign in [-1, +1], order-matched to area_labels 1..N.", attrs: &[] },
    Dataset { name: n::AREA_BORDERS, dtype: "u8 (bool)", shape: "(H, W)", presence: Always, doc: "Area border mask.", attrs: MAP_META },
    result_f64(n::ECCENTRICITY, Always, "Visual-field eccentricity (deg)."),
    result_f64(n::POLAR_ANGLE, Always, "Visual-field polar angle (deg, wrap ±180) about V1 center."),
    result_f64(n::MAGNIFICATION, Always, "Cortical magnification (px²/deg², ROI-masked)."),
    result_f64(n::MAGNIFICATION_RAW, Always, "Unmasked |det J| (deg²/px²); split-criterion input."),
    result_f64(n::MAGNIFICATION_AXIS, Always, "Magnification preferred axis (deg, wrap 180; SNLC prefAxisMF)."),
    result_f64(n::MAGNIFICATION_DISTORTION, Always, "Magnification distortion / anisotropy coherence [0,1] (SNLC Distrtion)."),
    Dataset { name: n::CONTOURS_AZI, dtype: "u8 (bool)", shape: "(H, W)", presence: Always, doc: "Azimuth iso-contour mask.", attrs: MAP_META },
    Dataset { name: n::CONTOURS_ALT, dtype: "u8 (bool)", shape: "(H, W)", presence: Always, doc: "Altitude iso-contour mask.", attrs: MAP_META },
    result_f64(n::SPECTRAL_SNR_AZI, When("raw acquisition path"), "Azimuth spectral SNR."),
    result_f64(n::SPECTRAL_SNR_ALT, When("raw acquisition path"), "Altitude spectral SNR."),
    result_f64(n::ALLEN_POWER_SNR_AZI, When("raw acquisition path"), "Azimuth Allen power SNR."),
    result_f64(n::ALLEN_POWER_SNR_ALT, When("raw acquisition path"), "Altitude Allen power SNR."),
    result_f64(n::RELIABILITY_AZI_FWD, When("raw acquisition path"), "Azimuth fwd cross-cycle reliability."),
    result_f64(n::RELIABILITY_AZI_REV, When("raw acquisition path"), "Azimuth rev cross-cycle reliability."),
    result_f64(n::RELIABILITY_ALT_FWD, When("raw acquisition path"), "Altitude fwd cross-cycle reliability."),
    result_f64(n::RELIABILITY_ALT_REV, When("raw acquisition path"), "Altitude rev cross-cycle reliability."),
    result_f64(n::AZI_DELAY, When("delay-subtraction cycle-combine"), "Azimuth hemodynamic delay (deg, SNLC delay_hor)."),
    result_f64(n::ALT_DELAY, When("delay-subtraction cycle-combine"), "Altitude hemodynamic delay (deg, SNLC delay_vert)."),
];

/// The canonical `.oisi` layout.
pub const SCHEMA: Schema = Schema {
    format_version: "1.0",
    root_attrs: &[
        Attr { name: n::VERSION, dtype: "VarLenUnicode", presence: Always, doc: "Format version (FORMAT_VERSION); gated by verify_format_version." },
        Attr { name: n::SOURCE_TYPE, dtype: "VarLenUnicode", presence: Always, doc: "Origin: \"raw_acquisition\" or an importer source tag." },
        Attr { name: n::CREATED_AT, dtype: "VarLenUnicode", presence: Always, doc: "ISO-8601 UTC creation timestamp." },
        Attr { name: n::SOFTWARE_VERSION, dtype: "VarLenUnicode", presence: When("raw acquisition capture"), doc: "OpenISI CARGO_PKG_VERSION at capture." },
        Attr { name: n::STIMULUS_METADATA, dtype: "VarLenUnicode (JSON)", presence: When("raw acquisition capture"), doc: "StimulusDataset export metadata, JSON." },
        Attr { name: n::RIG_PARAMS, dtype: "VarLenUnicode (JSON)", presence: When("raw acquisition capture"), doc: "serde JSON of the typed RigConfig." },
        Attr { name: n::EXPERIMENT_PARAMS, dtype: "VarLenUnicode (JSON)", presence: When("raw acquisition capture"), doc: "serde JSON of the typed ExperimentConfig." },
        Attr { name: n::ANALYSIS_PARAMS, dtype: "VarLenUnicode (JSON)", presence: When("after first analysis run"), doc: "serde JSON of the tagged AnalysisConfig." },
        Attr { name: n::ANIMAL_ID, dtype: "VarLenUnicode", presence: When("session metadata with a non-empty animal id"), doc: "Free-form animal identifier." },
        Attr { name: n::NOTES, dtype: "VarLenUnicode", presence: When("session metadata with non-empty notes"), doc: "Free-form session notes." },
    ],
    root_datasets: &[
        Dataset { name: n::ANATOMICAL, dtype: "u8", shape: "(H, W)", presence: When("an anatomical image was captured"), doc: "Anatomical (vasculature) image captured at session start.", attrs: &[] },
    ],
    groups: &[
        Group {
            path: "/hardware", presence: When("raw acquisition with a hardware snapshot"),
            doc: "Per-acquisition hardware snapshot (monitor + camera identity + viewing geometry).",
            attrs: HARDWARE_ATTRS, dynamic_attrs: None, datasets: &[], subgroups: &[],
        },
        Group {
            path: "/acquisition", presence: When("raw acquisition path"),
            doc: "Raw acquisition payload — camera frames, per-frame stimulus state, schedule, quality.",
            attrs: &[], dynamic_attrs: None, datasets: &[], subgroups: ACQUISITION_SUBGROUPS,
        },
        Group {
            path: "/complex_maps", presence: When("after first analysis run OR a complex-map import"),
            doc: "Per-orientation fwd/rev complex maps (real/imag split: f64 (H,W,2)).",
            attrs: &[], dynamic_attrs: None, datasets: COMPLEX_MAPS_DATASETS, subgroups: &[],
        },
        Group {
            path: "/results", presence: When("after first analysis run"),
            doc: "Analysis output maps. Every dataset except area_signs carries MapMeta attrs.",
            attrs: &[], dynamic_attrs: None, datasets: RESULTS_DATASETS, subgroups: &[],
        },
        Group {
            path: "/analysis_state", presence: When("after first analysis run (incremental cache populated)"),
            doc: "Incremental-cache fingerprints: one attribute per pipeline stage, BLAKE3 Merkle key of that stage's inputs.",
            attrs: &[], dynamic_attrs: Some("one VarLenUnicode attribute per cacheable stage, keyed by the stage's fingerprint_key (BLAKE3 hex)"),
            datasets: &[], subgroups: &[],
        },
        Group {
            path: "/cache", presence: When("an analysis run persisted the patch-threshold tail"),
            doc: "Non-result intermediates the incremental cache restores (regenerable; safe to delete).",
            attrs: &[Attr { name: n::THRESHOLD_APPLIED, dtype: "f64", presence: Always, doc: "The scalar |VFS| threshold PatchThreshold applied." }],
            dynamic_attrs: None,
            datasets: &[Dataset { name: n::IMSEG, dtype: "u8 (bool)", shape: "(H, W)", presence: Always, doc: "Binary candidate-patch mask (PatchThreshold output).", attrs: &[] }],
            subgroups: &[],
        },
    ],
};

// ---------------------------------------------------------------------------
// JSON Schema generation (docs/oisi.schema.json is generated from SCHEMA).
// ---------------------------------------------------------------------------

fn presence_json(p: Presence) -> Option<&'static str> {
    match p {
        Presence::Always => None,
        Presence::When(s) => Some(s),
    }
}

fn attr_json(a: &Attr) -> Value {
    let mut m = Map::new();
    m.insert("dtype".into(), json!(a.dtype));
    if let Some(w) = presence_json(a.presence) {
        m.insert("present_when".into(), json!(w));
    }
    m.insert("doc".into(), json!(a.doc));
    Value::Object(m)
}

fn attrs_json(attrs: &[Attr]) -> Value {
    let mut m = Map::new();
    for a in attrs {
        m.insert(a.name.into(), attr_json(a));
    }
    Value::Object(m)
}

fn dataset_json(d: &Dataset) -> Value {
    let mut m = Map::new();
    m.insert("dtype".into(), json!(d.dtype));
    m.insert("shape".into(), json!(d.shape));
    if let Some(w) = presence_json(d.presence) {
        m.insert("present_when".into(), json!(w));
    }
    m.insert("doc".into(), json!(d.doc));
    if !d.attrs.is_empty() {
        m.insert("attributes".into(), attrs_json(d.attrs));
    }
    Value::Object(m)
}

fn datasets_json(datasets: &[Dataset]) -> Value {
    let mut m = Map::new();
    for d in datasets {
        m.insert(d.name.into(), dataset_json(d));
    }
    Value::Object(m)
}

fn group_json(g: &Group) -> Value {
    let mut m = Map::new();
    m.insert("doc".into(), json!(g.doc));
    if let Some(w) = presence_json(g.presence) {
        m.insert("present_when".into(), json!(w));
    }
    if !g.attrs.is_empty() {
        m.insert("attributes".into(), attrs_json(g.attrs));
    }
    if let Some(pat) = g.dynamic_attrs {
        m.insert("dynamic_attributes".into(), json!(pat));
    }
    if !g.datasets.is_empty() {
        m.insert("datasets".into(), datasets_json(g.datasets));
    }
    if !g.subgroups.is_empty() {
        let mut sub = Map::new();
        for s in g.subgroups {
            sub.insert(s.path.into(), group_json(s));
        }
        m.insert("subgroups".into(), Value::Object(sub));
    }
    Value::Object(m)
}

// ---------------------------------------------------------------------------
// Contract checking: does a real .oisi conform to SCHEMA?
// ---------------------------------------------------------------------------

fn add_group_names(g: &Group, s: &mut BTreeSet<String>) {
    s.insert(g.path.rsplit('/').next().unwrap_or(g.path).to_string());
    for a in g.attrs {
        s.insert(a.name.to_string());
    }
    for d in g.datasets {
        s.insert(d.name.to_string());
        for a in d.attrs {
            s.insert(a.name.to_string());
        }
    }
    for sub in g.subgroups {
        add_group_names(sub, s);
    }
}

/// Every entity name (basename) SCHEMA documents.
fn documented_basenames() -> BTreeSet<String> {
    let mut s = BTreeSet::new();
    for a in SCHEMA.root_attrs {
        s.insert(a.name.to_string());
    }
    for d in SCHEMA.root_datasets {
        s.insert(d.name.to_string());
    }
    for g in SCHEMA.groups {
        add_group_names(g, &mut s);
    }
    s
}

/// Paths of groups whose attributes are dynamically named (e.g. `/analysis_state`).
fn dynamic_attr_paths() -> BTreeSet<String> {
    fn rec(g: &Group, s: &mut BTreeSet<String>) {
        if g.dynamic_attrs.is_some() {
            s.insert(g.path.to_string());
        }
        for sub in g.subgroups {
            rec(sub, s);
        }
    }
    let mut s = BTreeSet::new();
    for g in SCHEMA.groups {
        rec(g, &mut s);
    }
    s
}

fn collect_undocumented(
    path: &str,
    group: &hdf5::Group,
    documented: &BTreeSet<String>,
    dynamic: &BTreeSet<String>,
    out: &mut Vec<String>,
) {
    if !dynamic.contains(path) {
        for a in group.attr_names().unwrap_or_default() {
            if !documented.contains(&a) {
                out.push(format!("UNDOCUMENTED attribute {path}@{a}"));
            }
        }
    }
    for nm in group.member_names().unwrap_or_default() {
        let child = if path == "/" {
            format!("/{nm}")
        } else {
            format!("{path}/{nm}")
        };
        if let Ok(sub) = group.group(&nm) {
            if !documented.contains(&nm) {
                out.push(format!("UNDOCUMENTED group {child}"));
            }
            collect_undocumented(&child, &sub, documented, dynamic, out);
        } else if let Ok(ds) = group.dataset(&nm) {
            if !documented.contains(&nm) {
                out.push(format!("UNDOCUMENTED dataset {child}"));
            }
            for a in ds.attr_names().unwrap_or_default() {
                if !documented.contains(&a) {
                    out.push(format!("UNDOCUMENTED attribute {child}@{a}"));
                }
            }
        }
    }
}

fn is_always(p: Presence) -> bool {
    matches!(p, Presence::Always)
}

fn check_group_present(file: &hdf5::File, g: &Group, out: &mut Vec<String>) {
    let Ok(grp) = file.group(g.path.trim_start_matches('/')) else {
        return; // absent group → its own (conditional) presence not checked here
    };
    let attrs: BTreeSet<String> = grp.attr_names().unwrap_or_default().into_iter().collect();
    for a in g.attrs {
        if is_always(a.presence) && !attrs.contains(a.name) {
            out.push(format!("MISSING attribute {}@{}", g.path, a.name));
        }
    }
    let members: BTreeSet<String> = grp.member_names().unwrap_or_default().into_iter().collect();
    for d in g.datasets {
        if is_always(d.presence) && !members.contains(d.name) {
            out.push(format!("MISSING dataset {}/{}", g.path, d.name));
        }
    }
    for sub in g.subgroups {
        let present = file.group(sub.path.trim_start_matches('/')).is_ok();
        if is_always(sub.presence) && !present {
            out.push(format!("MISSING subgroup {}", sub.path));
        }
        if present {
            check_group_present(file, sub, out);
        }
    }
}

/// Check a real `.oisi` against [`SCHEMA`] in both directions and return all
/// contract violations (empty ⇒ the file conforms):
///
/// * **undocumented** — a group/dataset/attribute present in the file that
///   `SCHEMA` does not declare (catches "code grew a field, schema didn't");
/// * **missing** — an always-present `SCHEMA` entity absent from a group that
///   *is* present (catches "schema declares a field the code stopped writing").
///
/// `present_when` (conditional) entities are not required to be present.
/// `dynamic_attrs` group attributes (e.g. `/analysis_state` stage keys) are not
/// name-checked. Shared by the contract tests on both writer sides.
pub fn contract_violations(file: &hdf5::File) -> Vec<String> {
    let documented = documented_basenames();
    let dynamic = dynamic_attr_paths();
    let mut out = Vec::new();

    // Direction A — nothing present is undocumented (root attrs + the tree).
    for a in file.attr_names().unwrap_or_default() {
        if !documented.contains(&a) {
            out.push(format!("UNDOCUMENTED root attribute @{a}"));
        }
    }
    collect_undocumented("/", file, &documented, &dynamic, &mut out);

    // Direction B — every always-present documented entity exists.
    let root_attrs: BTreeSet<String> = file.attr_names().unwrap_or_default().into_iter().collect();
    for a in SCHEMA.root_attrs {
        if is_always(a.presence) && !root_attrs.contains(a.name) {
            out.push(format!("MISSING root attribute @{}", a.name));
        }
    }
    for g in SCHEMA.groups {
        check_group_present(file, g, &mut out);
    }
    out
}

/// Render [`SCHEMA`] to the canonical `docs/oisi.schema.json` value. This is the
/// generator the golden test pins the committed doc against.
pub fn to_json_schema() -> Value {
    let mut groups = Map::new();
    for g in SCHEMA.groups {
        groups.insert(g.path.into(), group_json(g));
    }
    let mut root_datasets = Map::new();
    for d in SCHEMA.root_datasets {
        root_datasets.insert(format!("/{}", d.name), dataset_json(d));
    }
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://openisi.dev/schema/oisi.schema.json",
        "title": "OpenISI .oisi file structure (HDF5)",
        "description": "GENERATED from crates/isi-analysis/src/oisi_schema.rs (SCHEMA) — do not hand-edit. Regenerate with OISI_REGEN_SCHEMA=1 cargo test -p isi-analysis --test oisi_schema_contract. Documents every group, dataset, and attribute an .oisi file can contain; `present_when` marks conditional entities.",
        "format_version": SCHEMA.format_version,
        "root_attributes": attrs_json(SCHEMA.root_attrs),
        "root_datasets": Value::Object(root_datasets),
        "groups": Value::Object(groups),
    })
}
