//! StimulusSequencer — Timing state machine for stimulus execution.
//!
//! Port of `stimulus_sequencer.gd`. Manages:
//! - Direction ordering (sequential/interleaved/randomized)
//! - Phase transitions (baseline → sweep → interval → ...)
//! - Repetition counting
//! - Timing signals for data synchronization
//!
//! Time-based: tracks elapsed seconds directly so timing is correct at ANY
//! rendering rate. Frame counts are kept as metrics only — never used for
//! state transitions.

use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

/// Sequencer states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum State {
    Idle,
    BaselineStart,
    Sweep,
    InterStimulus,
    InterDirection,
    BaselineEnd,
    Complete,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::Idle => "Idle",
            State::BaselineStart => "Baseline (Start)",
            State::Sweep => "Sweep",
            State::InterStimulus => "Inter-Stimulus",
            State::InterDirection => "Inter-Direction",
            State::BaselineEnd => "Baseline (End)",
            State::Complete => "Complete",
        }
    }
}

/// Sweep ordering mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    Sequential,
    Interleaved,
    Randomized,
}

impl Order {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "sequential" => Some(Order::Sequential),
            "interleaved" => Some(Order::Interleaved),
            "randomized" => Some(Order::Randomized),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Order::Sequential => "sequential",
            Order::Interleaved => "interleaved",
            Order::Randomized => "randomized",
        }
    }
}

/// Events emitted by the sequencer. Collected by the caller after each `advance()`.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    StateChanged { new_state: State, old_state: State },
    SweepStarted { sweep_index: usize, direction: String },
    SweepCompleted { sweep_index: usize, direction: String },
    DirectionChanged { new_direction: String, old_direction: String },
    SequenceStarted,
    SequenceCompleted,
}

/// Configuration snapshot taken at start(). Immutable for the duration of a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequencerConfig {
    pub conditions: Vec<String>,
    pub repetitions: u32,
    pub order: Order,
    pub baseline_start_sec: f64,
    pub baseline_end_sec: f64,
    pub inter_stimulus_sec: f64,
    pub inter_direction_sec: f64,
    /// Sweep duration in seconds (computed from display geometry + stimulus params).
    pub sweep_duration_sec: f64,
}

/// The stimulus sequencer state machine.
pub struct Sequencer {
    // Current state
    pub state: State,
    pub current_sweep_index: usize,
    pub current_direction: String,

    // Durations (from config, locked at start)
    baseline_start_sec: f64,
    sweep_duration_sec: f64,
    inter_stimulus_sec: f64,
    inter_direction_sec: f64,
    baseline_end_sec: f64,
    total_duration_sec: f64,

    // Elapsed time tracking
    total_elapsed_sec: f64,
    state_elapsed_sec: f64,

    // Frame counters (metrics only — NOT used for timing)
    total_frame_count: u64,
    state_frame_count: u64,

    // Sweep sequence — generated at start, locked for duration of run
    sweep_sequence: Vec<String>,

    // Condition occurrence tracking
    condition_counts: std::collections::HashMap<String, u32>,
    current_condition_occurrence: u32,

    // Event queue — drained by caller after each advance()
    events: Vec<Event>,
}

impl Sequencer {
    pub fn new() -> Self {
        Self {
            state: State::Idle,
            current_sweep_index: 0,
            current_direction: String::new(),
            baseline_start_sec: 0.0,
            sweep_duration_sec: 0.0,
            inter_stimulus_sec: 0.0,
            inter_direction_sec: 0.0,
            baseline_end_sec: 0.0,
            total_duration_sec: 0.0,
            total_elapsed_sec: 0.0,
            state_elapsed_sec: 0.0,
            total_frame_count: 0,
            state_frame_count: 0,
            sweep_sequence: Vec::new(),
            condition_counts: std::collections::HashMap::new(),
            current_condition_occurrence: 0,
            events: Vec::new(),
        }
    }

