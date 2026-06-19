//! Deterministic, reproducible randomness for the realism layer (layer 3).
//!
//! A synthetic recording must be **bit-identical from its seed** on any platform —
//! committable fixtures and clean one-axis stress sweeps both depend on it, and the
//! codebase forbids nondeterminism (no `ThreadRng`, time, or entropy). We use
//! **ChaCha8** (portable, version-stable) and give every stochastic knob its own
//! *named substream*:
//!
//! - same seed ⇒ identical movie everywhere;
//! - toggling one knob never perturbs another knob's realization (each substream's
//!   seed is derived independently from `(seed, label)`, not by drawing from a
//!   shared parent stream in call order).
//!
//! Substream seeds are mixed with a **fixed** FNV-1a hash of the label (NOT
//! `DefaultHasher`, which is randomized per-process and would break reproducibility).

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// The seed source for a recording. Hand out one [`Substream`] per stochastic knob
/// (and per direction) via [`SynthRng::substream`].
#[derive(Clone, Copy, Debug)]
pub struct SynthRng {
    seed: u64,
}

impl SynthRng {
    /// Root a recording's randomness at `seed`.
    pub fn from_seed(seed: u64) -> Self {
        Self { seed }
    }

    /// The root seed (recorded in provenance so a recording is reproducible).
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// An independent, reproducible substream for a named knob/direction
    /// (e.g. `"sensor"`, `"dir:LR"`). Independent of draw order, so adding or
    /// removing a knob leaves every other substream's realization unchanged.
    pub fn substream(&self, label: &str) -> Substream {
        Substream(ChaCha8Rng::seed_from_u64(self.seed ^ fnv1a64(label)))
    }
}

/// A single knob's reproducible random stream. Exposes only what the realism layer
/// needs: standard-uniform and standard-normal draws.
pub struct Substream(ChaCha8Rng);

impl Substream {
    /// A uniform draw in `[0, 1)`.
    pub fn uniform(&mut self) -> f64 {
        self.0.random::<f64>()
    }

    /// A standard-normal draw `N(0, 1)` via Box-Muller. Generating Gaussians from
    /// ChaCha uniforms keeps the dependency surface minimal and the result
    /// platform-stable; the cosine branch alone is used (the sine companion is
    /// discarded, trading a little throughput for simpler, order-stable draws).
    pub fn normal(&mut self) -> f64 {
        // Guard the log against an exact zero (u1 ∈ [0,1) can be 0).
        let u1 = (1.0 - self.uniform()).max(f64::MIN_POSITIVE);
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

/// FNV-1a (64-bit) over the label bytes — a fixed, deterministic hash so substream
/// seeds are stable across processes and platforms (unlike `std`'s `DefaultHasher`).
fn fnv1a64(s: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for &b in s.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same seed + same label ⇒ identical stream (reproducibility).
    #[test]
    fn same_seed_same_label_reproduces_stream() {
        let a: Vec<f64> = {
            let mut s = SynthRng::from_seed(42).substream("sensor");
            (0..16).map(|_| s.normal()).collect()
        };
        let b: Vec<f64> = {
            let mut s = SynthRng::from_seed(42).substream("sensor");
            (0..16).map(|_| s.normal()).collect()
        };
        assert_eq!(a, b, "same seed+label must reproduce the stream bit-for-bit");
    }

    /// Different labels ⇒ independent streams, so toggling one knob can't perturb
    /// another's realization.
    #[test]
    fn distinct_labels_are_independent() {
        let r = SynthRng::from_seed(7);
        let sensor: Vec<f64> = {
            let mut s = r.substream("sensor");
            (0..16).map(|_| s.uniform()).collect()
        };
        let physio: Vec<f64> = {
            let mut s = r.substream("physio");
            (0..16).map(|_| s.uniform()).collect()
        };
        assert_ne!(sensor, physio, "distinct substreams must differ");
    }

    /// Box-Muller normals have ~unit variance and ~zero mean over a modest sample
    /// (a loose sanity bound, not a distribution test).
    #[test]
    fn normal_has_unit_variance_roughly() {
        let mut s = SynthRng::from_seed(1).substream("n");
        let n = 20_000;
        let xs: Vec<f64> = (0..n).map(|_| s.normal()).collect();
        let mean = xs.iter().sum::<f64>() / n as f64;
        let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
        assert!(mean.abs() < 0.05, "mean ≈ 0, got {mean}");
        assert!((var - 1.0).abs() < 0.1, "var ≈ 1, got {var}");
    }
}
