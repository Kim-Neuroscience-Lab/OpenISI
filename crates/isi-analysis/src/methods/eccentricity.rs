//! Stage 10 — Eccentricity map computation.

use ndarray::Array2;

use crate::math::{compute_eccentricity, compute_eccentricity_snlc};

/// Method choice for the eccentricity map.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum EccentricityMethod {
    /// **OpenISI** whole-cortex V1-centric eccentricity. V1 = the largest
    /// segmented area; the reference point is V1's center of mass in
    /// **visual-field coordinates** (the mean of azi/alt over all V1 pixels),
    /// and eccentricity uses the **Allen cos-on-altitude** great-circle formula
    /// ([`crate::math::eccentricity_pixel_deg`]).
    ///
    /// This is an OpenISI composition — it pairs an Allen-convention formula
    /// with a mean-over-pixels center, which is **neither** Allen's nor SNLC's
    /// exact recipe. For the faithful SNLC reference-point selection use
    /// [`Self::SnlcGetAreaBordersV1Center`].
    OpenIsiWholeCortexV1,

    /// **Faithful SNLC** V1-center selection (`getAreaBorders.m` +
    /// `getV1id.m` + `getPatchCoM.m`). The mask is `imopen(disk-10)`'d before
    /// V1 is taken as the largest 4-connected component; the reference point is
    /// a single-pixel sample of azi/alt at that component's **pixel-space**
    /// centroid (off-patch-snapped); the formula is SNLC cos-on-azimuth
    /// ([`crate::math::compute_eccentricity_snlc`]).
    SnlcGetAreaBordersV1Center,
}

impl EccentricityMethod {
    pub fn open_isi_whole_cortex_v1() -> Self {
        Self::OpenIsiWholeCortexV1
    }
    pub fn snlc_get_area_borders_v1_center() -> Self {
        Self::SnlcGetAreaBordersV1Center
    }

    /// Compute the eccentricity map.
    pub fn apply(
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