    /// Start sequence execution with the given configuration.
    pub fn start(&mut self, config: &SequencerConfig) {
        assert!(
            self.state == State::Idle,
            "Cannot start sequencer: already running (state={:?})",
            self.state
        );

        // Generate sweep sequence (locked for this run)
        self.sweep_sequence = generate_sweep_sequence(
            &config.conditions,
            config.repetitions,
            config.order,
        );

        if self.sweep_sequence.is_empty() {
            // No conditions configured — caller should have validated
            return;
        }

        // Snapshot durations
        self.baseline_start_sec = config.baseline_start_sec;
        self.sweep_duration_sec = config.sweep_duration_sec;
        self.inter_stimulus_sec = config.inter_stimulus_sec;
        self.inter_direction_sec = config.inter_direction_sec;
        self.baseline_end_sec = config.baseline_end_sec;
        self.total_duration_sec = self.compute_total_duration();

        // Initialize counters
        self.total_elapsed_sec = 0.0;
        self.state_elapsed_sec = 0.0;
        self.total_frame_count = 0;
        self.state_frame_count = 0;
        self.current_sweep_index = 0;
        self.condition_counts.clear();
        self.current_condition_occurrence = 0;
        self.events.clear();

        // Start with baseline if configured
        if self.baseline_start_sec > 0.0 {
            self.transition_to(State::BaselineStart);
        } else {
            self.start_next_sweep();
        }

        self.events.push(Event::SequenceStarted);
    }

    /// Advance timing by delta_sec (called at frame boundary).
    /// This is the ONLY place timing advances.
    pub fn advance(&mut self, delta_sec: f64) {
        assert!(delta_sec >= 0.0, "Negative delta_sec: {delta_sec}");

        if self.state == State::Idle || self.state == State::Complete {
            return;
        }

        self.total_elapsed_sec += delta_sec;
        self.state_elapsed_sec += delta_sec;
        self.total_frame_count += 1;
        self.state_frame_count += 1;

        // Check for state transitions (time-based)
        let duration = self.current_state_duration();
        if self.state_elapsed_sec >= duration {
            match self.state {
                State::BaselineStart => self.start_next_sweep(),
                State::Sweep => self.complete_current_sweep(),
                State::InterStimulus => self.start_next_sweep(),
                State::InterDirection => self.start_next_sweep(),
                State::BaselineEnd => {
                    self.transition_to(State::Complete);
                    self.events.push(Event::SequenceCompleted);
                }
                State::Idle | State::Complete => {
                    // No-op — advance() guard at the top should prevent reaching here,
                    // but if state just transitioned to Complete, this is harmless.
                }
            }
        }
    }

    /// Stop sequence execution.
    pub fn stop(&mut self) {
        self.transition_to(State::Idle);
        self.sweep_sequence.clear();
    }

    /// Drain all pending events.
    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// Check if sequence is running.
    pub fn is_running(&self) -> bool {
        self.state != State::Idle && self.state != State::Complete
    }

    /// Check if sequence is complete.
    pub fn is_complete(&self) -> bool {
        self.state == State::Complete
    }

    /// Get total sweep count.
    pub fn get_total_sweeps(&self) -> usize {
        self.sweep_sequence.len()
    }

    /// Get completed sweep count.
    pub fn get_completed_sweeps(&self) -> usize {
        if self.state == State::Complete {
            self.sweep_sequence.len()
        } else {
            self.current_sweep_index
        }
    }

    /// Get total duration in seconds.
    pub fn get_total_duration(&self) -> f64 {
        self.total_duration_sec
    }

    /// Get elapsed time in seconds.
    pub fn get_elapsed_time(&self) -> f64 {
        if self.state == State::Idle {
            0.0
        } else {
            self.total_elapsed_sec
        }
    }

    /// Get remaining time in seconds.
    pub fn get_remaining_time(&self) -> f64 {
        if !self.is_running() {
            return 0.0;
        }
        (self.total_duration_sec - self.total_elapsed_sec).max(0.0)
    }

    /// Get progress within current state (0.0–1.0).
    pub fn get_state_progress(&self) -> f64 {
        let dur = self.current_state_duration();
        if dur <= 0.0 {
            return 1.0;
        }
        (self.state_elapsed_sec / dur).clamp(0.0, 1.0)
    }

    /// Get frame index within current state.
    pub fn get_state_frame_index(&self) -> u64 {
        self.state_frame_count
    }

