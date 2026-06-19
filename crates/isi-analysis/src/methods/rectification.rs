//! Pre-DFT rectification method (the `Rectification` projection-stage axis).
//!
//! Optional half-wave rectification of the per-cycle response movie before the
//! bin-1 DFT — Allen `HighLevel.getMappingMovies(isRectify=...)`. The default
//! (`None`) is a no-op, so the projection output is bit-identical to the
//! pre-existing pipeline; only the explicit `AllenZhuang2017ClipNegative`
//! variant alters the movie.

use burn_tensor::Tensor;

use crate::compute::Backend;

/// Method choice for pre-DFT rectification (the `Rectification` stage).
///
/// Canonical type: [`openisi_params::config::analysis::Rectification`] (the
/// garde-validated, internally-tagged config enum; variants documented there).
/// Compute behavior is attached via [`RectificationExt`].
pub use openisi_params::config::analysis::Rectification as RectificationMethod;

/// Compute behavior for the rectification stage (extension trait).
pub trait RectificationExt {
    /// Apply the selected rectification to a per-cycle response movie tensor
    /// (`frames × H × W`) just before the bin-1 DFT. `None` returns the input
    /// unchanged (the validated default); the clip variant zeroes negative
    /// samples — Allen `aveMovNorRec[aveMovNorRec < 0] = 0`.
    fn apply(&self, movie: Tensor<Backend, 3>) -> Tensor<Backend, 3>;
}

impl RectificationExt for RectificationMethod {
    fn apply(&self, movie: Tensor<Backend, 3>) -> Tensor<Backend, 3> {
        match self {
            Self::None => movie,
            // Half-wave rectify: clip negatives to zero. On OpenISI's response
            // movie this zeroes the same samples Allen's clip does (the sign of
            // `F − F0` is preserved by any positive per-pixel normalization).
            Self::AllenZhuang2017ClipNegative => movie.clamp_min(0.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compute::{device, tensor_to_array2_f64};
    use burn_tensor::TensorData;

    /// `AllenZhuang2017ClipNegative` is exactly Allen's
    /// `aveMovNorRec[aveMovNorRec < 0] = 0` — an elementwise `max(x, 0)`. Pin
    /// that semantics (negatives → 0, non-negatives unchanged, incl. −0/0); and
    /// `None` is a true pass-through.
    #[test]
    fn clip_negative_matches_allen_half_wave_rectify() {
        // A 1×2×3 movie with mixed signs incl. a boundary 0.0.
        let input = vec![-3.5_f32, -0.0, 0.0, 2.25, -1e-7, 100.0];
        let expected = [0.0_f64, 0.0, 0.0, 2.25, 0.0, 100.0];
        let mk = || {
            Tensor::<Backend, 3>::from_data(TensorData::new(input.clone(), [1, 2, 3]), &device())
        };

        let rect = RectificationMethod::AllenZhuang2017ClipNegative.apply(mk());
        // collapse the singleton frame dim for the f64 readback helper.
        let got = tensor_to_array2_f64(rect.reshape([2, 3])).unwrap();
        for (g, e) in got.iter().zip(expected.iter()) {
            assert_eq!(*g, *e, "clip-negative must match max(x,0) bit-for-bit");
        }

        // None passes through unchanged.
        let none = RectificationMethod::None.apply(mk());
        let got_none = tensor_to_array2_f64(none.reshape([2, 3])).unwrap();
        let in_f64: Vec<f64> = input.iter().map(|&v| f64::from(v)).collect();
        for (g, e) in got_none.iter().zip(in_f64.iter()) {
            assert_eq!(*g, *e, "None rectification must be a pass-through");
        }
    }
}
