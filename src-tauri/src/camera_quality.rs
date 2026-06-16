//! Camera acquisition-quality assessment.
//!
//! Given the per-frame stamps captured from a real PCO acquisition — the
//! camera's hardware **sequence number** and **hardware timestamp** — this
//! computes whether the capture met the rig's requirements: no dropped frames,
//! a steady frame rate, and bounded jitter. The assessment is a **pure
//! function** of the stamps (no hardware, no IO), so it is unit-tested in CI
//! with synthetic captures; the actual camera capture that feeds it lives in
//! the headless `camera-check` command.
//!
//! Two independent drop signals are used, because they catch different
//! failures:
//! - **Sequence gaps** — the camera's hardware image counter skipping a value
//!   is the *authoritative* "a frame was lost" signal (sensor drop or recorder
//!   ring overflow). This is the primary pass/fail criterion.
//! - **Timing anomalies** — an inter-frame interval far above the median flags
//!   a late frame even when none was lost (USB stall, scheduler hiccup).
//!
//! The thresholds are passed in (sourced from the config / sensible defaults)
//! so the policy lives with the caller, not buried here.

/// One captured frame's identity + arrival time, from the camera hardware.
#[derive(Debug, Clone, Copy)]
pub struct FrameStamp {
    /// Camera hardware image counter (monotonic, +1 per sensor frame).
    pub sequence_number: u64,
    /// Camera hardware timestamp (µs).
    pub hardware_timestamp_us: i64,
}

/// Pass/fail thresholds for a capture, sourced from the config by the caller.
#[derive(Debug, Clone, Copy)]
pub struct QualityThresholds {
    /// An inter-frame interval above `median × this` is a timing anomaly.
    /// Mirrors the runtime `drop_detection_threshold` so the test and the live
    /// path agree on what "too long" means.
    pub timing_anomaly_factor: f64,
    /// Frames to skip at the start before timing checks (sensor/USB settling).
    pub warmup_frames: usize,
    /// Maximum acceptable jitter (std-dev of inter-frame intervals) as a
    /// fraction of the median frame period.
    pub max_jitter_fraction: f64,
}

impl Default for QualityThresholds {
    fn default() -> Self {
        Self {
            timing_anomaly_factor: 1.5,
            warmup_frames: 10,
            max_jitter_fraction: 0.10,
        }
    }
}

/// The verdict + the numbers behind it.
#[derive(Debug, Clone)]
pub struct CameraQualityReport {
    pub n_frames: usize,
    pub duration_sec: f64,
    pub mean_fps: f64,
    pub median_period_us: f64,
    pub jitter_us: f64,
    pub jitter_fraction: f64,
    /// Dropped frames per the hardware sequence counter (the authoritative loss
    /// signal). MUST be zero to pass.
    pub sequence_drops: u64,
    /// Inter-frame intervals (after warmup) exceeding the anomaly threshold.
    pub timing_anomalies: usize,
    pub max_delta_us: i64,
    pub passed: bool,
    /// Human-readable reasons the capture failed (empty if it passed).
    pub failures: Vec<String>,
}

