//! Compare the ΔF/F-baseline methods on a real recording.
//!
//! The baseline `F0` is the ΔF/F denominator: it sets what counts as "no
//! response" before the bin-1 DFT. Four methods are compared, all on the SAME
//! recording and the SAME first-direction cycles, so only the baseline differs:
//!
//!   1. Allen all-frame MEAN    (`temporal_mean_baseline`, production default)
//!   2. Allen all-frame MEDIAN  (`temporal_median_baseline`)
//!   3. OpenISI inter-sweep MEAN   (`inter_sweep_baseline`, rest frames only)
//!   4. OpenISI inter-sweep MEDIAN
//!
//! For each baseline we emit the per-pixel `F0` map and the F1 ΔF/F amplitude
//! map (`|mean_k DFT_bin1(ΔF/F_k)|` over the first direction's cycles). The
//! amplitude is where a contaminated baseline shows up: the all-frame methods
//! average over the stimulus sweeps too, so sustained / aperture-locked activity
//! leaks into `F0` and rescales the amplitude; the inter-sweep methods take `F0`
//! only from the rest periods (before the first sweep + the inter-sweep gaps).
//!
//! VFS is deliberately NOT compared here: it is a phase-gradient sign and is
//! essentially invariant to the baseline (the baseline rescales amplitude, not
//! phase), so a VFS grid would look identical across methods and hide where they
//! actually differ. F0 and amplitude are the baseline-sensitive quantities.
//!
//! Run: cargo run --release -p isi-analysis --example baseline_compare -- <file.oisi>

use std::path::Path;

use hdf5::File;
use isi_analysis::compute::{
    dft_projection_at_freq, dff_denominator_floor, frames_u16_subset_to_dff_tensor,
    inter_sweep_baseline, temporal_mean_baseline, temporal_median_baseline, BaselineAggregate,
};
use ndarray::{Array2, Array3};

fn nearest_index(sorted: &[f64], target: f64) -> usize {
    let mut best = 0usize;
    let mut best_d = f64::INFINITY;
    for (i, &v) in sorted.iter().enumerate() {
        let d = (v - target).abs();
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    best
}

/// First-direction grouping uses the same label prefixes as the pipeline's
/// `classify_cycle_name` (we only need to know which sweeps share direction 0).
fn dir_key(name: &str) -> Option<&'static str> {
    let l = name.to_lowercase();
    for p in ["lr", "rl", "tb", "bt", "ccw", "cw", "expand", "contract"] {
        if l.starts_with(p) {
            return Some(match p {
                "ccw" => "rl",      // same Direction bucket as RL
                "cw" => "lr",       // same Direction bucket as LR
                other => leak(other),
            });
        }
    }
    None
}
fn leak(s: &str) -> &'static str {
    match s {
        "lr" => "lr",
        "rl" => "rl",
        "tb" => "tb",
        "bt" => "bt",
        "expand" => "expand",
        "contract" => "contract",
        _ => "?",
    }
}

/// F1 ΔF/F amplitude for one baseline: average the bin-1 complex map over the
/// first direction's cycles (simple complex mean), return `|·|` as a host array.
// Justified `#[allow]`: a self-contained DFT helper in an example binary; the
// args are the standard Allen DFT inputs (frames, baseline, onsets, timing,
// dims). Example code, not a production API surface.
#[allow(clippy::too_many_arguments)]
fn f1_amplitude(
    frames: &Array3<u16>,
    baseline: &Array2<f64>,
    cycle_onsets: &[usize],
    chunk_frame_dur: usize,
    dt: f64,
    freq: f64,
    h: usize,
    w: usize,
) -> Array2<f64> {
    let floor = dff_denominator_floor(baseline);
    let mut re_sum = Array2::<f64>::zeros((h, w));
    let mut im_sum = Array2::<f64>::zeros((h, w));
    let mut n_used = 0usize;
    for &onset in cycle_onsets {
        let indices: Vec<usize> = (onset..onset + chunk_frame_dur).collect();
        let dff = frames_u16_subset_to_dff_tensor(frames, &indices, baseline, floor);
        let cm = dft_projection_at_freq(dff, dt, freq);
        let re = isi_analysis::compute::tensor_to_array2_f64(cm.real()).unwrap();
        let im = isi_analysis::compute::tensor_to_array2_f64(cm.imag()).unwrap();
        re_sum += &re;
        im_sum += &im;
        n_used += 1;
    }
    let k = n_used.max(1) as f64;
    Array2::from_shape_fn((h, w), |(r, c)| {
        let re = re_sum[[r, c]] / k;
        let im = im_sum[[r, c]] / k;
        (re * re + im * im).sqrt()
    })
}

