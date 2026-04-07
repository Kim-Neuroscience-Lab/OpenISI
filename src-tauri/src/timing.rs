//! Timing validation — characterizes the phase relationship between camera
//! and stimulus clocks before acquisition begins.
//!
//! Two periodic processes (camera at f_cam, stimulus at f_stim) run on
//! independent oscillators. The phase of stimulus onset within the camera
//! frame interval determines sub-frame timing bias. This module measures
//! both rates, computes the beat period, classifies the timing regime,
//! and produces a characterization block for session metadata.

use serde::Serialize;

/// Result of timing validation — travels with the data.
#[derive(Debug, Clone, Serialize)]
pub struct TimingCharacterization {
    /// Measured camera frame rate (Hz), from hardware timestamps.
    pub f_cam_hz: f64,
    /// Measured stimulus presentation rate (Hz), from vsync timestamps.
    pub f_stim_hz: f64,

    /// Camera frame period (seconds).
    pub t_cam_sec: f64,
    /// Stimulus frame period (seconds).
    pub t_stim_sec: f64,

    /// Rate ratio f_stim / f_cam.
    pub rate_ratio: f64,

    /// Beat period (seconds) = 1 / |f_cam - f_stim|.
    /// Time for the phase relationship to complete one full cycle.
    pub beat_period_sec: f64,

    /// Phase increment per stimulus onset (fraction of camera frame interval).
    /// The amount the onset phase advances with each stimulus period.
    pub phase_increment: f64,

    /// Timing regime classification.
    pub regime: TimingRegime,

    /// Expected number of distinct phase values sampled across all trials.
    pub expected_phase_samples: f64,

    /// Fraction of the camera frame interval covered by phase sampling
    /// across the full session (0..1). Higher is better.
    pub phase_coverage: f64,

    /// Onset uncertainty (seconds) — driven by clock offset measurement
    /// uncertainty and vsync jitter.
    pub onset_uncertainty_sec: f64,
    /// Onset uncertainty as fraction of camera frame interval.
    pub onset_uncertainty_fraction: f64,

    /// Warnings for the user.
    pub warnings: Vec<String>,

    /// Number of camera frames measured.
    pub cam_sample_count: u32,
    /// Number of stimulus vsyncs measured.
    pub stim_sample_count: u32,
    /// Camera frame rate jitter (std dev, seconds).
    pub cam_jitter_sec: f64,
    /// Stimulus vsync jitter (std dev, seconds).
    pub stim_jitter_sec: f64,
}

/// Timing regime classification.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum TimingRegime {
    /// Beat period < inter-trial interval. Phase cycles many times per session.
    /// Sub-frame jitter averages out. Favorable.
    Uniform,
    /// Beat period > session duration. Phase barely moves. Systematic bias.
    /// Hard warning — every trial sees the same sub-frame onset position.
    Systematic,
    /// Beat period between inter-trial interval and session duration.
    /// Partial phase coverage. Manageable but requires logging.
    Partial,
}

/// Input parameters for timing characterization.
pub struct TimingParams {
    /// Number of trials in the session.
    pub n_trials: usize,
    /// Inter-trial interval (seconds). Time between consecutive stimulus onsets.
    pub inter_trial_sec: f64,
    /// Total session duration (seconds).
    pub session_duration_sec: f64,
}

