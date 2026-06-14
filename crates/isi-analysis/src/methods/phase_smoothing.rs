//! Stage 2 — Phase / position phasor smoothing.
//!
//! Smooths the per-orientation position phasor `z = exp(i·φ)` before gradients
//! are computed for the visual sign map. Two published methods are offered; both
//! smooth on the complex/phasor representation (or rebuild a phasor) so the 2π
//! wrap discontinuity of raw-phase smoothing is avoided.

use crate::compute;

/// Method choice for smoothing the position phasor.
///
/// Canonical type: [`openisi_params::config::analysis::PhaseSmoothing`] (UNIFY);
/// both variants smooth on the complex/phasor representation so the 2π wrap of
/// raw-phase smoothing is avoided. Compute behavior is attached via
/// [`PhaseSmoothingExt`].
pub use openisi_params::config::analysis::PhaseSmoothing as PhaseSmoothingMethod;

/// Compute behavior for the phase-smoothing stage (extension trait).
pub trait PhaseSmoothingExt {
    /// Smooth the per-orientation position phasors. Returns
    /// `(azi_z_smoothed, alt_z_smoothed)` as [`compute::Complex2`] on the active
    /// device. The amplitude inputs are used only by the SNLC variant.
    fn apply(
        &self,
        azi_z: &compute::Complex2,
        alt_z: &compute::Complex2,
        azi_amp: burn_tensor::Tensor<compute::Backend, 2>,
        alt_amp: burn_tensor::Tensor<compute::Backend, 2>,
    ) -> (compute::Complex2, compute::Complex2);

    /// Sigma value carried by this variant, in pixels.
    fn sigma_px(&self) -> f64;
}

impl PhaseSmoothingExt for PhaseSmoothingMethod {
    fn apply(
        &self,
        azi_z: &compute::Complex2,
        alt_z: &compute::Complex2,
        azi_amp: burn_tensor::Tensor<compute::Backend, 2>,
        alt_amp: burn_tensor::Tensor<compute::Backend, 2>,
    ) -> (compute::Complex2, compute::Complex2) {
        match self {
            Self::SnlcAmpWeightedPhasor { sigma_px } => (
                compute::amp_weighted_complex_smooth(azi_z, azi_amp, *sigma_px),
                compute::amp_weighted_complex_smooth(alt_z, alt_amp, *sigma_px),
            ),
            Self::AllenZhuang2017PositionGaussian { sigma_px } => (
                compute::position_gaussian_smooth(azi_z, *sigma_px),
                compute::position_gaussian_smooth(alt_z, *sigma_px),
            ),
        }
    }

    fn sigma_px(&self) -> f64 {
        match self {
            Self::SnlcAmpWeightedPhasor { sigma_px }
            | Self::AllenZhuang2017PositionGaussian { sigma_px } => *sigma_px,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigma_round_trip() {
        let m = PhaseSmoothingMethod::SnlcAmpWeightedPhasor { sigma_px: 2.5 };
        assert_eq!(m.sigma_px(), 2.5);
        let a = PhaseSmoothingMethod::AllenZhuang2017PositionGaussian { sigma_px: 1.5 };
        assert_eq!(a.sigma_px(), 1.5);
    }
}
