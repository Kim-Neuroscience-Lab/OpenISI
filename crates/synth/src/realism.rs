//! The realism layer (layer 3) — corruptions that make the synthetic movie
//! *richer than the pipeline's internal assumptions*, so recovery tests measure
//! robustness to model mismatch rather than assumptions against themselves
//! (`docs/SYNTHETIC_VALIDATION.md` §3).
//!
//! **Phase A** implements the two knobs that make the full-pass golden
//! non-circular: a **hemodynamic response** (HRF — a known, recoverable delay) and
//! **sensor noise** (shot + read — drives the noise floor *above* the tiny ΔR/R
//! signal). [`Corruptions::default`] is the identity transform, so default-off
//! reproduces the clean layer-2 movie. The remaining knobs (spatial PSF,
//! physiological lines, drift, vascular F0, saturation) are Phase B.
//!
//! Citations: difference-of-gamma / triphasic IOS HRF — Sirotin & Das 2007;
//! ΔR/R ~1e-4–1e-3 and the noise-exceeds-signal regime — the IOS literature
//! reviewed in `SYNTHETIC_VALIDATION.md`.

use ndarray::Array3;

use crate::encode::{position_to_phase, Axis, Stim};
use crate::map::GroundTruth;
use crate::rng::SynthRng;

/// The set of layer-3 corruptions applied to a clean encoded movie. `Default` is
/// the **identity** (every knob off), so a default `Corruptions` reproduces the
/// clean layer-2 encoder bit-for-bit (pinned by a test).
#[derive(Clone, Copy, Debug, Default)]
pub struct Corruptions {
    /// (a) Hemodynamic response — a known, recoverable delay (+ low-pass gain).
    pub hemodynamic: Option<Hrf>,
    /// (d) Sensor noise — shot (Poisson, Gaussian-approx) + read (Gaussian).
    pub sensor: Option<SensorNoise>,
    // Phase B: psf, physio, drift, vascular, saturation.
}

impl Corruptions {
    /// The literature-grounded "benchmark" recording: a biphasic HRF + realistic
    /// sensor noise. The default magnitudes put the noise floor near/above the
    /// ΔR/R signal — the regime that actually tests the frequency-selective DFT.
    pub fn benchmark() -> Self {
        Self {
            hemodynamic: Some(Hrf::default()),
            sensor: Some(SensorNoise::default()),
        }
    }
}

/// Difference-of-gamma hemodynamic response function (knob a). The measured
/// intrinsic signal is the neural drive convolved with a biphasic kernel
/// `h(τ) = g(τ; peak1,width1) − undershoot·g(τ; peak2,width2)`, area-normalized to
/// unit DC gain. On a pure-cosine drive (the layer-2 encoding) this is LTI, so at
/// the stimulus fundamental it contributes a single, **direction-independent**
/// phase `∠H(ω)` — the hemodynamic delay the pipeline's delay-subtraction must
/// remove: `(fwd−rev)/2 = position`, `(fwd+rev)/2 = ∠H(ω)`.
#[derive(Clone, Copy, Debug)]
pub struct Hrf {
    /// First (positive) lobe peak time, seconds.
    pub peak1_s: f64,
    /// First lobe width (≈ the gamma's spread at the peak), seconds.
    pub width1_s: f64,
    /// Undershoot lobe peak time, seconds.
    pub peak2_s: f64,
    /// Undershoot lobe width, seconds.
    pub width2_s: f64,
    /// Undershoot relative weight `c` (0 ⇒ single-gamma).
    pub undershoot: f64,
}

impl Default for Hrf {
    fn default() -> Self {
        // Biphasic IOS HRF: ~1.5 s peak, small slow undershoot, ~10 s support
        // (Sirotin & Das 2007).
        Self {
            peak1_s: 1.5,
            width1_s: 0.9,
            peak2_s: 5.0,
            width2_s: 2.5,
            undershoot: 0.2,
        }
    }
}

impl Hrf {
    /// Total temporal support (seconds) past which the kernel is ~0.
    fn support_s(&self) -> f64 {
        self.peak2_s.max(self.peak1_s) + 5.0 * self.width2_s.max(self.width1_s)
    }

    /// The area-normalized kernel sampled at frame spacing `dt_sec` over `len`
    /// frames (zero past its support). Unit area ⇒ DC gain 1.
    pub fn kernel(&self, dt_sec: f64, len: usize) -> Vec<f64> {
        let support = (self.support_s() / dt_sec).ceil() as usize;
        let mut k = vec![0.0; len];
        for (j, kj) in k.iter_mut().enumerate() {
            if j > support {
                break;
            }
            let tau = j as f64 * dt_sec;
            *kj = gamma_lobe(tau, self.peak1_s, self.width1_s)
                - self.undershoot * gamma_lobe(tau, self.peak2_s, self.width2_s);
        }
        let sum: f64 = k.iter().sum();
        if sum.abs() > 1e-12 {
            for kj in &mut k {
                *kj /= sum;
            }
        }
        k
    }
}

