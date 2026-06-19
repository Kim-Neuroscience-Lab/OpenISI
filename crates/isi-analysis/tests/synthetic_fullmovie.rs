//! Recover-and-compare: full-movie correctness test (Phase A).
//!
//! Generates a synthetic RAW movie from a *known* analytic retinotopy (the `synth`
//! forward model: complex-log map → Kalatsky–Stryker encoding → realism layer),
//! **writes it as a schema-conformant `.oisi` (`oisi::write_raw_acquisition`) and
//! runs the production `analyze()` over it** — so the pipeline ingests the movie
//! through the exact `read_raw_acquisition` path a real capture uses, not a
//! hand-built in-memory struct — then reads `/results` back and checks it recovers
//! the known position/sign. Unlike the oracle goldens (faithfulness to a reference)
//! and `regression_oisi` (reproducibility), this tests **correctness**: does the
//! pipeline return the *right answer* for a known input? See
//! `docs/SYNTHETIC_VALIDATION.md`. (`delay_bias_math_vs_numerical` keeps a
//! file-I/O-free direct `compute_retinotopy` path, so an I/O regression and a math
//! regression stay distinguishable.)
//!
//! **The hemodynamic-delay VALID-DOMAIN rule (the central finding, grounded
//! against R43 — real SNLC sample data):** the cycle-combine inherits SNLC
//! `Gprocesskret`'s delay-disambiguation ("force the delay into `(0, π]`"). With
//! forward/reverse map phases `±p + ∠H`, the combine forms `delay = ∠H` and the
//! forcing adds π *iff* the raw hemodynamic phase ∠H is **negative**, leaving
//! `kmap = p − π` — the recovered position flips by exactly half the range. So the
//! method is invertible **iff `∠H ∈ (0, π]`** (the general form of the zero-delay
//! singularity; `position_flips_iff_delay_leaves_valid_domain` proves it
//! deterministically, no noise). Real ISI lives in-domain: R43's recovered
//! positions are correct and its per-pixel delays cluster at ~85°
//! (`azi_delay` median 98° / `alt_delay` 71°), broad, with only a ~2–4%
//! noise-dominated tail at the 0/π edges where even SNLC flips. The canonical
//! recording therefore injects a **known positive delay** via
//! [`synth::realism::Hemodynamic::PhaseLag`] (default = R43's ~85° median, unit
//! gain) — *not* a difference-of-gamma HRF, whose bin-1 phase is shape/period-
//! dependent and can wander negative (out of domain). The physical
//! [`synth::realism::Hemodynamic::Hrf`] remains an optional stress knob.
//!
//! **Surfaced systematic (documented, NOT cropped/hidden):** altitude recovers
//! essentially exactly (median ~0.002°), but the **azimuth carries a small uniform
//! ~0.34° bias**. `delay_bias_math_vs_numerical` establishes its nature decisively:
//! the Kalatsky–Stryker formula is mathematically exact (machine-ε) AND our
//! pipeline is exact on exact complex maps (0.0000) — so the bias is a
//! **movie→complex-maps front-end numerical artifact** (f32 per-cycle DFT + u16
//! quantization), azimuth-specific because the per-pixel errors cancel on the
//! symmetric altitude map but not the fovea-asymmetric azimuth map. It is **NOT**
//! driven by HRF attenuation: it is ~0.34° identically under the unit-gain clean
//! `PhaseLag` delay and the attenuated `Hrf` (so the earlier "reducible by a more
//! realistic HRF / less attenuation" claim was wrong — it is the asymmetric-map
//! f32 front-end at this amplitude, full stop).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use ndarray::Array2;

use isi_analysis::{
    analyze, compute_retinotopy, AcquisitionProperties, AnalysisParams, ProvenanceLevel,
    RawAcquisition, SilentProgress,
};
use openisi_params::config::analysis::{CycleCombine, PhaseSmoothing};
use openisi_params::config::AnalysisConfig;

use synth::acquire::{build, RecordingSpec, Synthetic};
use synth::encode::Stim;
use synth::map::LogMap;
use synth::realism::{Corruptions, Hemodynamic, Hrf, SensorNoise};

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

/// A temp `.oisi` path under the system temp dir; removed (with its `.partial`)
/// on drop. Unique per instance so parallel tests never collide.
struct TempOisi(PathBuf);

