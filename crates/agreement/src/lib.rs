//! Numerical-agreement test utility — **one** comparator + **one** grounded
//! tolerance vocabulary, shared by every golden and the equivalence harness.
//!
//! ## Why this crate exists
//!
//! Validating "does our float output agree with the oracle/reference" has two
//! parts that were previously hand-rolled and scattered across ~12 golden tests
//! (each re-deriving a `max((a−b).abs())` loop and picking its own magic-number
//! threshold), while the equivalence harness had a separate, better
//! implementation. This crate is the single source of truth for both:
//!
//! 1. **The comparator** — [`Tol::check`]: a NaN/Inf-position-aware drift
//!    accumulator over `approx` (the project's standard float comparator). The
//!    only float-agreement loop in the codebase lives here.
//! 2. **The grounding** — [`Tol`]: a tolerance is a *value of a type that
//!    encodes its IEEE-754 grounding*. A bare `1e-3` literal is **not
//!    expressible** as a `Tol`; every bound is `K·ε`, `κ·K·ε`, or a wrap-aware
//!    variant of those. Grounding stops being a convention and becomes a
//!    property of the type system.
//!
//! It lives in its own crate because the goldens are unit tests (in
//! `isi-analysis/src`) and the equivalence harness is an integration test (in
//! `isi-analysis/tests`); Rust can't share a `#[cfg(test)]` module across that
//! boundary, and a shared *dev-dependency* crate keeps the utility out of the
//! shipping library.
//!
//! ## What this is NOT for
//!
//! *Domain* assertions — "VFS recovers to <5% and clearly diverges by >0.5",
//! "threshold ∈ [0.1, 1.0]" — are claims about the science, not agreement
//! tolerances. Keep those as plain asserts; forcing them through this crate
//! would be false precision.

use approx::{abs_diff_eq, relative_eq};

/// IEEE-754 machine epsilon for the precision the agreement is grounded in.
/// `ε_f32 = 2⁻²³ ≈ 1.19e-7`, `ε_f64 = 2⁻⁵² ≈ 2.22e-16`. Use [`Eps::F32`] when a
/// stage runs on the f32 compute backend (cross-implementation / cross-backend
/// drift), [`Eps::F64`] for a pure-f64 reference comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eps {
    F32,
    F64,
}

impl Eps {
    /// The machine-epsilon value as `f64`.
    pub fn value(self) -> f64 {
        match self {
            Eps::F32 => f64::from(f32::EPSILON),
            Eps::F64 => f64::EPSILON,
        }
    }
}

/// How two finite values are compared element-wise.
#[derive(Clone, Copy, Debug)]
enum Metric {
    /// Linear distance `|c − b|` with `approx`'s `relative_eq!` (rtol·max + atol).
    Linear,
    /// Circular distance with the given `period` (e.g. `2π` radians, `180°` for
    /// an axis), checked absolutely against `atol`.
    Wrap(f64),
    /// Exact equality — discrete masks/labels that must not drift at all.
    Exact,
}

/// A numerical-agreement tolerance, **grounded in IEEE-754 ε** — there is no
/// constructor that takes a raw bound, so a magic-number threshold cannot be
/// expressed. Build one with the named constructors; the conditioning rationale
/// belongs in a doc comment at the call site (or in `tolerances.toml`).
#[derive(Clone, Copy, Debug)]
pub struct Tol {
    /// Relative bound `rtol·max(|c|,|b|)`; `0` for purely-absolute tolerances.
    rtol: f64,
    /// Absolute bound (the floor, or the whole bound for zero-crossing /
    /// angular / κ-scaled quantities).
    atol: f64,
    metric: Metric,
}

impl Tol {
    /// Absolute `|c − b| ≤ K·ε`. For quantities that cross zero or are bounded
    /// (phase in radians, VFS, distortion). `k` is the integer error-propagation
    /// factor (op count / smallest power-of-two bounding the observed drift).
    pub fn abs(k: u32, base: Eps) -> Self {
        Self {
            rtol: 0.0,
            atol: f64::from(k) * base.value(),
            metric: Metric::Linear,
        }
    }

    /// Relative `|c − b| ≤ K·ε·max(|c|,|b|) + Kfloor·ε`. For positive-magnitude
    /// quantities (amplitudes, eccentricity). `k_floor` floors it where the
    /// value passes through zero.
    pub fn rel(k: u32, base: Eps, k_floor: u32) -> Self {
        let e = base.value();
        Self {
            rtol: f64::from(k) * e,
            atol: f64::from(k_floor) * e,
            metric: Metric::Linear,
        }
    }

