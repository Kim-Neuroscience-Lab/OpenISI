//! Stage 6 — Patch threshold (binary mask of candidate patch pixels).
//!
//! Given the smoothed VFS, produces a binary mask of pixels that are
//! candidate patch material. Downstream stages (extraction, refinement)
//! consume this mask.

use ndarray::Array2;
use openisi_params::{PatchThresholdAllenValue, PatchThresholdGarrettK, Tagged};

/// Method choice for the per-pixel patch threshold.
///
/// `#[non_exhaustive]` + per-variant constructors enforce registry-
/// sourced tunables.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum PatchThresholdMethod {
    /// Fixed absolute threshold on `|signMapf|` — Allen
    /// `retinotopic_mapping` `_getRawPatchMap` (Zhuang 2017, eLife
    /// 6:e18372; `RetinotopicMapping.py` L1099–1103, default
    /// `signMapThr = 0.35`). Pixels with `|signMapf| ≥ T` are
    /// foreground.
    ///
    /// This is the canonical Allen Python pipeline threshold. The
    /// fixed absolute value depends on the sign map being heavily
    /// smoothed (Allen `signMapFilterSigma = 9`) so noise tails are
    /// suppressed before thresholding.
    AllenZhuang2017FixedSignMapThr {
        value: f64,
    },

    /// σ-scaled threshold — Garrett et al. 2014, J Neurosci
    /// 34(37):12587-12600 (SNLC MATLAB `getMouseAreasX.m`:
    /// `threshSeg = k*std(VFS(:))`, then `|VFS| > threshSeg/2`).
    /// Actual threshold = `k · σ(VFS_smooth) / 2` where σ is computed
    /// over `vfs_smoothed` within the cortex mask.
    ///
    /// **Known failure mode**: σ inflates when the VFS distribution is
    /// bimodal-but-noisy (e.g. apertured single-cycle data), causing
    /// the threshold to collapse to ≈ noise median.
    Garrett2014SigmaScaled {
        k: f64,
    },
}

impl PatchThresholdMethod {
    pub fn allen_zhuang2017_fixed_sign_map_thr(value: Tagged<PatchThresholdAllenValue>) -> Self {
        Self::AllenZhuang2017FixedSignMapThr { value: value.into_inner() }
    }

    pub fn garrett2014_sigma_scaled(k: Tagged<PatchThresholdGarrettK>) -> Self {
        Self::Garrett2014SigmaScaled { k: k.into_inner() }
    }
}

/// Result of applying a `PatchThresholdMethod`. Both the binary mask
/// and the actual scalar threshold applied to `|signMapf|` are returned
/// so downstream stages and diagnostic figures can use the same value.
pub struct PatchThresholdOutput {
    /// Binary mask of candidate-patch pixels, masked to within
    /// `cortex_mask`.
    pub imseg: Array2<bool>,
    /// Actual threshold value applied to `|signMapf|`. For variants
    /// whose threshold is data-derived (e.g. `Garrett2014SigmaScaled`)
    /// this is the runtime value, not a carried parameter.
    pub threshold_applied: f64,
}

impl PatchThresholdMethod {
    /// Apply the threshold. Returns the binary mask plus the actual
    /// scalar threshold applied to `|signMapf|` (so diagnostic stages
    /// like `vfs_smoothed_thresholded` can use the same value).
    pub fn apply(
        &self,
        vfs_smoothed: &Array2<f64>,
        cortex_mask: &Array2<bool>,
    ) -> PatchThresholdOutput {
        let (h, w) = vfs_smoothed.dim();
        debug_assert_eq!(cortex_mask.dim(), (h, w));
        match self {
            Self::AllenZhuang2017FixedSignMapThr { value } => {
                let value = *value;
                let imseg = Array2::from_shape_fn((h, w), |(r, c)| {
                    let v = vfs_smoothed[[r, c]];
                    cortex_mask[[r, c]] && v.is_finite() && v.abs() >= value
                });
                PatchThresholdOutput { imseg, threshold_applied: value }
            }
            Self::Garrett2014SigmaScaled { k } => {
                // SNLC `getMouseAreasX.m` (Garrett 2014, J Neurosci
                // 34:12587): threshSeg = k * std(VFS); imseg = |VFS| >
                // threshSeg/2. σ is computed over `vfs_smoothed`
                // **within `cortex_mask`** when the cortex mask is
                // smaller than the full frame, so the threshold is
                // calibrated to cortex-resident signal rather than the
                // whole-frame noise floor.
                let std_vfs = std_of_finite_within(vfs_smoothed, cortex_mask);
                let thr_mask = *k * std_vfs * 0.5;
                let imseg = Array2::from_shape_fn((h, w), |(r, c)| {
                    let v = vfs_smoothed[[r, c]];
                    cortex_mask[[r, c]] && v.is_finite() && v.abs() > thr_mask
                });
                PatchThresholdOutput { imseg, threshold_applied: thr_mask }
            }
        }
    }
}

fn std_of_finite_within(data: &Array2<f64>, mask: &Array2<bool>) -> f64 {
    let (h, w) = data.dim();
    debug_assert_eq!(mask.dim(), (h, w));
    let mut n = 0usize;
    let mut sum = 0.0_f64;
    let mut sum_sq = 0.0_f64;
    for r in 0..h {
        for c in 0..w {
            if !mask[[r, c]] { continue; }
            let v = data[[r, c]];
            if v.is_finite() {
                n += 1;
                sum += v;
                sum_sq += v * v;
            }
        }
    }
    if n < 2 { return 0.0; }
    let mean = sum / n as f64;
    let var = (sum_sq / n as f64) - mean * mean;
    var.max(0.0).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;

    #[test]
    fn allen_threshold_gates_by_magnitude_and_cortex() {
        let mut v = Array2::<f64>::zeros((3, 3));
        v[[0, 0]] = 0.8;  // passes
        v[[0, 1]] = 0.2;  // below
        v[[0, 2]] = -0.5; // passes (|...|)
        let cortex = Array2::from_shape_fn((3, 3), |(r, _)| r < 1);
        let m = PatchThresholdMethod::AllenZhuang2017FixedSignMapThr { value: 0.35 };
        let out = m.apply(&v, &cortex);
        assert!(out.imseg[[0, 0]]);
        assert!(!out.imseg[[0, 1]]);
        assert!(out.imseg[[0, 2]]);
        assert_eq!(out.threshold_applied, 0.35);
        for c in 0..3 { assert!(!out.imseg[[1, c]], "outside cortex must be false"); }
    }

    #[test]
    fn garrett_returns_runtime_threshold_not_k() {
        let v = ndarray::array![[-0.8, -0.4, 0.0, 0.4, 0.8]];
        let cortex = Array2::from_elem((1, 5), true);
        let m = PatchThresholdMethod::Garrett2014SigmaScaled { k: 1.5 };
        let out = m.apply(&v, &cortex);
        assert!(out.threshold_applied < 1.0);
        assert!(out.threshold_applied > 0.1);
    }
}
