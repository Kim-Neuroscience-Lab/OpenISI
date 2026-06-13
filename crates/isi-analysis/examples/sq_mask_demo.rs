//! Prototype: apply each signal-quality mask to a recording's smoothed VFS and
//! dump the masked maps for visual comparison. The masks use the
//! oracle-validated metrics already stored in the analyzed `.oisi`:
//! cross-cycle reliability, multi-bin SNR, F1 amplitude. (Allen spectral
//! power-SNR needs the raw-movie power spectrum — not stored — so it is not
//! shown here; that arrives with the DFT-path integration.)
//!
//! To compare fairly, SNR and amplitude are thresholded to keep the SAME
//! fraction of pixels as reliability @ 0.85, so the figure shows *where* each
//! metric concentrates, not merely how much it keeps.
//!
//! Run: cargo run -p isi-analysis --example sq_mask_demo -- <file.oisi>

use std::path::Path;

use isi_analysis::io;
use ndarray::Array2;

fn read(path: &Path, name: &str) -> Array2<f64> {
    io::read_result_map(path, name).unwrap_or_else(|e| panic!("read {name}: {e}"))
}

/// Elementwise min over maps (the per-pixel weakest direction governs).
fn min_of(maps: &[&Array2<f64>]) -> Array2<f64> {
    let (h, w) = maps[0].dim();
    Array2::from_shape_fn((h, w), |(r, c)| {
        maps.iter().fold(f64::INFINITY, |m, a| m.min(a[[r, c]]))
    })
}

/// Threshold value such that exactly `keep_frac` of finite pixels are `>=` it.
fn quantile_threshold(metric: &Array2<f64>, keep_frac: f64) -> f64 {
    let mut v: Vec<f64> = metric.iter().copied().filter(|x| x.is_finite()).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = (((1.0 - keep_frac) * v.len() as f64).floor() as usize).min(v.len() - 1);
    v[idx]
}

fn apply_mask(vfs: &Array2<f64>, mask: &Array2<bool>) -> Array2<f64> {
    let (h, w) = vfs.dim();
    Array2::from_shape_fn((h, w), |(r, c)| {
        if mask[[r, c]] {
            vfs[[r, c]]
        } else {
            f64::NAN
        }
    })
}

fn write_f64(path: &str, a: &Array2<f64>) {
    let mut bytes = Vec::with_capacity(a.len() * 8);
    for &v in a.iter() {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

fn frac(mask: &Array2<bool>) -> f64 {
    mask.iter().filter(|&&b| b).count() as f64 / mask.len() as f64
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = Path::new(&args[1]);

    let vfs = read(path, "vfs_smoothed");

    // Metrics (oracle-validated): cross-cycle reliability (min over 4
    // directions), multi-bin SNR (min over orientations), F1 amplitude (min
    // over orientations).
    let (raf, rar, ralf, ralr) = (
        read(path, "reliability_azi_fwd"),
        read(path, "reliability_azi_rev"),
        read(path, "reliability_alt_fwd"),
        read(path, "reliability_alt_rev"),
    );
    let reliability = min_of(&[&raf, &rar, &ralf, &ralr]);
    let snr = min_of(&[&read(path, "spectral_snr_azi"), &read(path, "spectral_snr_alt")]);
    let amp = min_of(&[&read(path, "azi_amplitude"), &read(path, "alt_amplitude")]);

    // Reliability mask at the value we found isolates the aperture.
    let rel_mask = reliability.mapv(|v| v.is_finite() && v >= 0.85);
    let keep = frac(&rel_mask);

    // SNR / amplitude thresholded to the SAME kept fraction for a fair compare.
    let snr_thr = quantile_threshold(&snr, keep);
    let amp_thr = quantile_threshold(&amp, keep);
    let snr_mask = snr.mapv(|v| v.is_finite() && v >= snr_thr);
    let amp_mask = amp.mapv(|v| v.is_finite() && v >= amp_thr);

    std::fs::create_dir_all("target/sq_mask_demo").unwrap();
    write_f64("target/sq_mask_demo/vfs_smoothed.bin", &vfs);
    write_f64("target/sq_mask_demo/masked_reliability.bin", &apply_mask(&vfs, &rel_mask));
    write_f64("target/sq_mask_demo/masked_snr.bin", &apply_mask(&vfs, &snr_mask));
    write_f64("target/sq_mask_demo/masked_amplitude.bin", &apply_mask(&vfs, &amp_mask));

    let (h, w) = vfs.dim();
    println!("dims {h}x{w}");
    println!("  reliability >= 0.85    keeps {:.1}%   thr=0.85", 100.0 * keep);
    println!("  snr (matched fraction) keeps {:.1}%   thr={:.4}", 100.0 * frac(&snr_mask), snr_thr);
    println!("  amplitude (matched)    keeps {:.1}%   thr={:.4}", 100.0 * frac(&amp_mask), amp_thr);
    println!("  wrote target/sq_mask_demo/*.bin");
}
