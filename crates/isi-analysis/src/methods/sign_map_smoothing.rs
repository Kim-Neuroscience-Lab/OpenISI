//! Stage 4 — Sign map smoothing.
//!
//! Smooths the raw VFS to suppress high-frequency noise tails before the
//! patch threshold is applied. Heavy smoothing is essential for the fixed
//! absolute threshold (Allen `signMapThr = 0.35`) to cleanly separate
//! real patches from noise on the smoothed map.

use ndarray::Array2;
use openisi_params::{SignMapSmoothingGaussianSigmaUm, Tagged};

use crate::segmentation::gaussian_smooth_f64;

/// Method choice for smoothing the visual field sign map.
///
/// `#[non_exhaustive]` + per-variant constructors below force every
/// construction to flow through `Self::gaussian(snap.typed::<…>())`,
/// which structurally proves the sigma value originated in the
/// canonical param registry (no inline `60.0` literals allowed).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum SignMapSmoothingMethod {
    /// Gaussian filter — Allen `retinotopic_mapping` `_getSignMap`
    /// (Zhuang 2017, eLife 6:e18372; `RetinotopicMapping.py` L1016–1017,
    /// L1002 default `signMapFilterSigma = 9.0` px). σ specified in
    /// **pixels** in Allen's code; OpenISI accepts σ in micrometers and
    /// converts at runtime via the rig's `camera_um_per_pixel` so the
    /// spatial extent of smoothing is constant across rig resolutions.
    Gaussian {
        sigma_um: f64,
    },
}

impl SignMapSmoothingMethod {
    /// Construct the Gaussian variant from a registry-sourced σ value.
    /// The `Tagged<SignMapSmoothingGaussianSigmaUm>` argument can only
    /// be produced by `RegistrySnapshot::typed::<SignMapSmoothingGaussianSigmaUm>`.
    pub fn gaussian(sigma_um: Tagged<SignMapSmoothingGaussianSigmaUm>) -> Self {
        Self::Gaussian { sigma_um: sigma_um.into_inner() }
    }

    /// Smooth the raw VFS, given the rig's spatial resolution.
    pub fn apply(&self, vfs: &Array2<f64>, um_per_pixel: f64) -> Array2<f64> {
        match self {
            Self::Gaussian { sigma_um } => {
                let sigma_px = *sigma_um / um_per_pixel.max(1e-6);
                gaussian_smooth_f64(vfs, sigma_px)
            }
        }
    }

    /// σ in pixels at the given imaging resolution. For diagnostics /
    /// figure captions.
    pub fn sigma_px(&self, um_per_pixel: f64) -> f64 {
        match self {
            Self::Gaussian { sigma_um } => *sigma_um / um_per_pixel.max(1e-6),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;

    #[test]
    fn shape_preserved() {
        let data = Array2::<f64>::zeros((32, 32));
        let m = SignMapSmoothingMethod::Gaussian { sigma_um: 100.0 };
        let out = m.apply(&data, 20.0);
        assert_eq!(out.dim(), (32, 32));
    }

    #[test]
    fn sigma_px_at_20um_per_px() {
        let m = SignMapSmoothingMethod::Gaussian { sigma_um: 60.0 };
        assert!((m.sigma_px(20.0) - 3.0).abs() < 1e-9, "σ_px=3 at 20 µm/px");
    }
}
