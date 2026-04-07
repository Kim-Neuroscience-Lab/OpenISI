//! StimulusDataset — Complete per-frame stimulus data for analysis.
//!
//! Port of `stimulus_dataset.gd`. Captures everything needed for downstream analysis:
//! - Hardware timestamps for each frame
//! - Sequence state (condition, sweep, progress)
//! - Timing quality metrics (jitter, dropped frames)
//!
//! Uses Vec-based storage with pre-allocation for zero-reallocation recording.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::geometry::DisplayGeometry;
use crate::sequencer::Order;

/// Frame state IDs — integer-encoded to avoid per-frame String allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum FrameState {
    Idle = 0,
    BaselineStart = 1,
    Stimulus = 2,
    InterStimulus = 3,
    InterDirection = 4,
    BaselineEnd = 5,
    Complete = 6,
}

impl FrameState {
    pub fn name(self) -> &'static str {
        match self {
            FrameState::Idle => "idle",
            FrameState::BaselineStart => "baseline_start",
            FrameState::Stimulus => "stimulus",
            FrameState::InterStimulus => "inter_stimulus",
            FrameState::InterDirection => "inter_direction",
            FrameState::BaselineEnd => "baseline_end",
            FrameState::Complete => "complete",
        }
    }

    pub fn from_sequencer_state(state: crate::sequencer::State) -> Self {
        match state {
            crate::sequencer::State::Idle => FrameState::Idle,
            crate::sequencer::State::BaselineStart => FrameState::BaselineStart,
            crate::sequencer::State::Sweep => FrameState::Stimulus,
            crate::sequencer::State::InterStimulus => FrameState::InterStimulus,
            crate::sequencer::State::InterDirection => FrameState::InterDirection,
            crate::sequencer::State::BaselineEnd => FrameState::BaselineEnd,
            crate::sequencer::State::Complete => FrameState::Complete,
        }
    }
}

/// Envelope type (stimulus shape).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnvelopeType {
    None = 0,
    Bar = 1,
    Wedge = 2,
    Ring = 3,
}

impl EnvelopeType {
    pub fn from_int(v: i32) -> Option<Self> {
        match v {
            0 => Some(EnvelopeType::None),
            1 => Some(EnvelopeType::Bar),
            2 => Some(EnvelopeType::Wedge),
            3 => Some(EnvelopeType::Ring),
            _ => None,
        }
    }

    pub fn stimulus_type_name(self) -> &'static str {
        match self {
            EnvelopeType::None => "full_field",
            EnvelopeType::Bar => "drifting_bar",
            EnvelopeType::Wedge => "rotating_wedge",
            EnvelopeType::Ring => "expanding_ring",
        }
    }
}

/// Dataset configuration — snapshot of all settings at recording start.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetConfig {
    /// Envelope type
    pub envelope: EnvelopeType,
    /// All stimulus parameters (from config JSON)
    pub stimulus_params: HashMap<String, serde_json::Value>,
    /// Conditions list
    pub conditions: Vec<String>,
    /// Number of repetitions
    pub repetitions: u32,
    /// Ordering
    pub order: Order,
    /// Timing
    pub baseline_start_sec: f64,
    pub baseline_end_sec: f64,
    pub inter_stimulus_sec: f64,
    pub inter_direction_sec: f64,
    /// Sweep duration (computed)
    pub sweep_duration_sec: f64,
    /// Display geometry snapshot
    pub geometry: DisplayGeometry,
    /// Display source ("edid", "user_override", etc.)
    pub display_physical_source: String,
    /// Refresh rate info
    pub reported_refresh_hz: f64,
    pub measured_refresh_hz: f64,
    pub target_stimulus_fps: u32,
    /// Number of initial frames to skip before dropped-frame detection.
    pub drop_detection_warmup_frames: usize,
    /// Dropped frame detection threshold as ratio of expected frame delta.
    pub drop_detection_threshold: f64,
    /// FPS calculation rolling window size.
    pub fps_window_frames: usize,
}

