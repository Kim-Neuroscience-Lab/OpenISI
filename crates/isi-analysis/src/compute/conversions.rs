//! Conversions between host `ndarray` and Burn tensors.
//!
//! The analysis pipeline keeps host data as `ndarray` (from HDF5) and
//! converts to Burn tensors only at the compute boundary. These helpers
//! are the only place ndarray ↔ Burn conversion happens.
//!
//! On-device tensors are `f32`; complex fields are [`Complex2`] pairs.
//! The host→device direction narrows `f64`/`Complex64` to `f32` — a
//! deliberate, documented precision boundary
//! (`docs/UNIFIED_COMPUTE_ARCHITECTURE.md` §1.3: the device pipeline is
//! `f32` throughout). The device→host direction promotes `f32`→`f64` so
//! downstream segmentation/HDF5 code sees uniform precision.

use ndarray::{Array2, Array3};
use num_complex::Complex64;

use super::backend::{device, Backend};
use super::complex::Complex2;
use crate::{AnalysisError, Result};
use burn_tensor::{Tensor, TensorData};

/// Upload an `Array2<Complex64>` to a Burn [`Complex2`] (paired f32
/// real/imag planes).
pub fn array2_complex_to_complex2(arr: &Array2<Complex64>) -> Complex2 {
    let (h, w) = arr.dim();
    let re: Vec<f32> = arr.iter().map(|z| z.re as f32).collect();
    let im: Vec<f32> = arr.iter().map(|z| z.im as f32).collect();
    let device = device();
    let re_t = Tensor::<Backend, 2>::from_data(TensorData::new(re, [h, w]), &device);
    let im_t = Tensor::<Backend, 2>::from_data(TensorData::new(im, [h, w]), &device);
    Complex2::new(re_t, im_t)
}

/// Upload an `Array2<f64>` to a Burn `f32` tensor. Used for the optional
/// host-side rotation path before compute.
pub fn array2_f64_to_tensor(arr: &Array2<f64>) -> Tensor<Backend, 2> {
    let (h, w) = arr.dim();
    let data: Vec<f32> = arr.iter().map(|&v| v as f32).collect();
    Tensor::<Backend, 2>::from_data(TensorData::new(data, [h, w]), &device())
}

/// Download a 2D Burn tensor to a host `Array2<f64>`. Promotes `f32` →
/// `f64` so downstream f64 ndarray code (segmentation, derived maps,
/// HDF5 write) sees uniform precision.
pub fn tensor_to_array2_f64(t: Tensor<Backend, 2>) -> Result<Array2<f64>> {
    let [h, w] = t.dims();
    let flat: Vec<f32> = t
        .into_data()
        .to_vec()
        .map_err(|e| AnalysisError::Compute(format!("burn tensor → Vec<f32>: {e:?}")))?;
    let data: Vec<f64> = flat.into_iter().map(|v| v as f64).collect();
    Array2::from_shape_vec((h, w), data)
        .map_err(|e| AnalysisError::Compute(format!("shape mismatch in tensor_to_array2_f64: {e}")))
}

/// Download a [`Complex2`] field to a host `Array2<Complex64>`; promotes
/// both planes f32→f64.
pub fn complex2_to_array2(z: &Complex2) -> Result<Array2<Complex64>> {
    let re = tensor_to_array2_f64(z.real())?;
    let im = tensor_to_array2_f64(z.imag())?;
    let (h, w) = re.dim();
    Ok(Array2::from_shape_fn((h, w), |(r, c)| {
        Complex64::new(re[[r, c]], im[[r, c]])
    }))
}

/// Upload a subset of u16 camera frames as a Burn `f32` tensor `[n, H, W]`,
/// in the order given by `indices`: the full frame stack stays in host
/// memory; only the indexed subset is uploaded.
pub fn frames_u16_subset_to_tensor(frames: &Array3<u16>, indices: &[usize]) -> Tensor<Backend, 3> {
    let (_, h, w) = frames.dim();
    let n = indices.len();
    let plane = h * w;
    let mut flat = vec![0.0f32; n * plane];
    for (out_i, &src_i) in indices.iter().enumerate() {
        let src_plane = frames.slice(ndarray::s![src_i, .., ..]);
        let dst = out_i * plane;
        for (px, &v) in src_plane.iter().enumerate() {
            flat[dst + px] = v as f32;
        }
    }
    Tensor::<Backend, 3>::from_data(TensorData::new(flat, [n, h, w]), &device())
}

