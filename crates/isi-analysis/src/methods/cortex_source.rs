//! Stage 5 — Cortex / ROI source.
//!
//! Resolves the binary mask defining the spatial extent of the imaged
//! cortex. Patch detection happens only within this mask. Different
//! methods have different prerequisites: reliability needs per-cycle
//! complex maps; the ring aperture needs a rig calibration; full-frame
//! is universally available but adds no restriction.

use ndarray::Array2;
use openisi_params::{
    CortexSourceReliabilityThreshold, CortexSourceSnlcClose, CortexSourceSnlcDilate,
    CortexSourceSnlcK, Tagged,
};

use crate::{AnalysisError, ReliabilityMaps};

/// Method choice for resolving the cortex mask.
///
/// `#[non_exhaustive]` + per-variant constructors enforce registry-
/// sourced tunables.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum CortexSourceMethod {
    /// Cross-cycle reliability cortex (Allen Brain Observatory; Zhuang,
    /// Ng, Williams, Valley, Li, Garrett, Waters 2017, eLife 6:e18372).
    /// Largest connected component of pixels where the per-direction
    /// reliability (amp-weighted vector coherence across cycles) exceeds
    /// `threshold` for *all four* directions, with `largest_cc →
    /// fill_holes` cleanup. Requires per-cycle data — fails over to a
    /// different method (orchestrator's choice) for cycle-averaged
    /// imports without reliability.
    Reliability { threshold: f64 },

    /// User-drawn polygon stored at `.oisi /anatomical/cortex_roi` as a
    /// bool mask. Pure user input, no inference. The orchestrator reads
    /// the mask from the file via `crate::io::read_cortex_roi`. Variant
    /// carries no parameters — the mask is pulled from the file.
    UserPolygon,

    /// SNLC `imbound` from Garrett, Nauhaus, Marshel, Callaway 2014,
    /// J Neurosci 34(37):12587-12600; implementation in
    /// `getMouseAreasX.m` lines 76–95 of SNLC MATLAB toolbox.
    /// VFS-structure morphology: σ-scaled threshold of smoothed VFS
    /// (`|VFS| > k · σ(VFS) / 2`) → imopen(disk(2)) →
    /// imclose(disk(`close`)) → imfill → imdilate(disk(`dilate`)) →
    /// imfill → keep largest 4-connected component.
    ///
    /// Known failure mode: σ self-cancels on noise-dominated data
    /// (apertured single-cycle, etc.), expanding cortex to most of the
    /// frame. Use only for clean signal-dominated data per the Garrett
    /// 2014 assumption.
    SnlcGarrett2014ImBound { k: f64, close: i32, dilate: i32 },

    /// No cortex restriction — analysis runs over the full frame and
    /// the sign-map threshold + morphology does all the patch gating.
    /// Allen `retinotopic_mapping` happens to operate this way by default
    /// (Zhuang 2017; `RetinotopicMapping.py` operates on full frames)
    /// but they did not introduce "full frame" as a distinct method —
    /// they simply omitted the restriction. Named for what it does.
    /// Used for cycle-averaged imports where reliability isn't
    /// available and no user override is provided.
    NoRestriction,
}

impl CortexSourceMethod {
    pub fn reliability(threshold: Tagged<CortexSourceReliabilityThreshold>) -> Self {
        Self::Reliability {
            threshold: threshold.into_inner(),
        }
    }

    pub fn user_polygon() -> Self {
        Self::UserPolygon
    }

    pub fn snlc_garrett2014_im_bound(
        k: Tagged<CortexSourceSnlcK>,
        close: Tagged<CortexSourceSnlcClose>,
        dilate: Tagged<CortexSourceSnlcDilate>,
    ) -> Self {
        Self::SnlcGarrett2014ImBound {
            k: k.into_inner(),
            close: close.into_inner(),
            dilate: dilate.into_inner(),
        }
    }

    pub fn no_restriction() -> Self {
        Self::NoRestriction
    }

    /// Short label for this variant — used in figure-grid headers.
    pub fn short_label(&self) -> &'static str {
        match self {
            Self::Reliability { .. } => "reliability",
            Self::UserPolygon => "user_polygon",
            Self::SnlcGarrett2014ImBound { .. } => "snlc_imbound",
            Self::NoRestriction => "no_restriction",
        }
    }
}

