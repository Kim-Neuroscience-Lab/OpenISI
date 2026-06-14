//! Stage 10 — Eccentricity map computation.

use ndarray::Array2;

use crate::math::{compute_eccentricity, compute_eccentricity_snlc};

/// Method choice for the eccentricity map.
///
/// Canonical type: [`openisi_params::config::analysis::Eccentricity`] (UNIFY);
/// compute behavior is attached via [`EccentricityExt`].
pub use openisi_params::config::analysis::Eccentricity as EccentricityMethod;

/// Compute behavior for the eccentricity stage (extension trait).
pub trait EccentricityExt {
    /// Compute the eccentricity map.
    fn apply(
        &self,
        azi_phase_degrees: &Array2<f64>,
        alt_phase_degrees: &Array2<f64>,
        area_labels: &Array2<i32>,
    ) -> Array2<f64>;
}

impl EccentricityExt for EccentricityMethod {
    fn apply(
        &self,
        azi_phase_degrees: &Array2<f64>,
        alt_phase_degrees: &Array2<f64>,
        area_labels: &Array2<i32>,
    ) -> Array2<f64> {
        match self {
            Self::OpenIsiWholeCortexV1 => {
                compute_eccentricity(azi_phase_degrees, alt_phase_degrees, area_labels)
            }
            Self::SnlcGetAreaBordersV1Center => {
                compute_eccentricity_snlc(azi_phase_degrees, alt_phase_degrees, area_labels)
            }
        }
    }
}
