//! Projection stage — per-cycle ΔF/F → bin-1 DFT → per-direction complex maps
//! (+ reliability, SNR). The *universal compute* half of stage 0.
//!
//! Separation of concerns: the ΔF/F baseline `F0` is chosen by a selectable
//! [`crate::methods::BaselineMethod`] (its own stage); this module takes that
//! `F0` as an input and does the Fourier projection. The two are distinct
//! concerns with a tiny `[H, W]` boundary (`F0`), so they are separate pipeline
//! stages.
//!
//! Why ΔF/F-apply and the DFT are NOT split further: the ΔF/F movie is never
//! materialized — each cycle's contiguous frame slice is ΔF/F'd on the fly and
//! immediately projected. A stage boundary between "apply ΔF/F" and "DFT" would
//! force materializing the whole movie (multi-GB), so they stay one streaming
//! pass here.
//!
//! Allen-aligned per-direction cycle averaging — matches
//! `corticalmapping/core/ImageAnalysis.py::get_average_movie`.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use ndarray::Array2;

use crate::compute::{self};
use crate::io::{classify_cycle_name, nearest_index_sorted};
use crate::{AnalysisError, ProgressSink, RawAcquisition, RawProcessingResult, Result};

/// Run the projection: ΔF/F (using the given per-pixel baseline `F0` and
/// denominator `floor`) → per-cycle bin-1 DFT → accumulate → per-direction
/// complex maps + cross-cycle reliability + SNR.
///
///   1. `meanFrameDur = mean(diff(cam_ts))` — uniform-regime camera period.
///   2. For each direction: gather its sweeps; `chunkFrameDur =
///      ceil(mean(sweep_end − sweep_start) / meanFrameDur)`; per cycle,
///      `onset_idx = argmin(|cam_ts − sweep_start[k]|)`; ΔF/F the contiguous
///      slice `[onset_idx, onset_idx+chunkFrameDur)`; bin-1 DFT at
///      `freq = 1/(chunkFrameDur·meanFrameDur)`; push into the accumulator.
///   3. SNR on the frame-domain cycle-averaged movie for the first fwd sweep
///      per orientation. `accumulator.finalize(cycle_average)` combines the
///      per-cycle complex maps via the selected `CycleAverageMethod` (default
///      plain complex average = Allen `get_average_movie`) → per-direction complex
///      maps + `Option<ResponsivenessMaps>` + reliability.
pub fn run(
    raw: &RawAcquisition,
    baseline: &Array2<f64>,
    dff_floor: f64,
    cycle_average: &crate::methods::CycleAverageMethod,
    cancel: &AtomicBool,
    progress: &dyn ProgressSink,
) -> Result<RawProcessingResult> {
    let all_frames = &raw.frames;
    let (t_cam, _h, _w) = all_frames.dim();
    if t_cam < 2 {
        return Err(AnalysisError::MissingData(
            "fewer than 2 camera frames".into(),
        ));
    }

    let cam_ts_sec = &raw.cam_ts_sec;
    let sweep_start_sec = &raw.sweep_start_sec;
    let sweep_end_sec = &raw.sweep_end_sec;

    let n_sweeps = raw
        .sweep_sequence
        .len()
        .min(sweep_start_sec.len())
        .min(sweep_end_sec.len());
    if n_sweeps == 0 {
        return Err(AnalysisError::MissingData("no sweeps in schedule".into()));
    }

    if cancel.load(Ordering::Relaxed) {
        return Err(AnalysisError::Cancelled);
    }

    // `meanFrameDur` — Allen `ImageAnalysis.py:1184`.
    let mean_frame_dur = (cam_ts_sec[t_cam - 1] - cam_ts_sec[0]) / (t_cam - 1) as f64;

    // Group sweep indices by direction.
    let mut dir_groups: BTreeMap<compute::Direction, Vec<usize>> = BTreeMap::new();
    for (k, name) in raw.sweep_sequence.iter().enumerate().take(n_sweeps) {
        if let Some(direction) = classify_cycle_name(name) {
            dir_groups.entry(direction).or_default().push(k);
        }
    }
    if dir_groups.is_empty() {
        return Err(AnalysisError::InvalidPackage(
            "no sweeps with recognized direction names".into(),
        ));
    }

    let mut accumulator = compute::CycleAccumulator::new();
    let n_dirs = dir_groups.len() as f64;
    for (dir_idx, (direction, sweep_ks)) in dir_groups.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(AnalysisError::Cancelled);
        }

        // Allen-style chunk duration: per-direction `sweepDur` is the mean
        // of `sweep_end - sweep_start` over this direction's cycles.
        let chunk_dur: f64 = sweep_ks
            .iter()
            .map(|&k| sweep_end_sec[k] - sweep_start_sec[k])
            .sum::<f64>()
            / sweep_ks.len() as f64;
        // `chunkFrameDur = ceil(chunkDur / meanFrameDur)` — Allen
        // `ImageAnalysis.py:1187`.
        let chunk_frame_dur = (chunk_dur / mean_frame_dur).ceil() as usize;

        progress.set_stage(&format!(
            "Direction {}: {} cycles × {chunk_frame_dur} frames",
            direction.label(),
            sweep_ks.len(),
        ));
        progress.set_progress(0.1 + 0.2 * dir_idx as f64 / n_dirs);

        let period_sec = chunk_frame_dur as f64 * mean_frame_dur;
        let freq_bin1 = 1.0 / period_sec;

        // For each cycle: upload frames, compute bin-1 complex map +
        // global phase, push into the accumulator. The accumulator combines
        // the per-cycle maps via the selected `CycleAverageMethod` at finalize
        // time (the global phase is consumed only by the phase-locked variant).
        for &k in sweep_ks {
            // Per-cycle cancel check — a long-cycle DFT (large K × many
            // frames per cycle) was previously uninterruptable for tens
            // of seconds. Checking here lets a new param edit preempt
            // the run within one cycle of work, not one direction's.
            if cancel.load(Ordering::Relaxed) {
                return Err(AnalysisError::Cancelled);
            }
            let onset = sweep_start_sec[k];
            if onset < cam_ts_sec[0] || onset + chunk_dur > cam_ts_sec[t_cam - 1] {
                continue;
            }
            let onset_idx = nearest_index_sorted(cam_ts_sec, onset);
            if onset_idx + chunk_frame_dur > t_cam {
                continue;
            }

            let frame_indices: Vec<usize> = (onset_idx..onset_idx + chunk_frame_dur).collect();
            let cycle_t = compute::frames_u16_subset_to_dff_tensor(
                all_frames,
                &frame_indices,
                baseline,
                dff_floor,
            );

            // `cycle_t` is consumed by the DFT and also kept by the
            // accumulator (frame-domain sum for SNR). Burn tensors are
            // refcounted, so the clone is an Arc bump, not a copy.
            let cm_k = compute::dft_projection_at_freq(cycle_t.clone(), mean_frame_dur, freq_bin1);

            // Global per-cycle phase: arg(Σ_pixels cm_k).
            let (re_sum, im_sum) = cm_k.real_imag_sum();
            let phi_k = im_sum.atan2(re_sum);

            accumulator.add_cycle(*direction, cm_k, phi_k, cycle_t);
        }

        // SNR on Allen's frame-domain cycle-averaged movie. Once per
        // orientation on the first fwd direction.
        let is_first_fwd_for_orientation = direction.is_fwd();
        if is_first_fwd_for_orientation {
            if let Some(averaged) = accumulator.averaged_movie(*direction) {
                let uniform_ts: Vec<f64> = (0..chunk_frame_dur as i64)
                    .map(|k| k as f64 * mean_frame_dur)
                    .collect();
                let spectral =
                    compute::responsiveness::spectral_snr(averaged.clone(), &uniform_ts);
                // The averaged movie spans one stimulus period → F1 is FFT bin 1.
                let allen = compute::responsiveness::allen_power_snr_device(averaged, 1);
                accumulator.record_responsiveness(*direction, spectral, allen)?;
            }
        }
    }

    progress.set_progress(0.95);
    accumulator.finalize(cycle_average)
}
