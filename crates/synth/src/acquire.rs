//! Recording assembly (layer 4) — compose the four sweep directions into one
//! coherent, pipeline-ingestible synthetic recording.
//!
//! Produces a **synth-native** [`Synthetic`] (this crate stays an independent leaf;
//! the conversion to the pipeline's `RawAcquisition` lives in the dev/golden glue,
//! per `docs/SYNTHETIC_VALIDATION.md` + the Phase-A plan). The timeline is built to
//! satisfy the projection stage's ingest rules exactly:
//!
//! - uniform frame period `dt` ⇒ `meanFrameDur == dt` ⇒ the projection's bin-1
//!   frequency `1/(fpc·dt)` equals the encoder's stimulus frequency (no leakage);
//! - each stimulus cycle is **one sweep** with `sweep_end = start + fpc·dt` ⇒ the
//!   projected chunk is exactly `fpc` frames (no off-by-one DFT smear);
//! - **≥2 cycles per direction** (cross-cycle reliability is defined);
//! - lead-in + inter-direction rest gaps + a trailing gap, so every sweep chunk
//!   sits strictly inside `[cam_ts[0], cam_ts[T-1]]` (no silent skips) and the
//!   inter-sweep baseline has rest frames to find.
//!
//! Rest/blank frames are the clean resting reflectance `F0` (no modulation, no
//! noise in Phase A), so the inter-sweep baseline recovers `F0` exactly.

use ndarray::{s, Array3};

use crate::encode::{Axis, Stim};
use crate::map::{GroundTruth, LogMap};
use crate::realism::{encode_direction_realistic, quantize_to_u16, Corruptions};
use crate::rng::SynthRng;

/// The four cardinal sweep directions, in acquisition order, with the axis +
/// reverse flag each encodes. Labels match the pipeline's `classify_cycle_name`
/// (`LR`→azi-fwd, `RL`→azi-rev, `TB`→alt-fwd, `BT`→alt-rev).
///
/// `TB`→alt-fwd (`+alt`), `BT`→alt-rev (`−alt`): the recovered altitude matches the
/// ground truth directly (the pipeline's `classify_cycle_name` flip-absorption
/// cancels with this assignment — verified by the recover-and-compare test).
const DIRECTIONS: [(&str, Axis, bool); 4] = [
    ("LR", Axis::Azimuth, false),
    ("RL", Axis::Azimuth, true),
    ("TB", Axis::Altitude, false),
    ("BT", Axis::Altitude, true),
];

/// Capture-time geometry of a synthetic recording — mirrors the fields the
/// pipeline reads as `AcquisitionProperties`. The azimuth/altitude ranges equal
/// the stimulus `angular_range_deg` (the position→phase span), so recovery is
/// self-consistent.
#[derive(Clone, Copy, Debug)]
pub struct SynthGeometry {
    pub azi_range_deg: f64,
    pub alt_range_deg: f64,
    pub offset_azi_deg: f64,
    pub offset_alt_deg: f64,
    pub um_per_pixel: f64,
}

/// A complete synthetic recording: the raw u16 movie + acquisition schedule (the
/// pipeline's input), plus the **known ground truth** and geometry to validate
/// recovery against.
pub struct Synthetic {
    pub frames: Array3<u16>,
    pub cam_ts_sec: Vec<f64>,
    pub sweep_start_sec: Vec<f64>,
    pub sweep_end_sec: Vec<f64>,
    pub sweep_sequence: Vec<String>,
    pub ground_truth: GroundTruth,
    pub geom: SynthGeometry,
    /// The seed this recording was generated from (reproducible-from-seed).
    pub seed: u64,
}

/// The spec for one synthetic recording. Everything is explicit and seed-
/// reproducible; `Corruptions::default()` yields a clean recording, `benchmark()`
/// a literature-grounded noisy one.
#[derive(Clone, Copy, Debug)]
pub struct RecordingSpec {
    pub map: LogMap,
    pub stim: Stim,
    pub corruptions: Corruptions,
    /// Camera frame period (seconds). Uniform ⇒ exact bin-1 frequency match.
    pub dt_sec: f64,
    pub um_per_pixel: f64,
    /// Rest frames before the first sweep (baseline headroom).
    pub lead_in_frames: usize,
    /// Rest frames after each direction's epoch (inter-sweep baseline + headroom).
    pub inter_dir_gap_frames: usize,
    pub seed: u64,
}

impl RecordingSpec {
    /// A small, CI-sized clean recording (knobs off) — the smoke/recovery baseline.
    pub fn clean_smoke() -> Self {
        Self {
            map: LogMap::default(),
            stim: Stim::default(),
            corruptions: Corruptions::default(),
            dt_sec: 0.1,
            um_per_pixel: 20.0,
            lead_in_frames: 8,
            inter_dir_gap_frames: 8,
            seed: 0,
        }
    }
}