fn write_f64(path: &str, a: &Array2<f64>) {
    let mut bytes = Vec::with_capacity(a.len() * 8);
    for &v in a.iter() {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

fn main() {
    // The repeated Burn DFT passes build/drop large tensor graphs; give the
    // work a generous stack (the 1 MB default main-thread stack overflows on
    // the second baseline's amplitude pass).
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}

fn run() {
    let args: Vec<String> = std::env::args().collect();
    let path = Path::new(&args[1]);
    let f = File::open(path).expect("open oisi");

    let frames_ds = f.dataset("acquisition/camera/frames").unwrap();
    let shape = frames_ds.shape();
    let (h, w) = (shape[1], shape[2]);
    let frames: Array3<u16> = frames_ds.read().expect("read frames");
    let t_cam = shape[0];

    let ts: Vec<f64> = f
        .dataset("acquisition/camera/timestamps_sec")
        .unwrap()
        .read_1d::<f64>()
        .unwrap()
        .to_vec();
    let sweep_start: Vec<f64> = f
        .dataset("acquisition/schedule/sweep_start_sec")
        .unwrap()
        .read_1d::<f64>()
        .unwrap()
        .to_vec();
    let sweep_end: Vec<f64> = f
        .dataset("acquisition/schedule/sweep_end_sec")
        .unwrap()
        .read_1d::<f64>()
        .unwrap()
        .to_vec();
    let seq_json: hdf5::types::VarLenUnicode = f
        .group("acquisition/schedule")
        .unwrap()
        .attr("sweep_sequence")
        .unwrap()
        .read_scalar()
        .unwrap();
    let sweep_sequence: Vec<String> = serde_json::from_str(seq_json.as_str()).unwrap();

    let mean_frame_dur = (ts[t_cam - 1] - ts[0]) / (t_cam - 1) as f64;

    // First direction = the direction of sweep 0; collect its cycles' onsets.
    let n_sweeps = sweep_sequence
        .len()
        .min(sweep_start.len())
        .min(sweep_end.len());
    let dir0 = (0..n_sweeps)
        .find_map(|k| dir_key(&sweep_sequence[k]))
        .expect("no recognized direction labels");
    let dir_ks: Vec<usize> = (0..n_sweeps)
        .filter(|&k| dir_key(&sweep_sequence[k]) == Some(dir0))
        .collect();
    let chunk_dur: f64 = dir_ks
        .iter()
        .map(|&k| sweep_end[k] - sweep_start[k])
        .sum::<f64>()
        / dir_ks.len() as f64;
    let chunk_frame_dur = (chunk_dur / mean_frame_dur).ceil() as usize;
    let period = chunk_frame_dur as f64 * mean_frame_dur;
    let freq = 1.0 / period;
    let cycle_onsets: Vec<usize> = dir_ks
        .iter()
        .map(|&k| nearest_index(&ts, sweep_start[k]))
        .filter(|&o| o + chunk_frame_dur <= t_cam)
        .collect();

    // --- the four baselines ---
    let allen_mean = temporal_mean_baseline(&frames);
    let allen_median = temporal_median_baseline(&frames);
    let inter_mean = inter_sweep_baseline(
        &frames,
        &ts,
        &sweep_start,
        &sweep_end,
        BaselineAggregate::Mean,
    );
    let inter_median = inter_sweep_baseline(
        &frames,
        &ts,
        &sweep_start,
        &sweep_end,
        BaselineAggregate::Median,
    );

    let rest = isi_analysis::compute::rest_frame_indices(&ts, &sweep_start, &sweep_end);
    println!(
        "frames={t_cam}  rest_frames={} ({:.1}%)  dir0={dir0}  cycles={}  chunk_frames={chunk_frame_dur}",
        rest.len(),
        100.0 * rest.len() as f64 / t_cam as f64,
        cycle_onsets.len(),
    );

    let methods: Vec<(&str, Option<Array2<f64>>)> = vec![
        ("allen_mean", Some(allen_mean)),
        ("allen_median", Some(allen_median)),
        ("inter_mean", inter_mean),
        ("inter_median", inter_median),
    ];

    std::fs::create_dir_all("target/baseline_compare").unwrap();
    for (name, base) in &methods {
        let Some(base) = base else {
            println!("  {name}: no rest frames — skipped (would fall back to all-frame)");
            continue;
        };
        let amp = f1_amplitude(
            &frames,
            base,
            &cycle_onsets,
            chunk_frame_dur,
            mean_frame_dur,
            freq,
            h,
            w,
        );
        write_f64(&format!("target/baseline_compare/f0_{name}.bin"), base);
        write_f64(&format!("target/baseline_compare/amp_{name}.bin"), &amp);

        let f0_vals: Vec<f64> = base.iter().copied().filter(|v| v.is_finite()).collect();
        let f0_med = {
            let mut v = f0_vals.clone();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v[v.len() / 2]
        };
        let amp_mean = amp.iter().sum::<f64>() / amp.len() as f64;
        println!("  {name:14}  F0 median={f0_med:8.1}  mean |F1 dF/F|={amp_mean:.5}");
    }
    println!("  wrote target/baseline_compare/{{f0,amp}}_*.bin  ({h}x{w})");
}
