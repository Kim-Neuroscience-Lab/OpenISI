//! On-device per-direction sweep accumulator.
//!
//! Single source of truth for "combine per-cycle complex maps into a
//! direction-averaged complex map." Owns the `Direction` enum and the
//! phase-locked averaging algorithm; the I/O layer feeds cycles in and
//! calls `finalize()` to get the final `RawProcessingResult`.
//!
//! Per `docs/ANALYSIS_COMPUTE.md` Principles 4 and 7, the accumulator
//! keeps cycle complex maps on the active device as native
//! `Kind::ComplexFloat` tensors and the frame-domain cycle-averaged movie
//! as `Kind::Float`. `finalize()` is the single device→CPU boundary for
//! the sweep-processing stage.

use ndarray::Array2;
use std::collections::BTreeMap;
use tch::Tensor;

use crate::{AnalysisError, ComplexMaps, RawProcessingResult, ReliabilityMaps, SnrMaps};
use super::conversions;

/// Stimulus direction. Replaces the `(is_azi, is_fwd)` tuple that used to
/// be encoded in four different places in the codebase. `BTreeMap` keys
/// here also depend on the `Ord` derive (alphabetical: AltFwd < AltRev <
/// AziFwd < AziRev) — the order is incidental, not semantic.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Direction {
    /// Azimuth forward (LR — left-to-right bar sweep).
    AziFwd,
    /// Azimuth reverse (RL).
    AziRev,
    /// Altitude forward (label-mapped from TB by `classify_cycle_name`
    /// to absorb the camera vertical flip — see io.rs).
    AltFwd,
    /// Altitude reverse (label-mapped from BT for the same reason).
    AltRev,
}

impl Direction {
    /// Human-readable label including the rig label (`LR`/`RL`/`TB`/`BT`).
    pub fn label(self) -> &'static str {
        match self {
            Direction::AziFwd => "azi_fwd (LR)",
            Direction::AziRev => "azi_rev (RL)",
            Direction::AltFwd => "alt_fwd (TB)",
            Direction::AltRev => "alt_rev (BT)",
        }
    }

    /// True iff this is an azimuth-orientation direction (LR or RL).
    pub fn is_azi(self) -> bool {
        matches!(self, Direction::AziFwd | Direction::AziRev)
    }

    /// True iff this is a forward-sweep direction (LR or TB).
    pub fn is_fwd(self) -> bool {
        matches!(self, Direction::AziFwd | Direction::AltFwd)
    }
}

/// Per-direction accumulation state: collects each cycle's complex map and
/// its per-cycle global phase, plus the frame-domain running sum used for
/// the spectral SNR computation. `finalize_direction` consumes these to
/// produce the direction's averaged complex map.
struct DirectionAcc {
    /// Per-cycle complex maps `[H, W]` `Kind::ComplexFloat` on device.
    cycles: Vec<Tensor>,
    /// Per-cycle global phase `arg(Σ_pixels cm_k)` — used to align cycles
    /// to a consensus phase before summing (phase-locked averaging).
    phases: Vec<f64>,
    /// Frame-domain running sum `[N, H, W]` f32 on device, used for SNR.
    frame_sum: Option<Tensor>,
    /// Number of frames per cycle (frame_sum / n_used yields the averaged
    /// movie shape `[chunk_frame_dur, H, W]`).
    n_used: u32,
}

impl DirectionAcc {
    fn new() -> Self {
        Self { cycles: Vec::new(), phases: Vec::new(), frame_sum: None, n_used: 0 }
    }

    fn add_cycle(
        &mut self,
        complex_map: Tensor,
        phase: f64,
        cycle_frames: Tensor,
    ) -> Result<(), AnalysisError> {
        self.cycles.push(complex_map);
        self.phases.push(phase);
        match self.frame_sum.take() {
            Some(mut s) => {
                let _ = s.f_add_(&cycle_frames)
                    .map_err(|e| AnalysisError::Hdf5(
                        format!("cycle-average in-place add failed: {e}")
                    ))?;
                self.frame_sum = Some(s);
            }
            None => {
                self.frame_sum = Some(cycle_frames);
            }
        }
        self.n_used += 1;
        Ok(())
    }

