//! ΔF/F baseline method (the `Baseline` pipeline stage).
//!
//! The ΔF/F baseline `F0` is the denominator the bin-1 DFT runs on: each cycle
//! is uploaded as `(F − F0)/max(F0, floor)` rather than raw counts, so the
//! amplitude that drives cortex masking / phase weighting reflects *fractional*
//! response. This method's sole concern is *how `F0` is estimated*; the Fourier
//! projection that consumes it is a separate stage
//! ([`crate::compute::projection::run`]).

use ndarray::Array2;

use crate::compute::{self, BaselineAggregate};
use crate::RawAcquisition;

/// Method choice for the ΔF/F baseline `F0` (the `Baseline` stage).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum BaselineMethod {
    /// Allen `ImageAnalysis.normalizeMovie` baseline: per-pixel temporal **mean**
    /// over all frames (`np.mean(movie, axis=0)`). The faithful oracle path;
    /// includes the stimulus sweeps in `F0`.
    AllenAllFrameMean,
    /// Allen `normalizeMovie(baselineType='median')`: per-pixel temporal
    /// **median** over all frames (`np.median`, axis=0). Robust to a transient
    /// bright frame; otherwise equivalent to the mean on real data.
    AllenAllFrameMedian,
    /// OpenISI inter-sweep baseline: per-pixel **mean** over only the rest frames
    /// (before the first sweep + the inter-sweep gaps), so stimulus-driven
    /// activity does not contaminate `F0`. The more principled resting baseline;
    /// falls back to the all-frame mean when a schedule has no rest gaps.
    OpenIsiInterSweepMean,
    /// OpenISI inter-sweep baseline using the per-pixel **median** of the rest
    /// frames. Same rest-frame selection as [`Self::OpenIsiInterSweepMean`].
    OpenIsiInterSweepMedian,
}

impl BaselineMethod {
    pub fn allen_all_frame_mean() -> Self {
        Self::AllenAllFrameMean
    }
    pub fn allen_all_frame_median() -> Self {
        Self::AllenAllFrameMedian
    }
    pub fn open_isi_inter_sweep_mean() -> Self {
        Self::OpenIsiInterSweepMean
    }
    pub fn open_isi_inter_sweep_median() -> Self {
        Self::OpenIsiInterSweepMedian
    }

    /// Estimate the ΔF/F baseline: the per-pixel `F0` map plus its paired
    /// denominator `floor`. Single entry-point matching every other method
    /// node's `apply()` (the two outputs are always produced together — the
    /// floor is derived from `F0` — so they are returned as one
    /// [`BaselineResult`]).
    ///
    /// The inter-sweep variants fall back to the all-frame mean when there are
    /// no rest frames (a gapless schedule) — see
    /// [`compute::inter_sweep_baseline`].
    pub fn apply(&self, raw: &RawAcquisition) -> BaselineResult {
        let f0 = match self {
            Self::AllenAllFrameMean => compute::temporal_mean_baseline(&raw.frames),
            Self::AllenAllFrameMedian => compute::temporal_median_baseline(&raw.frames),
            Self::OpenIsiInterSweepMean => compute::inter_sweep_baseline(
                &raw.frames,
                &raw.cam_ts_sec,
                &raw.sweep_start_sec,
                &raw.sweep_end_sec,
                BaselineAggregate::Mean,
            )
            .unwrap_or_else(|| compute::temporal_mean_baseline(&raw.frames)),
            Self::OpenIsiInterSweepMedian => compute::inter_sweep_baseline(
                &raw.frames,
                &raw.cam_ts_sec,
                &raw.sweep_start_sec,
                &raw.sweep_end_sec,
                BaselineAggregate::Median,
            )
            .unwrap_or_else(|| compute::temporal_mean_baseline(&raw.frames)),
        };
        let floor = compute::dff_denominator_floor(&f0);
        BaselineResult { f0, floor }
    }
}

/// The output of [`BaselineMethod::apply`]: the per-pixel `F0` baseline map and
/// the ΔF/F denominator `floor` (half the median `F0`) it is paired with when
/// uploading each cycle's ΔF/F.
#[derive(Clone, Debug)]
pub struct BaselineResult {
    pub f0: Array2<f64>,
    pub floor: f64,
}