/// Per-frame record (append-only during recording).
#[derive(Debug, Clone)]
pub struct FrameRecord {
    pub timestamp_us: i64,
    pub condition_index: u8,
    pub sweep_index: u32,
    pub frame_in_sweep: u32,
    pub sweep_progress: f32,
    pub state_id: FrameState,
    pub condition_occurrence: u32,
    pub is_baseline: bool,
}


/// The stimulus dataset — accumulates per-frame data during acquisition.
pub struct StimulusDataset {
    // --- Metadata ---
    pub session_id: String,
    pub session_start_time: String,

    // --- Config snapshot ---
    config: DatasetConfig,
    stimulus_type: String,

    // --- Condition index map ---
    condition_index_map: HashMap<String, u8>,

    // --- Pre-computed sweep sequence ---
    pub sweep_sequence: Vec<String>,

    // --- Per-frame arrays (pre-allocated) ---
    pub timestamps_us: Vec<i64>,
    pub condition_indices: Vec<u8>,
    pub sweep_indices: Vec<u32>,
    pub frame_indices: Vec<u32>,
    pub progress: Vec<f32>,
    pub state_ids: Vec<u8>,
    pub condition_occurrences: Vec<u32>,
    pub is_baseline: Vec<u8>,

    // --- Timing quality ---
    pub frame_deltas_us: Vec<i64>,
    pub dropped_frame_indices: Vec<u32>,
    last_timestamp_us: i64,
    expected_delta_us: i64,

    // --- State ---
    frame_count: usize,
    frame_budget: usize,
    is_recording: bool,

    // --- Hardware timestamp tracking ---
    hardware_timestamps: bool,
    timestamps_finalized: bool,
    timestamp_source: String,
}

impl StimulusDataset {
    /// Create a new dataset from configuration.
    pub fn new(config: DatasetConfig) -> Self {
        let stimulus_type = config.envelope.stimulus_type_name().to_string();

        // Build condition index map
        let mut condition_index_map = HashMap::new();
        for (i, cond) in config.conditions.iter().enumerate() {
            condition_index_map.insert(cond.clone(), i as u8);
        }

        // Generate sweep sequence
        let sweep_sequence = crate::sequencer::generate_sweep_sequence(
            &config.conditions,
            config.repetitions,
            config.order,
        );

        // Compute expected delta
        let effective_fps = if config.target_stimulus_fps > 0 {
            config.target_stimulus_fps as f64
        } else {
            config.measured_refresh_hz
        };
        let expected_delta_us = (1_000_000.0 / effective_fps) as i64;

        Self {
            session_id: String::new(),
            session_start_time: String::new(),
            config,
            stimulus_type,
            condition_index_map,
            sweep_sequence,
            timestamps_us: Vec::new(),
            condition_indices: Vec::new(),
            sweep_indices: Vec::new(),
            frame_indices: Vec::new(),
            progress: Vec::new(),
            state_ids: Vec::new(),
            condition_occurrences: Vec::new(),
            is_baseline: Vec::new(),
            frame_deltas_us: Vec::new(),
            dropped_frame_indices: Vec::new(),
            last_timestamp_us: 0,
            expected_delta_us,
            frame_count: 0,
            frame_budget: 0,
            is_recording: false,
            hardware_timestamps: false,
            timestamps_finalized: false,
            timestamp_source: "software".to_string(),
        }
    }

    /// Start recording frames. Pre-allocates arrays for the expected duration.
    pub fn start_recording(&mut self) {
        if self.is_recording {
            return;
        }

        let budget = self.compute_frame_budget();
        self.preallocate(budget);

        self.is_recording = true;
        self.last_timestamp_us = 0;
        self.frame_count = 0;

        // Generate session ID from current time
        let now_us = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("System clock is before Unix epoch")
            .as_micros();
        self.session_id = format!("session_{now_us}");
    }

    /// Stop recording.
    pub fn stop_recording(&mut self) {
        self.is_recording = false;
    }

