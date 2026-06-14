//! Cycle averaging — combine the K per-cycle complex maps of one direction into
//! a single direction-averaged complex map (within the `Projection` stage).
//!
//! The faithful default is the plain complex average: because the per-cycle DFT
//! uses the same kernel for every cycle, averaging the per-cycle complex maps is
//! algebraically identical to DFT-ing the cycle-averaged *movie* (Allen
//! `corticalmapping` `get_average_movie` → single `generatePhaseMap`; SNLC
//! `Gf1image` accumulates over all frames). Phase-locked averaging — aligning each
//! cycle's global phase to a consensus before averaging — is an **OpenISI
//! deviation no oracle performs**; it is kept as an explicit, selectable option.

use crate::compute::Complex2;

/// Method choice for combining a direction's per-cycle complex maps.
///
/// Canonical type: [`openisi_params::config::analysis::CycleAverage`] (UNIFY);
/// compute behavior is attached via [`CycleAverageExt`].
pub use openisi_params::config::analysis::CycleAverage as CycleAverageMethod;

/// Compute behavior for the cycle-average stage (extension trait).
pub trait CycleAverageExt {
    /// Combine the per-cycle complex maps into the direction-averaged map.
    /// `phases[k]` is cycle `k`'s global phase `arg(Σ_pixels Z_k)` (used only by
    /// the phase-locked variant). Returns `None` for an empty cycle set.
    fn apply(&self, cycles: Vec<Complex2>, phases: &[f64]) -> Option<Complex2>;
}

impl CycleAverageExt for CycleAverageMethod {
    fn apply(&self, cycles: Vec<Complex2>, phases: &[f64]) -> Option<Complex2> {
        let n = cycles.len();
        // Complex addition of two `Complex2` planes; folded over the cycles.
        let add = |a: Complex2, b: Complex2| {
            Complex2::new(a.real() + b.real(), a.imag() + b.imag())
        };
        // `reduce` yields `None` for an empty cycle set — the `?` then returns the
        // method's `None` contract directly, so the non-empty invariant is carried
        // by the type rather than an `expect`. The `1/n` divisor below is reached
        // only on the `Some` path, where `n ≥ 1`.
        let summed = match self {
            Self::SimpleComplexAverage => cycles.into_iter().reduce(add)?,
            Self::PhaseLockedAverage => {
                // Consensus phase φ̄ = arg(Σ_k exp(i·φ_k)).
                let (mut sr, mut si) = (0.0_f64, 0.0_f64);
                for &p in phases {
                    sr += p.cos();
                    si += p.sin();
                }
                let phi_bar = si.atan2(sr);
                cycles
                    .into_iter()
                    .enumerate()
                    .map(|(k, cm_k)| cm_k.phase_shift(-(phases[k] - phi_bar)))
                    .reduce(add)?
            }
        };
        Some(summed.mul_scalar(1.0 / n as f32))
    }
}

#[cfg(test)]
mod tests {
    //! Goldens for the two cycle-averaging methods.
    //!
    //! `SimpleComplexAverage` is the **faithful** default: averaging the per-cycle
    //! complex maps is algebraically identical to DFT-ing the cycle-averaged movie
    //! (Allen `get_average_movie` + single `generatePhaseMap`), because the DFT is
    //! linear and uses the same kernel for every cycle. The first test proves that
    //! equivalence on synthetic frames through the *actual* production DFT.
    //!
    //! `PhaseLockedAverage` is an OpenISI deviation no oracle performs; the second
    //! test pins its defining property (per-cycle global-phase alignment preserves
    //! amplitude where a plain average would attenuate).
    use super::*;
    use crate::compute::{device, dft_projection_at_freq, Backend};
    use burn_tensor::{Tensor, TensorData};

    /// Build a `[N, H, W]` frame stack from a per-frame, per-pixel closure.
    fn frames(n: usize, h: usize, w: usize, f: impl Fn(usize, usize) -> f32) -> Tensor<Backend, 3> {
        let dev = device();
        let mut data: Vec<f32> = Vec::with_capacity(n * h * w);
        for t in 0..n {
            for p in 0..h * w {
                data.push(f(t, p));
            }
        }
        Tensor::<Backend, 3>::from_data(TensorData::new(data, [n, h, w]), &dev)
    }

    /// Per-pixel (re, im) of a `Complex2`, row-major.
    fn parts(z: &Complex2) -> (Vec<f32>, Vec<f32>) {
        let re = z
            .real()
            .into_data()
            .into_vec::<f32>()
            .expect("re f32 vec");
        let im = z
            .imag()
            .into_data()
            .into_vec::<f32>()
            .expect("im f32 vec");
        (re, im)
    }