    /// Get total frame count.
    pub fn get_total_frame_count(&self) -> u64 {
        self.total_frame_count
    }

    /// Get the occurrence count for the current condition (1-indexed).
    pub fn get_current_condition_occurrence(&self) -> u32 {
        self.current_condition_occurrence
    }

    /// Get sweep direction at index.
    pub fn get_sweep_direction(&self, index: usize) -> Option<&str> {
        self.sweep_sequence.get(index).map(|s| s.as_str())
    }

    /// Check if currently in a baseline state (not actively showing stimulus).
    pub fn is_baseline(&self) -> bool {
        self.state != State::Sweep
    }

    /// Get the sweep sequence (read-only).
    pub fn sweep_sequence(&self) -> &[String] {
        &self.sweep_sequence
    }

    // --- Internal ---

    fn transition_to(&mut self, new_state: State) {
        let old_state = self.state;
        self.state = new_state;
        self.state_elapsed_sec = 0.0;
        self.state_frame_count = 0;
        self.events.push(Event::StateChanged {
            new_state,
            old_state,
        });
    }

    fn start_next_sweep(&mut self) {
        if self.current_sweep_index >= self.sweep_sequence.len() {
            // All sweeps complete
            if self.baseline_end_sec > 0.0 {
                self.transition_to(State::BaselineEnd);
            } else {
                self.transition_to(State::Complete);
                self.events.push(Event::SequenceCompleted);
            }
            return;
        }

        let new_direction = self.sweep_sequence[self.current_sweep_index].clone();

        // Handle blank trials
        if new_direction == "BLANK" {
            self.transition_to(State::InterStimulus);
            self.current_sweep_index += 1;
            return;
        }

        // Check for direction change
        let old_direction = self.current_direction.clone();
        if new_direction != old_direction && !old_direction.is_empty() {
            self.events.push(Event::DirectionChanged {
                new_direction: new_direction.clone(),
                old_direction,
            });
        }

        self.current_direction = new_direction.clone();

        // Track condition occurrence
        let count = self
            .condition_counts
            .entry(new_direction.clone())
            .or_insert(0);
        *count += 1;
        self.current_condition_occurrence = *count;

        self.transition_to(State::Sweep);
        self.events.push(Event::SweepStarted {
            sweep_index: self.current_sweep_index,
            direction: new_direction,
        });
    }

    fn complete_current_sweep(&mut self) {
        let direction = self.current_direction.clone();
        self.events.push(Event::SweepCompleted {
            sweep_index: self.current_sweep_index,
            direction: direction.clone(),
        });
        self.current_sweep_index += 1;

        if self.current_sweep_index >= self.sweep_sequence.len() {
            // All sweeps complete
            if self.baseline_end_sec > 0.0 {
                self.transition_to(State::BaselineEnd);
            } else {
                self.transition_to(State::Complete);
                self.events.push(Event::SequenceCompleted);
            }
            return;
        }

        // Determine next interval state
        let next_direction = &self.sweep_sequence[self.current_sweep_index];
        let next_dir = if next_direction == "BLANK" {
            ""
        } else {
            next_direction.as_str()
        };

        if next_dir != self.current_direction && !next_dir.is_empty() {
            // Direction change
            if self.inter_direction_sec > 0.0 {
                self.transition_to(State::InterDirection);
            } else {
                self.start_next_sweep();
            }
        } else {
            // Same direction
            if self.inter_stimulus_sec > 0.0 {
                self.transition_to(State::InterStimulus);
            } else {
                self.start_next_sweep();
            }
        }
    }

    fn current_state_duration(&self) -> f64 {
        match self.state {
            State::BaselineStart => self.baseline_start_sec,
            State::Sweep => self.sweep_duration_sec,
            State::InterStimulus => self.inter_stimulus_sec,
            State::InterDirection => self.inter_direction_sec,
            State::BaselineEnd => self.baseline_end_sec,
            // Idle and Complete have no duration — returning 0 causes immediate
            // transition check to pass, but advance() guards against these states.
            State::Idle | State::Complete => 0.0,
        }
    }