    /// Record a single frame.
    pub fn record_frame(&mut self, record: &FrameRecord) {
        assert!(self.is_recording, "record_frame() called while not recording");

        self.timestamps_us.push(record.timestamp_us);
        self.condition_indices.push(record.condition_index);
        self.sweep_indices.push(record.sweep_index);
        self.frame_indices.push(record.frame_in_sweep);
        self.progress.push(record.sweep_progress);
        self.state_ids.push(record.state_id as u8);
        self.condition_occurrences.push(record.condition_occurrence);
        self.is_baseline.push(if record.is_baseline { 1 } else { 0 });

        // Timing quality — compute frame delta
        if self.last_timestamp_us > 0 {
            let delta_us = record.timestamp_us - self.last_timestamp_us;
            self.frame_deltas_us.push(delta_us);

            // Detect dropped frames (delta > threshold * expected)
            if self.frame_count >= self.config.drop_detection_warmup_frames
                && (delta_us as f64) > self.expected_delta_us as f64 * self.config.drop_detection_threshold
            {
                self.dropped_frame_indices.push(self.frame_count as u32);
            }
        }

        self.last_timestamp_us = record.timestamp_us;
        self.frame_count += 1;
    }

    /// Get current frame count.
    pub fn frame_count(&self) -> usize {
        self.frame_count
    }

    /// Check if recording.
    pub fn is_recording(&self) -> bool {
        self.is_recording
    }

    /// Get the effective stimulus FPS.
    pub fn get_effective_stimulus_fps(&self) -> f64 {
        if self.config.target_stimulus_fps > 0 {
            self.config.target_stimulus_fps as f64
        } else {
            self.config.measured_refresh_hz
        }
    }

    /// Get current FPS from recent frame deltas.
    pub fn get_current_fps(&self) -> f64 {
        if self.frame_deltas_us.len() < 2 {
            return 0.0;
        }
        let count = self.frame_deltas_us.len().min(self.config.fps_window_frames);
        let start = self.frame_deltas_us.len() - count;
        let sum: i64 = self.frame_deltas_us[start..].iter().sum();
        let avg_us = sum as f64 / count as f64;
        if avg_us <= 0.0 {
            0.0
        } else {
            1_000_000.0 / avg_us
        }
    }

    /// Look up condition index from string.
    pub fn get_condition_index(&self, condition: &str) -> Option<u8> {
        self.condition_index_map.get(condition).copied()
    }

    /// Get condition string from index.
    pub fn get_condition_name(&self, index: u8) -> Option<&str> {
        self.config
            .conditions
            .get(index as usize)
            .map(|s| s.as_str())
    }

    /// Mark timestamps as hardware vsync timestamps.
    pub fn set_hardware_timestamps(&mut self, enabled: bool, source: &str) {
        self.hardware_timestamps = enabled;
        if !enabled {
            self.timestamp_source = "software".to_string();
        } else if !source.is_empty() {
            self.timestamp_source = source.to_string();
        } else {
            self.timestamp_source = "hardware".to_string();
        }
    }

    /// Get the config snapshot.
    pub fn config(&self) -> &DatasetConfig {
        &self.config
    }

    /// Get the stimulus type name.
    pub fn stimulus_type(&self) -> &str {
        &self.stimulus_type
    }