/// Assemble a synthetic recording over an `H×W` cortical grid.
pub fn build(spec: &RecordingSpec, h: usize, w: usize) -> Synthetic {
    let gt = spec.map.generate(h, w);
    let rng = SynthRng::from_seed(spec.seed);
    let dt = spec.dt_sec;
    let fpc = spec.stim.frames_per_cycle;
    let cycles = spec.stim.cycles;
    let epoch_len = cycles * fpc;
    let gap = spec.inter_dir_gap_frames;

    // total = lead-in + 4·(epoch + trailing gap). The gap after the LAST direction
    // is the trailing headroom that keeps the final chunk inside the timeline.
    let total = spec.lead_in_frames + DIRECTIONS.len() * (epoch_len + gap);

    // Blank baseline everywhere (rest = clean resting reflectance F0).
    let mut movie = Array3::from_elem((total, h, w), spec.stim.baseline);

    let mut sweep_start_sec = Vec::new();
    let mut sweep_end_sec = Vec::new();
    let mut sweep_sequence = Vec::new();

    let mut t0 = spec.lead_in_frames;
    for &(label, axis, reverse) in &DIRECTIONS {
        let epoch =
            encode_direction_realistic(&gt, axis, reverse, &spec.stim, dt, &spec.corruptions, &rng, label);
        movie.slice_mut(s![t0..t0 + epoch_len, .., ..]).assign(&epoch);
        for c in 0..cycles {
            let start_frame = t0 + c * fpc;
            sweep_start_sec.push(start_frame as f64 * dt);
            // start + exactly fpc frames ⇒ chunkFrameDur == fpc.
            sweep_end_sec.push((start_frame + fpc) as f64 * dt);
            sweep_sequence.push(label.to_string());
        }
        t0 += epoch_len + gap;
    }

    let cam_ts_sec: Vec<f64> = (0..total).map(|i| i as f64 * dt).collect();
    let frames = quantize_to_u16(&movie);

    Synthetic {
        frames,
        cam_ts_sec,
        sweep_start_sec,
        sweep_end_sec,
        sweep_sequence,
        ground_truth: gt,
        geom: SynthGeometry {
            azi_range_deg: spec.stim.angular_range_deg,
            alt_range_deg: spec.stim.angular_range_deg,
            offset_azi_deg: spec.stim.offset_deg,
            offset_alt_deg: spec.stim.offset_deg,
            um_per_pixel: spec.um_per_pixel,
        },
        seed: spec.seed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_satisfies_the_projection_ingest_contract() {
        let spec = RecordingSpec::clean_smoke();
        let (h, w) = (16, 24);
        let syn = build(&spec, h, w);
        let fpc = spec.stim.frames_per_cycle;
        let cycles = spec.stim.cycles;

        // Schedule shape: 4 directions × cycles sweeps, all labels present.
        assert_eq!(syn.sweep_sequence.len(), 4 * cycles);
        assert_eq!(syn.sweep_start_sec.len(), syn.sweep_sequence.len());
        for label in ["LR", "RL", "TB", "BT"] {
            let n = syn.sweep_sequence.iter().filter(|s| *s == label).count();
            assert_eq!(n, cycles, "{label} must have {cycles} sweeps (≥2 for reliability)");
        }
        assert!(cycles >= 2, "reliability needs ≥2 cycles/direction");

        // Frames shape + uniform timeline.
        assert_eq!(syn.frames.dim(), (syn.cam_ts_sec.len(), h, w));
        let dt = spec.dt_sec;
        for (i, &t) in syn.cam_ts_sec.iter().enumerate() {
            assert!((t - i as f64 * dt).abs() < 1e-12, "uniform cam timeline");
        }

        // Every sweep chunk is exactly fpc frames and sits strictly inside the
        // timeline (no silent projection skip).
        let t_last = *syn.cam_ts_sec.last().unwrap();
        for k in 0..syn.sweep_start_sec.len() {
            let dur = syn.sweep_end_sec[k] - syn.sweep_start_sec[k];
            let chunk_frames = (dur / dt).round() as usize;
            assert_eq!(chunk_frames, fpc, "chunk must be exactly fpc frames");
            assert!(syn.sweep_start_sec[k] >= syn.cam_ts_sec[0]);
            assert!(syn.sweep_end_sec[k] <= t_last + 1e-9, "chunk fits in timeline");
        }

        // Ground truth + geometry are self-consistent (ranges cover the map).
        assert_eq!(syn.ground_truth.azi.dim(), (h, w));
        assert_eq!(syn.geom.azi_range_deg, spec.stim.angular_range_deg);
        let max_azi = syn.ground_truth.azi.iter().cloned().fold(0.0_f64, |a, v| a.max(v.abs()));
        assert!(max_azi <= syn.geom.azi_range_deg / 2.0, "map azimuth fits the stimulus range");
    }

    #[test]
    fn same_seed_reproduces_the_whole_recording() {
        let mut spec = RecordingSpec::clean_smoke();
        spec.corruptions = Corruptions::benchmark();
        spec.seed = 99;
        let a = build(&spec, 12, 12);
        let b = build(&spec, 12, 12);
        assert_eq!(a.frames, b.frames, "same seed ⇒ identical u16 movie");
    }
}
