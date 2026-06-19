//! Response-normalization method (the `ResponseNormalization` stage).
//!
//! Selects how the per-cycle response movie is formed against the baseline `F0`
//! before the bin-1 DFT: OpenISI's fractional ΔF/F (`(F − F0)/max(F0, floor)`,
//! the default) or the oracle-faithful absolute response (`F − F0`, no division
//! — SNLC `Gf1image.m` / Allen `generatePhaseMap2`).
//!
//! Both formulations yield the **same F1 phase** (the per-pixel `1/F0` factor is
//! a positive real scale, invisible to `arg`); they differ only in the F1
//! **magnitude**, which feeds cortex masking and amplitude-weighted smoothing.
//! See the `response_normalization_phase_equivalence` golden.

/// Method choice for response normalization (the `ResponseNormalization` stage).
///
/// Canonical type: [`openisi_params::config::analysis::ResponseNormalization`]
/// (the garde-validated, internally-tagged config enum; variants documented
/// there). Compute behavior is attached via [`ResponseNormalizationExt`].
pub use openisi_params::config::analysis::ResponseNormalization as ResponseNormalizationMethod;

/// Compute behavior for the response-normalization stage (extension trait).
pub trait ResponseNormalizationExt {
    /// Whether the per-cycle response is divided by the baseline `F0`
    /// (fractional ΔF/F). `false` selects the absolute response `F − F0` with
    /// no division — the oracle-faithful F1 amplitude. Consumed by
    /// [`crate::compute::frames_u16_subset_to_dff_tensor`] via `projection::run`.
    fn divides_by_baseline(&self) -> bool;
}

impl ResponseNormalizationExt for ResponseNormalizationMethod {
    fn divides_by_baseline(&self) -> bool {
        match self {
            Self::OpenIsiFractionalDff => true,
            Self::OracleAbsoluteDeltaF => false,
        }
    }
}