/// Per-pixel temporal-mean baseline `F0 [H, W]` over the full frame stack,
/// accumulated in `f64` in a single pass.
///
/// This is the ΔF/F denominator (Allen `ImageAnalysis.py` `dFoverF`): the
/// bin-1 DFT must run on `(F − F0)/F0`, not raw counts, so the amplitude that
/// drives cortex masking / phase weighting reflects *fractional* response
/// (responsive cortex) rather than raw brightness (which large vessels
/// dominate). The tch→burn port dropped this step; [`frames_u16_subset_to_dff_tensor`]
/// restores it.
pub fn temporal_mean_baseline(frames: &Array3<u16>) -> Array2<f64> {
    let (t, h, w) = frames.dim();
    let mut acc = Array2::<f64>::zeros((h, w));
    for frame in frames.outer_iter() {
        acc.zip_mut_with(&frame, |a, &v| *a += v as f64);
    }
    if t > 0 {
        acc /= t as f64;
    }
    acc
}

/// Per-pixel temporal MEDIAN baseline `F0` — the faithful Allen
/// `ImageAnalysis.normalizeMovie(baselineType='median')` baseline
/// (`np.median(movie, axis=0)`). Uses numpy's median convention: the middle
/// value for an odd frame count, the average of the two middle values for an
/// even count. More robust than the mean to transient outliers (a bright
/// artifact frame doesn't drag F0).
///
/// Validated vs `np.median` by `median_baseline_matches_numpy`.
pub fn temporal_median_baseline(frames: &Array3<u16>) -> Array2<f64> {
    let (t, h, w) = frames.dim();
    let mut out = Array2::<f64>::zeros((h, w));
    if t == 0 {
        return out;
    }
    let mut col = vec![0.0f64; t];
    for r in 0..h {
        for c in 0..w {
            for (k, ck) in col.iter_mut().enumerate() {
                *ck = frames[[k, r, c]] as f64;
            }
            col.sort_by(|a, b| a.partial_cmp(b).unwrap());
            out[[r, c]] = if t % 2 == 1 {
                col[t / 2]
            } else {
                0.5 * (col[t / 2 - 1] + col[t / 2])
            };
        }
    }
    out
}

/// How an inter-sweep baseline aggregates the rest frames into `F0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineAggregate {
    /// Per-pixel temporal mean of the rest frames.
    Mean,
    /// Per-pixel temporal median of the rest frames (numpy convention;
    /// robust to a transient artifact frame in a rest period).
    Median,
}

/// Indices of frames captured OUTSIDE every stimulus sweep window — the
/// pre-stimulus and inter-sweep rest periods.
///
/// A frame at camera time `t` is a *rest* frame iff `t` falls in no
/// `[sweep_start[k], sweep_end[k]]` interval. `cam_ts_sec` is the per-frame
/// camera timestamp (length = frame count); `sweep_start_sec`/`sweep_end_sec`
/// are the schedule's per-sweep onset/offset times (the same SSoT the DFT uses
/// for cycle onsets). Intervals are treated as inclusive; the comparison is the
/// natural `start <= t <= end`, so a frame exactly on a boundary is considered
/// *in* the sweep (excluded from rest) — the conservative choice for a
/// stimulus-free baseline.
pub fn rest_frame_indices(
    cam_ts_sec: &[f64],
    sweep_start_sec: &[f64],
    sweep_end_sec: &[f64],
) -> Vec<usize> {
    let n_sweeps = sweep_start_sec.len().min(sweep_end_sec.len());
    cam_ts_sec
        .iter()
        .enumerate()
        .filter(|&(_, &t)| {
            // A rest frame is in NO sweep window.
            !(0..n_sweeps).any(|k| sweep_start_sec[k] <= t && t <= sweep_end_sec[k])
        })
        .map(|(i, _)| i)
        .collect()
}