    /// Export metadata to a JSON-compatible structure.
    pub fn export_metadata(&self) -> serde_json::Value {
        serde_json::json!({
            "session": {
                "id": self.session_id,
                "start_time": self.session_start_time,
            },
            "stimulus": {
                "type": self.stimulus_type,
                "params": self.config.stimulus_params,
            },
            "sequence": {
                "conditions": self.config.conditions,
                "repetitions": self.config.repetitions,
                "order": self.config.order,
                "sweep_sequence": self.sweep_sequence,
            },
            "timing": {
                "baseline_start_sec": self.config.baseline_start_sec,
                "baseline_end_sec": self.config.baseline_end_sec,
                "inter_trial_sec": self.config.inter_stimulus_sec,
                "sweep_duration_sec": self.config.sweep_duration_sec,
            },
            "display": {
                "width_px": self.config.geometry.display_width_px,
                "height_px": self.config.geometry.display_height_px,
                "width_cm": self.config.geometry.display_width_cm,
                "height_cm": self.config.geometry.display_height_cm,
                "physical_source": self.config.display_physical_source,
                "reported_refresh_hz": self.config.reported_refresh_hz,
                "measured_refresh_hz": self.config.measured_refresh_hz,
                "target_stimulus_fps": self.config.target_stimulus_fps,
                "visual_field_width_deg": self.config.geometry.visual_field_width_deg(),
                "visual_field_height_deg": self.config.geometry.visual_field_height_deg(),
                "viewing_distance_cm": self.config.geometry.viewing_distance_cm,
                "center_azimuth_deg": self.config.geometry.center_azimuth_deg,
                "center_elevation_deg": self.config.geometry.center_elevation_deg,
                "projection": self.config.geometry.projection_type.as_str(),
            },
            "recording": {
                "frame_count": self.frame_count,
                "dropped_frames": self.dropped_frame_indices.len(),
                "hardware_timestamps": self.hardware_timestamps,
                "timestamps_finalized": self.timestamps_finalized,
                "timestamp_source": self.timestamp_source,
            },
        })
    }

    // --- Internal ---

    fn compute_frame_budget(&self) -> usize {
        let effective_fps = self.get_effective_stimulus_fps();
        let mut total_sec = self.config.baseline_start_sec + self.config.baseline_end_sec;
        total_sec += self.config.sweep_duration_sec * self.sweep_sequence.len() as f64;
        total_sec +=
            self.config.inter_stimulus_sec * (self.sweep_sequence.len().saturating_sub(1)) as f64;
        (total_sec * effective_fps * 1.1).ceil() as usize
    }