/// Compute timing characterization from measured rates.
pub fn characterize_timing(
    cam_deltas_us: &[f64],
    stim_deltas_us: &[f64],
    clock_offset_uncertainty_us: f64,
    params: &TimingParams,
) -> TimingCharacterization {
    // Compute measured rates from deltas.
    let cam_n = cam_deltas_us.len() as f64;
    let cam_mean_us = cam_deltas_us.iter().sum::<f64>() / cam_n;
    let f_cam = 1_000_000.0 / cam_mean_us;
    let t_cam = cam_mean_us / 1_000_000.0;

    let stim_n = stim_deltas_us.len() as f64;
    let stim_mean_us = stim_deltas_us.iter().sum::<f64>() / stim_n;
    let f_stim = 1_000_000.0 / stim_mean_us;
    let t_stim = stim_mean_us / 1_000_000.0;

    // Jitter (std dev).
    let cam_variance = cam_deltas_us.iter()
        .map(|d| (d - cam_mean_us).powi(2))
        .sum::<f64>() / cam_n;
    let cam_jitter_sec = cam_variance.sqrt() / 1_000_000.0;

    let stim_variance = stim_deltas_us.iter()
        .map(|d| (d - stim_mean_us).powi(2))
        .sum::<f64>() / stim_n;
    let stim_jitter_sec = stim_variance.sqrt() / 1_000_000.0;

    // Rate ratio.
    let rate_ratio = f_stim / f_cam;

    // Beat period.
    let freq_diff = (f_cam - f_stim).abs();
    let beat_period_sec = if freq_diff > 1e-10 {
        1.0 / freq_diff
    } else {
        f64::INFINITY // Rates are identical — phase is locked.
    };

    // Phase increment: stimulus period modulo camera frame period,
    // expressed as fraction of camera frame interval.
    let phase_increment = (t_stim % t_cam) / t_cam;
    // Normalize to [0, 1)
    let phase_increment = phase_increment - phase_increment.floor();

    // Classify regime.
    let regime = if beat_period_sec < params.inter_trial_sec {
        TimingRegime::Uniform
    } else if beat_period_sec > params.session_duration_sec {
        TimingRegime::Systematic
    } else {
        TimingRegime::Partial
    };

    // Expected phase coverage.
    // Number of distinct phase values = session_duration / beat_period (capped at 1 full cycle).
    let phase_cycles = params.session_duration_sec / beat_period_sec;
    let phase_coverage = phase_cycles.min(1.0);
    let expected_phase_samples = (params.n_trials as f64 * phase_coverage).min(params.n_trials as f64);

    // Onset uncertainty from clock offset measurement + vsync jitter.
    let onset_uncertainty_sec = ((clock_offset_uncertainty_us / 1_000_000.0).powi(2)
        + stim_jitter_sec.powi(2))
        .sqrt();
    let onset_uncertainty_fraction = onset_uncertainty_sec / t_cam;

    // Warnings.
    let mut warnings = Vec::new();
    if regime == TimingRegime::Systematic {
        warnings.push(format!(
            "SYSTEMATIC TIMING: Beat period ({:.1}s) exceeds session duration ({:.1}s). \
             Phase barely moves across trials — every trial sees approximately the same \
             sub-frame onset position. This adds a consistent bias that cannot be detected \
             or corrected without additional information.",
            beat_period_sec, params.session_duration_sec
        ));
    }
    if regime == TimingRegime::Partial && phase_coverage < 0.5 {
        warnings.push(format!(
            "LOW PHASE COVERAGE: Only {:.0}% of the camera frame interval is sampled \
             across the session. Sub-frame timing bias is partially averaged.",
            phase_coverage * 100.0
        ));
    }
    if onset_uncertainty_fraction > 0.5 {
        warnings.push(format!(
            "HIGH ONSET UNCERTAINTY: Onset position uncertainty ({:.1}µs) exceeds \
             50% of camera frame interval ({:.1}µs). Per-trial phase assignment is unreliable.",
            onset_uncertainty_sec * 1_000_000.0, t_cam * 1_000_000.0
        ));
    }

    TimingCharacterization {
        f_cam_hz: f_cam,
        f_stim_hz: f_stim,
        t_cam_sec: t_cam,
        t_stim_sec: t_stim,
        rate_ratio,
        beat_period_sec,
        phase_increment,
        regime,
        expected_phase_samples,
        phase_coverage,
        onset_uncertainty_sec,
        onset_uncertainty_fraction,
        warnings,
        cam_sample_count: cam_deltas_us.len() as u32,
        stim_sample_count: stim_deltas_us.len() as u32,
        cam_jitter_sec,
        stim_jitter_sec,
    }
}

