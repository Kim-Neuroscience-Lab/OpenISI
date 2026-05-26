//! Stage 10 — Eccentricity map computation.

use ndarray::Array2;

use crate::math::compute_eccentricity;

/// Method choice for the eccentricity map.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum EccentricityMethod {
    /// Whole-cortex V1-centric eccentricity (Garrett et al. 2014,
    /// J Neurosci 34(37):12587-12600). Single reference point at V1's
    /// center of mass in visual-field coordinates; eccentricity =
    /// great-circle distance from that point. The largest segmented
    /// area is taken as V1.
    Garrett2014WholeCortexV1,
}

impl EccentricityMethod {
    pub fn garrett2014_whole_cortex_v1() -> Self {
        Self::Garrett2014WholeCortexV1
    }

    /// Compute the eccentricity map.
    pub fn apply(
        &self,
        azi_phase_degrees: &Array2<f64>,
        alt_phase_degrees: &Array2<f64>,
        area_labels: &Array2<i32>,
    ) -> Array2<f64> {
        match self {
            Self::Garrett2014WholeCortexV1 => {
                compute_eccentricity(azi_phase_degrees, alt_phase_degrees, area_labels)
            }
        }
    }
}
