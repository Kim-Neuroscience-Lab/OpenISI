//! Per-direction sweep accumulator.
//!
//! Combines per-cycle complex maps into a direction-averaged complex map
//! via the selected [`CycleAverageMethod`] (the faithful simple complex
//! average by default; phase-locked averaging as an OpenISI option), and
//! carries the frame-domain running sum used for spectral SNR. Owns the
//! backend-agnostic [`Direction`] enum (a plain enum, no tensor state).
//!
//! Validated end-to-end against a raw-frames file (the small fixtures use
//! cached complex_maps and never run the DFT path) via the `#[ignore]`
//! regression test.

use ndarray::Array2;
use std::collections::BTreeMap;

use burn_tensor::Tensor;

use super::backend::Backend;
use super::complex::Complex2;
use super::conversions;
use crate::methods::CycleAverageMethod;
use crate::{AnalysisError, ComplexMaps, RawProcessingResult, ReliabilityMaps, ResponsivenessMaps};

/// Stimulus direction. Replaces the `(is_azi, is_fwd)` tuple that used to
/// be encoded in several places. `BTreeMap` keys here also depend on the
/// `Ord` derive (alphabetical: AltFwd < AltRev < AziFwd < AziRev) — the
/// order is incidental, not semantic.
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

/// Per-direction accumulation state — Burn types.
struct DirectionAcc {
    /// Per-cycle complex maps `[H, W]`.
    cycles: Vec<Complex2>,
    /// Per-cycle global phase `arg(Σ_pixels cm_k)`, for phase-locked averaging.
    phases: Vec<f64>,
    /// Frame-domain running sum `[N, H, W]` f32, used for SNR.
    frame_sum: Option<Tensor<Backend, 3>>,
    n_used: u32,
}

impl DirectionAcc {
    fn new() -> Self {
        Self {
            cycles: Vec::new(),
            phases: Vec::new(),
            frame_sum: None,
            n_used: 0,
        }
    }

    fn add_cycle(&mut self, complex_map: Complex2, phase: f64, cycle_frames: Tensor<Backend, 3>) {
        self.cycles.push(complex_map);
        self.phases.push(phase);
        self.frame_sum = Some(match self.frame_sum.take() {
            Some(s) => s + cycle_frames,
            None => cycle_frames,
        });
        self.n_used += 1;
    }

    /// Combine the per-cycle complex maps via the selected [`CycleAverageMethod`]
    /// and divide by the cycle count. Returns the direction-averaged complex map
    /// plus the per-pixel cross-cycle reliability (`None` when `K < 2`).
    fn finalize_direction(
        self,
        label: &str,
        method: &CycleAverageMethod,
    ) -> Result<(Array2<num_complex::Complex64>, Option<Array2<f64>>), AnalysisError> {
        if self.n_used == 0 {
            return Err(AnalysisError::MissingData(format!(
                "{label}: no cycles fit within the recorded camera window"
            )));
        }

        // Reliability is computed from the per-cycle maps before they are
        // consumed by the averaging method.
        let reliability = if self.n_used >= 2 {
            let rel_t = super::responsiveness::reliability(&self.cycles);
            Some(conversions::tensor_to_array2_f64(rel_t)?)
        } else {
            None
        };

        let averaged = method.apply(self.cycles, &self.phases).ok_or_else(|| {
            AnalysisError::MissingData(format!("{label}: cycle average produced no result"))
        })?;
        Ok((conversions::complex2_to_array2(&averaged)?, reliability))
    }

    /// Frame-domain cycle-averaged movie `[N, H, W]` f32. `None` if empty.
    fn averaged_movie(&self) -> Option<Tensor<Backend, 3>> {
        self.frame_sum
            .as_ref()
            .map(|s| s.clone().mul_scalar(1.0 / self.n_used as f32))
    }
}

/// On-device sweep accumulator.
#[derive(Default)]
pub struct CycleAccumulator {
    slots: BTreeMap<Direction, DirectionAcc>,
    spectral_snr_azi: Option<Tensor<Backend, 2>>,
    spectral_snr_alt: Option<Tensor<Backend, 2>>,
    allen_power_snr_azi: Option<Tensor<Backend, 2>>,
    allen_power_snr_alt: Option<Tensor<Backend, 2>>,
}

