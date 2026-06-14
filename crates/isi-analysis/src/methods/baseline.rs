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
///
/// The canonical type is [`openisi_params::config::analysis::Baseline`] (the
/// garde-validated, internally-tagged config enum; variants documented there).
/// `isi-analysis` consumes it directly (UNIFY); compute behavior is attached via
/// [`BaselineExt`]. `BaselineMethod` is a transitional alias (renamed in cleanup).
pub use openisi_params::config::analysis::Baseline as BaselineMethod;

/// Compute behavior for the ΔF/F-baseline stage (extension trait).
pub trait BaselineExt {
    /// Estimate the ΔF/F baseline: the per-pixel `F0` map plus its paired
    /// denominator `floor`. The two outputs are always produced together (the
    /// floor is derived from `F0`), so they return as one [`BaselineResult`].
    /// Inter-sweep variants fall back to the all-frame mean when there are no
    /// rest frames (a gapless schedule) — see [`compute::inter_sweep_baseline`].
    fn apply(&self, raw: &RawAcquisition) -> BaselineResult;
}

impl BaselineExt for BaselineMethod {
    fn apply(&self, raw: &RawAcquisition) -> BaselineResult {
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
