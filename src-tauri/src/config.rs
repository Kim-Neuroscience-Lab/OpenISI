//! Configuration (TOML).
//!
//! Two config files:
//! - `rig.toml` — per-installation rig settings (hardware, geometry, system tuning)
//! - `experiment.toml` — current working experiment (stimulus, presentation, timing)
//!
//! Config files are the ONLY source of truth. No hardcoded defaults in code.
//! All fields are required — if a field is missing from the TOML file, parsing fails.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// =============================================================================
// Shared enums — string-serialized, used across rig and experiment configs
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    Cartesian,
    Spherical,
    Cylindrical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Envelope {
    Bar,
    Wedge,
    Ring,
    Fullfield,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Carrier {
    Solid,
    Checkerboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Structure {
    Blocked,
    Interleaved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    Sequential,
    Interleaved,
    Randomized,
}

// =============================================================================
// Rig Config (rig.toml) — per-installation settings
// =============================================================================

/// Top-level rig configuration. Loaded from `config/rig.toml`.
/// All fields required — no serde defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RigConfig {
    pub camera: CameraDefaults,
    pub geometry: RigGeometry,
    pub ring_overlay: RingOverlay,
    pub display: DisplaySettings,
    pub analysis: AnalysisDefaults,
    pub system: SystemTuning,
    pub ui: UiPreferences,
    pub window: WindowState,
    pub paths: Paths,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraDefaults {
    pub exposure_us: u32,
    pub gain: i32,
    pub target_fps: f64,
    /// Pixel binning factor (1=none, 2=2x2, 4=4x4). Applied symmetrically.
    pub binning: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RigGeometry {
    pub viewing_distance_cm: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RingOverlay {
    pub enabled: bool,
    pub radius_px: u32,
    pub center_x_px: u32,
    pub center_y_px: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplaySettings {
    pub target_stimulus_fps: u32,
    pub monitor_rotation_deg: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisDefaults {
    pub smoothing_sigma: f64,
    pub rotation_k: i32,
    pub azi_angular_range: f64,
    pub alt_angular_range: f64,
    pub offset_azi: f64,
    pub offset_alt: f64,
    pub epsilon: f64,
    #[serde(default)]
    pub segmentation: Option<SegmentationDefaults>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentationDefaults {
    pub sign_map_filter_sigma: f64,
    pub sign_map_threshold: f64,
    pub open_radius: usize,
    pub close_radius: usize,
    pub dilate_radius: usize,
    pub pad_border: usize,
    pub spur_iterations: usize,
    pub split_overlap_threshold: f64,
    pub merge_overlap_threshold: f64,
    pub merge_dilate_radius: usize,
    pub merge_close_radius: usize,
    pub eccentricity_radius: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemTuning {
    pub camera_frame_send_interval_ms: u32,
    pub camera_poll_interval_ms: u32,
    pub camera_first_frame_timeout_ms: u32,
    pub camera_first_frame_poll_ms: u32,
    pub display_validation_sample_count: u32,
    pub preview_width_px: u32,
    pub preview_interval_ms: u32,
    pub preview_cycle_sec: f64,
    pub idle_sleep_ms: u32,
    pub fps_window_frames: usize,
    pub drop_detection_warmup_frames: usize,
    pub drop_detection_threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiPreferences {
    pub show_debug_overlay: bool,
    pub show_timing_info: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    pub maximized: bool,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paths {
    pub data_directory: String,
    pub experiments_directory: String,
    pub last_experiment_path: String,
}

// =============================================================================
// Rig Config I/O
// =============================================================================

impl RigConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        toml::from_str(&contents)
            .map_err(|e| format!("Failed to parse {}: {e}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let toml_str = toml::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize rig config: {e}"))?;
        std::fs::write(path, toml_str)
            .map_err(|e| format!("Failed to write {}: {e}", path.display()))
    }
}

// =============================================================================
// Experiment (experiment.toml + saved .experiment.toml files)
// =============================================================================

/// Experiment definition — what stimulus to present and how.
/// Used for both the working experiment (experiment.toml) and saved experiment files.
/// Metadata fields (name, description, timestamps) are present in saved files only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experiment {
    /// Human-readable name. Present in saved experiment files only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Description. Present in saved experiment files only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// ISO 8601 creation timestamp. Present in saved experiment files only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,

    /// ISO 8601 last-modified timestamp. Present in saved experiment files only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,

    pub geometry: ExperimentGeometry,
    pub stimulus: StimulusSpec,
    pub presentation: PresentationSpec,
    pub timing: TimingSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentGeometry {
    pub horizontal_offset_deg: f64,
    pub vertical_offset_deg: f64,
    pub projection: Projection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StimulusSpec {
    pub envelope: Envelope,
    pub carrier: Carrier,
    pub params: StimulusParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StimulusParams {
    // Carrier
    pub contrast: f64,
    pub mean_luminance: f64,
    pub background_luminance: f64,
    pub check_size_deg: f64,
    pub check_size_cm: f64,
    pub strobe_frequency_hz: f64,

    // Envelope (all present; only relevant ones used at runtime based on envelope type)
    pub stimulus_width_deg: f64,
    pub sweep_speed_deg_per_sec: f64,
    pub rotation_speed_deg_per_sec: f64,
    pub expansion_speed_deg_per_sec: f64,
    pub rotation_deg: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresentationSpec {
    pub conditions: Vec<String>,
    pub repetitions: u32,
    pub structure: Structure,
    pub order: Order,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingSpec {
    pub baseline_start_sec: f64,
    pub baseline_end_sec: f64,
    pub inter_stimulus_sec: f64,
    pub inter_direction_sec: f64,
}

// =============================================================================
// Experiment I/O
// =============================================================================

impl Experiment {
    pub fn load(path: &Path) -> Result<Self, String> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        toml::from_str(&contents)
            .map_err(|e| format!("Failed to parse {}: {e}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let toml_str = toml::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize experiment: {e}"))?;
        std::fs::write(path, toml_str)
            .map_err(|e| format!("Failed to write {}: {e}", path.display()))
    }
}

// =============================================================================
// Config Manager — manages rig.toml + experiment discovery
// =============================================================================

/// Manages the rig config file and experiment directory.
pub struct ConfigManager {
    config_dir: PathBuf,
    pub rig: RigConfig,
}

impl ConfigManager {
    /// Load rig config from `<config_dir>/rig.toml`.
    /// Fails if rig.toml doesn't exist — no fallbacks.
    pub fn load(config_dir: &Path) -> Result<Self, String> {
        let toml_path = config_dir.join("rig.toml");
        let rig = RigConfig::load(&toml_path)?;

        Ok(Self {
            config_dir: config_dir.to_path_buf(),
            rig,
        })
    }

    /// Path to rig.toml.
    pub fn rig_path(&self) -> PathBuf {
        self.config_dir.join("rig.toml")
    }

    /// Path to experiment.toml (the current working experiment).
    pub fn experiment_path(&self) -> PathBuf {
        self.config_dir.join("experiment.toml")
    }

    /// Path to saved experiments directory.
    pub fn experiments_dir(&self) -> PathBuf {
        if !self.rig.paths.experiments_directory.is_empty() {
            PathBuf::from(&self.rig.paths.experiments_directory)
        } else {
            self.config_dir.join("experiments")
        }
    }

    /// Save the current rig config to disk.
    pub fn save(&self) -> Result<(), String> {
        self.rig.save(&self.rig_path())
    }

    /// List available saved experiment files.
    pub fn list_experiments(&self) -> Vec<PathBuf> {
        let dir = self.experiments_dir();
        if !dir.exists() {
            return Vec::new();
        }
        let mut experiments = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "toml")
                    && path.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.ends_with(".experiment.toml"))
                {
                    experiments.push(path);
                }
            }
        }
        experiments.sort();
        experiments
    }

    /// Config directory accessor.
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }
}

// =============================================================================
// Enum helpers for renderer integration
// =============================================================================

impl Envelope {
    /// Convert to the integer used by the WGSL shader.
    /// This is the ONLY place enum→integer conversion happens.
    pub fn to_shader_int(self) -> i32 {
        match self {
            Envelope::Fullfield => 0,
            Envelope::Bar => 1,
            Envelope::Wedge => 2,
            Envelope::Ring => 3,
        }
    }
}

impl Carrier {
    pub fn to_shader_int(self) -> i32 {
        match self {
            Carrier::Solid => 0,
            Carrier::Checkerboard => 1,
        }
    }
}

impl Projection {
    pub fn to_shader_int(self) -> i32 {
        match self {
            Projection::Cartesian => 0,
            Projection::Spherical => 1,
            Projection::Cylindrical => 2,
        }
    }
}

impl Order {
    /// Convert to the sequencer crate's Order type.
    pub fn to_sequencer_order(self) -> openisi_stimulus::sequencer::Order {
        match self {
            Order::Sequential => openisi_stimulus::sequencer::Order::Sequential,
            Order::Interleaved => openisi_stimulus::sequencer::Order::Interleaved,
            Order::Randomized => openisi_stimulus::sequencer::Order::Randomized,
        }
    }
}

// =============================================================================
// Luminance helpers
// =============================================================================

impl StimulusParams {
    /// Compute luminance high (for checkerboard bright squares / solid carrier).
    /// luminance_high = mean_luminance + contrast * mean_luminance
    pub fn luminance_high(&self) -> f64 {
        self.mean_luminance + self.contrast * self.mean_luminance
    }

    /// Compute luminance low (for checkerboard dark squares).
    /// luminance_low = mean_luminance - contrast * mean_luminance
    pub fn luminance_low(&self) -> f64 {
        self.mean_luminance - self.contrast * self.mean_luminance
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rig_toml_round_trips() {
        let config_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../config");
        let toml_path = config_dir.join("rig.toml");
        let cfg = RigConfig::load(&toml_path).expect("rig.toml should parse");
        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let _parsed: RigConfig = toml::from_str(&toml_str).expect("round-trip should succeed");
    }

    #[test]
    fn experiment_toml_round_trips() {
        let config_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../config");
        let toml_path = config_dir.join("experiment.toml");
        let cfg = Experiment::load(&toml_path).expect("experiment.toml should parse");
        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let _parsed: Experiment = toml::from_str(&toml_str).expect("round-trip should succeed");
    }

    #[test]
    fn luminance_computation() {
        let params = StimulusParams {
            contrast: 1.0,
            mean_luminance: 0.5,
            background_luminance: 0.0,
            check_size_deg: 5.0,
            check_size_cm: 1.0,
            strobe_frequency_hz: 0.0,
            stimulus_width_deg: 20.0,
            sweep_speed_deg_per_sec: 9.0,
            rotation_speed_deg_per_sec: 15.0,
            expansion_speed_deg_per_sec: 5.0,
            rotation_deg: 0.0,
        };
        assert!((params.luminance_high() - 1.0).abs() < 1e-10);
        assert!((params.luminance_low() - 0.0).abs() < 1e-10);
    }

    #[test]
    fn string_enums_serialize_correctly() {
        assert_eq!(
            serde_json::to_string(&Envelope::Bar).unwrap(),
            "\"bar\""
        );
        assert_eq!(
            serde_json::to_string(&Carrier::Checkerboard).unwrap(),
            "\"checkerboard\""
        );
        assert_eq!(
            serde_json::to_string(&Projection::Spherical).unwrap(),
            "\"spherical\""
        );
        assert_eq!(
            serde_json::to_string(&Order::Randomized).unwrap(),
            "\"randomized\""
        );
    }
}