/// Assess a captured sequence of frame stamps against the thresholds.
///
/// Pass requires: ≥2 frames, **zero** sequence-counter drops, **zero** timing
/// anomalies after warmup, and jitter within `max_jitter_fraction` of the
/// median period. The median period (not a configured nominal fps) is the
/// baseline, so the assessment self-calibrates to the camera's actual rate.
pub fn assess_capture(frames: &[FrameStamp], thr: &QualityThresholds) -> CameraQualityReport {
    let mut failures = Vec::new();

    if frames.len() < 2 {
        return CameraQualityReport {
            n_frames: frames.len(),
            duration_sec: 0.0,
            mean_fps: 0.0,
            median_period_us: 0.0,
            jitter_us: 0.0,
            jitter_fraction: 0.0,
            sequence_drops: 0,
            timing_anomalies: 0,
            max_delta_us: 0,
            passed: false,
            failures: vec![format!(
                "insufficient frames: captured {}, need at least 2",
                frames.len()
            )],
        };
    }

    // ── Sequence drops: gaps in the hardware image counter ──
    let mut sequence_drops: u64 = 0;
    for w in frames.windows(2) {
        let gap = w[1].sequence_number.saturating_sub(w[0].sequence_number);
        if gap > 1 {
            sequence_drops += gap - 1;
        }
    }

    // ── Inter-frame intervals (hardware timestamps) ──
    let deltas: Vec<i64> = frames
        .windows(2)
        .map(|w| w[1].hardware_timestamp_us - w[0].hardware_timestamp_us)
        .collect();
    let max_delta_us = deltas.iter().copied().max().unwrap_or(0);

    // Median period — robust baseline (drops/spikes don't move it).
    let mut sorted = deltas.clone();
    sorted.sort_unstable();
    let median_period_us = if sorted.len() % 2 == 1 {
        sorted[sorted.len() / 2] as f64
    } else {
        let mid = sorted.len() / 2;
        (sorted[mid - 1] + sorted[mid]) as f64 / 2.0
    };

    // Jitter — std-dev of intervals (population).
    let mean_delta = deltas.iter().map(|&d| d as f64).sum::<f64>() / deltas.len() as f64;
    let variance =
        deltas.iter().map(|&d| (d as f64 - mean_delta).powi(2)).sum::<f64>() / deltas.len() as f64;
    let jitter_us = variance.sqrt();
    let jitter_fraction = if median_period_us > 0.0 {
        jitter_us / median_period_us
    } else {
        0.0
    };

    // Timing anomalies — intervals past the threshold, after warmup.
    let anomaly_limit = median_period_us * thr.timing_anomaly_factor;
    let timing_anomalies = deltas
        .iter()
        .enumerate()
        .filter(|(i, d)| *i >= thr.warmup_frames && **d as f64 > anomaly_limit)
        .count();

    let span_us = (frames.last().unwrap().hardware_timestamp_us
        - frames.first().unwrap().hardware_timestamp_us) as f64;
    let duration_sec = span_us / 1e6;
    let mean_fps = if duration_sec > 0.0 {
        (frames.len() - 1) as f64 / duration_sec
    } else {
        0.0
    };

    if sequence_drops > 0 {
        failures.push(format!(
            "{sequence_drops} dropped frame(s) — gaps in the camera hardware sequence counter \
             (sensor drop or recorder-ring overflow)"
        ));
    }
    if timing_anomalies > 0 {
        failures.push(format!(
            "{timing_anomalies} late frame(s) — inter-frame interval exceeded {:.0}µs \
             ({:.1}× the {:.0}µs median); max was {max_delta_us}µs",
            anomaly_limit, thr.timing_anomaly_factor, median_period_us
        ));
    }
    if jitter_fraction > thr.max_jitter_fraction {
        failures.push(format!(
            "frame-timing jitter {:.1}% of the period exceeds the {:.1}% limit ({:.0}µs std-dev)",
            jitter_fraction * 100.0,
            thr.max_jitter_fraction * 100.0,
            jitter_us
        ));
    }

    CameraQualityReport {
        n_frames: frames.len(),
        duration_sec,
        mean_fps,
        median_period_us,
        jitter_us,
        jitter_fraction,
        sequence_drops,
        timing_anomalies,
        max_delta_us,
        passed: failures.is_empty(),
        failures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a clean capture: contiguous sequence, uniform `period_us` spacing.
    fn clean_capture(n: usize, period_us: i64) -> Vec<FrameStamp> {
        (0..n)
            .map(|i| FrameStamp {
                sequence_number: i as u64,
                hardware_timestamp_us: i as i64 * period_us,
            })
            .collect()
    }

    #[test]
    fn clean_capture_passes() {
        let frames = clean_capture(100, 10_000); // 100 fps, no drops, no jitter
        let r = assess_capture(&frames, &QualityThresholds::default());
        assert!(r.passed, "clean capture should pass: {:?}", r.failures);
        assert_eq!(r.sequence_drops, 0);
        assert_eq!(r.timing_anomalies, 0);
        assert!((r.mean_fps - 100.0).abs() < 1.0, "mean_fps={}", r.mean_fps);
        assert!(r.jitter_us < 1.0);
    }

    #[test]
    fn sequence_gap_is_a_drop() {
        let mut frames = clean_capture(100, 10_000);
        // Frame 50 never arrives: its sequence number is skipped, and the
        // timestamp gap doubles.
        for f in frames.iter_mut().skip(50) {
            f.sequence_number += 1;
            f.hardware_timestamp_us += 10_000;
        }
        let r = assess_capture(&frames, &QualityThresholds::default());
        assert!(!r.passed);
        assert_eq!(r.sequence_drops, 1, "should detect exactly one dropped frame");
        assert!(r.failures.iter().any(|f| f.contains("dropped frame")));
    }

    #[test]
    fn multiple_consecutive_drops_counted() {
        let mut frames = clean_capture(100, 10_000);
        // 3 frames lost at once: sequence jumps by 4.
        for f in frames.iter_mut().skip(50) {
            f.sequence_number += 3;
        }
        let r = assess_capture(&frames, &QualityThresholds::default());
        assert_eq!(r.sequence_drops, 3);
    }

    #[test]
    fn timing_anomaly_without_sequence_drop() {
        let mut frames = clean_capture(100, 10_000);
        // Frame 60 is late (3× period) but no sequence number is skipped —
        // a stall, not a loss. Shift all subsequent timestamps.
        for f in frames.iter_mut().skip(60) {
            f.hardware_timestamp_us += 20_000;
        }
        let r = assess_capture(&frames, &QualityThresholds::default());
        assert_eq!(r.sequence_drops, 0, "no frame lost");
        assert!(r.timing_anomalies >= 1, "the late frame should be flagged");
        assert!(!r.passed);
    }

    #[test]
    fn warmup_frames_are_exempt_from_timing_checks() {
        let mut frames = clean_capture(100, 10_000);
        // A spike at frame 3 (inside the default 10-frame warmup) is ignored.
        for f in frames.iter_mut().skip(3) {
            f.hardware_timestamp_us += 20_000;
        }
        let r = assess_capture(&frames, &QualityThresholds::default());
        assert_eq!(r.timing_anomalies, 0, "warmup spike must be exempt");
    }

    #[test]
    fn high_jitter_fails() {
        // Alternating short/long intervals → large std-dev, ~same median.
        let mut frames = Vec::new();
        let mut t = 0i64;
        for i in 0..100 {
            frames.push(FrameStamp {
                sequence_number: i,
                hardware_timestamp_us: t,
            });
            t += if i % 2 == 0 { 6_000 } else { 14_000 };
        }
        let r = assess_capture(&frames, &QualityThresholds::default());
        assert!(
            r.jitter_fraction > 0.10,
            "jitter_fraction={}",
            r.jitter_fraction
        );
        assert!(!r.passed);
        assert!(r.failures.iter().any(|f| f.contains("jitter")));
    }

    #[test]
    fn too_few_frames_fails() {
        let r = assess_capture(&clean_capture(1, 10_000), &QualityThresholds::default());
        assert!(!r.passed);
        assert!(r.failures.iter().any(|f| f.contains("insufficient")));
    }
}