    /// Scale an existing tolerance by a **measured** condition number κ — the
    /// principled alternative to a loose magic number at an ill-conditioned
    /// pixel. Composes with any base (`abs`/`rel`/`wrap`), so e.g. the
    /// magnification map (`1/det`, relative) and the anisotropy axis (`∠z`,
    /// κ = 1/|z|, wrap-180) both express their conditioning the same way. `kappa`
    /// must be measured from the data (e.g. `1/min(det)`, `1/min(distortion)`),
    /// never hand-set.
    #[must_use]
    pub fn with_kappa(mut self, kappa: f64) -> Self {
        self.rtol *= kappa;
        self.atol *= kappa;
        self
    }

    /// Wrap-aware absolute: the circular distance (given `period`) `≤ K·ε·scale`.
    /// For angular quantities — phase (`period = 2π`, radians) or an axis
    /// (`period = 180`, degrees, `scale` = deg conversion). A near-0/near-period
    /// pixel can't create a false diff.
    pub fn wrap(period: f64, k: u32, base: Eps, scale: f64) -> Self {
        Self {
            rtol: 0.0,
            atol: f64::from(k) * base.value() * scale,
            metric: Metric::Wrap(period),
        }
    }

    /// Exact equality — discrete masks / labels / contours that must not drift.
    pub fn exact() -> Self {
        Self {
            rtol: 0.0,
            atol: 0.0,
            metric: Metric::Exact,
        }
    }

    /// The effective absolute bound (for diagnostics / messages).
    pub fn atol(&self) -> f64 {
        self.atol
    }

    /// Compare two flat row-major slices, returning [`Drift`] diagnostics.
    /// NaN/Inf positions must MATCH (both non-finite, or both finite); a
    /// position where they differ is a structural failure regardless of bound.
    pub fn check(&self, computed: &[f64], reference: &[f64]) -> Drift {
        assert_eq!(
            computed.len(),
            reference.len(),
            "agreement: element-count mismatch ({} vs {})",
            computed.len(),
            reference.len()
        );
        let (rtol, atol) = (self.rtol, self.atol);
        // Each arm calls `drift_with` with closures (which can capture `period`
        // / `atol`); no unified `dist`/`pass` type, so no `fn`-can't-capture hack.
        match self.metric {
            Metric::Linear => drift_with(
                computed,
                reference,
                |c, b| (c - b).abs(),
                |c, b| relative_eq!(c, b, max_relative = rtol, epsilon = atol),
            ),
            Metric::Wrap(period) => drift_with(
                computed,
                reference,
                move |c, b| wrap_distance(c, b, period),
                move |c, b| abs_diff_eq!(wrap_distance(c, b, period), 0.0, epsilon = atol),
            ),
            // Discrete data must not drift at all (treats −0.0 == 0.0; NaN never
            // reaches here — the NaN-position gate handles non-finite).
            Metric::Exact => drift_with(computed, reference, |c, b| (c - b).abs(), |c, b| c == b),
        }
    }

    /// Compare and panic with a descriptive message unless every element agrees
    /// (no tolerance failures, no NaN-position mismatches). The single assertion
    /// entry point for goldens — replaces hand-rolled `assert!(max < magic)`.
    pub fn assert(&self, label: &str, computed: &[f64], reference: &[f64]) {
        let d = self.check(computed, reference);
        assert!(
            d.is_agreement(),
            "{label}: {} px exceed tolerance + {} NaN-position mismatches \
             (max_abs={:.3e}, max_rel={:.3e}, atol={:.3e}, over {} finite px)",
            d.n_fail,
            d.n_nan_mismatch,
            d.max_abs,
            d.max_rel,
            self.atol,
            d.n_finite,
        );
    }
}

/// Per-comparison diagnostics. `is_agreement()` is the pass condition; the rest
/// are for reporting.
#[derive(Debug, Default, Clone, Copy)]
pub struct Drift {
    /// Worst absolute (or wrap) distance over finite pairs.
    pub max_abs: f64,
    /// Worst relative `|c−b|/max(|c|,|b|)` over finite pairs.
    pub max_rel: f64,
    /// Finite pairs that breached the tolerance. `0` ⇔ pass.
    pub n_fail: usize,
    /// Finite pairs compared.
    pub n_finite: usize,
    /// Positions where exactly one of (computed, reference) is non-finite — a
    /// structural disagreement on *where* data exists; always a failure.
    pub n_nan_mismatch: usize,
}