    fn compute_total_duration(&self) -> f64 {
        let mut total = self.baseline_start_sec + self.baseline_end_sec;
        let num_sweeps = self.sweep_sequence.len();
        total += num_sweeps as f64 * self.sweep_duration_sec;

        // Walk sequence for correct inter-direction vs inter-stimulus intervals
        for i in 1..num_sweeps {
            if self.sweep_sequence[i] != self.sweep_sequence[i - 1] {
                total += self.inter_direction_sec;
            } else {
                total += self.inter_stimulus_sec;
            }
        }

        total
    }
}

impl Default for Sequencer {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate the sweep sequence from configuration.
pub fn generate_sweep_sequence(
    conditions: &[String],
    repetitions: u32,
    order: Order,
) -> Vec<String> {
    let mut sequence = Vec::new();

    match order {
        Order::Interleaved => {
            for _ in 0..repetitions {
                for cond in conditions {
                    sequence.push(cond.clone());
                }
            }
        }
        Order::Randomized => {
            for cond in conditions {
                for _ in 0..repetitions {
                    sequence.push(cond.clone());
                }
            }
            let mut rng = rand::rng();
            sequence.shuffle(&mut rng);
        }
        Order::Sequential => {
            for cond in conditions {
                for _ in 0..repetitions {
                    sequence.push(cond.clone());
                }
            }
        }
    }

    sequence
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> SequencerConfig {
        SequencerConfig {
            conditions: vec!["LR".into(), "RL".into(), "UD".into(), "DU".into()],
            repetitions: 2,
            order: Order::Sequential,
            baseline_start_sec: 2.0,
            baseline_end_sec: 2.0,
            inter_stimulus_sec: 1.0,
            inter_direction_sec: 1.5,
            sweep_duration_sec: 5.0,
        }
    }

    /// Collect all sweep directions by running the full sequence.
    fn collect_sweep_directions(seq: &mut Sequencer) -> Vec<String> {
        let mut directions = Vec::new();
        let dt = 0.01;
        let max_iterations = 100_000;

        for _ in 0..max_iterations {
            if !seq.is_running() {
                break;
            }
            seq.advance(dt);
            for event in seq.drain_events() {
                if let Event::SweepStarted { direction, .. } = event {
                    directions.push(direction);
                }
            }
        }

        directions
    }

    // --- State ---

    #[test]
    fn test_starts_idle() {
        let seq = Sequencer::new();
        assert_eq!(seq.state, State::Idle);
        assert!(!seq.is_running());
        assert!(!seq.is_complete());
    }

    // --- Sweep Ordering ---

    #[test]
    fn test_sequential_order() {
        let config = SequencerConfig {
            conditions: vec!["LR".into(), "RL".into()],
            repetitions: 3,
            order: Order::Sequential,
            sweep_duration_sec: 1.0,
            baseline_start_sec: 0.0,
            baseline_end_sec: 0.0,
            inter_stimulus_sec: 0.0,
            inter_direction_sec: 0.0,
        };
        let mut seq = Sequencer::new();
        seq.start(&config);
        // Drain the initial SweepStarted from start()
        let mut directions = Vec::new();
        for event in seq.drain_events() {
            if let Event::SweepStarted { direction, .. } = event {
                directions.push(direction);
            }
        }
        // Collect remaining
        directions.extend(collect_sweep_directions(&mut seq));
        assert_eq!(
            directions,
            vec!["LR", "LR", "LR", "RL", "RL", "RL"],
            "Sequential: all reps of each condition in order"
        );
    }

    #[test]
    fn test_interleaved_order() {
        let config = SequencerConfig {
            conditions: vec!["LR".into(), "RL".into()],
            repetitions: 3,
            order: Order::Interleaved,
            sweep_duration_sec: 1.0,
            baseline_start_sec: 0.0,
            baseline_end_sec: 0.0,
            inter_stimulus_sec: 0.0,
            inter_direction_sec: 0.0,
        };
        let mut seq = Sequencer::new();
        seq.start(&config);
        let mut directions = Vec::new();
        for event in seq.drain_events() {
            if let Event::SweepStarted { direction, .. } = event {
                directions.push(direction);
            }
        }
        directions.extend(collect_sweep_directions(&mut seq));
        assert_eq!(
            directions,
            vec!["LR", "RL", "LR", "RL", "LR", "RL"],
            "Interleaved: cycle through conditions for each rep"
        );
    }

    #[test]
    fn test_randomized_order_has_correct_counts() {
        let config = SequencerConfig {
            conditions: vec!["A".into(), "B".into(), "C".into()],
            repetitions: 4,
            order: Order::Randomized,
            sweep_duration_sec: 1.0,
            baseline_start_sec: 0.0,
            baseline_end_sec: 0.0,
            inter_stimulus_sec: 0.0,
            inter_direction_sec: 0.0,
        };
        let mut seq = Sequencer::new();
        seq.start(&config);
        let mut directions = Vec::new();
        for event in seq.drain_events() {
            if let Event::SweepStarted { direction, .. } = event {
                directions.push(direction);
            }
        }
        directions.extend(collect_sweep_directions(&mut seq));
        assert_eq!(directions.len(), 12, "3 conditions x 4 reps = 12 sweeps");

        let mut counts = std::collections::HashMap::new();
        for d in &directions {
            *counts.entry(d.as_str()).or_insert(0) += 1;
        }
        assert_eq!(counts["A"], 4);
        assert_eq!(counts["B"], 4);
        assert_eq!(counts["C"], 4);
    }

    // --- State Transitions ---

    #[test]
    fn test_baseline_start_transition() {
        let config = SequencerConfig {
            conditions: vec!["LR".into()],
            repetitions: 1,
            order: Order::Sequential,
            baseline_start_sec: 3.0,
            sweep_duration_sec: 5.0,
            ..default_config()
        };
        let mut seq = Sequencer::new();
        seq.start(&config);
        seq.drain_events();
        assert_eq!(seq.state, State::BaselineStart);

        // Advance partway — still in baseline
        seq.advance(1.5);
        seq.drain_events();
        assert_eq!(seq.state, State::BaselineStart);

        // Advance past baseline
        seq.advance(2.0);
        seq.drain_events();
        assert_eq!(seq.state, State::Sweep);
    }

    #[test]
    fn test_no_baseline_skips_to_sweep() {
        let config = SequencerConfig {
            conditions: vec!["LR".into()],
            repetitions: 1,
            order: Order::Sequential,
            baseline_start_sec: 0.0,
            sweep_duration_sec: 5.0,
            ..default_config()
        };
        let mut seq = Sequencer::new();
        seq.start(&config);
        seq.drain_events();
        assert_eq!(seq.state, State::Sweep);
    }

    #[test]
    fn test_sweep_to_inter_stimulus_same_direction() {
        let config = SequencerConfig {
            conditions: vec!["LR".into()],
            repetitions: 2,
            order: Order::Sequential,
            baseline_start_sec: 0.0,
            baseline_end_sec: 0.0,
            inter_stimulus_sec: 1.0,
            inter_direction_sec: 1.5,
            sweep_duration_sec: 2.0,
        };
        let mut seq = Sequencer::new();
        seq.start(&config);
        seq.drain_events();
        assert_eq!(seq.state, State::Sweep);

        // Advance past first sweep
        seq.advance(2.01);
        seq.drain_events();
        assert_eq!(seq.state, State::InterStimulus);
    }

    #[test]
    fn test_sweep_to_inter_direction_different_direction() {
        let config = SequencerConfig {
            conditions: vec!["LR".into(), "RL".into()],
            repetitions: 1,
            order: Order::Sequential,
            baseline_start_sec: 0.0,
            baseline_end_sec: 0.0,
            inter_stimulus_sec: 0.5,
            inter_direction_sec: 2.0,
            sweep_duration_sec: 2.0,
        };
        let mut seq = Sequencer::new();
        seq.start(&config);
        seq.drain_events();

        // Advance past first sweep
        seq.advance(2.01);
        seq.drain_events();
        assert_eq!(seq.state, State::InterDirection);
    }

    #[test]
    fn test_full_sequence_completion() {
        let config = SequencerConfig {
            conditions: vec!["LR".into()],
            repetitions: 1,
            order: Order::Sequential,
            baseline_start_sec: 0.0,
            baseline_end_sec: 0.0,
            inter_stimulus_sec: 0.0,
            inter_direction_sec: 0.0,
            sweep_duration_sec: 2.0,
        };
        let mut seq = Sequencer::new();
        seq.start(&config);
        seq.drain_events();

        seq.advance(2.01);
        seq.drain_events();
        assert_eq!(seq.state, State::Complete);
        assert!(seq.is_complete());
        assert!(!seq.is_running());
    }

    #[test]
    fn test_baseline_end_before_complete() {
        let config = SequencerConfig {
            conditions: vec!["LR".into()],
            repetitions: 1,
            order: Order::Sequential,
            baseline_start_sec: 0.0,
            baseline_end_sec: 2.0,
            inter_stimulus_sec: 0.0,
            inter_direction_sec: 0.0,
            sweep_duration_sec: 2.0,
        };
        let mut seq = Sequencer::new();
        seq.start(&config);
        seq.drain_events();

        seq.advance(2.01);
        seq.drain_events();
        assert_eq!(seq.state, State::BaselineEnd);

        seq.advance(2.5);
        seq.drain_events();
        assert_eq!(seq.state, State::Complete);
    }

    // --- Duration Calculation ---

    #[test]
    fn test_total_duration_includes_inter_direction() {
        let config = SequencerConfig {
            conditions: vec!["LR".into(), "RL".into()],
            repetitions: 1,
            order: Order::Sequential,
            baseline_start_sec: 1.0,
            baseline_end_sec: 1.0,
            inter_stimulus_sec: 0.5,
            inter_direction_sec: 2.0,
            sweep_duration_sec: 3.0,
        };
        let mut seq = Sequencer::new();
        seq.start(&config);

        let total = seq.get_total_duration();
        // Expected: baseline_start + 2*sweep + inter_direction + baseline_end
        // (inter_stimulus not used because directions differ)
        let expected = 1.0 + 2.0 * 3.0 + 2.0 + 1.0;
        assert!(
            (total - expected).abs() < 0.001,
            "Total duration {total} != expected {expected}"
        );
    }

    // --- Condition Occurrence Tracking ---

    #[test]
    fn test_condition_occurrence_tracking() {
        let config = SequencerConfig {
            conditions: vec!["LR".into(), "RL".into()],
            repetitions: 2,
            order: Order::Interleaved,
            baseline_start_sec: 0.0,
            baseline_end_sec: 0.0,
            inter_stimulus_sec: 0.0,
            inter_direction_sec: 0.0,
            sweep_duration_sec: 1.0,
        };
        let mut seq = Sequencer::new();
        seq.start(&config);
        seq.drain_events();

        // First sweep (LR): occurrence 1
        assert_eq!(seq.get_current_condition_occurrence(), 1);
    }

    // --- Signal Emission ---

    #[test]
    fn test_sequence_started_event() {
        let config = default_config();
        let mut seq = Sequencer::new();
        seq.start(&config);

        let events = seq.drain_events();
        assert!(
            events.iter().any(|e| matches!(e, Event::SequenceStarted)),
            "Should emit SequenceStarted"
        );
    }

    #[test]
    fn test_sweep_started_event() {
        let config = SequencerConfig {
            baseline_start_sec: 0.0,
            ..default_config()
        };
        let mut seq = Sequencer::new();
        seq.start(&config);

        let events = seq.drain_events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::SweepStarted { .. })),
            "Should emit SweepStarted"
        );
    }

    #[test]
    fn test_sequence_completed_event() {
        let config = SequencerConfig {
            conditions: vec!["LR".into()],
            repetitions: 1,
            order: Order::Sequential,
            baseline_start_sec: 0.0,
            baseline_end_sec: 0.0,
            inter_stimulus_sec: 0.0,
            inter_direction_sec: 0.0,
            sweep_duration_sec: 1.0,
        };
        let mut seq = Sequencer::new();
        seq.start(&config);
        seq.drain_events();

        seq.advance(1.01);
        let events = seq.drain_events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::SequenceCompleted)),
            "Should emit SequenceCompleted"
        );
    }
}
