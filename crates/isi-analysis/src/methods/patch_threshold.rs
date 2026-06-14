//! Stage 6 — Patch threshold (binary mask of candidate patch pixels).
//!
//! Given the smoothed VFS, produces a binary mask of pixels that are
//! candidate patch material. Downstream stages (extraction, refinement)
//! consume this mask.

use ndarray::Array2;

/// Method choice for the per-pixel patch threshold.
///
/// Canonical type: [`openisi_params::config::analysis::PatchThreshold`] (UNIFY);
/// compute behavior is attached via [`PatchThresholdExt`].
pub use openisi_params::config::analysis::PatchThreshold as PatchThresholdMethod;

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

/// Compute behavior for the patch-threshold stage (extension trait).
pub trait PatchThresholdExt {
    /// Apply the threshold. Returns the binary mask plus the actual scalar
    /// threshold applied to `|signMapf|` (so diagnostic stages like
    /// `vfs_smoothed_thresholded` can use the same value).
    fn apply(&self, vfs_smoothed: &Array2<f64>, cortex_mask: &Array2<bool>)
        -> PatchThresholdOutput;
}

impl PatchThresholdExt for PatchThresholdMethod {
    fn apply(
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
                PatchThresholdOutput {
                    imseg,
                    threshold_applied: value,
                }
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
                PatchThresholdOutput {
                    imseg,
                    threshold_applied: thr_mask,
                }
            }
        }
    }
}

fn std_of_finite_within(data: &Array2<f64>, mask: &Array2<bool>) -> f64 {
    debug_assert_eq!(mask.dim(), data.dim());
    // Sample (N−1) std over finite in-mask values — MATLAB-faithful two-pass
    // `.std(ddof=1)` (SNLC/Garrett 2014).
    let finite: Vec<f64> = data
        .iter()
        .zip(mask.iter())
        .filter(|(v, &m)| m && v.is_finite())
        .map(|(v, _)| *v)
        .collect();
    if finite.len() < 2 {
        return 0.0;
    }
    ndarray::Array1::from_vec(finite).std(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;

    #[test]
    fn allen_threshold_gates_by_magnitude_and_cortex() {
        let mut v = Array2::<f64>::zeros((3, 3));
        v[[0, 0]] = 0.8; // passes
        v[[0, 1]] = 0.2; // below
        v[[0, 2]] = -0.5; // passes (|...|)
        let cortex = Array2::from_shape_fn((3, 3), |(r, _)| r < 1);
        let m = PatchThresholdMethod::AllenZhuang2017FixedSignMapThr { value: 0.35 };
        let out = m.apply(&v, &cortex);
        assert!(out.imseg[[0, 0]]);
        assert!(!out.imseg[[0, 1]]);
        assert!(out.imseg[[0, 2]]);
        assert_eq!(out.threshold_applied, 0.35);
        for c in 0..3 {
            assert!(!out.imseg[[1, c]], "outside cortex must be false");
        }
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