impl std::fmt::Display for TimingRegime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TimingRegime::Uniform => write!(f, "uniform"),
            TimingRegime::Systematic => write!(f, "SYSTEMATIC"),
            TimingRegime::Partial => write!(f, "partial"),
        }
    }
}

impl std::fmt::Display for TimingCharacterization {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Camera:    {:.3} Hz (jitter {:.1}µs, {} samples)",
            self.f_cam_hz, self.cam_jitter_sec * 1e6, self.cam_sample_count)?;
        writeln!(f, "Stimulus:  {:.3} Hz (jitter {:.1}µs, {} samples)",
            self.f_stim_hz, self.stim_jitter_sec * 1e6, self.stim_sample_count)?;
        writeln!(f, "Ratio:     {:.6} (f_stim / f_cam)", self.rate_ratio)?;
        writeln!(f, "Beat:      {:.3}s", self.beat_period_sec)?;
        writeln!(f, "Regime:    {}", self.regime)?;
        writeln!(f, "Phase:     increment={:.6}, coverage={:.1}%, samples={:.0}",
            self.phase_increment, self.phase_coverage * 100.0, self.expected_phase_samples)?;
        writeln!(f, "Onset:     ±{:.1}µs ({:.1}% of frame interval)",
            self.onset_uncertainty_sec * 1e6, self.onset_uncertainty_fraction * 100.0)?;
        for w in &self.warnings {
            writeln!(f, "WARNING:   {w}")?;
        }
        Ok(())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uniform_regime() {
        // Camera 20fps, stimulus 60Hz. Beat = 1/40 = 25ms. Very short.
        let cam_deltas: Vec<f64> = vec![50000.0; 100]; // 50ms = 20fps
        let stim_deltas: Vec<f64> = vec![16666.7; 300]; // 16.67ms = 60Hz
        let params = TimingParams {
            n_trials: 40,
            inter_trial_sec: 8.0,
            session_duration_sec: 320.0,
        };
        let tc = characterize_timing(&cam_deltas, &stim_deltas, 10.0, &params);
        assert_eq!(tc.regime, TimingRegime::Uniform);
        assert!((tc.f_cam_hz - 20.0).abs() < 0.1);
        assert!((tc.f_stim_hz - 60.0).abs() < 0.1);
        assert!(tc.beat_period_sec < 0.1);
    }

    #[test]
    fn test_systematic_regime() {
        // Camera 30fps, stimulus 30.001Hz. Beat = 1/0.001 = 1000s.
        // Session is 320s. Phase barely moves.
        let cam_deltas: Vec<f64> = vec![33333.3; 100]; // 33.33ms = 30fps
        let stim_deltas: Vec<f64> = vec![33332.2; 300]; // 1e6/33332.2 ≈ 30.001Hz
        let params = TimingParams {
            n_trials: 40,
            inter_trial_sec: 8.0,
            session_duration_sec: 320.0,
        };
        let tc = characterize_timing(&cam_deltas, &stim_deltas, 10.0, &params);
        assert_eq!(tc.regime, TimingRegime::Systematic);
        assert!(tc.beat_period_sec > 320.0);
        assert!(!tc.warnings.is_empty());
    }

    #[test]
    fn test_partial_regime() {
        // Camera 30fps, stimulus 30.02Hz. Beat = 1/0.02 = 50s.
        // inter_trial=8s < 50s < session=320s → Partial.
        let cam_deltas: Vec<f64> = vec![33333.3; 100]; // 30fps
        let stim_deltas: Vec<f64> = vec![33311.1; 300]; // 1e6/33311.1 ≈ 30.02Hz
        let params = TimingParams {
            n_trials: 40,
            inter_trial_sec: 8.0,
            session_duration_sec: 320.0,
        };
        let tc = characterize_timing(&cam_deltas, &stim_deltas, 10.0, &params);
        assert_eq!(tc.regime, TimingRegime::Partial);
    }
}