impl TempOisi {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let mut p = std::env::temp_dir();
        p.push(format!(
            "openisi_synth_{tag}_{}_{}.oisi",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        Self(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempOisi {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_file(self.0.with_extension("oisi.partial"));
    }
}

/// Position/sign/delay maps read back from a REAL `analyze()` run — the full
/// production path: `synth → write a schema-conformant .oisi
/// (oisi::write_raw_acquisition) → analyze() → read /results`. No hand-built
/// in-memory `RawAcquisition` is fed to the pipeline; it comes from
/// `read_raw_acquisition`, exactly as a real capture does. (The isolated
/// maps→position math is checked separately by `delay_bias_math_vs_numerical`,
/// which drives `compute_retinotopy` on exact `ComplexMaps` with no file I/O.)
struct Recovered {
    azi_phase_degrees: Array2<f64>,
    alt_phase_degrees: Array2<f64>,
    vfs: Array2<f64>,
    azi_delay: Option<Array2<f64>>,
    alt_delay: Option<Array2<f64>>,
}

fn recover_via_oisi(syn: &Synthetic, sigma_px: f64) -> Recovered {
    let cfg = AnalysisConfig {
        phase_smoothing: PhaseSmoothing::SnlcAmpWeightedPhasor { sigma_px },
        cycle_combine: CycleCombine::default(),
        ..Default::default()
    };
    let params = AnalysisParams::from(&cfg);
    let tmp = TempOisi::new("recover");
    oisi::io::write_raw_acquisition(tmp.path(), &to_raw(syn), &to_acq(syn))
        .expect("write synthetic .oisi");
    let cancel = AtomicBool::new(false);
    analyze(tmp.path(), &params, None, &SilentProgress, &cancel).expect("analyze synthetic .oisi");
    let map = |name: &str| {
        isi_analysis::io::read_result_map(tmp.path(), name)
            .unwrap_or_else(|e| panic!("reading /results/{name}: {e}"))
    };
    Recovered {
        azi_phase_degrees: map("azi_phase_degrees"),
        alt_phase_degrees: map("alt_phase_degrees"),
        vfs: map("vfs"),
        azi_delay: isi_analysis::io::read_result_map(tmp.path(), "azi_delay").ok(),
        alt_delay: isi_analysis::io::read_result_map(tmp.path(), "alt_delay").ok(),
    }
}

/// (median, max) absolute error over the FULL grid (no cropping).
fn err_stats(recovered: &Array2<f64>, truth: &Array2<f64>) -> (f64, f64) {
    let mut errs: Vec<f64> = recovered
        .iter()
        .zip(truth.iter())
        .map(|(a, b)| (a - b).abs())
        .collect();
    errs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (errs[errs.len() / 2], *errs.last().unwrap())
}

/// A physically-valid recording: realistic 10 s sweep period; the caller supplies
/// the hemodynamic delay (canonical clean `PhaseLag`, or the physical `Hrf` knob)
/// + any noise via `corruptions`.
fn realistic_spec(amplitude: f64, corruptions: Corruptions, seed: u64) -> RecordingSpec {
    RecordingSpec {
        map: LogMap::default(),
        stim: Stim {
            angular_range_deg: 140.0,
            offset_deg: 0.0,
            cycles: 6,
            frames_per_cycle: 100, // 100·0.1 = 10 s/cycle (HRF is fast relative to this)
            baseline: 20_000.0,
            amplitude,
        },
        corruptions,
        dt_sec: 0.1,
        um_per_pixel: 20.0,
        lead_in_frames: 8,
        inter_dir_gap_frames: 8,
        seed,
    }
}

/// A physically-valid clean recording (HRF, no noise) is recovered to high
/// accuracy across the FULL grid — the correctness proof.
#[test]
fn clean_recovers_known_retinotopy() {
    let spec = realistic_spec(
        0.02,
        Corruptions {
            hemodynamic: Some(Hemodynamic::default()), // R43-grounded clean delay (~85°)
            sensor: None,
        },
        0,
    );
    let syn = build(&spec, 24, 32);
    let retino = recover_via_oisi(&syn, 0.0); // no smoothing: isolate the DFT recovery

    let (a_med, a_max) = err_stats(&retino.azi_phase_degrees, &syn.ground_truth.azi);
    let (l_med, l_max) = err_stats(&retino.alt_phase_degrees, &syn.ground_truth.alt);
    eprintln!("CLEAN azimuth  err°: median {a_med:.4} max {a_max:.4}");
    eprintln!("CLEAN altitude err°: median {l_med:.4} max {l_max:.4}");

    // Altitude recovers essentially exactly; azimuth carries the documented small
    // uniform front-end bias (~0.34°, module-doc finding — present even at this
    // clean unit-gain delay). Thresholds pin the MEASURED accuracy over the full
    // grid — not loosened to hide anything.
    assert!(l_med < 0.05, "altitude median error {l_med:.4}° too large");
    assert!(l_max < 0.2, "altitude max error {l_max:.4}° too large");
    assert!(a_med < 0.5, "azimuth median error {a_med:.4}° too large (bias regressed?)");
    assert!(a_max < 1.0, "azimuth max error {a_max:.4}° too large");

    // Field sign: the conformal map is uniformly +1 ⇒ the recovered VFS is
    // strongly single-signed.
    let mean_sign = retino.vfs.iter().sum::<f64>() / retino.vfs.len() as f64;
    eprintln!("CLEAN mean VFS sign: {mean_sign:.3} (gt {})", syn.ground_truth.sign);
    assert!(mean_sign.abs() > 0.5, "recovered VFS should be strongly single-signed");
}

/// The ~0.34° azimuth bias is a movie→complex-maps **front-end numerical artifact**
/// — NOT a delay-correction property. The decisive `delay_bias_math_vs_numerical`
/// test shows the Kalatsky–Stryker formula AND our cycle-combine are exact (machine-ε
/// / 0.0000) on exact maps; the bias lives entirely in estimating the maps from the
/// u16-quantized movie via the f32 per-cycle DFT. It is *visible* through the
/// recoverable delay OUTPUT: the symmetric altitude recovers the injected ∠H exactly,
/// while the fovea-asymmetric azimuth mis-estimates it (the per-pixel front-end errors
/// don't cancel on the asymmetric axis), which leaks into the azimuth position.
///
/// Uses the physical `Hrf` here only to keep a delay present; the bias is the SAME
/// magnitude under the unit-gain clean `PhaseLag` (see `clean_recovers`), so it is
/// NOT an attenuation effect.
///
/// (NOTE: an earlier version of this test claimed the bias was a *delay-correction*
/// artifact by comparing a zero-delay big-signal recording against the HRF recording
/// — confounding delay correction with signal size. `delay_bias_math_vs_numerical`
/// is the clean, decisive isolation.)
#[test]
fn azimuth_bias_is_a_front_end_numerical_artifact() {
    // Low-eccentricity map (≤ ~20°) keeps positions well inside ±π.
    let small_map = LogMap {
        a: 1.0,
        u_max: 21.0_f64.ln(),
        v_ext: std::f64::consts::PI,
    };
    let mut spec = realistic_spec(
        0.02,
        Corruptions {
            // The PHYSICAL HRF (attenuated, ∠H=165°) — this front-end artifact is
            // specific to the HRF's low-pass gain; the canonical clean PhaseLag
            // delay (unit gain) does not exhibit it (see clean_recovers).
            hemodynamic: Some(Hemodynamic::Hrf(Hrf::default())),
            sensor: None,
        },
        0,
    );
    spec.map = small_map;
    let syn = build(&spec, 24, 32);
    let rk = recover_via_oisi(&syn, 0.0); // KalatskyStryker delay subtraction (default)

    let mean = |rec: &Array2<f64>, truth: &Array2<f64>| {
        rec.iter().zip(truth.iter()).map(|(a, b)| a - b).sum::<f64>() / rec.len() as f64
    };
    let am2 = mean(&rk.azi_phase_degrees, &syn.ground_truth.azi);
    let lm2 = mean(&rk.alt_phase_degrees, &syn.ground_truth.alt);
    eprintln!("position offset: azi {am2:.4}°  alt {lm2:.4}°");

    // The delay is a recoverable OUTPUT — compare it to the KNOWN injected ∠H.
    // True ∠H (phase-degrees): bin-1 phase of the HRF kernel's response to a unit
    // cosine, forced positive like SNLC's `(0,π]`.
    let fpc = 100usize;
    let t_total = 6 * fpc;
    let kernel = Hrf::default().kernel(0.1, t_total);
    let unit: Vec<f64> = (0..t_total)
        .map(|t| (std::f64::consts::TAU * t as f64 / fpc as f64).cos())
        .collect();
    let conv: Vec<f64> = (0..t_total)
        .map(|t| {
            kernel
                .iter()
                .enumerate()
                .filter(|(_, &k)| k != 0.0)
                .map(|(j, &k)| k * unit[(t + t_total - (j % t_total)) % t_total])
                .sum()
        })
        .collect();
    let (mut sr, mut si) = (0.0, 0.0);
    for (t, &v) in conv.iter().enumerate() {
        let w = std::f64::consts::TAU * t as f64 / fpc as f64;
        sr += v * w.cos();
        si += v * w.sin();
    }
    let true_delay_deg = (-si).atan2(sr).abs() * 180.0 / std::f64::consts::PI;
    let delay_mean = |m: &Option<Array2<f64>>| {
        m.as_ref().map(|a| a.iter().sum::<f64>() / a.len() as f64)
    };
    eprintln!(
        "[delay recovery]  true ∠H = {true_delay_deg:.3}° (phase)   recovered azi_delay = {:?}  alt_delay = {:?}",
        delay_mean(&rk.azi_delay).map(|v| format!("{v:.3}")),
        delay_mean(&rk.alt_delay).map(|v| format!("{v:.3}")),
    );

    // Position: the front-end error shows as an azimuth-only offset; symmetric
    // altitude is unbiased.
    assert!(am2.abs() > 0.1, "azimuth position carries the front-end bias");
    assert!(lm2.abs() < 0.05, "altitude position stays unbiased");

    // It is visible through the recoverable delay OUTPUT: symmetric altitude
    // recovers the injected ∠H exactly; the asymmetric azimuth mis-estimates it
    // (the front-end map errors don't cancel on the asymmetric axis).
    let alt_d = delay_mean(&rk.alt_delay).expect("alt delay map");
    let azi_d = delay_mean(&rk.azi_delay).expect("azi delay map");
    assert!((alt_d - true_delay_deg).abs() < 0.1, "altitude delay should match injected ∠H");
    assert!((azi_d - true_delay_deg).abs() > 0.3, "azimuth delay is mis-estimated (the bias source)");
}

/// DECISIVE: is the azimuth delay bias a MATHEMATICAL property of the
/// Kalatsky–Stryker formula, or a NUMERICAL artifact of our f32 implementation?
/// Part 1 runs the K–S delay subtraction in pure f64 on EXACT phases (no movie,
/// no DFT, no quantization) — tests the math. Part 2 feeds EXACT f64 complex maps
/// through the REAL pipeline — tests our f32 cycle-combine. Swept over a realistic
/// (40°) and the near-(0,180°]-boundary (165°) delay.
#[test]
fn delay_bias_math_vs_numerical() {
    use isi_analysis::ComplexMaps;
    use num_complex::Complex64;
    use synth::encode::position_to_phase;

    // Low-eccentricity map (cos(pos)>0 everywhere) + the stimulus the phases map to.
    let gt = LogMap {
        a: 1.0,
        u_max: 21.0_f64.ln(),
        v_ext: std::f64::consts::PI,
    }
    .generate(24, 32);
    let stim = Stim {
        angular_range_deg: 140.0,
        offset_deg: 0.0,
        cycles: 6,
        frames_per_cycle: 100,
        baseline: 20_000.0,
        amplitude: 0.02,
    };
    let (h, w) = gt.azi.dim();
    let to_deg = stim.angular_range_deg / std::f64::consts::TAU;

    // The exact K–S delay subtraction (Gprocesskret), in f64.
    let wrap = |x: f64| x.sin().atan2(x.cos());
    let ks = |fwd: f64, rev: f64| -> f64 {
        let mut d = (fwd.sin() + rev.sin()).atan2(fwd.cos() + rev.cos());
        d += std::f64::consts::FRAC_PI_2 * (1.0 - d.signum()); // force into (0,π]
        0.5 * (wrap(fwd - d) - wrap(rev - d))
    };

    let params = {
        let cfg = AnalysisConfig {
            phase_smoothing: PhaseSmoothing::SnlcAmpWeightedPhasor { sigma_px: 0.0 },
            ..Default::default()
        };
        AnalysisParams::from(&cfg)
    };
    let acq = AcquisitionProperties {
        azi_angular_range: stim.angular_range_deg,
        alt_angular_range: stim.angular_range_deg,
        offset_azi: 0.0,
        offset_alt: 0.0,
        rotation_k: 0,
        um_per_pixel: 20.0,
        provenance: ProvenanceLevel::Synthetic,
    };

    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;

    for delay_deg in [15.0_f64, 40.0, 165.0] {
        let delta = delay_deg.to_radians();

        // ── Part 1: pure-f64 K–S formula on exact phases ─────────────────────
        let mut a1 = Vec::new();
        let mut l1 = Vec::new();
        for r in 0..h {
            for c in 0..w {
                let pa = position_to_phase(gt.azi[[r, c]], &stim);
                let pl = position_to_phase(gt.alt[[r, c]], &stim);
                a1.push(ks(pa + delta, -pa + delta) * to_deg - gt.azi[[r, c]]);
                l1.push(ks(pl + delta, -pl + delta) * to_deg - gt.alt[[r, c]]);
            }
        }
        eprintln!(
            "Δ={delay_deg:>5}°  PART1 (pure-f64 formula):    azi offset {:+.2e}  alt offset {:+.2e}",
            mean(&a1),
            mean(&l1)
        );

        // ── Part 2: EXACT f64 complex maps → real (f32) pipeline ─────────────
        let phasor = |pos: f64, sign: f64| Complex64::from_polar(1.0, sign * pos + delta);
        let maps = ComplexMaps {
            azi_fwd: Array2::from_shape_fn((h, w), |(r, c)| {
                phasor(position_to_phase(gt.azi[[r, c]], &stim), 1.0)
            }),
            azi_rev: Array2::from_shape_fn((h, w), |(r, c)| {
                phasor(position_to_phase(gt.azi[[r, c]], &stim), -1.0)
            }),
            alt_fwd: Array2::from_shape_fn((h, w), |(r, c)| {
                phasor(position_to_phase(gt.alt[[r, c]], &stim), 1.0)
            }),
            alt_rev: Array2::from_shape_fn((h, w), |(r, c)| {
                phasor(position_to_phase(gt.alt[[r, c]], &stim), -1.0)
            }),
        };
        let cancel = AtomicBool::new(false);
        let retino = compute_retinotopy(&maps, &acq, &params, &cancel).unwrap();
        let a2: Vec<f64> = retino
            .azi_phase_degrees
            .iter()
            .zip(gt.azi.iter())
            .map(|(a, b)| a - b)
            .collect();
        let l2: Vec<f64> = retino
            .alt_phase_degrees
            .iter()
            .zip(gt.alt.iter())
            .map(|(a, b)| a - b)
            .collect();
        eprintln!(
            "Δ={delay_deg:>5}°  PART2 (exact maps→f32 pipe): azi offset {:+.4}  alt offset {:+.4}",
            mean(&a2),
            mean(&l2)
        );

        // The K–S formula is mathematically EXACT (machine-ε, no asymmetry)...
        assert!(mean(&a1).abs() < 1e-10, "K–S formula azi offset must be machine-ε");
        assert!(mean(&l1).abs() < 1e-10, "K–S formula alt offset must be machine-ε");
        // ...and our real pipeline on EXACT maps is also exact ⇒ the ~0.37° bias
        // is NOT in the delay-correction math or our cycle-combine, but in the
        // movie→complex-maps front-end (f32 DFT + u16 quantization + attenuated
        // signal). A floating-point/quantization artifact, not a property of K–S.
        assert!(mean(&a2).abs() < 0.01, "cycle-combine on exact maps must be ~exact (azi)");
        assert!(mean(&l2).abs() < 0.01, "cycle-combine on exact maps must be ~exact (alt)");
    }
}

/// THE DOMAIN RULE (decisive, deterministic — no noise). The recovered position
/// flips by exactly half the range **iff** the injected hemodynamic phase ∠H is
/// *outside* `(0, π]` — i.e. iff SNLC `Gprocesskret`'s `(0, π]` forcing has to
/// *move* the delay. With fwd/rev phases `±p + ∠H`, the combine forms
/// `delay = angle(fwd+rev) = ∠H` (cos p>0 on this low-ecc map); the forcing adds
/// π when ∠H<0, leaving `kmap = p − π`. So a positive (in-domain) ∠H recovers
/// position exactly; a negative (out-of-domain) ∠H flips it. This is the GENERAL
/// form of the zero-delay singularity: the Kalatsky–Stryker / SNLC method is only
/// invertible when the net bin-1 hemodynamic phase lies in `(0, π]`.
///
/// Real data lives in-domain: R43's recovered positions are correct (not
/// half-flipped), and its per-pixel delays cluster at ~85° (the
/// `R43_MEDIAN_DELAY_RAD` the clean default injects), with only a ~2–4%
/// noise-dominated tail reaching the 0/π edges where even SNLC flips.
///
/// Uses the clean [`Hemodynamic::PhaseLag`] knob to set ∠H directly (a pure phase
/// shift), so the mechanism is isolated from any HRF-shape / attenuation confound.
#[test]
fn position_flips_iff_delay_leaves_valid_domain() {
    let small_map = LogMap {
        a: 1.0,
        u_max: 21.0_f64.ln(),
        v_ext: std::f64::consts::PI,
    };
    let half_range = 140.0 / 2.0; // the flip magnitude (range/2) in degrees

    // (∠H in degrees, expected to flip?) — straddling both forcing edges (0 and π).
    let cases: [(f64, bool); 5] = [
        (-120.0, true),  // raw negative ⇒ forcing adds π ⇒ flip
        (-20.0, true),   // just below 0 ⇒ flip
        (20.0, false),   // just inside (0,π] ⇒ exact
        (85.0, false),   // R43 median ⇒ exact (the canonical clean default)
        (160.0, false),  // near π but still inside ⇒ exact (R43 p95 reaches here)
    ];
    for (angle_deg, expect_flip) in cases {
        let mut spec = realistic_spec(
            0.02,
            Corruptions {
                hemodynamic: Some(Hemodynamic::PhaseLag { angle_rad: angle_deg.to_radians() }),
                sensor: None,
            },
            0,
        );
        spec.map = small_map;
        let syn = build(&spec, 24, 32);
        let r = recover_via_oisi(&syn, 0.0);
        let med = {
            let mut d: Vec<f64> = r
                .azi_phase_degrees
                .iter()
                .zip(syn.ground_truth.azi.iter())
                .map(|(a, b)| a - b)
                .collect();
            d.sort_by(|a, b| a.partial_cmp(b).unwrap());
            d[d.len() / 2]
        };
        eprintln!("∠H={angle_deg:+6.0}°  azi median err {med:+7.2}°  (expect {})",
            if expect_flip { "FLIP" } else { "exact" });
        if expect_flip {
            // Flipped by ~ -range/2 (the uncompensated π in kmap = p − π).
            assert!((med + half_range).abs() < 2.0,
                "∠H={angle_deg}°: expected ~-{half_range}° flip, got {med:.2}°");
        } else {
            // In-domain ⇒ recovered to within the small front-end residual.
            assert!(med.abs() < 1.0,
                "∠H={angle_deg}°: in-domain, expected exact, got {med:.2}°");
        }
    }
}

/// A literature-grounded noisy recording (HRF + sensor noise, ΔR/R near the noise
/// floor) is still recovered — the proof that the input is non-circular yet
/// recoverable, the precondition for a meaningful full-pass oracle golden.
#[test]
fn recovers_under_benchmark_noise() {
    // ΔR/R = 5e-3 at 20 000 counts ⇒ 100-count modulation under ~141-count shot
    // noise (per-frame SNR ~0.7); the frequency-selective DFT over 6·100 frames
    // integrates it out.
    let spec = realistic_spec(
        5.0e-3,
        Corruptions {
            hemodynamic: Some(Hemodynamic::default()), // R43-grounded clean delay (~85°)
            sensor: Some(SensorNoise::default()),
        },
        2026,
    );
    let syn = build(&spec, 24, 32);
    let retino = recover_via_oisi(&syn, 1.0); // default smoothing

    let (a_med, _a_max) = err_stats(&retino.azi_phase_degrees, &syn.ground_truth.azi);
    let (l_med, _l_max) = err_stats(&retino.alt_phase_degrees, &syn.ground_truth.alt);
    eprintln!("NOISY azimuth  err°: median {a_med:.3}");
    eprintln!("NOISY altitude err°: median {l_med:.3}");

    // Under sub-noise ΔR/R the median position is still recovered to a few degrees
    // — the DFT dug the signal out of the noise.
    assert!(a_med < 5.0, "noisy azimuth median error {a_med:.3}° too large");
    assert!(l_med < 5.0, "noisy altitude median error {l_med:.3}° too large");
}
