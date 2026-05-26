//! Stage 1 — Cycle combination (fwd+rev complex maps per orientation).
//!
//! Given the four direction-averaged complex maps (`azi_fwd`, `azi_rev`,
//! `alt_fwd`, `alt_rev`), produce per-orientation position phasors
//! `z_azi = exp(i·φ_azi)` and `z_alt = exp(i·φ_alt)`. The phasor
//! representation lets downstream stages (smoothing, gradients, VFS)
//! operate on continuous real and imaginary components without phase-wrap
//! discontinuities.

use tch::Tensor;

use crate::compute;

/// Method choice for combining forward and reverse sweep complex maps
/// into a per-orientation position phasor.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum CycleCombineMethod {
    /// Marshel-Garrett delay subtraction (Marshel, Garrett, Nauhaus,
    /// Callaway 2011, Neuron 76:713-720; Garrett, Nauhaus, Marshel,
    /// Callaway 2014, J Neurosci 34(37):12587-12600). Implemented in
    /// `crate::compute::position_phasor_delay_subtracted`. The position
    /// phasor `z = sqrt(z_fwd / z_rev)` removes the hemodynamic delay
    /// common to both directions while preserving the position phase.
    /// Reference MATLAB: SNLC `Gprocesskret.m`.
    MarshelGarrett2011DelaySubtraction,

    /// Kalatsky-Stryker raw cycle averaging (Kalatsky & Stryker 2003,
    /// Neuron 38:529-545). Take the per-direction averaged complex map
    /// directly without delay subtraction. Phase carries the hemodynamic
    /// delay (~1.5-3 s offset for mouse cortex); use only when the
    /// hemodynamic delay is separately measured and corrected, or for
    /// reproducing the original Kalatsky 2003 analysis exactly.
    KalatskyStryker2003RawAverage,
}

impl CycleCombineMethod {
    pub fn marshel_garrett2011_delay_subtraction() -> Self {
        Self::MarshelGarrett2011DelaySubtraction
    }

    pub fn kalatsky_stryker2003_raw_average() -> Self {
        Self::KalatskyStryker2003RawAverage
    }

    /// Combine the four direction-averaged complex maps into per-orientation
    /// position phasors.
    pub fn apply(
        &self,
        azi_fwd: &Tensor,
        azi_rev: &Tensor,
        alt_fwd: &Tensor,
        alt_rev: &Tensor,
    ) -> (Tensor, Tensor) {
        match self {
            Self::MarshelGarrett2011DelaySubtraction => (
                compute::position_phasor_delay_subtracted(azi_fwd, azi_rev),
                compute::position_phasor_delay_subtracted(alt_fwd, alt_rev),
            ),
            Self::KalatskyStryker2003RawAverage => {
                (azi_fwd.shallow_clone(), alt_fwd.shallow_clone())
            }
        }
    }
}
