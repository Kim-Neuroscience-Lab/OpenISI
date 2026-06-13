//! Stage 2 — Phase / position phasor smoothing.
//!
//! Smooths the per-orientation position phasor `z = exp(i·φ)` before gradients
//! are computed for the visual sign map. Two published methods are offered; both
//! smooth on the complex/phasor representation (or rebuild a phasor) so the 2π
//! wrap discontinuity of raw-phase smoothing is avoided.

use openisi_params::{
    PhaseSmoothingAllenZhuang2017PositionGaussianSigmaPx,
    PhaseSmoothingSnlcAmpWeightedPhasorSigmaPx, Tagged,
};

use crate::compute;

/// Method choice for smoothing the position phasor.
///
/// `#[non_exhaustive]` + per-variant constructors force registry-sourced
/// tunables; no inline literals can enter the pipeline.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum PhaseSmoothingMethod {
    /// **Amplitude-weighted** phasor smoothing — phase-equivalent to SNLC
    /// `Gprocesskret.m`, which smooths the complex F1 map `amp·e^{iφ}` directly
    /// (`roifilt2(hl, -ang, bw)` then `angle`). We compute the normalized
    /// convolution `smooth(amp·z) / smooth(amp)` on the phasor `z = c + i·s`
    /// (amplitude `amp` = F1 magnitude). The `/smooth(amp)` divide is by a
    /// positive real, so it **does not change the phase**
    /// (`angle(S/r) == angle(S)`): the smoothed phase — all VFS consumes — is
    /// identical to SNLC's. The normalization (normalized convolution, Knutsson &
    /// Westin 1993) only rescales the magnitude and is the sole OpenISI part.
    /// Down-weights near-zero-amplitude background so it doesn't pollute the
    /// smoothed phase near the cortex boundary. Implemented in
    /// `crate::compute::amp_weighted_complex_smooth`. `sigma_px = 1.0` default
    /// mirrors Allen `phaseMapFilterSigma`.
    SnlcAmpWeightedPhasor { sigma_px: f64 },

    /// **Unweighted** scalar Gaussian on the phase — Allen
    /// `RetinotopicMapping.py::_getSignMap` (Zhuang 2017 eLife 6:e18372,
    /// `gaussian_filter(positionMap, phaseMapFilterSigma)`, default 1). Every
    /// pixel contributes equally regardless of response amplitude. Implemented in
    /// `crate::compute::position_gaussian_smooth` (Gaussian-smooth the phase
    /// angle, rebuild a unit phasor); the position map is a linear remap of the
    /// phase, so this reproduces Allen's gradient directions / VFS.
    AllenZhuang2017PositionGaussian { sigma_px: f64 },
}

impl PhaseSmoothingMethod {
    /// SNLC amplitude-weighted phasor smoothing, from a registry-sourced σ.
    pub fn snlc_amp_weighted_phasor(
        sigma_px: Tagged<PhaseSmoothingSnlcAmpWeightedPhasorSigmaPx>,
    ) -> Self {
        Self::SnlcAmpWeightedPhasor {
            sigma_px: sigma_px.into_inner(),
        }
    }

    /// Allen unweighted scalar-Gaussian position smoothing, from a registry σ.
    pub fn allen_zhuang2017_position_gaussian(
        sigma_px: Tagged<PhaseSmoothingAllenZhuang2017PositionGaussianSigmaPx>,
    ) -> Self {
        Self::AllenZhuang2017PositionGaussian {
            sigma_px: sigma_px.into_inner(),
        }
    }

    /// Smooth the per-orientation position phasors. Returns
    /// `(azi_z_smoothed, alt_z_smoothed)` as [`compute::Complex2`] on the active
    /// device. The amplitude inputs are used only by the SNLC variant.
    pub fn apply(
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

    /// Sigma value carried by this variant, in pixels.
    pub fn sigma_px(&self) -> f64 {
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
