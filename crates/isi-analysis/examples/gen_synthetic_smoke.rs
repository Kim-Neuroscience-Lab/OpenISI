//! Generate the committed synthetic smoke `.oisi` fixture used by the e2e
//! pipeline tests (`equivalence`, `incremental`, `oisi_schema_contract`).
//!
//! Unlike the gitignored real R43 data, this fixture is SYNTHETIC, tiny, and
//! deterministic (`RecordingSpec::clean_smoke`, fixed seed) — so it is committed
//! and the pipeline e2e gates run everywhere, including CI. Regenerate with:
//!
//! ```text
//! cargo run -p isi-analysis --example gen_synthetic_smoke -- \
//!     crates/isi-analysis/tests/fixtures/synthetic/smoke.oisi
//! cargo run -p isi-analysis --example capture_baseline -- \
//!     crates/isi-analysis/tests/fixtures/synthetic/smoke.oisi \
//!     crates/isi-analysis/tests/fixtures/synthetic/smoke.baseline.oisi
//! ```
//!
//! The generator writes ONLY the raw acquisition (camera frames + schedule +
//! geometry) via `oisi::write_raw_acquisition`, exactly as a real capture does;
//! `capture_baseline` then runs the production `analyze()` to produce the
//! comparison baseline. Determinism: synth uses a seeded ChaCha RNG, so the same
//! code + seed yields byte-identical frames on any machine.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use ndarray::Array2;

use isi_analysis::{
    analyze, AcquisitionProperties, AnalysisParams, ProvenanceLevel, RawAcquisition, SilentProgress,
};
use synth::acquire::{build, RecordingSpec, Synthetic};
use synth::encode::Stim;
use synth::map::LogMap;
use synth::realism::{Corruptions, Hemodynamic};

// The committed smoke fixture is intentionally small (a clean, knobs-off
// recording over a 24×32 grid) so the e2e gates stay fast and the file stays
// in the low-100s-of-KB range.
const H: usize = 24;
const W: usize = 32;

fn to_raw(syn: &Synthetic) -> RawAcquisition {
    RawAcquisition {
        frames: syn.frames.clone(),
        cam_ts_sec: syn.cam_ts_sec.clone(),
        sweep_start_sec: syn.sweep_start_sec.clone(),
        sweep_end_sec: syn.sweep_end_sec.clone(),
        sweep_sequence: syn.sweep_sequence.clone(),
    }
}

fn to_acq(syn: &Synthetic) -> AcquisitionProperties {
    AcquisitionProperties {
        azi_angular_range: syn.geom.azi_range_deg,
        alt_angular_range: syn.geom.alt_range_deg,
        offset_azi: syn.geom.offset_azi_deg,
        offset_alt: syn.geom.offset_alt_deg,
        rotation_k: 0,
        um_per_pixel: syn.geom.um_per_pixel,
        provenance: ProvenanceLevel::Synthetic,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: gen_synthetic_smoke <out.oisi>");
        std::process::exit(2);
    }
    let out = Path::new(&args[1]);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("create fixture dir");
    }

    // A genuinely CLEAN, high-SNR recording — NOT `RecordingSpec::clean_smoke()`,
    // whose `Corruptions::default()` turns sensor noise ON and uses a low baseline
    // (1000), drowning the tiny ΔR/R at this small grid (verified: degenerate
    // recovery). This mirrors `synthetic_fullmovie::clean_recovers`: baseline
    // 20_000 (modulation well above u16 quantization), the canonical in-domain
    // PhaseLag hemodynamic delay (~85°), and NO sensor noise — so the pipeline
    // recovers the analytic ground truth tightly and the pinned baseline is correct.
    let spec = RecordingSpec {
        map: LogMap::default(),
        stim: Stim {
            angular_range_deg: 140.0,
            offset_deg: 0.0,
            cycles: 6,
            frames_per_cycle: 40,
            baseline: 20_000.0,
            amplitude: 0.02,
        },
        corruptions: Corruptions {
            hemodynamic: Some(Hemodynamic::default()), // in-domain (0, π] delay
            sensor: None,                              // clean: no noise floor
        },
        dt_sec: 0.1,
        um_per_pixel: 20.0,
        lead_in_frames: 8,
        inter_dir_gap_frames: 8,
        seed: 0,
    };
    let syn = build(&spec, H, W);
    let nframes = syn.frames.shape()[0];

    oisi::io::write_raw_acquisition(out, &to_raw(&syn), &to_acq(&syn))
        .expect("write synthetic smoke .oisi");

    let bytes = out.metadata().map(|m| m.len()).unwrap_or(0);
    println!(
        "[gen_synthetic_smoke] wrote {} ({} frames {H}×{W}, {:.0} KB, seed={})",
        out.display(),
        nframes,
        bytes as f64 / 1024.0,
        spec.seed,
    );

    // CORRECTNESS gate: a regression baseline is only meaningful if the pipeline
    // recovers the KNOWN synthetic retinotopy from this fixture. Analyze a copy
    // and compare the recovered azimuth/altitude against the analytic ground truth
    // (`clean_smoke` is a clean recording, so recovery should be tight). If this
    // fails, the fixture is wrong and must NOT be pinned as a baseline.
    verify_recovers_ground_truth(out, &syn);
    println!("[gen_synthetic_smoke] correctness verified — fixture recovers ground truth");
}