impl Drift {
    /// True iff every finite pair is within tolerance and the NaN/Inf masks
    /// match.
    pub fn is_agreement(&self) -> bool {
        self.n_fail == 0 && self.n_nan_mismatch == 0
    }
}

/// Wrapped circular distance between two values with the given `period`, in
/// `[0, period/2]`. `2π` for radian phase, `180` for a degree axis.
pub fn wrap_distance(c: f64, b: f64, period: f64) -> f64 {
    let mut d = (c - b).rem_euclid(period);
    if d > period / 2.0 {
        d = period - d;
    }
    d
}

/// Shared NaN/Inf-aware accumulator. `dist` reports per-element drift for
/// diagnostics; `pass` is the `approx` tolerance check. The loop owns only the
/// domain discipline — NaN-position matching and aggregation — not the bound.
fn drift_with(
    computed: &[f64],
    reference: &[f64],
    dist: impl Fn(f64, f64) -> f64,
    pass: impl Fn(f64, f64) -> bool,
) -> Drift {
    let mut d = Drift::default();
    for (&c, &b) in computed.iter().zip(reference.iter()) {
        match (c.is_finite(), b.is_finite()) {
            (true, true) => {
                let dd = dist(c, b);
                d.max_abs = d.max_abs.max(dd);
                let scale = c.abs().max(b.abs());
                if scale > 0.0 {
                    d.max_rel = d.max_rel.max(dd / scale);
                }
                if !pass(c, b) {
                    d.n_fail += 1;
                }
                d.n_finite += 1;
            }
            (false, false) => {}
            _ => d.n_nan_mismatch += 1,
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eps_values_are_ieee_754() {
        assert_eq!(Eps::F32.value(), f64::from(f32::EPSILON));
        assert_eq!(Eps::F64.value(), f64::EPSILON);
        assert!((Eps::F32.value() - 1.1920929e-7).abs() < 1e-12);
    }

    #[test]
    fn abs_keps_passes_within_and_fails_beyond() {
        let tol = Tol::abs(256, Eps::F32); // ≈ 3.05e-5
        let r = [1.0, 2.0, 3.0];
        // Within: a drift of 1e-6 < 3.05e-5.
        assert!(tol.check(&[1.0 + 1e-6, 2.0, 3.0], &r).is_agreement());
        // Beyond: 1e-3 > 3.05e-5 → one failure.
        let d = tol.check(&[1.0 + 1e-3, 2.0, 3.0], &r);
        assert_eq!(d.n_fail, 1);
        assert!(!d.is_agreement());
    }

    #[test]
    fn nan_positions_must_match() {
        let tol = Tol::abs(256, Eps::F32);
        // Both NaN at the same spot → fine.
        assert!(tol
            .check(&[f64::NAN, 1.0], &[f64::NAN, 1.0])
            .is_agreement());
        // One NaN, one finite → structural mismatch, always a failure.
        let d = tol.check(&[f64::NAN, 1.0], &[0.0, 1.0]);
        assert_eq!(d.n_nan_mismatch, 1);
        assert!(!d.is_agreement());
    }

    #[test]
    fn wrap_handles_period_boundary() {
        // Axis period 180°: 0.01 and 179.99 are 0.02 apart, not 179.98.
        assert!((wrap_distance(0.01, 179.99, 180.0) - 0.02).abs() < 1e-9);
        let tol = Tol::wrap(180.0, 16, Eps::F32, 1.0); // atol ≈ 1.9e-6 deg
        // Straddling the wrap by a hair → agreement (circular distance tiny).
        assert!(tol.check(&[0.0000001], &[179.9999999]).is_agreement());
        // A real 1° gap → failure.
        assert_eq!(tol.check(&[10.0], &[11.0]).n_fail, 1);
    }

    #[test]
    fn with_kappa_scales_the_bound_by_the_condition_number() {
        // κ=12 lifts the bound exactly 12× (the magnification/axis ill-
        // conditioning). Bit-exact: with_kappa multiplies atol by κ.
        let base = Tol::abs(2, Eps::F32).atol();
        let scaled = Tol::abs(2, Eps::F32).with_kappa(12.0).atol();
        assert_eq!(scaled, base * 12.0);
    }

    #[test]
    fn exact_requires_bit_equality() {
        let tol = Tol::exact();
        assert!(tol.check(&[1.0, -0.0], &[1.0, -0.0]).is_agreement());
        // Even a 1-ULP difference fails.
        let one_ulp = f64::from_bits(1.0_f64.to_bits() + 1);
        assert_eq!(tol.check(&[one_ulp], &[1.0]).n_fail, 1);
    }
}
