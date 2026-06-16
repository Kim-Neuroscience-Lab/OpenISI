//! Complex tensors as paired real/imaginary `f32` tensors.
//!
//! Burn has no native complex dtype (neither does Candle — it's an ML
//! gap, since ML doesn't carry complex numbers). Per the dtype
//! conventions in `docs/COMPUTE.md` we represent a complex
//! field as a `Complex2` pair of `Tensor<Backend, 2>` — one real plane,
//! one imaginary plane — with the complex operations the analysis
//! pipeline needs: `from_phase`, `real`, `imag`, `abs`, `angle`,
//! `phase_shift`, `real_imag_sum`. This is the single complex
//! abstraction; the complex ops in `ops` are written against it.
//! (Operations like complex `mul`/`conj` are deliberately absent — no
//! pipeline stage needs them; they're added if and when one does.)
//!
//! `Complex2` is wired into production via `math::compute_retinotopy`, and
//! the DFT-path complex ops that also use it feed the cycle accumulator
//! and `io::compute_complex_maps_from_raw`.

use burn_tensor::Tensor;

use super::backend::Backend;

/// A complex 2D field as a (real, imaginary) pair of `f32` tensors of
/// identical shape. Cheap to clone (Burn tensors are reference-counted).
#[derive(Clone, Debug)]
pub struct Complex2 {
    pub re: Tensor<Backend, 2>,
    pub im: Tensor<Backend, 2>,
}

impl Complex2 {
    /// Construct from explicit real and imaginary planes.
    pub fn new(re: Tensor<Backend, 2>, im: Tensor<Backend, 2>) -> Self {
        Self { re, im }
    }

    /// Unit phasor `exp(i·φ) = (cos φ, sin φ)` from a real phase field.
    pub fn from_phase(phi: Tensor<Backend, 2>) -> Self {
        Self {
            re: phi.clone().cos(),
            im: phi.sin(),
        }
    }

    /// Real part (clone — the field stays owned by `self`).
    pub fn real(&self) -> Tensor<Backend, 2> {
        self.re.clone()
    }

    /// Imaginary part.
    pub fn imag(&self) -> Tensor<Backend, 2> {
        self.im.clone()
    }

    /// Magnitude `|z| = sqrt(re² + im²)`.
    pub fn abs(&self) -> Tensor<Backend, 2> {
        let re2 = self.re.clone() * self.re.clone();
        let im2 = self.im.clone() * self.im.clone();
        (re2 + im2).sqrt()
    }

    /// Argument `atan2(im, re) ∈ (−π, π]`.
    pub fn angle(&self) -> Tensor<Backend, 2> {
        self.im.clone().atan2(self.re.clone())
    }

    /// Rotate every element by `exp(i·offset)` (uniform phase shift).
    /// `(re + i·im)·(cos θ + i·sin θ)`. Used by the cycle accumulator's
    /// phase-locked averaging.
    pub fn phase_shift(self, offset: f64) -> Self {
        let (c, s) = (offset.cos() as f32, offset.sin() as f32);
        let new_re = self.re.clone().mul_scalar(c) - self.im.clone().mul_scalar(s);
        let new_im = self.re.mul_scalar(s) + self.im.mul_scalar(c);
        Self {
            re: new_re,
            im: new_im,
        }
    }

    /// Sum of all real parts and all imaginary parts, as `(f64, f64)`.
    /// Used to compute a cycle's global phase `arg(Σ_pixels z)`.
    pub fn real_imag_sum(&self) -> (f64, f64) {
        let re_sum: f32 = self.re.clone().sum().into_scalar();
        let im_sum: f32 = self.im.clone().sum().into_scalar();
        (re_sum as f64, im_sum as f64)
    }

    /// Scale both planes by a real scalar. Used by the cycle accumulator's
    /// `1/K` averaging after phase-locked summation.
    pub fn mul_scalar(self, k: f32) -> Self {
        Self {
            re: self.re.mul_scalar(k),
            im: self.im.mul_scalar(k),
        }
    }
}

#[cfg(test)]
mod tests {
    //! Core-op goldens for `Complex2`. These are the foundational complex
    //! primitives every retinotopy stage is built on, so they are pinned
    //! directly (not just implicitly through the DFT/VFS goldens): a unit phasor
    //! round-trips through `angle`/`abs`, `phase_shift` rotates correctly, and
    //! `real_imag_sum` matches the manual sum.
    use super::*;
    use super::super::backend::device;
    use std::f64::consts::PI;

    fn c2(re: Vec<f32>, im: Vec<f32>, h: usize, w: usize) -> Complex2 {
        let dev = device();
        Complex2::new(
            Tensor::from_data(burn_tensor::TensorData::new(re, [h, w]), &dev),
            Tensor::from_data(burn_tensor::TensorData::new(im, [h, w]), &dev),
        )
    }
    fn flat(t: Tensor<Backend, 2>) -> Vec<f32> {
        t.into_data().into_vec::<f32>().unwrap()
    }

    #[test]
    fn from_phase_is_unit_phasor_and_round_trips_through_angle() {
        // Phases kept strictly inside (−π, π) so the atan2 round-trip doesn't
        // straddle the branch cut at ±π (where +π and −π are the same point).
        let phis = [0.0_f32, (PI / 2.0) as f32, (3.0 * PI / 4.0) as f32, (-PI / 3.0) as f32];
        let phi_t: Tensor<Backend, 2> =
            Tensor::from_data(burn_tensor::TensorData::new(phis.to_vec(), [1, 4]), &device());
        let z = Complex2::from_phase(phi_t);
        let (re, im) = (flat(z.real()), flat(z.imag()));
        let abs = flat(z.abs());
        let ang = flat(z.angle());
        for k in 0..4 {
            assert!((re[k] - phis[k].cos()).abs() < 1e-6, "re cos at {k}");
            assert!((im[k] - phis[k].sin()).abs() < 1e-6, "im sin at {k}");
            assert!((abs[k] - 1.0).abs() < 1e-6, "unit magnitude at {k}");
            assert!((ang[k] - phis[k]).abs() < 1e-6, "angle round-trip at {k}");
        }
    }

    #[test]
    fn abs_is_hypot_of_planes() {
        let z = c2(vec![3.0, -5.0], vec![4.0, 12.0], 1, 2);
        let abs = flat(z.abs());
        assert!((abs[0] - 5.0).abs() < 1e-6, "3,4 → 5");
        assert!((abs[1] - 13.0).abs() < 1e-6, "5,12 → 13");
    }

    #[test]
    fn phase_shift_rotates_by_offset() {
        // (1, 0) rotated by +π/2 → (0, 1); (0, 1) rotated by +π/2 → (-1, 0).
        let z = c2(vec![1.0, 0.0], vec![0.0, 1.0], 1, 2);
        let r = z.phase_shift(PI / 2.0);
        let (re, im) = (flat(r.real()), flat(r.imag()));
        assert!(re[0].abs() < 1e-6 && (im[0] - 1.0).abs() < 1e-6, "1+0i → i");
        assert!((re[1] + 1.0).abs() < 1e-6 && im[1].abs() < 1e-6, "0+1i → -1");
    }

    #[test]
    fn real_imag_sum_matches_manual_sum() {
        let re = vec![1.0, 2.0, -3.0, 0.5];
        let im = vec![-1.0, 0.0, 4.0, 2.5];
        let z = c2(re.clone(), im.clone(), 2, 2);
        let (sr, si) = z.real_imag_sum();
        assert!((sr - 0.5).abs() < 1e-5, "re sum = 0.5");
        assert!((si - 5.5).abs() < 1e-5, "im sum = 5.5");
    }
}