    /// Phase-lock each cycle's complex map to the consensus phase, sum, and
    /// divide by the cycle count. Reduces destructive interference from
    /// cycle-to-cycle camera-sampling phase jitter. Returns the
    /// direction-averaged complex map plus the per-pixel cross-cycle
    /// reliability map (amp-weighted vector coherence — Allen / Engel).
    ///
    /// Reliability is computed on the raw per-cycle complex projections
    /// before phase-locking. Phase-locking applies the same global
    /// rotation to every pixel in a cycle, so it doesn't change the
    /// per-pixel phase-consistency across cycles — reliability is
    /// invariant to phase-lock alignment.
    ///
    /// `K = 1` is rejected as `MissingData`: reliability is undefined
    /// with a single sample (it's trivially `1.0`), so we fail loudly
    /// rather than silently emit a meaningless map.
    fn finalize_direction(
        self,
        label: &str,
    ) -> Result<(Array2<num_complex::Complex64>, Array2<f64>), AnalysisError> {
        if self.n_used == 0 {
            return Err(AnalysisError::MissingData(format!(
                "{label}: no cycles fit within the recorded camera window"
            )));
        }
        if self.n_used < 2 {
            return Err(AnalysisError::MissingData(format!(
                "{label}: cross-cycle reliability requires ≥ 2 cycles, got {} \
                 (acquire more cycles or run a longer session)",
                self.n_used,
            )));
        }

        // Stack [K, H, W] complex once; serves both reliability and the
        // phase-locked averaging loop.
        let cycle_refs: Vec<&Tensor> = self.cycles.iter().collect();
        let stack = Tensor::stack(&cycle_refs, 0);
        let reliability_t = super::ops::compute_reliability(&stack);
        let reliability = conversions::tensor_to_array2_f64(&reliability_t)?;

        // Consensus phase φ̄ = arg(Σ_k exp(i·φ_k)).
        let (mut sr, mut si) = (0.0_f64, 0.0_f64);
        for &p in &self.phases {
            sr += p.cos();
            si += p.sin();
        }
        let phi_bar = si.atan2(sr);

        let mut summed: Option<Tensor> = None;
        for (k_idx, cm_k) in self.cycles.into_iter().enumerate() {
            let delta = self.phases[k_idx] - phi_bar;
            let aligned = super::complex_phase_shift(&cm_k, -delta);
            summed = match summed {
                Some(mut s) => {
                    let _ = s.f_add_(&aligned).map_err(|e| AnalysisError::Hdf5(
                        format!("phase-locked complex add failed: {e}")
                    ))?;
                    Some(s)
                }
                None => Some(aligned),
            };
        }
        let summed = summed.ok_or_else(|| AnalysisError::MissingData(
            format!("{label}: phase-lock sum produced no result"),
        ))?;
        let averaged = summed * (1.0 / self.n_used as f64);
        Ok((conversions::complex_tensor_to_array2(&averaged)?, reliability))
    }

    /// Frame-domain cycle-averaged movie `[N, H, W]` f32 on device.
    /// Returns `None` if no cycles were added. Caller uses this for SNR
    /// computation on the first fwd sweep of each orientation.
    fn averaged_movie(&self) -> Option<Tensor> {
        self.frame_sum.as_ref()
            .map(|s| s * (1.0 / self.n_used as f64))
    }
}

/// On-device sweep accumulator. Cycles for each `Direction` are pushed in
/// via `add_cycle`; SNR tensors are recorded once per orientation via
/// `record_snr`. `finalize` consumes the accumulator and returns the
/// pipeline's `RawProcessingResult`.
#[derive(Default)]
pub struct CycleAccumulator {
    /// Per-direction state. `BTreeMap` so iteration order is deterministic.
    slots: BTreeMap<Direction, DirectionAcc>,
    /// Spectral SNR for the azimuth orientation (f32 `[H, W]`) if computed.
    snr_azi: Option<Tensor>,
    /// Spectral SNR for the altitude orientation (f32 `[H, W]`) if computed.
    snr_alt: Option<Tensor>,
}

