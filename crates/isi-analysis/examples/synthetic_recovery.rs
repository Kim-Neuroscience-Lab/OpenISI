//! Dump the synthetic-ground-truth recovery for visualization: forward-model a
//! KNOWN mirror-pair retinotopy (tent azimuth, monotonic altitude), encode it
//! into the four complex maps with a hemodynamic delay, run the REAL pipeline
//! (`compute_retinotopy`), and write the known-truth and recovered maps to raw
//! f64 bins. A companion Python script renders the side-by-side figure.
//!
//! Run: cargo run -p isi-analysis --example synthetic_recovery

use std::f64::consts::PI;
use std::path::Path;

use ndarray::Array2;
use num_complex::Complex64;

use isi_analysis::{AcquisitionProperties, ComplexMaps, ProvenanceLevel};

const H: usize = 128;
const W: usize = 128;

fn pa(_r: usize, c: usize) -> f64 {
    let xmid = (W as f64 - 1.0) / 2.0;
    let slope = 1.6 / xmid;
    slope * (xmid - (c as f64 - xmid).abs()) // tent: ∂/∂x flips sign at midline
}
fn pl(r: usize, _c: usize) -> f64 {
    let ymid = (H as f64 - 1.0) / 2.0;
    let slope = 1.4 / ymid;
    slope * (r as f64 - ymid) // monotonic in y
}

fn write_f64(path: &str, a: &Array2<f64>) {
    let mut bytes = Vec::with_capacity(a.len() * 8);
    for &v in a.iter() {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

fn main() {
    let delay = PI / 4.0;
    let enc = |sign: f64, pos: fn(usize, usize) -> f64| {
        Array2::from_shape_fn((H, W), |(r, c)| {
            Complex64::from_polar(1.0, sign * pos(r, c) + delay)
        })
    };
    let maps = ComplexMaps {
        azi_fwd: enc(1.0, pa),
        azi_rev: enc(-1.0, pa),
        alt_fwd: enc(1.0, pl),
        alt_rev: enc(-1.0, pl),
    };

    let acq = AcquisitionProperties {
        azi_angular_range: 120.0,
        alt_angular_range: 110.0,
        offset_azi: 0.0,
        offset_alt: 0.0,
        rotation_k: 0,
        um_per_pixel: 10.0,
        provenance: ProvenanceLevel::Full,
    };
    let here = Path::new(".");
    let snap = openisi_params::Registry::new(here, here).snapshot();
    let params = isi_analysis::bridge::analysis_params_from_snapshot(&snap);

    let never_cancel = std::sync::atomic::AtomicBool::new(false);
    let retino = isi_analysis::compute_retinotopy(&maps, &acq, &params, &never_cancel)
        .expect("compute_retinotopy");

    // Known truth.
    let known_azi = Array2::from_shape_fn((H, W), |(r, c)| pa(r, c));
    // Known VFS: +1 left of midline, −1 right (the mirror pair).
    let xmid = (W as f64 - 1.0) / 2.0;
    let known_vfs =
        Array2::from_shape_fn((H, W), |(_r, c)| if (c as f64) < xmid { 1.0 } else { -1.0 });

    std::fs::create_dir_all("target/synthetic_recovery").unwrap();
    write_f64("target/synthetic_recovery/known_azi.bin", &known_azi);
    write_f64("target/synthetic_recovery/known_vfs.bin", &known_vfs);
    write_f64("target/synthetic_recovery/recovered_azi.bin", &retino.azi_phase);
    write_f64("target/synthetic_recovery/recovered_vfs.bin", &retino.vfs);

    // Report the recovery error (the proof, mirroring the test).
    let margin = 4usize;
    let mut max_azi_err = 0.0f64;
    let (mut checked, mut correct) = (0usize, 0usize);
    for r in margin..H - margin {
        for c in margin..W - margin {
            if (c as f64 - xmid).abs() < margin as f64 {
                continue;
            }
            let mut d = retino.azi_phase[[r, c]] - pa(r, c);
            while d > PI {
                d -= 2.0 * PI;
            }
            while d <= -PI {
                d += 2.0 * PI;
            }
            max_azi_err = max_azi_err.max(d.abs());
            let expected = if (c as f64) < xmid { 1.0 } else { -1.0 };
            checked += 1;
            if retino.vfs[[r, c]].signum() == expected && retino.vfs[[r, c]].abs() > 0.5 {
                correct += 1;
            }
        }
    }
    println!("H={H} W={W}");
    println!("  max azimuth phase recovery error = {max_azi_err:.3e} rad");
    println!("  VFS sign recovered correctly: {correct}/{checked}");
    println!("  wrote target/synthetic_recovery/*.bin");
}