/// (median, max) absolute error over the full grid (no cropping).
fn err_stats(recovered: &Array2<f64>, truth: &Array2<f64>) -> (f64, f64) {
    let mut errs: Vec<f64> = recovered
        .iter()
        .zip(truth.iter())
        .map(|(a, b)| (a - b).abs())
        .collect();
    errs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (errs[errs.len() / 2], *errs.last().unwrap())
}

fn verify_recovers_ground_truth(fixture: &Path, syn: &Synthetic) {
    // Analyze a temp COPY so the committed fixture stays raw.
    let tmp = std::env::temp_dir().join(format!("smoke_verify_{}.oisi", std::process::id()));
    std::fs::copy(fixture, &tmp).expect("copy fixture for verification");
    let params = AnalysisParams::from(&openisi_params::config::AnalysisConfig::default());
    let cancel = AtomicBool::new(false);
    analyze(&tmp, &params, None, &SilentProgress, &cancel).expect("analyze synthetic smoke fixture");

    let read = |name: &str| {
        isi_analysis::io::read_result_map(&tmp, name)
            .unwrap_or_else(|e| panic!("read /results/{name}: {e}"))
    };
    let azi = read("azi_phase_degrees");
    let alt = read("alt_phase_degrees");
    let vfs = read("vfs");
    let _ = std::fs::remove_file(&tmp);

    // No NaN/degenerate maps.
    assert!(azi.iter().all(|v| v.is_finite()), "azi_phase has non-finite values");
    assert!(alt.iter().all(|v| v.is_finite()), "alt_phase has non-finite values");

    let (a_med, a_max) = err_stats(&azi, &syn.ground_truth.azi);
    let (l_med, l_max) = err_stats(&alt, &syn.ground_truth.alt);
    let mean_sign = vfs.iter().sum::<f64>() / vfs.len() as f64;
    println!("[gen_synthetic_smoke] recovery vs ground truth:");
    println!("    azimuth  err°: median {a_med:.4}  max {a_max:.4}");
    println!("    altitude err°: median {l_med:.4}  max {l_max:.4}");
    println!("    mean VFS sign: {mean_sign:.3}  (ground truth {})", syn.ground_truth.sign);

    // Clean recording → tight recovery (same envelope as synthetic_fullmovie's
    // clean_recovers, with headroom for the smaller grid/period). Azimuth carries
    // the documented ~0.34° front-end bias; altitude is near-exact.
    assert!(l_med < 0.1, "altitude median error {l_med:.4}° too large — fixture not recovering");
    assert!(a_med < 1.0, "azimuth median error {a_med:.4}° too large — fixture not recovering");
    assert!(
        mean_sign.abs() > 0.5,
        "recovered VFS not strongly single-signed ({mean_sign:.3}) — fixture degenerate"
    );
}