    /// The faithful default: the simple complex average of the per-cycle DFT maps
    /// equals the DFT of the cycle-averaged movie. This is Allen's
    /// `get_average_movie` → single DFT, reached the other way around — and the two
    /// must agree to f32 precision because the DFT is linear.
    #[test]
    fn simple_complex_average_equals_dft_of_averaged_frames() {
        let (n, h, w) = (16usize, 2usize, 3usize);
        let dt = 0.1_f64;
        let freq = 1.0 / (n as f64 * dt); // one full period over the window (bin 1)

        // Three distinct synthetic cycles: a per-cycle phase offset plus a
        // per-pixel DC term, so the cycles genuinely differ.
        let make_cycle = |c: usize| {
            let phi0 = 0.3 + 0.7 * c as f64;
            frames(n, h, w, move |t, p| {
                let theta = 2.0 * std::f64::consts::PI * freq * (t as f64) * dt + phi0;
                (theta.cos() + 0.05 * p as f64 + 0.2 * c as f64) as f32
            })
        };
        let cyc_frames: Vec<Tensor<Backend, 3>> = (0..3).map(make_cycle).collect();

        // Per-cycle DFT maps → simple complex average.
        let cycles: Vec<Complex2> = cyc_frames
            .iter()
            .map(|f| dft_projection_at_freq(f.clone(), dt, freq))
            .collect();
        let phases: Vec<f64> = cycles
            .iter()
            .map(|z| {
                let (r, i) = z.real_imag_sum();
                i.atan2(r)
            })
            .collect();
        let averaged = CycleAverageMethod::SimpleComplexAverage
            .apply(cycles, &phases)
            .expect("non-empty");

        // Frame-averaged movie → single DFT (Allen path).
        let mut sum = cyc_frames[0].clone();
        for f in &cyc_frames[1..] {
            sum = sum + f.clone();
        }
        let avg_movie = sum.mul_scalar(1.0 / 3.0);
        let expected = dft_projection_at_freq(avg_movie, dt, freq);

        let (ar, ai) = parts(&averaged);
        let (er, ei) = parts(&expected);
        for k in 0..ar.len() {
            assert!(
                (ar[k] - er[k]).abs() < 1e-4 && (ai[k] - ei[k]).abs() < 1e-4,
                "pixel {k}: avg-of-DFTs ({}, {}) != DFT-of-avg ({}, {})",
                ar[k],
                ai[k],
                er[k],
                ei[k],
            );
        }
    }

    /// The OpenISI deviation: phase-locking aligns each cycle's global phase to the
    /// consensus before averaging, so cycles that differ only by a global phase
    /// rotation combine without amplitude loss. A plain average of the same cycles
    /// attenuates. This pins the defining behavior (and that it is *not* a no-op).
    #[test]
    fn phase_locked_average_preserves_amplitude_under_global_phase_drift() {
        let dev = device();
        let (h, w) = (2usize, 2usize);
        // A fixed base complex map.
        let base = Complex2::new(
            Tensor::from_data(TensorData::new(vec![1.0f32, 2.0, -0.5, 0.3], [h, w]), &dev),
            Tensor::from_data(TensorData::new(vec![0.4f32, -1.0, 0.7, 1.2], [h, w]), &dev),
        );
        // Three cycles = base rotated by distinct global phases.
        let drifts = [0.0_f64, 1.1, -2.3];
        let cycles: Vec<Complex2> = drifts.iter().map(|&d| base.clone().phase_shift(d)).collect();
        let phases: Vec<f64> = cycles
            .iter()
            .map(|z| {
                let (r, i) = z.real_imag_sum();
                i.atan2(r)
            })
            .collect();

        let locked = CycleAverageMethod::PhaseLockedAverage
            .apply(cycles.clone(), &phases)
            .expect("non-empty");
        let simple = CycleAverageMethod::SimpleComplexAverage
            .apply(cycles, &phases)
            .expect("non-empty");

        let base_abs = base.abs().into_data().into_vec::<f32>().unwrap();
        let locked_abs = locked.abs().into_data().into_vec::<f32>().unwrap();
        let simple_abs = simple.abs().into_data().into_vec::<f32>().unwrap();

        // Phase-locking recovers the base amplitude exactly (alignment makes all
        // cycles identical up to a single consensus rotation).
        for k in 0..base_abs.len() {
            assert!(
                (locked_abs[k] - base_abs[k]).abs() < 1e-4,
                "pixel {k}: phase-locked |z| {} != base |z| {}",
                locked_abs[k],
                base_abs[k],
            );
        }
        // The plain average attenuates (proving the two methods genuinely differ).
        let simple_attenuates = (0..base_abs.len()).any(|k| simple_abs[k] < base_abs[k] - 1e-3);
        assert!(
            simple_attenuates,
            "simple average should attenuate under global-phase drift: base={base_abs:?} simple={simple_abs:?}",
        );
    }
}