/// OpenISI inter-sweep baseline: per-pixel `F0` aggregated over only the *rest*
/// frames (those outside every stimulus sweep window — see
/// [`rest_frame_indices`]).
///
/// Motivation (OpenISI method, not from an external oracle): the Allen baseline
/// ([`temporal_mean_baseline`]) averages over *all* frames, including the
/// stimulus sweeps. When the stimulus drives a sustained response (or the
/// aperture/edge introduces a sweep-locked DC change), that activity leaks into
/// `F0` and biases the ΔF/F denominator. Restricting `F0` to the resting
/// periods — before the first sweep and in the gaps between sweeps — gives a
/// stimulus-free baseline, the standard "pre-stimulus baseline" idea applied at
/// the inter-sweep granularity this protocol affords.
///
/// Returns `None` when there are no rest frames (e.g. a back-to-back schedule
/// with no gaps), so the caller falls back to the all-frame baseline rather
/// than dividing by an empty set.
///
/// Validated by property tests (`inter_sweep_baseline_*` in this module): the
/// rest-frame selection is exact, and the baseline recovers the true resting
/// `F0` even when the sweep frames are biased — which the all-frame mean cannot.
pub fn inter_sweep_baseline(
    frames: &Array3<u16>,
    cam_ts_sec: &[f64],
    sweep_start_sec: &[f64],
    sweep_end_sec: &[f64],
    aggregate: BaselineAggregate,
) -> Option<Array2<f64>> {
    let (t, _, _) = frames.dim();
    let mut rest = rest_frame_indices(cam_ts_sec, sweep_start_sec, sweep_end_sec);
    // Guard against a timestamp/frame-count mismatch: only index frames that
    // actually exist in the stack.
    rest.retain(|&i| i < t);
    if rest.is_empty() {
        return None;
    }
    Some(aggregate_over_indices(frames, &rest, aggregate))
}

/// Per-pixel mean/median of `frames` over the given frame `indices`. The mean
/// branch accumulates in `f64`; the median branch uses numpy's convention
/// (middle value for an odd count, average of the two middle values for an
/// even count) — the same convention as [`temporal_median_baseline`].
fn aggregate_over_indices(
    frames: &Array3<u16>,
    indices: &[usize],
    aggregate: BaselineAggregate,
) -> Array2<f64> {
    let (_, h, w) = frames.dim();
    match aggregate {
        BaselineAggregate::Mean => {
            let mut acc = Array2::<f64>::zeros((h, w));
            for &i in indices {
                acc.zip_mut_with(&frames.slice(ndarray::s![i, .., ..]), |a, &v| {
                    *a += v as f64
                });
            }
            acc /= indices.len() as f64;
            acc
        }
        BaselineAggregate::Median => {
            let m = indices.len();
            let mut out = Array2::<f64>::zeros((h, w));
            let mut col = vec![0.0f64; m];
            for r in 0..h {
                for c in 0..w {
                    for (slot, &i) in col.iter_mut().zip(indices.iter()) {
                        *slot = frames[[i, r, c]] as f64;
                    }
                    col.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    out[[r, c]] = if m % 2 == 1 {
                        col[m / 2]
                    } else {
                        0.5 * (col[m / 2 - 1] + col[m / 2])
                    };
                }
            }
            out
        }
    }
}

