//! Stage 2 — Phase / position phasor smoothing.
//!
//! Smooths the per-orientation position phasor `z = exp(i·φ)` before
//! gradients are computed for the visual sign map. Smoothing is performed
//! on the complex components to avoid the phase-wrap discontinuities that
//! linear smoothing of wrapped phase would produce.

use openisi_params::{PhaseSmoothingOpenIsiAmpWeightedPhasorSigmaPx, Tagged};
use tch::Tensor;

use crate::compute;

/// Method choice for smoothing the position phasor.
///
/// `#[non_exhaustive]` + per-variant constructors force registry-sourced
/// tunables; no inline literals can enter the pipeline.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum PhaseSmoothingMethod {
    /// OpenISI amp-weighted phasor smoothing — documented deviation from
    /// Allen `retinotopic_mapping`'s `phaseFilter` (Zhuang 2017, eLife
    /// 6:e18372; `RetinotopicMapping.py` L269–296). Computes a normalized
    /// convolution `smooth(amp·z) / smooth(amp)`, equivalent to a
    /// Gaussian on each of the real and imaginary components weighted
    /// by amplitude. Background pixels with `|F1| ≈ 0` contribute
    /// negligibly, preventing them from polluting smoothed values near
    /// the cortex boundary. Numerically `sigma_px = 1.0` matches Allen's
    /// `phaseMapFilterSigma` default. Implemented in
    /// `crate::compute::amp_weighted_complex_smooth`.
    OpenIsiAmpWeightedPhasor {
        sigma_px: f64,
    },
}

impl PhaseSmoothingMethod {
    /// Construct the OpenIsiAmpWeightedPhasor variant from a registry-sourced σ.
    pub fn open_isi_amp_weighted_phasor(
        sigma_px: Tagged<PhaseSmoothingOpenIsiAmpWeightedPhasorSigmaPx>,
    ) -> Self {
        Self::OpenIsiAmpWeightedPhasor { sigma_px: sigma_px.into_inner() }
    }

    /// Smooth the per-orientation position phasors. Returns
    /// `(azi_z_smoothed, alt_z_smoothed)` as `Kind::ComplexFloat` on the
    /// active device.
    pub fn apply(
        &self,
        azi_z: &Tensor,
        alt_z: &Tensor,
        azi_amp: &Tensor,
        alt_amp: &Tensor,
    ) -> (Tensor, Tensor) {
        match self {
            Self::OpenIsiAmpWeightedPhasor { sigma_px } => (
                compute::amp_weighted_complex_smooth(azi_z, azi_amp, *sigma_px),
                compute::amp_weighted_complex_smooth(alt_z, alt_amp, *sigma_px),
            ),
        }
    }

    /// Sigma value carried by this variant, in pixels.
    pub fn sigma_px(&self) -> f64 {
        match self {
            Self::OpenIsiAmpWeightedPhasor { sigma_px } => *sigma_px,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigma_round_trip() {
        let m = PhaseSmoothingMethod::OpenIsiAmpWeightedPhasor { sigma_px: 2.5 };
        assert_eq!(m.sigma_px(), 2.5);
    }
}
