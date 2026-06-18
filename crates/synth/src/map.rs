//! The analytic ground-truth retinotopy map — the bedrock primitive.
//!
//! Uses the **complex-logarithmic (conformal) model** of striate cortex
//! (Schwartz 1980; the V1–V2–V3 extension is Balasubramanian, Polimeni &
//! Schwartz 2002). We need the *inverse* of the visual→cortex log map — given a
//! cortical pixel, its visual-field position — which is the exponential:
//!
//! ```text
//! w = u + i·v               cortical complex coordinate (u horizontal, v vertical)
//! z = a · (exp(w) − 1)      visual complex coordinate, z = azi + i·alt  (degrees)
//! ```
//!
//! The `−1` (equivalently the `+a` inside the forward `ln(z+a)`) tames the foveal
//! singularity. Because `z(w)` is holomorphic, the map is **conformal**, so its
//! Jacobian is a scaled rotation: `det J > 0` everywhere ⇒ a single, uniform
//! field sign (`+1`), and the areal magnification is `|dz/dw|² = a²·e^{2u}` — the
//! realistic foveal-magnification falloff, in closed form. Sign *reversals* (the
//! V1/V2 mirror borders) come with the multi-area wedge-dipole extension later.
//!
//! These closed forms are what the unit tests below pin, and what the eventual
//! recover-and-compare step checks the pipeline's output against.

use ndarray::Array2;

/// Parameters of the complex-log (monopole) ground-truth map.
#[derive(Clone, Copy, Debug)]
pub struct LogMap {
    /// Foveal scale `a` (degrees-per-unit near the fovea; the `+a` that tames the
    /// log singularity).
    pub a: f64,
    /// Horizontal cortical extent: `u = cx · u_max` for `cx ∈ [0,1]`. Larger →
    /// more eccentricity (peripheral representation).
    pub u_max: f64,
    /// Vertical cortical extent: `v = (cy − ½) · v_ext` for `cy ∈ [0,1]`. Sets the
    /// polar-angle span.
    pub v_ext: f64,
}

impl Default for LogMap {
    fn default() -> Self {
        // A V1-like single area: ~0–60° eccentricity (e^{u_max}−1 ≈ 60) over a
        // half-plane of polar angle (v ∈ [−π/2, π/2]).
        Self {
            a: 1.0,
            u_max: 61.0_f64.ln(),
            v_ext: std::f64::consts::PI,
        }
    }
}

impl LogMap {
    /// Visual-field position `(azi, alt)` in degrees for a normalized cortical
    /// coordinate `(cx, cy) ∈ [0,1]²` (column, row fractions).
    pub fn visual(&self, cx: f64, cy: f64) -> (f64, f64) {
        let u = cx * self.u_max;
        let v = (cy - 0.5) * self.v_ext;
        let e = u.exp();
        (self.a * (e * v.cos() - 1.0), self.a * (e * v.sin()))
    }

    /// Analytic **areal magnification** `|dz/dw|² = a²·e^{2u}` at `(cx, ·)` — the
    /// known `|det J(azi,alt / u,v)|`. The Jacobian w.r.t. the *normalized* grid
    /// `(cx, cy)` is this times the constant `u_max · v_ext` (chain rule).
    pub fn areal_magnification(&self, cx: f64) -> f64 {
        let u = cx * self.u_max;
        self.a * self.a * (2.0 * u).exp()
    }

    /// Generate the ground-truth maps over an `H×W` cortical grid.
    pub fn generate(&self, h: usize, w: usize) -> GroundTruth {
        assert!(h > 1 && w > 1, "grid must be at least 2×2");
        let mut azi = Array2::zeros((h, w));
        let mut alt = Array2::zeros((h, w));
        let mut mag = Array2::zeros((h, w));
        for r in 0..h {
            let cy = r as f64 / (h - 1) as f64;
            for c in 0..w {
                let cx = c as f64 / (w - 1) as f64;
                let (a, l) = self.visual(cx, cy);
                azi[[r, c]] = a;
                alt[[r, c]] = l;
                mag[[r, c]] = self.areal_magnification(cx);
            }
        }
        GroundTruth {
            azi,
            alt,
            sign: 1.0,
            mag,
        }
    }
}