/// Like [`frames_u16_subset_to_tensor`], but uploads the ΔF/F movie
/// `(F − F0) / max(F0, denom_floor)` for the indexed subset, given the
/// per-pixel baseline `F0` from [`temporal_mean_baseline`].
///
/// The denominator is *floored* (not offset by a small eps): pixels outside
/// the illuminated cortex have `F0 → 0`, and a naive `(F − F0)/(F0 + eps)`
/// blows those up into amplitude outliers that hijack masking/weighting (one
/// such recording collapsed a real gradient to 2 signal pixels). Flooring at
/// a fraction of the median baseline (the cortical scale) leaves real cortex
/// undistorted while bounding dark/vignette pixels. See
/// [`dff_denominator_floor`] for the floor.
///
/// Frames read from HDF5 are standard (row-major) layout, so `frames`' inner
/// `[H, W]` planes and `baseline`'s contiguous slice iterate in the same
/// pixel order; the `as_slice` index `px` therefore aligns with the plane.
pub fn frames_u16_subset_to_dff_tensor(
    frames: &Array3<u16>,
    indices: &[usize],
    baseline: &Array2<f64>,
    denom_floor: f64,
) -> Tensor<Backend, 3> {
    let (_, h, w) = frames.dim();
    let n = indices.len();
    let plane = h * w;
    let f0 = baseline
        .as_slice()
        .expect("baseline from temporal_mean_baseline is contiguous");
    let mut flat = vec![0.0f32; n * plane];
    for (out_i, &src_i) in indices.iter().enumerate() {
        let src_plane = frames.slice(ndarray::s![src_i, .., ..]);
        let dst = out_i * plane;
        for (px, &v) in src_plane.iter().enumerate() {
            let base = f0[px];
            flat[dst + px] = ((v as f64 - base) / base.max(denom_floor)) as f32;
        }
    }
    Tensor::<Backend, 3>::from_data(TensorData::new(flat, [n, h, w]), &device())
}

/// ΔF/F denominator floor for [`frames_u16_subset_to_dff_tensor`]: half the
/// median baseline. The median tracks the cortical brightness (the FOV is
/// filled by illuminated cortex), so this floors only the dim/vignette tail
/// while leaving the cortex's true `F0` in the denominator.
pub fn dff_denominator_floor(baseline: &Array2<f64>) -> f64 {
    use ndarray_stats::{interpolate::Higher, Quantile1dExt};
    use noisy_float::types::{n64, N64};

    // Median of the finite baseline via ndarray-stats `quantile_mut(0.5, Higher)`.
    // ndarray-stats requires `Ord` elements, so the finite `f64` values are
    // lifted into `N64` (the noisy-float wrapper it is built for). `Higher`
    // interpolation returns the ⌈0.5·(n−1)⌉-th order statistic — exactly the
    // value the previous `select_nth_unstable` picked, so no numeric change.
    let mut vals: ndarray::Array1<N64> = baseline
        .iter()
        .filter(|v| v.is_finite())
        .map(|&v| n64(v))
        .collect();
    if vals.is_empty() {
        return 1.0;
    }
    let median = vals
        .quantile_mut(n64(0.5), &Higher)
        .expect("non-empty checked above")
        .raw();
    (0.5 * median).max(1.0)
}

#[cfg(test)]
mod inter_sweep_baseline_tests {
    //! Property/synthetic validation of the OpenISI inter-sweep baseline.
    //! This is our own method (no external oracle), so it is pinned by
    //! properties rather than a reference fixture, the same way the
    //! reliability metric is.
    use super::*;
    use ndarray::Array3;

    /// Build a tiny synthetic recording: `n` frames at 10 Hz (0.1 s apart),
    /// every pixel set to `level(frame)`. Two sweep windows are returned along
    /// with the frames so the tests can reason about which frames are "rest".
    fn synth(level: impl Fn(usize) -> u16, n: usize) -> (Array3<u16>, Vec<f64>) {
        let h = 2;
        let w = 3;
        let frames =
            Array3::from_shape_fn((n, h, w), |(t, _, _)| level(t));
        let cam_ts: Vec<f64> = (0..n).map(|t| t as f64 * 0.1).collect();
        (frames, cam_ts)
    }

    /// `rest_frame_indices` selects exactly the frames whose timestamp lies in
    /// no sweep window — pre-stimulus, the inter-sweep gap, and the tail.
    #[test]
    fn rest_frame_selection_is_exact() {
        // 20 frames, t = 0.0 .. 1.9 s. Sweeps chosen with margins off the frame
        // grid (0.1 s spacing) so no frame sits on a boundary: [0.25,0.75]
        // captures idx 3..=7, [1.05,1.55] captures idx 11..=15.
        let (_frames, cam_ts) = synth(|_| 0, 20);
        let starts = vec![0.25, 1.05];
        let ends = vec![0.75, 1.55];
        let rest = rest_frame_indices(&cam_ts, &starts, &ends);

        // In-sweep frames: idx 3..=7 and 11..=15.
        let expected: Vec<usize> = (0..20)
            .filter(|&i| !((3..=7).contains(&i) || (11..=15).contains(&i)))
            .collect();
        assert_eq!(rest, expected);
        // Sanity: pre (0,1,2), gap (8,9,10), tail (16..19) all present.
        for i in [0, 1, 2, 8, 9, 10, 16, 19] {
            assert!(rest.contains(&i), "expected rest frame {i}");
        }
    }

