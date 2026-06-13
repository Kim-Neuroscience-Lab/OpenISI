//! Compute the Allen spectral power-SNR signal mask on a real recording and
//! apply it to the smoothed VFS, for the signal-quality comparison.
//!
//! Allen's criterion needs the per-pixel power spectrum of the stimulus movie
//! (not stored in the .oisi), so we read one stimulus cycle's raw frames
//! directly (the first sweep), demean per pixel (remove the DC bin so the
//! power@F1-vs-broadband test is meaningful — Allen runs on dF/F), and call the
//! oracle-validated `allen_spectral_power_snr_mask` (cycles = 1: one cycle per
//! sweep → F1 at FFT bin 1).
//!
//! Run: cargo run --release -p isi-analysis --example sq_allen_demo -- <file.oisi> [sigma]

use std::path::Path;

use hdf5::File;
use isi_analysis::io;
use isi_analysis::compute::responsiveness::allen_spectral_power_snr_mask;
use ndarray::{s, Array2, Array3};

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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = Path::new(&args[1]);
    let sigma: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1.0);

    let f = File::open(path).expect("open oisi");
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

    let t_cam = ts.len();
    let mean_frame_dur = (ts[t_cam - 1] - ts[0]) / (t_cam - 1) as f64;
    let chunk_dur = sweep_end[0] - sweep_start[0];
    let chunk = (chunk_dur / mean_frame_dur).ceil() as usize;
    let onset = nearest_index(&ts, sweep_start[0]);

    let frames_ds = f.dataset("acquisition/camera/frames").unwrap();
    let shape = frames_ds.shape();
    let (h, w) = (shape[1], shape[2]);
    // Read only this cycle's frames (hyperslab), not the whole movie.
    let raw: Array3<u16> = frames_ds
        .read_slice(s![onset..onset + chunk, .., ..])
        .expect("read frame slice");

    // Demean per pixel → f64 movie (kills the DC bin; Allen runs on dF/F).
    let n = chunk;
    let mut movie = Array3::<f64>::zeros((n, h, w));
    for r in 0..h {
        for c in 0..w {
            let mut mean = 0.0;
            for t in 0..n {
                mean += raw[[t, r, c]] as f64;
            }
            mean /= n as f64;
            for t in 0..n {
                movie[[t, r, c]] = raw[[t, r, c]] as f64 - mean;
            }
        }
    }

    let mask = allen_spectral_power_snr_mask(&movie, 1, sigma);
    let vfs = io::read_result_map(path, "vfs_smoothed").unwrap();
    let masked = Array2::from_shape_fn((h, w), |(r, c)| {
        if mask[[r, c]] {
            vfs[[r, c]]
        } else {
            f64::NAN
        }
    });

    std::fs::create_dir_all("target/sq_mask_demo").unwrap();
    let mut bytes = Vec::with_capacity(masked.len() * 8);
    for &v in masked.iter() {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write("target/sq_mask_demo/masked_allen.bin", bytes).unwrap();

    let kept = mask.iter().filter(|&&b| b).count() as f64 / mask.len() as f64;
    println!("onset_idx={onset} chunk_frames={chunk} sigma={sigma}");
    println!("  Allen power-SNR keeps {:.1}%", 100.0 * kept);
    println!("  wrote target/sq_mask_demo/masked_allen.bin");
}