impl CycleAccumulator {
    pub fn new() -> Self { Self::default() }

    /// Push one cycle: its complex map, global phase, and the raw frame
    /// stack (kept on device, used by SNR computation on the first fwd
    /// cycle of each orientation).
    ///
    /// `complex_map` is a `Kind::ComplexFloat` `[H, W]` tensor.
    /// `cycle_frames` is `[chunk_frame_dur, H, W]` f32.
    pub fn add_cycle(
        &mut self,
        direction: Direction,
        complex_map: Tensor,
        phase: f64,
        cycle_frames: Tensor,
    ) -> Result<(), AnalysisError> {
        self.slots.entry(direction)
            .or_insert_with(DirectionAcc::new)
            .add_cycle(complex_map, phase, cycle_frames)
    }

    /// Borrow the frame-domain averaged movie for the given direction, if
    /// at least one cycle has been added. Used by the SNR computation
    /// path: we compute SNR once per orientation on the first fwd cycle's
    /// averaged movie.
    pub fn averaged_movie(&self, direction: Direction) -> Option<Tensor> {
        self.slots.get(&direction).and_then(DirectionAcc::averaged_movie)
    }

    /// Record SNR for an orientation (azimuth or altitude). Once-per-
    /// orientation: subsequent calls for the same orientation are
    /// rejected as a programmer error rather than silently overwriting.
    pub fn record_snr(&mut self, direction: Direction, snr: Tensor) -> Result<(), AnalysisError> {
        let slot = if direction.is_azi() { &mut self.snr_azi } else { &mut self.snr_alt };
        if slot.is_some() {
            return Err(AnalysisError::InvalidPackage(format!(
                "record_snr called twice for {} orientation",
                if direction.is_azi() { "azimuth" } else { "altitude" },
            )));
        }
        *slot = Some(snr);
        Ok(())
    }

    /// Consume the accumulator. Every one of the four directions must have
    /// produced ≥ 2 cycles (reliability requirement); otherwise the result
    /// is malformed and we fail loudly. SNR is paired: both azimuth and
    /// altitude must be present or both absent.
    pub fn finalize(mut self) -> Result<RawProcessingResult, AnalysisError> {
        let (azi_fwd, rel_azi_fwd) = self.take_direction(Direction::AziFwd)?;
        let (azi_rev, rel_azi_rev) = self.take_direction(Direction::AziRev)?;
        let (alt_fwd, rel_alt_fwd) = self.take_direction(Direction::AltFwd)?;
        let (alt_rev, rel_alt_rev) = self.take_direction(Direction::AltRev)?;

        let complex_maps = ComplexMaps { azi_fwd, azi_rev, alt_fwd, alt_rev };
        let reliability = Some(ReliabilityMaps {
            rel_azi_fwd, rel_azi_rev, rel_alt_fwd, rel_alt_rev,
        });

        let snr = match (self.snr_azi.take(), self.snr_alt.take()) {
            (Some(azi), Some(alt)) => Some(SnrMaps {
                snr_azi: conversions::tensor_to_array2_f64(&azi)?,
                snr_alt: conversions::tensor_to_array2_f64(&alt)?,
            }),
            (None, None) => None,
            (Some(_), None) | (None, Some(_)) => {
                return Err(AnalysisError::InvalidPackage(
                    "SNR computed for one orientation but not the other".into(),
                ));
            }
        };

        Ok(RawProcessingResult { complex_maps, snr, reliability })
    }

    fn take_direction(
        &mut self,
        direction: Direction,
    ) -> Result<(Array2<num_complex::Complex64>, Array2<f64>), AnalysisError> {
        let acc = self.slots.remove(&direction).ok_or_else(|| AnalysisError::MissingData(
            format!("no cycles found for {}", direction.label()),
        ))?;
        acc.finalize_direction(direction.label())
    }
}