/// A gamma-shaped lobe peaking at `peak_s` with spread `width_s`:
/// `g(τ) = τ^{α−1} e^{−τ/β}`, with `α,β` chosen so the mode `(α−1)β = peak_s`.
fn gamma_lobe(tau_s: f64, peak_s: f64, width_s: f64) -> f64 {
    if tau_s <= 0.0 {
        return 0.0;
    }
    let alpha = (peak_s / width_s).powi(2) + 1.0;
    let beta = width_s * width_s / peak_s;
    tau_s.powf(alpha - 1.0) * (-tau_s / beta).exp()
}

/// Circular convolution of `signal` with `kernel` (both length `n`) — the
/// steady-state response of an LTI system to a periodic input (the recording is
/// whole stimulus cycles, so the wrap is exact, no transient).
fn circular_convolve(signal: &[f64], kernel: &[f64]) -> Vec<f64> {
    let n = signal.len();
    (0..n)
        .map(|t| {
            let mut s = 0.0;
            for (j, &kj) in kernel.iter().enumerate() {
                if kj != 0.0 {
                    s += kj * signal[(t + n - (j % n)) % n];
                }
            }
            s
        })
        .collect()
}

/// Camera sensor noise (knob d): photon shot noise + read noise. Shot noise uses
/// the Gaussian approximation `N(s, √s)` (valid for the large ISI counts; true
/// Poisson is a Phase-B refinement for small counts). This is the knob that pushes
/// the noise floor above the ΔR/R signal — the regime that tests the DFT's
/// frequency selectivity.
#[derive(Clone, Copy, Debug)]
pub struct SensorNoise {
    /// Add photon shot noise `N(0, √signal)`.
    pub shot: bool,
    /// Read-noise standard deviation (counts), `N(0, read_sigma)`.
    pub read_sigma: f64,
}

impl Default for SensorNoise {
    fn default() -> Self {
        Self {
            shot: true,
            read_sigma: 3.0,
        }
    }
}

/// Encode one sweep direction into an `[T, H, W]` f64 movie **with** the layer-3
/// corruptions applied. With [`Corruptions::default`] this equals
/// [`crate::encode::encode_direction`] exactly. `dt_sec` is the camera frame
/// period (needed to sample the HRF in physical time); `rng` + `dir_label` seed
/// the per-direction noise substream.
// The 8 inputs are distinct, mostly type-disjoint (no swap hazard) and each is a
// genuine, independent forward-model parameter — grouping them into a struct would
// only add ceremony to a dev-only generator entry point.
#[allow(clippy::too_many_arguments)]
pub fn encode_direction_realistic(
    gt: &GroundTruth,
    axis: Axis,
    reverse: bool,
    stim: &Stim,
    dt_sec: f64,
    corr: &Corruptions,
    rng: &SynthRng,
    dir_label: &str,
) -> Array3<f64> {
    let (h, w) = gt.azi.dim();
    let pos = match axis {
        Axis::Azimuth => &gt.azi,
        Axis::Altitude => &gt.alt,
    };
    let sign = if reverse { -1.0 } else { 1.0 };
    let t_total = stim.cycles * stim.frames_per_cycle;
    let kernel = corr.hemodynamic.map(|hrf| hrf.kernel(dt_sec, t_total));

    let mut frames = Array3::zeros((t_total, h, w));
    let mut modulation = vec![0.0_f64; t_total];
    for r in 0..h {
        for c in 0..w {
            let phase = sign * position_to_phase(pos[[r, c]], stim);
            for (t, m) in modulation.iter_mut().enumerate() {
                let omega_t = std::f64::consts::TAU * (t as f64) / (stim.frames_per_cycle as f64);
                *m = stim.amplitude * (omega_t + phase).cos();
            }
            let response = match &kernel {
                Some(k) => circular_convolve(&modulation, k),
                None => modulation.clone(),
            };
            for t in 0..t_total {
                frames[[t, r, c]] = stim.baseline * (1.0 + response[t]);
            }
        }
    }

    // Sensor noise: drawn in fixed (t, r, c) order from the per-direction substream
    // so the realization is reproducible and independent of other knobs.
    if let Some(noise) = corr.sensor {
        let mut s = rng.substream(&format!("sensor:{dir_label}"));
        for t in 0..t_total {
            for r in 0..h {
                for c in 0..w {
                    let mut v = frames[[t, r, c]];
                    if noise.shot {
                        v += s.normal() * v.max(0.0).sqrt();
                    }
                    if noise.read_sigma > 0.0 {
                        v += s.normal() * noise.read_sigma;
                    }
                    frames[[t, r, c]] = v;
                }
            }
        }
    }
    frames
}

