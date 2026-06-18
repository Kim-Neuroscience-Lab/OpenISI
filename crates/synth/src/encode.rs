//! The Kalatsky–Stryker forward encoder (layer 2) — ground truth → periodic
//! movie.
//!
//! Given the layer-1 [`GroundTruth`](crate::map::GroundTruth), synthesize each
//! pixel's time series under periodic drifting-bar stimulation: a pixel
//! oscillates at the stimulus frequency with **phase encoding its visual
//! position** (Kalatsky & Stryker 2003). Forward and reverse sweeps encode
//! `+position` and `−position`, so the pipeline's delay-subtraction `(fwd−rev)/2`
//! recovers the position and `(fwd+rev)/2` recovers the (here zero) delay.
//!
//! Scope: this is the **practical / smoke-level** encoder — a clean single-
//! frequency sinusoid with no realism layer, using the pipeline's own
//! phase↔degrees convention. The realism layer (delay, hemodynamic PSF,
//! harmonics, noise) and the rigorous, geometry-mediated (provably non-circular)
//! coordinate mapping are the deferred benchmark work; see
//! [`docs/SYNTHETIC_VALIDATION.md`](../../../docs/SYNTHETIC_VALIDATION.md).

use ndarray::Array3;

use crate::map::GroundTruth;

/// Which visual axis a sweep encodes.
#[derive(Clone, Copy, Debug)]
pub enum Axis {
    Azimuth,
    Altitude,
}

/// Synthetic stimulus parameters for one periodic recording.
#[derive(Clone, Copy, Debug)]
pub struct Stim {
    /// Visual-angle range the bar sweeps (degrees) — the pipeline's
    /// `angular_range`; visual position maps to phase over this range.
    pub angular_range_deg: f64,
    /// Visual-angle offset (degrees) — the pipeline's `offset`.
    pub offset_deg: f64,
    /// Stimulus cycles (sweeps) in this direction's epoch.
    pub cycles: usize,
    /// Camera frames per stimulus cycle.
    pub frames_per_cycle: usize,
    /// Resting fluorescence/reflectance `F0` (arbitrary units).
    pub baseline: f64,
    /// Modulation depth `ΔF/F` amplitude.
    pub amplitude: f64,
}

impl Default for Stim {
    fn default() -> Self {
        Self {
            angular_range_deg: 140.0,
            offset_deg: 0.0,
            cycles: 10,
            frames_per_cycle: 24,
            baseline: 1000.0,
            amplitude: 0.02,
        }
    }
}

/// Map a visual position (degrees) to stimulus phase (radians) — the inverse of
/// the pipeline's `phase → degrees`. Position spanning `offset ± range/2` maps to
/// `(−π, π]`.
pub fn position_to_phase(pos_deg: f64, stim: &Stim) -> f64 {
    (pos_deg - stim.offset_deg) * std::f64::consts::TAU / stim.angular_range_deg
}

/// Encode one sweep direction into a synthetic frame stack `[T, H, W]` (f64
/// intensities). `reverse` negates the position phase (the reverse sweep), so a
/// zero-delay recording satisfies `(fwd − rev)/2 = position`.
pub fn encode_direction(gt: &GroundTruth, axis: Axis, reverse: bool, stim: &Stim) -> Array3<f64> {
    let (h, w) = gt.azi.dim();
    let pos = match axis {
        Axis::Azimuth => &gt.azi,
        Axis::Altitude => &gt.alt,
    };
    let sign = if reverse { -1.0 } else { 1.0 };
    let t_total = stim.cycles * stim.frames_per_cycle;
    let mut frames = Array3::zeros((t_total, h, w));
    for t in 0..t_total {
        let omega_t = std::f64::consts::TAU * (t as f64) / (stim.frames_per_cycle as f64);
        for r in 0..h {
            for c in 0..w {
                let phase = sign * position_to_phase(pos[[r, c]], stim);
                frames[[t, r, c]] = stim.baseline * (1.0 + stim.amplitude * (omega_t + phase).cos());
            }
        }
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::LogMap;
    use agreement::{Eps, Tol};

    /// Single-bin (stimulus-frequency) DFT phase of pixel `(r,c)`, the standard
    /// forward transform `Σ v·e^{−iωt}` → `+φ` for `v = B(1 + A·cos(ωt + φ))`.
    fn bin1_phase(frames: &Array3<f64>, r: usize, c: usize, fpc: usize) -> f64 {
        let t_total = frames.dim().0;
        let (mut sr, mut si) = (0.0_f64, 0.0_f64);
        for t in 0..t_total {
            let w = std::f64::consts::TAU * (t as f64) / (fpc as f64);
            let v = frames[[t, r, c]];
            sr += v * w.cos();
            si += v * w.sin();
        }
        (-si).atan2(sr)
    }

    #[test]
    fn forward_dft_recovers_the_encoded_position_phase() {
        let gt = LogMap::default().generate(24, 32);
        let stim = Stim::default();
        let f = encode_direction(&gt, Axis::Azimuth, false, &stim);
        // The bin-1 phase at every pixel must equal the encoded position phase.
        // f64 encode → exact-frequency DFT over whole cycles → near machine
        // precision; angular phase (period 2π) → wrap-aware, K=64·ε_f64.
        let mut recovered = Vec::with_capacity(24 * 32);
        let mut expected = Vec::with_capacity(24 * 32);
        for r in 0..24 {
            for c in 0..32 {
                recovered.push(bin1_phase(&f, r, c, stim.frames_per_cycle));
                expected.push(position_to_phase(gt.azi[[r, c]], &stim));
            }
        }
        Tol::wrap(std::f64::consts::TAU, 64, Eps::F64, 1.0).assert(
            "encoded azimuth phase vs DFT recovery",
            &recovered,
            &expected,
        );
    }

    #[test]
    fn delay_subtraction_of_fwd_rev_recovers_position_zero_delay() {
        // With no injected delay, (fwd − rev)/2 = position phase and
        // (fwd + rev)/2 = 0, the Kalatsky–Stryker separation.
        let gt = LogMap::default().generate(16, 16);
        let stim = Stim::default();
        let fwd = encode_direction(&gt, Axis::Altitude, false, &stim);
        let rev = encode_direction(&gt, Axis::Altitude, true, &stim);
        for r in 0..16 {
            for c in 0..16 {
                let pf = bin1_phase(&fwd, r, c, stim.frames_per_cycle);
                let pr = bin1_phase(&rev, r, c, stim.frames_per_cycle);
                let pos = position_to_phase(gt.alt[[r, c]], &stim);
                assert!((0.5 * (pf - pr) - pos).abs() < 1e-9, "position at {r},{c}");
                assert!((0.5 * (pf + pr)).abs() < 1e-9, "delay should be ~0 at {r},{c}");
            }
        }
    }

    #[test]
    fn frames_are_positive_and_periodic() {
        let gt = LogMap::default().generate(8, 8);
        let stim = Stim::default();
        let f = encode_direction(&gt, Axis::Azimuth, false, &stim);
        assert_eq!(f.dim(), (stim.cycles * stim.frames_per_cycle, 8, 8));
        assert!(f.iter().all(|&v| v > 0.0), "intensities stay positive");
        // Same phase one cycle later (periodicity).
        let fpc = stim.frames_per_cycle;
        for r in 0..8 {
            for c in 0..8 {
                assert!((f[[0, r, c]] - f[[fpc, r, c]]).abs() < 1e-9, "periodic at {r},{c}");
            }
        }
    }
}
