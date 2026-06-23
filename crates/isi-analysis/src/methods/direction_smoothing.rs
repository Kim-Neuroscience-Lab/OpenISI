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
    #[cfg(feature = "oracle_live")]
    use agreement::{Eps, Tol};

    /// `None` is a pure pass-through (the bit-identical default).
    #[test]
    fn none_passes_through_unchanged() {
        let m = Array2::from_shape_fn((4, 5), |(r, c)| Complex64::new(r as f64, c as f64));
        let out = DirectionSmoothingMethod::None.apply(&m);
        assert_eq!(out, m);
    }

    /// **Live genuine-oracle, SNLC**: our `SnlcAdaptiveSmoother` vs the GENUINE
    /// `adaptiveSmoother.m` (`h = fspecial('gaussian', 15, sigma)`), executed live
    /// under MATLAB. The genuine `.m` is the oracle; the bridge only calls it. A
    /// structured complex map with high-frequency texture (the retired generator's
    /// exact scene) makes the adaptive, local-variance-aware filter do visibly
    /// non-uniform work. f64 throughout; drift is the 225-tap filter2 sum order +
    /// the variance division across runtimes (≈30·ε_f64 → K=64). Gated behind
    /// `oracle_live`.
    #[cfg(feature = "oracle_live")]
    #[test]
    fn adaptive_smoother_matches_genuine_snlc_live() {
        use crate::test_support::oracle;
        if oracle::snlc_skip("adaptive_smoother_matches_genuine_snlc_live") {
            return;
        }
        const H: usize = 40;
        const W: usize = 48;
        let sigma = 2.0;
        // meshgrid(1:W, 1:H): xx = col+1, yy = row+1 (the generator's scene).
        let re = Array2::from_shape_fn((H, W), |(r, c)| {
            let (x, y) = ((c + 1) as f64, (r + 1) as f64);
            10.0 * (x / 6.0).sin() + 6.0 * (y / 5.0).cos() + 2.0 * (x / 2.0).sin() * (y / 2.0).cos()
        });
        let im = Array2::from_shape_fn((H, W), |(r, c)| {
            let (x, y) = ((c + 1) as f64, (r + 1) as f64);
            8.0 * (x / 7.0).cos() - 5.0 * (y / 4.0).sin() + 1.5 * ((x + y) / 2.0).cos()
        });
        let gcomp = Array2::from_shape_fn((H, W), |(r, c)| Complex64::new(re[[r, c]], im[[r, c]]));

        // Genuine adaptiveSmoother.m returns re, im (in that order).
        let mut genuine = oracle::snlc("adaptive_smoother", &[re, im], &[("sigma", sigma)]);
        let g_im = genuine.remove(1);
        let g_re = genuine.remove(0);

        let out = DirectionSmoothingMethod::SnlcAdaptiveSmoother { sigma_px: sigma }.apply(&gcomp);
        let got_re: Vec<f64> = out.iter().map(|z| z.re).collect();
        let got_im: Vec<f64> = out.iter().map(|z| z.im).collect();
        Tol::rel(64, Eps::F64, 64).assert(
            "adaptiveSmoother real vs GENUINE reference (live)",
            &got_re,
            g_re.as_slice().expect("contiguous"),
        );
        Tol::rel(64, Eps::F64, 64).assert(
            "adaptiveSmoother imag vs GENUINE reference (live)",
            &got_im,
            g_im.as_slice().expect("contiguous"),
        );
        eprintln!("adaptiveSmoother vs GENUINE reference (live): matched re+im");
    }
}
