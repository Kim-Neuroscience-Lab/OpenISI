//! Stage 5 — Cortex / ROI source.
//!
//! Resolves the binary mask defining the spatial extent of the imaged
//! cortex. Patch detection happens only within this mask. Different
//! methods have different prerequisites: reliability needs per-cycle
//! complex maps; the ring aperture needs a rig calibration; full-frame
//! is universally available but adds no restriction.

use ndarray::Array2;
use openisi_params::config::analysis::CortexSource;

use crate::{AnalysisError, ReliabilityMaps};

/// Method choice for resolving the cortex mask.
///
/// The canonical type is [`openisi_params::config::analysis::CortexSource`] — the
/// garde-validated, internally-tagged config enum (variants documented there).
/// `isi-analysis` consumes it directly (UNIFY: config tunables ≡ compute
/// tunables); the compute behavior is attached here via [`CortexSourceExt`].
/// The `CortexSourceMethod` alias keeps existing references stable during the
/// migration (renamed to `CortexSource` in the final cleanup pass).
pub use openisi_params::config::analysis::CortexSource as CortexSourceMethod;

/// Compute behavior for the cortex-source stage (extension trait — the data type
/// lives in `openisi-params`, the algorithm lives here).
pub trait CortexSourceExt {
    /// Resolve the cortex mask under the active method. Errors if the variant's
    /// required input isn't in `ctx`.
    fn apply(&self, ctx: &CortexResolveContext) -> Result<Array2<bool>, AnalysisError>;
    /// Short label for this variant — used in figure-grid headers.
    fn short_label(&self) -> &'static str;
}

/// Inputs available when resolving a cortex mask. Different variants
/// consume different fields. The orchestrator builds this and passes
/// it to [`CortexSourceExt::apply`].
pub struct CortexResolveContext<'a> {
    pub shape: (usize, usize),
    /// Per-direction reliability maps (raw acquisition path only).
    pub reliability: Option<&'a ReliabilityMaps>,
    /// User-drawn polygon mask from `.oisi /anatomical/cortex_roi`.
    pub user_polygon: Option<Array2<bool>>,
    /// Smoothed VFS, needed for `SnlcGarrett2014ImBound`.
    pub vfs_smoothed: Option<&'a Array2<f64>>,
    /// Combined per-pixel response magnitude (mean of the azimuth and altitude
    /// position amplitudes), needed for `SnlcMagThreshold`.
    pub response_magnitude: Option<&'a Array2<f64>>,
}

impl CortexSourceExt for CortexSource {
    fn short_label(&self) -> &'static str {
        match self {
            Self::Reliability { .. } => "reliability",
            Self::UserPolygon => "user_polygon",
            Self::SnlcGarrett2014ImBound { .. } => "snlc_imbound",
            Self::SnlcMagThreshold { .. } => "snlc_magthr",
            Self::NoRestriction => "no_restriction",
        }
    }

    /// Resolve the cortex mask under the active method. Returns an
    /// error if the variant's required input isn't in `ctx`.
    fn apply(&self, ctx: &CortexResolveContext) -> Result<Array2<bool>, AnalysisError> {
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
            Self::SnlcMagThreshold {
                exponent,
                threshold,
            } => {
                let mag = ctx.response_magnitude.ok_or_else(|| {
                    AnalysisError::MissingData(
                        "CortexSourceMethod::SnlcMagThreshold requires the response magnitude \
                     (response_magnitude) from the retinotopy stage"
                            .into(),
                    )
                })?;
                Ok(snlc_mag_threshold_roi(mag, *exponent, *threshold))
            }
        }
    }
}

/// SNLC response-magnitude ROI gate — verbatim `overlaymaps.m:205-215`:
///
/// ```text
/// mag = magf.^1.1;          % raise to the exponent (spreads the values)
/// mag = mag - min(mag(:));  % normalize to [0, 1] over the whole frame
/// mag = mag / max(mag(:));
/// magROI = mag >= thresh;   % keep pixels at/above the threshold
/// ```
///
/// Pure intensity gate — no morphology (faithful to the source, which thresholds
/// the normalized magnitude directly). Non-finite magnitude pixels are treated
/// as below threshold. A degenerate constant magnitude (`max == min`) yields an
/// empty ROI (matches MATLAB: `mag/0 → NaN`, and `NaN >= thresh` is false).
pub fn snlc_mag_threshold_roi(
    response_magnitude: &Array2<f64>,
    exponent: f64,
    threshold: f64,
) -> Array2<bool> {
    // mag = magf.^exponent over finite pixels (non-finite → −inf so it can't be
    // the running min/max and lands below threshold after normalization).
    let powered = response_magnitude.mapv(|v| if v.is_finite() { v.powf(exponent) } else { f64::NAN });
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for &v in powered.iter() {
        if v.is_finite() {
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    let span = hi - lo;
    Array2::from_shape_fn(powered.dim(), |(r, c)| {
        let v = powered[[r, c]];
        // mag normalized to [0,1]; degenerate span (≤0) ⇒ empty ROI.
        v.is_finite() && span > 0.0 && (v - lo) / span >= threshold
    })
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
        let m = CortexSource::NoRestriction;
        let ctx = CortexResolveContext {
            shape: (10, 10),
            reliability: None,
            user_polygon: None,
            vfs_smoothed: None,
            response_magnitude: None,
        };
        let mask = m.apply(&ctx).unwrap();
        assert_eq!(mask.dim(), (10, 10));
        assert!(mask.iter().all(|&b| b));
    }

    #[test]
    fn reliability_without_data_errors() {
        let m = CortexSource::Reliability { threshold: 0.5 };
        let ctx = CortexResolveContext {
            shape: (10, 10),
            reliability: None,
            user_polygon: None,
            vfs_smoothed: None,
            response_magnitude: None,
        };
        assert!(m.apply(&ctx).is_err());
    }
}