/// Quantize an f64 intensity movie to raw `u16` camera counts (clip to
/// `[0, 65535]`, round). The boundary between the analog forward model and the
/// `RawAcquisition.frames` the pipeline ingests.
pub fn quantize_to_u16(movie: &Array3<f64>) -> ndarray::Array3<u16> {
    movie.mapv(|v| v.round().clamp(0.0, u16::MAX as f64) as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::LogMap;
    use agreement::{Eps, Tol};

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

    /// Default-off corruptions reproduce the clean layer-2 encoder bit-for-bit.
    #[test]
    fn default_off_reproduces_clean_encoder() {
        let gt = LogMap::default().generate(12, 16);
        let stim = Stim::default();
        let rng = SynthRng::from_seed(0);
        let clean = crate::encode::encode_direction(&gt, Axis::Azimuth, false, &stim);
        let realistic = encode_direction_realistic(
            &gt,
            Axis::Azimuth,
            false,
            &stim,
            0.1,
            &Corruptions::default(),
            &rng,
            "LR",
        );
        assert_eq!(
            realistic, clean,
            "default corruptions must equal the clean encoder bit-for-bit"
        );
    }

    /// HRF-only (no noise): `(fwd−rev)/2` recovers the encoded position and
    /// `(fwd+rev)/2` recovers the HRF's own bin-1 delay `∠H(ω)` (direction-
    /// independent), the Kalatsky–Stryker separation under a hemodynamic delay.
    #[test]
    fn hrf_delay_separates_from_position() {
        let gt = LogMap::default().generate(16, 16);
        let stim = Stim::default();
        let rng = SynthRng::from_seed(0);
        let corr = Corruptions {
            hemodynamic: Some(Hrf::default()),
            sensor: None,
        };
        let fwd = encode_direction_realistic(&gt, Axis::Altitude, false, &stim, 0.1, &corr, &rng, "TB");
        let rev = encode_direction_realistic(&gt, Axis::Altitude, true, &stim, 0.1, &corr, &rng, "BT");

        // Expected delay = bin-1 phase of the HRF's response to a unit cosine,
        // computed self-consistently from the same kernel + convolution.
        let t_total = stim.cycles * stim.frames_per_cycle;
        let kernel = Hrf::default().kernel(0.1, t_total);
        let unit: Vec<f64> = (0..t_total)
            .map(|t| (std::f64::consts::TAU * t as f64 / stim.frames_per_cycle as f64).cos())
            .collect();
        let resp = circular_convolve(&unit, &kernel);
        let (mut sr, mut si) = (0.0, 0.0);
        for (t, &rt) in resp.iter().enumerate() {
            let w = std::f64::consts::TAU * t as f64 / stim.frames_per_cycle as f64;
            sr += rt * w.cos();
            si += rt * w.sin();
        }
        let expected_delay = (-si).atan2(sr);

        let mut pos_recovered = Vec::new();
        let mut pos_expected = Vec::new();
        let mut delay = Vec::new();
        let mut delay_expected = Vec::new();
        for r in 0..16 {
            for c in 0..16 {
                let pf = bin1_phase(&fwd, r, c, stim.frames_per_cycle);
                let pr = bin1_phase(&rev, r, c, stim.frames_per_cycle);
                pos_recovered.push(0.5 * (pf - pr));
                pos_expected.push(position_to_phase(gt.alt[[r, c]], &stim));
                delay.push(0.5 * (pf + pr));
                delay_expected.push(expected_delay);
            }
        }
        // K grounded to MEASURED drift: the 240-tap circular convolution + the
        // bin-1 DFT reduction inflate the f64 error well past machine-ε — ~1900·ε
        // for position (errors cancel in fwd−rev) and ~4350·ε for the delay
        // (errors add in fwd+rev). Next power of two bounding the max → K = 8192
        // (~1.8e-12 abs on radian-scale phases).
        Tol::wrap(std::f64::consts::TAU, 8192, Eps::F64, 1.0)
            .assert("(fwd−rev)/2 = position", &pos_recovered, &pos_expected);
        Tol::wrap(std::f64::consts::TAU, 8192, Eps::F64, 1.0)
            .assert("(fwd+rev)/2 = HRF delay ∠H(ω)", &delay, &delay_expected);
    }

    /// Sensor noise is reproducible from the seed and actually perturbs the movie.
    #[test]
    fn sensor_noise_is_reproducible_and_nonzero() {
        let gt = LogMap::default().generate(8, 8);
        let stim = Stim::default();
        let corr = Corruptions {
            hemodynamic: None,
            sensor: Some(SensorNoise::default()),
        };
        let a = encode_direction_realistic(&gt, Axis::Azimuth, false, &stim, 0.1, &corr, &SynthRng::from_seed(5), "LR");
        let b = encode_direction_realistic(&gt, Axis::Azimuth, false, &stim, 0.1, &corr, &SynthRng::from_seed(5), "LR");
        let clean = crate::encode::encode_direction(&gt, Axis::Azimuth, false, &stim);
        assert_eq!(a, b, "same seed ⇒ identical noisy movie");
        assert!(a != clean, "noise must perturb the movie");
    }
}