impl CycleAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push one cycle: complex map, global phase, raw frame stack.
    pub fn add_cycle(
        &mut self,
        direction: Direction,
        complex_map: Complex2,
        phase: f64,
        cycle_frames: Tensor<Backend, 3>,
    ) {
        self.slots
            .entry(direction)
            .or_insert_with(DirectionAcc::new)
            .add_cycle(complex_map, phase, cycle_frames);
    }

    /// Frame-domain averaged movie for a direction, if any cycle exists.
    pub fn averaged_movie(&self, direction: Direction) -> Option<Tensor<Backend, 3>> {
        self.slots
            .get(&direction)
            .and_then(DirectionAcc::averaged_movie)
    }

    /// Record the spectral responsiveness metrics (spectral SNR + Allen
    /// power-SNR) for an orientation. Once-per-orientation: a second call for the
    /// same orientation is a programmer error, not a silent overwrite.
    pub fn record_responsiveness(
        &mut self,
        direction: Direction,
        spectral_snr: Tensor<Backend, 2>,
        allen_power_snr: Tensor<Backend, 2>,
    ) -> Result<(), AnalysisError> {
        let (spectral_slot, allen_slot) = if direction.is_azi() {
            (&mut self.spectral_snr_azi, &mut self.allen_power_snr_azi)
        } else {
            (&mut self.spectral_snr_alt, &mut self.allen_power_snr_alt)
        };
        if spectral_slot.is_some() || allen_slot.is_some() {
            return Err(AnalysisError::InvalidPackage(format!(
                "record_responsiveness called twice for {} orientation",
                if direction.is_azi() {
                    "azimuth"
                } else {
                    "altitude"
                },
            )));
        }
        *spectral_slot = Some(spectral_snr);
        *allen_slot = Some(allen_power_snr);
        Ok(())
    }

    /// Consume the accumulator → `RawProcessingResult`. Pairing rules:
    /// every direction needs ≥ 1 cycle; reliability is present only if all
    /// four directions had K ≥ 2; SNR is both-or-neither.
    pub fn finalize(
        mut self,
        cycle_average: &CycleAverageMethod,
    ) -> Result<RawProcessingResult, AnalysisError> {
        let (azi_fwd, rel_azi_fwd) = self.take_direction(Direction::AziFwd, cycle_average)?;
        let (azi_rev, rel_azi_rev) = self.take_direction(Direction::AziRev, cycle_average)?;
        let (alt_fwd, rel_alt_fwd) = self.take_direction(Direction::AltFwd, cycle_average)?;
        let (alt_rev, rel_alt_rev) = self.take_direction(Direction::AltRev, cycle_average)?;

        let complex_maps = ComplexMaps {
            azi_fwd,
            azi_rev,
            alt_fwd,
            alt_rev,
        };
        let reliability = match (rel_azi_fwd, rel_azi_rev, rel_alt_fwd, rel_alt_rev) {
            (Some(rel_azi_fwd), Some(rel_azi_rev), Some(rel_alt_fwd), Some(rel_alt_rev)) => {
                Some(ReliabilityMaps {
                    rel_azi_fwd,
                    rel_azi_rev,
                    rel_alt_fwd,
                    rel_alt_rev,
                })
            }
            _ => None,
        };

        let responsiveness = match (
            self.spectral_snr_azi.take(),
            self.spectral_snr_alt.take(),
            self.allen_power_snr_azi.take(),
            self.allen_power_snr_alt.take(),
        ) {
            (Some(s_azi), Some(s_alt), Some(a_azi), Some(a_alt)) => Some(ResponsivenessMaps {
                spectral_snr_azi: conversions::tensor_to_array2_f64(s_azi)?,
                spectral_snr_alt: conversions::tensor_to_array2_f64(s_alt)?,
                allen_power_snr_azi: conversions::tensor_to_array2_f64(a_azi)?,
                allen_power_snr_alt: conversions::tensor_to_array2_f64(a_alt)?,
            }),
            (None, None, None, None) => None,
            _ => {
                return Err(AnalysisError::InvalidPackage(
                    "responsiveness computed for one orientation but not the other".into(),
                ));
            }
        };

        Ok(RawProcessingResult {
            complex_maps,
            responsiveness,
            reliability,
        })
    }

    fn take_direction(
        &mut self,
        direction: Direction,
        method: &CycleAverageMethod,
    ) -> Result<(Array2<num_complex::Complex64>, Option<Array2<f64>>), AnalysisError> {
        let acc = self.slots.remove(&direction).ok_or_else(|| {
            AnalysisError::MissingData(format!("no cycles found for {}", direction.label()))
        })?;
        acc.finalize_direction(direction.label(), method)
    }
}
