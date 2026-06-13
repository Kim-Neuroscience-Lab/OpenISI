//! Stage 1 — Cycle combination (fwd+rev complex maps per orientation).
//!
//! Given the four direction-averaged complex maps (`azi_fwd`, `azi_rev`,
//! `alt_fwd`, `alt_rev`), produce per-orientation position phasors
//! `z_azi = exp(i·φ_azi)` and `z_alt = exp(i·φ_alt)`. The phasor
//! representation lets downstream stages (smoothing, gradients, VFS)
//! operate on continuous real and imaginary components without phase-wrap
//! discontinuities.

use crate::compute;

/// Method choice for combining forward and reverse sweep complex maps
/// into a per-orientation position phasor.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum CycleCombineMethod {
    /// Per-cycle delay subtraction (Kalatsky & Stryker 2003,
    /// Neuron 38:529-545). Implemented in
    /// `crate::compute::position_phasor_delay_subtracted`. The position
    /// phasor `z = sqrt(z_fwd / z_rev)` removes the hemodynamic delay
    /// common to both directions while preserving the position phase.
    /// Marshel 2011 and Garrett 2014 inherit and use this technique;
    /// they do not introduce it. Reference MATLAB: SNLC `Gprocesskret.m`.
    KalatskyStryker2003DelaySubtraction,

    /// Unweighted cycle average — take the per-direction averaged
    /// complex map directly without any delay correction. The phase
    /// carries the hemodynamic delay (~1.5-3 s offset for mouse
    /// cortex); not a published method (Kalatsky's whole point was
    /// that this is wrong without separate hemodynamic correction).
    /// Kept as a fallback for debugging or when delay subtraction is
    /// somehow unwanted.
    UnweightedCycleAverage,
}

impl CycleCombineMethod {
    pub fn kalatsky_stryker2003_delay_subtraction() -> Self {
        Self::KalatskyStryker2003DelaySubtraction
    }

    pub fn unweighted_cycle_average() -> Self {
        Self::UnweightedCycleAverage
    }

    /// Combine the four direction-averaged complex maps into per-orientation
    /// position phasors. Operates on [`compute::Complex2`].
    pub fn apply(
        &self,
        azi_fwd: &compute::Complex2,
        azi_rev: &compute::Complex2,
        alt_fwd: &compute::Complex2,
        alt_rev: &compute::Complex2,
    ) -> (compute::Complex2, compute::Complex2) {
        match self {
            Self::KalatskyStryker2003DelaySubtraction => (
                compute::position_phasor_delay_subtracted(azi_fwd, azi_rev),
                compute::position_phasor_delay_subtracted(alt_fwd, alt_rev),
            ),
            Self::UnweightedCycleAverage => (azi_fwd.clone(), alt_fwd.clone()),
        }
    }
}