/// The known ground truth a synthetic recording is generated from and validated
/// against: per-pixel visual position, the (uniform) field sign, and the
/// analytic areal magnification.
pub struct GroundTruth {
    /// Azimuth (degrees) per cortical pixel.
    pub azi: Array2<f64>,
    /// Altitude (degrees) per cortical pixel.
    pub alt: Array2<f64>,
    /// Field sign: `+1` for a single conformal area. (Reversals arrive with the
    /// multi-area wedge-dipole extension.)
    pub sign: f64,
    /// Analytic areal magnification `a²·e^{2u}` per pixel.
    pub mag: Array2<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Central-difference Jacobian of `visual` at `(cx, cy)` w.r.t. the
    /// normalized grid, returning `[∂azi/∂cx, ∂azi/∂cy, ∂alt/∂cx, ∂alt/∂cy]`.
    fn jacobian(m: &LogMap, cx: f64, cy: f64, h: f64) -> [f64; 4] {
        let (axp, lxp) = m.visual(cx + h, cy);
        let (axm, lxm) = m.visual(cx - h, cy);
        let (ayp, lyp) = m.visual(cx, cy + h);
        let (aym, lym) = m.visual(cx, cy - h);
        [
            (axp - axm) / (2.0 * h),
            (ayp - aym) / (2.0 * h),
            (lxp - lxm) / (2.0 * h),
            (lyp - lym) / (2.0 * h),
        ]
    }

    fn det(j: [f64; 4]) -> f64 {
        j[0] * j[3] - j[1] * j[2]
    }

    #[test]
    fn eccentricity_increases_toward_the_periphery() {
        let m = LogMap::default();
        // azimuth (≈ eccentricity along the horizontal meridian, v=0) grows with cx.
        let (a_lo, _) = m.visual(0.2, 0.5);
        let (a_hi, _) = m.visual(0.8, 0.5);
        assert!(a_hi > a_lo, "azimuth should increase with cortical cx");
        // and the fovea (cx=0, cy=0.5) maps to the origin.
        let (a0, l0) = m.visual(0.0, 0.5);
        assert!(a0.abs() < 1e-12 && l0.abs() < 1e-12, "fovea → (0,0)");
    }

    #[test]
    fn altitude_is_antisymmetric_about_the_horizontal_meridian() {
        let m = LogMap::default();
        // v → −v flips altitude sign, leaves azimuth unchanged (mirror symmetry).
        let (a_up, l_up) = m.visual(0.5, 0.7);
        let (a_dn, l_dn) = m.visual(0.5, 0.3); // 0.5 − 0.2 vs 0.5 + 0.2
        assert!((a_up - a_dn).abs() < 1e-12, "azimuth symmetric in v");
        assert!((l_up + l_dn).abs() < 1e-12, "altitude antisymmetric in v");
    }

    #[test]
    fn field_sign_is_uniformly_positive_conformal() {
        // det J > 0 everywhere ⇔ a single conformal area, field sign +1.
        let m = LogMap::default();
        for &cx in &[0.05, 0.3, 0.6, 0.95] {
            for &cy in &[0.1, 0.5, 0.9] {
                assert!(
                    det(jacobian(&m, cx, cy, 1e-5)) > 0.0,
                    "det J must be > 0 (field sign +1) at ({cx},{cy})"
                );
            }
        }
    }

    #[test]
    fn areal_magnification_matches_the_closed_form() {
        // The finite-difference |det J(azi,alt / cx,cy)| equals the analytic
        // a²·e^{2u} times the grid chain-rule constant u_max·v_ext. Tolerance is
        // the central-difference truncation O(h²) (a numerical-method bound — NOT
        // an ε-grounded agreement), checked relative since the value spans orders.
        let m = LogMap::default();
        let chain = m.u_max * m.v_ext;
        for &cx in &[0.1, 0.4, 0.7, 0.9] {
            let fd = det(jacobian(&m, cx, 0.5, 1e-4));
            let analytic = m.areal_magnification(cx) * chain;
            let rel = (fd - analytic).abs() / analytic.abs();
            assert!(
                rel < 1e-4,
                "magnification at cx={cx}: finite-diff {fd:.4e} vs analytic {analytic:.4e} (rel {rel:.2e})"
            );
        }
    }

    #[test]
    fn generate_fills_grids_with_finite_values_and_uniform_sign() {
        let gt = LogMap::default().generate(48, 64);
        assert_eq!(gt.azi.dim(), (48, 64));
        assert_eq!(gt.sign, 1.0);
        assert!(gt.azi.iter().all(|v| v.is_finite()));
        assert!(gt.alt.iter().all(|v| v.is_finite()));
        assert!(gt.mag.iter().all(|&v| v > 0.0), "magnification is positive");
        // magnification grows monotonically along a cortical row (with cx).
        let row = gt.mag.row(20);
        assert!(
            row.windows(2).into_iter().all(|w| w[1] >= w[0]),
            "areal magnification increases toward the periphery"
        );
    }
}