    fn preallocate(&mut self, budget: usize) {
        self.frame_budget = budget;
        if budget == 0 {
            return;
        }
        self.timestamps_us.reserve(budget);
        self.condition_indices.reserve(budget);
        self.sweep_indices.reserve(budget);
        self.frame_indices.reserve(budget);
        self.progress.reserve(budget);
        self.state_ids.reserve(budget);
        self.condition_occurrences.reserve(budget);
        self.is_baseline.reserve(budget);
        self.frame_deltas_us.reserve(budget);
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::ProjectionType;

    fn test_config() -> DatasetConfig {
        DatasetConfig {
            envelope: EnvelopeType::Bar,
            stimulus_params: HashMap::new(),
            conditions: vec!["LR".into(), "RL".into()],
            repetitions: 2,
            order: Order::Sequential,
            baseline_start_sec: 2.0,
            baseline_end_sec: 2.0,
            inter_stimulus_sec: 1.0,
            inter_direction_sec: 1.5,
            sweep_duration_sec: 5.0,
            geometry: DisplayGeometry::new(
                ProjectionType::Cartesian,
                25.0,
                0.0,
                0.0,
                53.0,
                30.0,
                1920,
                1080,
            ),
            display_physical_source: "edid".into(),
            reported_refresh_hz: 60.0,
            measured_refresh_hz: 59.94,
            target_stimulus_fps: 0,
            drop_detection_warmup_frames: 10,
            drop_detection_threshold: 1.5,
            fps_window_frames: 10,
        }
    }

    #[test]
    fn test_new_dataset() {
        let ds = StimulusDataset::new(test_config());
        assert_eq!(ds.stimulus_type(), "drifting_bar");
        assert_eq!(ds.frame_count(), 0);
        assert!(!ds.is_recording());
    }

    #[test]
    fn test_condition_index_map() {
        let ds = StimulusDataset::new(test_config());
        assert_eq!(ds.get_condition_index("LR"), Some(0));
        assert_eq!(ds.get_condition_index("RL"), Some(1));
        assert_eq!(ds.get_condition_index("NOPE"), None);
    }

    #[test]
    fn test_sweep_sequence_generated() {
        let ds = StimulusDataset::new(test_config());
        // Sequential: LR, LR, RL, RL
        assert_eq!(ds.sweep_sequence, vec!["LR", "LR", "RL", "RL"]);
    }

    #[test]
    fn test_start_stop_recording() {
        let mut ds = StimulusDataset::new(test_config());
        ds.start_recording();
        assert!(ds.is_recording());
        assert!(!ds.session_id.is_empty());

        ds.stop_recording();
        assert!(!ds.is_recording());
    }

    #[test]
    fn test_record_frames() {
        let mut ds = StimulusDataset::new(test_config());
        ds.start_recording();

        let base_ts = 1_000_000i64; // 1 second in us
        let delta = 16_667i64; // ~60fps

        for i in 0..100 {
            ds.record_frame(&FrameRecord {
                timestamp_us: base_ts + delta * i,
                condition_index: 0,
                sweep_index: 0,
                frame_in_sweep: i as u32,
                sweep_progress: i as f32 / 100.0,
                state_id: FrameState::Stimulus,
                condition_occurrence: 1,
                is_baseline: false,
            });
        }

        assert_eq!(ds.frame_count(), 100);
        assert_eq!(ds.timestamps_us.len(), 100);
        assert_eq!(ds.frame_deltas_us.len(), 99); // N-1 deltas
    }

    #[test]
    fn test_dropped_frame_detection() {
        let cfg = test_config();
        let warmup = cfg.drop_detection_warmup_frames;
        let mut ds = StimulusDataset::new(cfg);
        ds.start_recording();

        let delta = 16_667i64; // ~60fps
        let base_ts = 1_000_000i64;

        // Record warmup frames (won't trigger drop detection)
        for i in 0..warmup {
            ds.record_frame(&FrameRecord {
                timestamp_us: base_ts + delta * i as i64,
                condition_index: 0,
                sweep_index: 0,
                frame_in_sweep: i as u32,
                sweep_progress: 0.0,
                state_id: FrameState::Stimulus,
                condition_occurrence: 1,
                is_baseline: false,
            });
        }

        // Record a normal frame
        let ts = base_ts + delta * warmup as i64;
        ds.record_frame(&FrameRecord {
            timestamp_us: ts,
            condition_index: 0,
            sweep_index: 0,
            frame_in_sweep: warmup as u32,
            sweep_progress: 0.0,
            state_id: FrameState::Stimulus,
            condition_occurrence: 1,
            is_baseline: false,
        });

        // Record a dropped frame (3x expected delta)
        ds.record_frame(&FrameRecord {
            timestamp_us: ts + delta * 3,
            condition_index: 0,
            sweep_index: 0,
            frame_in_sweep: (warmup + 1) as u32,
            sweep_progress: 0.0,
            state_id: FrameState::Stimulus,
            condition_occurrence: 1,
            is_baseline: false,
        });

        assert_eq!(
            ds.dropped_frame_indices.len(),
            1,
            "Should detect one dropped frame"
        );
    }

    #[test]
    fn test_effective_fps() {
        let config = test_config();
        let ds = StimulusDataset::new(config);
        // target_stimulus_fps = 0, so should use measured_refresh_hz
        assert!((ds.get_effective_stimulus_fps() - 59.94).abs() < 0.01);

        let mut config2 = test_config();
        config2.target_stimulus_fps = 30;
        let ds2 = StimulusDataset::new(config2);
        assert!((ds2.get_effective_stimulus_fps() - 30.0).abs() < 0.01);
    }

    #[test]
    fn test_export_metadata() {
        let ds = StimulusDataset::new(test_config());
        let meta = ds.export_metadata();
        assert_eq!(meta["stimulus"]["type"], "drifting_bar");
        assert_eq!(meta["sequence"]["conditions"][0], "LR");
        assert_eq!(meta["display"]["projection"], "cartesian");
    }
}
