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
///
/// Canonical type: [`openisi_params::config::analysis::CycleCombine`] (UNIFY);
/// compute behavior is attached via [`CycleCombineExt`].
pub use openisi_params::config::analysis::CycleCombine as CycleCombineMethod;

/// Compute behavior for the cycle-combine stage (extension trait).
pub trait CycleCombineExt {
    /// Combine the four direction-averaged complex maps into per-orientation
    /// position phasors. Operates on [`compute::Complex2`].
    fn apply(
        &self,
        azi_fwd: &compute::Complex2,
        azi_rev: &compute::Complex2,
        alt_fwd: &compute::Complex2,
        alt_rev: &compute::Complex2,
    ) -> (compute::Complex2, compute::Complex2);
}

impl CycleCombineExt for CycleCombineMethod {
    fn apply(
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