    /// The whole point: when the SWEEP frames are biased high (sustained
    /// stimulus response), the all-frame mean baseline is pulled up, but the
    /// inter-sweep baseline recovers the true resting F0. Rest level = 100,
    /// sweep level = 500.
    #[test]
    fn inter_sweep_baseline_recovers_resting_f0_when_sweeps_are_biased() {
        let rest_level = 100u16;
        let sweep_level = 500u16;
        let starts = vec![0.3, 1.1];
        let ends = vec![0.7, 1.5];
        let (frames, cam_ts) = synth(
            |t| {
                let tt = t as f64 * 0.1;
                let in_sweep = (starts[0]..=ends[0]).contains(&tt)
                    || (starts[1]..=ends[1]).contains(&tt);
                if in_sweep { sweep_level } else { rest_level }
            },
            20,
        );

        // All-frame mean is contaminated by the biased sweep frames.
        let all = temporal_mean_baseline(&frames);
        assert!(
            all[[0, 0]] > rest_level as f64 + 1.0,
            "all-frame baseline {} should be pulled above the resting level",
            all[[0, 0]]
        );

        // Inter-sweep baseline (both aggregates) recovers the resting F0 exactly.
        for agg in [BaselineAggregate::Mean, BaselineAggregate::Median] {
            let b = inter_sweep_baseline(&frames, &cam_ts, &starts, &ends, agg)
                .expect("there are rest frames");
            for &v in b.iter() {
                assert!(
                    (v - rest_level as f64).abs() < 1e-9,
                    "{agg:?} baseline {v} should equal resting F0 {rest_level}"
                );
            }
        }
    }

    /// The median aggregate is robust to a single bright artifact frame in a
    /// rest period; the mean is not. Rest frames are 100 except one spike of
    /// 10000.
    #[test]
    fn median_aggregate_rejects_a_rest_period_artifact() {
        let starts = vec![0.5];
        let ends = vec![0.9]; // frames 5..=9 are in-sweep
        // frame 2 is a rest-period artifact spike.
        let (frames, cam_ts) = synth(
            |t| {
                let tt = t as f64 * 0.1;
                if (0.5..=0.9).contains(&tt) {
                    500
                } else if t == 2 {
                    10_000
                } else {
                    100
                }
            },
            15,
        );

        let mean_b = inter_sweep_baseline(&frames, &cam_ts, &starts, &ends, BaselineAggregate::Mean)
            .unwrap();
        let med_b =
            inter_sweep_baseline(&frames, &cam_ts, &starts, &ends, BaselineAggregate::Median)
                .unwrap();

        // Mean is dragged up by the spike; median sits at the true 100.
        assert!(mean_b[[0, 0]] > 200.0, "mean {} should be inflated by the spike", mean_b[[0, 0]]);
        assert!(
            (med_b[[0, 0]] - 100.0).abs() < 1e-9,
            "median {} should reject the spike and equal 100",
            med_b[[0, 0]]
        );
    }

    /// No gaps in the schedule (the sweeps tile the whole recording) -> no rest
    /// frames -> `None`, so the caller falls back to the all-frame baseline.
    #[test]
    fn no_rest_frames_returns_none() {
        let (frames, cam_ts) = synth(|_| 100, 10);
        // One sweep covering the entire timeline.
        let starts = vec![cam_ts[0] - 0.01];
        let ends = vec![cam_ts[cam_ts.len() - 1] + 0.01];
        let b = inter_sweep_baseline(&frames, &cam_ts, &starts, &ends, BaselineAggregate::Mean);
        assert!(b.is_none(), "a gapless schedule has no rest frames");
    }
}