/// Inputs available when resolving a cortex mask. Different variants
/// consume different fields. The orchestrator builds this and passes
/// it to `CortexSourceMethod::apply`.
pub struct CortexResolveContext<'a> {
    pub shape: (usize, usize),
    /// Per-direction reliability maps (raw acquisition path only).
    pub reliability: Option<&'a ReliabilityMaps>,
    /// User-drawn polygon mask from `.oisi /anatomical/cortex_roi`.
    pub user_polygon: Option<Array2<bool>>,
    /// Smoothed VFS, needed for `SnlcGarrett2014ImBound`.
    pub vfs_smoothed: Option<&'a Array2<f64>>,
}

impl CortexSourceMethod {
    /// Resolve the cortex mask under the active method. Returns an
    /// error if the variant's required input isn't in `ctx`.
    pub fn apply(&self, ctx: &CortexResolveContext) -> Result<Array2<bool>, AnalysisError> {
        use crate::segmentation::connectivity::keep_largest_component;
        use crate::segmentation::morphology::{
            binary_closing_disk, binary_dilation_disk, binary_fill_holes, binary_opening_disk,
        };
        match self {
            Self::Reliability { threshold } => {
                let rel = ctx.reliability.ok_or_else(|| {
                    AnalysisError::MissingData(
                        "CortexSourceMethod::Reliability requires per-cycle reliability maps; \
                     the file has no raw acquisition data"
                            .into(),
                    )
                })?;
                Ok(crate::segmentation::cortex_from_reliability(
                    &rel.rel_azi_fwd,
                    &rel.rel_azi_rev,
                    &rel.rel_alt_fwd,
                    &rel.rel_alt_rev,
                    *threshold,
                ))
            }
            Self::UserPolygon => ctx.user_polygon.clone().ok_or_else(|| {
                AnalysisError::MissingData(
                    "CortexSourceMethod::UserPolygon requires /anatomical/cortex_roi in the .oisi file"
                        .into(),
                )
            }),
            Self::NoRestriction => Ok(Array2::from_elem(ctx.shape, true)),
            Self::SnlcGarrett2014ImBound { k, close, dilate } => {
                let (k, close, dilate) = (*k, *close, *dilate);
                let vfs = ctx.vfs_smoothed.ok_or_else(|| {
                    AnalysisError::MissingData(
                        "CortexSourceMethod::SnlcGarrett2014ImBound requires the smoothed VFS \
                     (vfs_smoothed) for the σ-scaled threshold"
                            .into(),
                    )
                })?;
                let std_vfs = std_of_finite(vfs);
                let thr_mask = k * std_vfs * 0.5;
                let imseg = Array2::from_shape_fn(vfs.dim(), |(r, c)| {
                    let v = vfs[[r, c]];
                    v.is_finite() && v.abs() > thr_mask
                });
                let opened = binary_opening_disk(&imseg, 2);
                let closed = binary_closing_disk(&opened, close);
                let filled = binary_fill_holes(&closed);
                let dilated = binary_dilation_disk(&filled, dilate);
                let filled2 = binary_fill_holes(&dilated);
                Ok(keep_largest_component(&filled2))
            }
        }
    }
}

/// σ of finite values in a 2D array. Used by `SnlcGarrett2014ImBound`
/// for the SNLC `threshSeg = 1.5 * std(VFS(:))` formula.
fn std_of_finite(data: &Array2<f64>) -> f64 {
    // Sample (N−1) std over finite values — SNLC/Garrett 2014 `std(VFS(:))`.
    // Uses ndarray's validated two-pass `.std(ddof=1)`, which matches MATLAB
    // `std` (also two-pass) bit-for-bit, rather than a one-pass
    // sum-of-squares (less numerically stable).
    let finite: Vec<f64> = data.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.len() < 2 {
        return 0.0;
    }
    ndarray::Array1::from_vec(finite).std(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_frame_resolves_to_all_true() {
        let m = CortexSourceMethod::NoRestriction;
        let ctx = CortexResolveContext {
            shape: (10, 10),
            reliability: None,
            user_polygon: None,
            vfs_smoothed: None,
        };
        let mask = m.apply(&ctx).unwrap();
        assert_eq!(mask.dim(), (10, 10));
        assert!(mask.iter().all(|&b| b));
    }

    #[test]
    fn reliability_without_data_errors() {
        let m = CortexSourceMethod::Reliability { threshold: 0.5 };
        let ctx = CortexResolveContext {
            shape: (10, 10),
            reliability: None,
            user_polygon: None,
            vfs_smoothed: None,
        };
        assert!(m.apply(&ctx).is_err());
    }
}
