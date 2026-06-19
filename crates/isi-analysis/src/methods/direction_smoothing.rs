//! Pre-combine per-direction smoothing (the `DirectionSmoothing` stage).
//!
//! SNLC `Gprocesskret.m` smooths the **four per-direction complex F1 maps**
//! *before* delay-subtraction / cycle-combine. OpenISI's default instead smooths
//! the *combined* phasor afterwards (see [`crate::methods::PhaseSmoothing`]), so
//! the default here is `None` and the pipeline stays bit-identical.
//!
//! The `SnlcAdaptiveSmoother` variant is the verbatim Wiener-type
//! `adaptiveSmoother.m`. It is **nonlinear** (it divides by the local variance),
//! so it is faithful *only* applied per-direction pre-combine — applying it to
//! the merged phasor would match neither oracle. That is exactly why this is a
//! distinct stage from [`crate::methods::PhaseSmoothing`].

use ndarray::Array2;
use num_complex::Complex64;

use crate::methods::patch_refinement::{filter2_same, fspecial_gaussian};

/// SNLC low-pass kernel size for the adaptive smoother: `fspecial('gaussian',
/// 15, sigma)` — a fixed 15×15 window (SNLC `generatekret.m:75`,
/// `L = fspecial('gaussian',15,LP)`).
const SNLC_ADAPTIVE_KERNEL_SIZE: usize = 15;

/// Method choice for pre-combine per-direction smoothing (the
/// `DirectionSmoothing` stage).
///
/// Canonical type: [`openisi_params::config::analysis::DirectionSmoothing`] (the
/// garde-validated, internally-tagged config enum; variants documented there).
/// Compute behavior is attached via [`DirectionSmoothingExt`].
pub use openisi_params::config::analysis::DirectionSmoothing as DirectionSmoothingMethod;

/// Compute behavior for the direction-smoothing stage (extension trait).
pub trait DirectionSmoothingExt {
    /// Smooth one per-direction complex F1 map. `None` returns a clone (the
    /// validated default — no pre-combine smoothing). Called once per direction
    /// (azi fwd/rev, alt fwd/rev) before cycle-combine.
    fn apply(&self, map: &Array2<Complex64>) -> Array2<Complex64>;
}

impl DirectionSmoothingExt for DirectionSmoothingMethod {
    fn apply(&self, map: &Array2<Complex64>) -> Array2<Complex64> {
        match self {
            Self::None => map.clone(),
            Self::SnlcAdaptiveSmoother { sigma_px } => {
                let h = fspecial_gaussian(
                    SNLC_ADAPTIVE_KERNEL_SIZE,
                    SNLC_ADAPTIVE_KERNEL_SIZE,
                    *sigma_px,
                );
                adaptive_smoother(map, &h)
            }
        }
    }
}

/// Verbatim SNLC `adaptiveSmoother.m` — a Wiener-type adaptive filter applied to
/// the real and imaginary parts of a complex map independently, with the
/// estimated noise power being the *mean* local variance:
///
/// ```text
/// g          = real(gcomp)                 % (then imag, separately)
/// localMean  = filter2(h, g)
/// localVar   = filter2(h, g.^2) - localMean.^2
/// noise      = mean2(localVar)
/// f          = (g - localMean) ./ max(localVar, noise) .* max(localVar - noise, 0) + localMean
/// ```
///
/// `filter2` is zero-padded `'same'` correlation; `h` is the low-pass kernel
/// (`fspecial('gaussian', 15, sigma)`). The arithmetic is reproduced in IEEE-754
/// f64 exactly as MATLAB performs it (a degenerate constant input yields the same
/// `0/0 → NaN` MATLAB would).
fn adaptive_smoother(gcomp: &Array2<Complex64>, h: &Array2<f64>) -> Array2<Complex64> {
    let fr = adaptive_smoother_channel(&gcomp.mapv(|z| z.re), h);
    let fi = adaptive_smoother_channel(&gcomp.mapv(|z| z.im), h);
    Array2::from_shape_fn(gcomp.dim(), |(r, c)| Complex64::new(fr[[r, c]], fi[[r, c]]))
}

/// One real channel of [`adaptive_smoother`].
fn adaptive_smoother_channel(g: &Array2<f64>, h: &Array2<f64>) -> Array2<f64> {
    let local_mean = filter2_same(h, g);
    let g_sq = g.mapv(|v| v * v);
    let mean_of_sq = filter2_same(h, &g_sq);
    // localVar = E[g²] − E[g]²
    let local_var =
        Array2::from_shape_fn(g.dim(), |(r, c)| mean_of_sq[[r, c]] - local_mean[[r, c]] * local_mean[[r, c]]);
    // noise = mean2(localVar) — the mean over all pixels.
    let n = local_var.len() as f64;
    let noise = local_var.iter().sum::<f64>() / n;

    Array2::from_shape_fn(g.dim(), |(r, c)| {
        let lv = local_var[[r, c]];
        let lm = local_mean[[r, c]];
        let signal = (lv - noise).max(0.0); // g = max(localVar − noise, 0)
        let denom = lv.max(noise); // localVar = max(localVar, noise)
        (g[[r, c]] - lm) / denom * signal + lm
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::load_f64;
    use agreement::{Eps, Tol};

    /// `None` is a pure pass-through (the bit-identical default).
    #[test]
    fn none_passes_through_unchanged() {
        let m = Array2::from_shape_fn((4, 5), |(r, c)| Complex64::new(r as f64, c as f64));
        let out = DirectionSmoothingMethod::None.apply(&m);
        assert_eq!(out, m);
    }

    /// `SnlcAdaptiveSmoother` matches the verbatim Octave `adaptiveSmoother.m`
    /// (with `fspecial('gaussian', 15, sigma)`), on the complex map's real and
    /// imaginary channels. Fixtures from `gen_adaptsmooth_golden.m` (40×48).
    #[test]
    fn adaptive_smoother_matches_snlc_octave() {
        let meta = load_f64(include_bytes!("../../tests/golden/fixtures/adaptsm_meta.bin"));
        let (h, w, sigma) = (meta[0] as usize, meta[1] as usize, meta[2]);
        let re_in = load_f64(include_bytes!("../../tests/golden/fixtures/adaptsm_re_in.bin"));
        let im_in = load_f64(include_bytes!("../../tests/golden/fixtures/adaptsm_im_in.bin"));
        let re_out = load_f64(include_bytes!("../../tests/golden/fixtures/adaptsm_re_out.bin"));
        let im_out = load_f64(include_bytes!("../../tests/golden/fixtures/adaptsm_im_out.bin"));

        let gcomp =
            Array2::from_shape_fn((h, w), |(r, c)| Complex64::new(re_in[r * w + c], im_in[r * w + c]));
        let out = DirectionSmoothingMethod::SnlcAdaptiveSmoother { sigma_px: sigma }.apply(&gcomp);

        let got_re: Vec<f64> = out.iter().map(|z| z.re).collect();
        let got_im: Vec<f64> = out.iter().map(|z| z.im).collect();
        // f64 throughout; drift is the 225-tap filter2 sum order + the local-
        // variance division across runtimes. Observed max rel ≈ 30·ε_f64 → K=64.
        Tol::rel(64, Eps::F64, 64).assert("adaptiveSmoother real", &got_re, &re_out);
        Tol::rel(64, Eps::F64, 64).assert("adaptiveSmoother imag", &got_im, &im_out);
    }
}
