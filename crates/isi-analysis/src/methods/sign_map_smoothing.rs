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
    Gaussian { sigma_um: f64 },
}

impl SignMapSmoothingMethod {
    /// Construct the Gaussian variant from a registry-sourced σ value.
    /// The `Tagged<SignMapSmoothingGaussianSigmaUm>` argument can only
    /// be produced by `RegistrySnapshot::typed::<SignMapSmoothingGaussianSigmaUm>`.
    pub fn gaussian(sigma_um: Tagged<SignMapSmoothingGaussianSigmaUm>) -> Self {
        Self::Gaussian {
            sigma_um: sigma_um.into_inner(),
        }
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
    use approx::assert_abs_diff_eq;
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

    // Property: σ=0 is identity (gaussian_smooth_f64 short-circuits to clone).
    // Verifies the well-known mathematical truth that a zero-width Gaussian is the
    // identity transform — bit-exact via the clone path.
    #[test]
    fn property_sigma_zero_is_identity() {
        let mut data = Array2::<f64>::zeros((8, 8));
        for (i, v) in data.iter_mut().enumerate() {
            *v = (i as f64).sin();
        }
        let m = SignMapSmoothingMethod::Gaussian { sigma_um: 0.0 };
        let out = m.apply(&data, 20.0);
        assert_eq!(out, data, "σ=0 must be bit-exact identity");
    }

    // Property: constant field is preserved (Gaussian kernel sums to 1; reflection
    // padding preserves the constant at all boundaries). Mass-preservation invariant.
    #[test]
    fn property_constant_field_preserved() {
        let k: f64 = 0.7;
        let data = Array2::<f64>::from_elem((16, 16), k);
        let m = SignMapSmoothingMethod::Gaussian { sigma_um: 60.0 };
        let out = m.apply(&data, 20.0);
        for &v in out.iter() {
            assert_abs_diff_eq!(v, k, epsilon = 1e-10);
        }
    }

    // Property: linearity. smooth(α·A + β·B) == α·smooth(A) + β·smooth(B).
    // Gaussian convolution is a linear operator; this is the defining linearity
    // invariant. Done at σ_um > 0 to exercise the actual filter path.
    #[test]
    fn property_linearity() {
        let mut a = Array2::<f64>::zeros((16, 16));
        let mut b = Array2::<f64>::zeros((16, 16));
        for ((r, c), v) in a.indexed_iter_mut() {
            *v = ((r * 3 + c * 5) as f64).sin();
        }
        for ((r, c), v) in b.indexed_iter_mut() {
            *v = ((r * 7 + c * 2) as f64).cos();
        }
        let alpha = 1.7_f64;
        let beta = -0.4_f64;
        let combo = &a * alpha + &b * beta;

        let m = SignMapSmoothingMethod::Gaussian { sigma_um: 40.0 };
        let smoothed_combo = m.apply(&combo, 20.0);
        let smoothed_combination = m.apply(&a, 20.0) * alpha + m.apply(&b, 20.0) * beta;

        for (l, r) in smoothed_combo.iter().zip(smoothed_combination.iter()) {
            assert_abs_diff_eq!(l, r, epsilon = 1e-10);
        }
    }
}
