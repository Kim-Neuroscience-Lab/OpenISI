//! Dump the **state of the oracle / regression cross-validation** for two paths.
//!
//! Two datasets, each written to its own `target/oracle_state/<dataset>/`:
//!
//!   * **synthetic** — every method run on its committed per-op golden fixture
//!     (`tests/golden/fixtures/*.bin`), compared against the verbatim reference
//!     output (Allen/Zhuang Python, SNLC/Garrett MATLAB, or the canonical
//!     numpy/scipy primitive). Column 1 is a true external **oracle**.
//!
//!   * **r43** — the full pipeline re-run on the real `R43_smoke.oisi`
//!     recording, every `/results` leaf compared against the committed
//!     `R43_smoke.baseline.oisi` (the equivalence harness's reference). Column 1
//!     is the **reference** baseline; this is the real-data regression view.
//!     ("For the ones possible" = the leaves actually present in the file.)
//!
//! The companion `render_oracle_state.py` renders one figure per (dataset,
//! group): rows = methods/leaves in pipeline-DAG order, columns =
//! `[oracle|reference | OpenISI | difference]`, colormapped by data kind.
//!
//! Run:
//!   cargo run -p isi-analysis --example oracle_state            # both paths
//!   cargo run -p isi-analysis --example oracle_state -- synthetic
//!   cargo run -p isi-analysis --example oracle_state -- r43
//! or `cargo xtask figures oracle_state` (dumps + renders both).
//!
//! This is the *visualization* sibling of `compute/golden_vfs.rs` (per-op
//! contract) and `tests/equivalence.rs` (R43 contract): the tests own the
//! ε-grounded tolerance asserts; this owns the picture. Both drive the same
//! public ops, so neither re-implements the other's math.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use burn_tensor::{Tensor, TensorData};
use ndarray::{Array2, Array3};

use isi_analysis::compute::responsiveness::reliability;
use isi_analysis::compute::{
    compute_magnification_jacobian, compute_vfs, delay_map, device, dft_projection_at_freq,
    frames_u16_subset_to_dff_tensor, gaussian_smooth, magnification_anisotropy, phase_gradients,
    position_amplitude, position_phasor_delay_subtracted, real_gradients, temporal_mean_baseline,
    temporal_median_baseline, tensor_to_array2_f64, Backend, Complex2,
};
use isi_analysis::math::{cortical_magnification_factor, eccentricity_pixel_deg};
use isi_analysis::methods::patch_threshold::{PatchThresholdExt, PatchThresholdMethod};
use isi_analysis::{self, SilentProgress};

// ── paths ─────────────────────────────────────────────────────────────────────

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixtures_dir() -> PathBuf {
    crate_dir().join("tests/golden/fixtures")
}

/// `<repo>/target/oracle_state` — never committed (under target/).
fn out_root() -> PathBuf {
    crate_dir().join("..").join("..").join("target").join("oracle_state")
}

// ── golden-fixture decoding (synthetic path) ────────────────────────────────────

fn read_bytes(name: &str) -> Vec<u8> {
    let p = fixtures_dir().join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("reading fixture {}: {e}", p.display()))
}

fn fx_f64(name: &str) -> Vec<f64> {
    read_bytes(name)
        .chunks_exact(8)
        .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

fn fx_f32(name: &str) -> Vec<f32> {
    read_bytes(name)
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

fn fx_u8(name: &str) -> Vec<u8> {
    read_bytes(name)
}

fn write_f64(path: &Path, data: &[f64]) {
    let mut bytes = Vec::with_capacity(data.len() * 8);
    for &v in data {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
}

fn tensor2(data: Vec<f32>, h: usize, w: usize) -> Tensor<Backend, 2> {
    Tensor::<Backend, 2>::from_data(TensorData::new(data, [h, w]), &device())
}

fn phase_tensor(phi: &[f64], h: usize, w: usize) -> Tensor<Backend, 2> {
    tensor2(phi.iter().map(|&v| v as f32).collect(), h, w)
}

fn flat(t: Tensor<Backend, 2>) -> Vec<f64> {
    tensor_to_array2_f64(t)
        .expect("tensor → array2")
        .iter()
        .copied()
        .collect()
}

// ── panel registry ───────────────────────────────────────────────────────────

/// How a panel's values should be colormapped (the renderer reads this).
/// `Periodic` carries its period + display range; the diff uses a wrap-aware
/// distance. `Mask` (boolean) and `Labels` (integer) diffs are categorical.
enum Kind {
    /// `period`, plus the fixed display `[vmin, vmax]`.
    Periodic { period: f64, vmin: f64, vmax: f64 },
    Diverging,
    Sequential,
    Mask,
    Labels,
}

impl Kind {
    fn tag(&self) -> &'static str {
        match self {
            Kind::Periodic { .. } => "periodic",
            Kind::Diverging => "diverging",
            Kind::Sequential => "sequential",
            Kind::Mask => "mask",
            Kind::Labels => "labels",
        }
    }
}

#[derive(serde::Serialize)]
struct PanelMeta {
    group: String,
    order: u32,
    name: String,
    title: String,
    oracle_ref: String,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    period: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vmin: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vmax: Option<f64>,
    h: usize,
    w: usize,
}

struct Gallery {
    dataset: String,
    /// Header for column 1 ("oracle" for synthetic, "OpenISI baseline" for r43).
    col1: String,
    /// Honest figure caption (what column 1 actually IS).
    caption: String,
    dir: PathBuf,
    panels: Vec<PanelMeta>,
}

impl Gallery {
    fn new(dataset: &str, col1: &str, caption: &str) -> Self {
        let dir = out_root().join(dataset);
        std::fs::create_dir_all(&dir).unwrap();
        Self {
            dataset: dataset.to_string(),
            col1: col1.to_string(),
            caption: caption.to_string(),
            dir,
            panels: Vec::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        group: &str,
        order: u32,
        name: &str,
        title: &str,
        oracle_ref: &str,
        kind: Kind,
        h: usize,
        w: usize,
        oracle: &[f64],
        ours: &[f64],
    ) {
        assert_eq!(oracle.len(), h * w, "oracle size for {name}");
        assert_eq!(ours.len(), h * w, "ours size for {name}");
        write_f64(&self.dir.join(format!("{name}.oracle.bin")), oracle);
        write_f64(&self.dir.join(format!("{name}.ours.bin")), ours);
        let (period, vmin, vmax) = match kind {
            Kind::Periodic { period, vmin, vmax } => (Some(period), Some(vmin), Some(vmax)),
            _ => (None, None, None),
        };
        self.panels.push(PanelMeta {
            group: group.to_string(),
            order,
            name: name.to_string(),
            title: title.to_string(),
            oracle_ref: oracle_ref.to_string(),
            kind: kind.tag().to_string(),
            period,
            vmin,
            vmax,
            h,
            w,
        });
    }

    fn finish(mut self) {
        self.panels.sort_by(|a, b| a.group.cmp(&b.group).then(a.order.cmp(&b.order)));
        let manifest = serde_json::json!({
            "dataset": self.dataset,
            "col1": self.col1,
            "caption": self.caption,
            "panels": self.panels,
        });
        let p = self.dir.join("manifest.json");
        std::fs::write(&p, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
        println!("  {} — {} panels → {}", self.dataset, self.panels.len(), p.display());
    }
}

// ── main ─────────────────────────────────────────────────────────────────────

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "both".into());
    let do_syn = which == "both" || which == "synthetic";
    let do_r43 = which == "both" || which == "r43";
    if !do_syn && !do_r43 {
        eprintln!("usage: oracle_state [both|synthetic|r43]");
        std::process::exit(2);
    }
    println!("oracle_state:");
    if do_syn {
        dump_synthetic();
    }
    if do_r43 {
        dump_r43();
    }
}

// ── synthetic path: per-op golden fixtures vs verbatim oracle output ────────────

fn dump_synthetic() {
    let mut g = Gallery::new(
        "synthetic",
        "oracle",
        "Oracle cross-validation — per-op vs verbatim reference output (Allen / SNLC / numpy / scipy)",
    );

    // ── NumLib group: canonical numerical-library primitives ────────────────
    // (numpy / scipy ARE the reference for these — full, correct coverage; they
    // are simply not OpenISI-specific science methods, so they form their own
    // reference group rather than the Allen/SNLC scientific-method groups.)
    {
        const NF: usize = 24;
        const HW: usize = 16;
        let movie: Vec<f32> = fx_f32("dft_movie.bin");
        let m = Tensor::<Backend, 3>::from_data(TensorData::new(movie, [NF, HW, HW]), &device());
        let f1 = dft_projection_at_freq(m, 1.0, 1.0 / NF as f64);
        g.add("NumLib", 20, "dft_f1_re", "F1 DFT — real part",
            "numpy.fft.fft(axis=0)[1].real", Kind::Diverging, HW, HW,
            &fx_f64("dft_f1_re.bin"), &flat(f1.real()));
        g.add("NumLib", 21, "dft_f1_im", "F1 DFT — imag part",
            "numpy.fft.fft(axis=0)[1].imag", Kind::Diverging, HW, HW,
            &fx_f64("dft_f1_im.bin"), &flat(f1.imag()));
    }
    {
        const G: usize = 96;
        let t = tensor2(fx_f64("gauss_input.bin").iter().map(|&v| v as f32).collect(), G, G);
        g.add("NumLib", 30, "gaussian", "Gaussian smooth (σ=4)",
            "scipy.ndimage.gaussian_filter", Kind::Sequential, G, G,
            &fx_f64("gauss_sigma4.bin"), &flat(gaussian_smooth(t, 4.0)));
    }
    {
        const K: usize = 5;
        const H: usize = 8;
        const W: usize = 8;
        let re = fx_f32("rel_z_re.bin");
        let im = fx_f32("rel_z_im.bin");
        let cycles: Vec<Complex2> = (0..K)
            .map(|k| {
                let r = re[k * H * W..(k + 1) * H * W].to_vec();
                let i = im[k * H * W..(k + 1) * H * W].to_vec();
                Complex2::new(tensor2(r, H, W), tensor2(i, H, W))
            })
            .collect();
        g.add("NumLib", 46, "reliability", "Reliability |ΣZ|/Σ|Z|",
            "Engel/Zhuang coherence (numpy)", Kind::Sequential, H, W,
            &fx_f64("rel_expected.bin"), &flat(reliability(&cycles)));
    }
    {
        const N: usize = 20;
        const H: usize = 16;
        const W: usize = 16;
        let frames = dff_frames::<N, H, W>();
        g.add("NumLib", 11, "median_baseline", "Median baseline F0",
            "numpy.median(movie, axis=0)", Kind::Sequential, H, W,
            &fx_f64("dff_f0_median.bin"),
            &temporal_median_baseline(&frames).into_iter().collect::<Vec<f64>>());
    }

    // ── Allen / Zhuang / Garrett group ──────────────────────────────────────
    {
        const N: usize = 20;
        const H: usize = 16;
        const W: usize = 16;
        let frames = dff_frames::<N, H, W>();
        let baseline = temporal_mean_baseline(&frames);
        g.add("Allen", 9, "f0_mean", "Baseline F0 (mean)",
            "Allen ImageAnalysis.normalizeMovie (mean)", Kind::Sequential, H, W,
            &fx_f64("dff_f0.bin"), &baseline.iter().copied().collect::<Vec<f64>>());

        let idx: Vec<usize> = (0..N).collect();
        let dff = frames_u16_subset_to_dff_tensor(&frames, &idx, &baseline, 0.0, true);
        let dff_v: Vec<f32> = dff.into_data().to_vec::<f32>().expect("dff vec");
        let ours0: Vec<f64> = dff_v[0..H * W].iter().map(|&v| f64::from(v)).collect();
        let oracle0: Vec<f64> =
            fx_f32("dff_dff.bin")[0..H * W].iter().map(|&v| f64::from(v)).collect();
        g.add("Allen", 10, "dff_frame0", "ΔF/F (frame 0)",
            "Allen normalizeMovie (F−F0)/F0", Kind::Diverging, H, W, &oracle0, &ours0);
    }
    {
        const N: usize = 64;
        let vfs_of = |p1: &str, p2: &str| {
            let z_azi = Complex2::from_phase(phase_tensor(&fx_f64(p1), N, N));
            let z_alt = Complex2::from_phase(phase_tensor(&fx_f64(p2), N, N));
            let (axx, axy) = phase_gradients(&z_azi);
            let (alx, aly) = phase_gradients(&z_alt);
            flat(compute_vfs(axx, axy, alx, aly))
        };
        g.add("Allen", 50, "vfs_smooth", "Visual field sign (smooth)",
            "Allen visualSignMap (RetinotopicMapping.py:446)", Kind::Diverging, N, N,
            &fx_f64("vfs_smooth_allen.bin"), &vfs_of("vfs_smooth_phi1.bin", "vfs_smooth_phi2.bin"));
        g.add("Allen", 51, "vfs_wrap", "Visual field sign (across phase wraps)",
            "Allen visualSignMap on unwrapped truth", Kind::Diverging, N, N,
            &fx_f64("vfs_wrap_allen_true.bin"), &vfs_of("vfs_wrap_phi1.bin", "vfs_wrap_phi2.bin"));
    }
    {
        const N: usize = 64;
        let vfs_flat = fx_f64("pthr_vfs.bin");
        let vfs = Array2::from_shape_fn((N, N), |(r, c)| vfs_flat[r * N + c]);
        let all_cortex = Array2::from_elem((N, N), true);
        let bool_to_f64 = |m: &Array2<bool>| m.iter().map(|&b| b as u8 as f64).collect::<Vec<f64>>();
        let u8_to_f64 = |b: &[u8]| b.iter().map(|&v| v as f64).collect::<Vec<f64>>();
        let allen = PatchThresholdMethod::AllenZhuang2017FixedSignMapThr { value: 0.35 }
            .apply(&vfs, &all_cortex).imseg;
        g.add("Allen", 60, "pthr_allen", "Patch threshold (Allen/Zhuang |VFS|≥0.35)",
            "Allen/Zhuang 2017 fixed sign-map threshold", Kind::Mask, N, N,
            &u8_to_f64(&fx_u8("pthr_allen.bin")), &bool_to_f64(&allen));
        let garrett = PatchThresholdMethod::Garrett2014SigmaScaled { k: 1.5 }
            .apply(&vfs, &all_cortex).imseg;
        g.add("Allen", 61, "pthr_garrett", "Patch threshold (Garrett k·σ/2)",
            "Garrett 2014 σ-scaled threshold (getMouseAreasX.m)", Kind::Mask, N, N,
            &u8_to_f64(&fx_u8("pthr_garrett.bin")), &bool_to_f64(&garrett));
    }
    {
        const N: usize = 64;
        let alt = fx_f64("ecc_alt.bin");
        let azi = fx_f64("ecc_azi.bin");
        const ALT_C: f64 = 5.0;
        const AZI_C: f64 = 10.0;
        let ours: Vec<f64> = (0..N * N)
            .map(|i| eccentricity_pixel_deg(alt[i], azi[i], ALT_C, AZI_C))
            .collect();
        g.add("Allen", 70, "eccentricity", "Eccentricity (deg)",
            "Allen eccentricityMap (RetinotopicMapping.py:729)", Kind::Sequential, N, N,
            &fx_f64("ecc_golden.bin"), &ours);
    }
    {
        const MG: usize = 48;
        let alt_t = tensor2(fx_f64("mag_alt.bin").iter().map(|&v| v as f32).collect(), MG, MG);
        let azi_t = tensor2(fx_f64("mag_azi.bin").iter().map(|&v| v as f32).collect(), MG, MG);
        let (d_alt_dx, d_alt_dy) = real_gradients(alt_t);
        let (d_azi_dx, d_azi_dy) = real_gradients(azi_t);
        let mag = compute_magnification_jacobian(d_azi_dx, d_azi_dy, d_alt_dx, d_alt_dy, 1.0, 1.0);
        let det = tensor_to_array2_f64(mag).unwrap();
        g.add("Allen", 80, "mag_det", "Magnification |det J|",
            "Allen _getDeterminantMap (RetinotopicMapping.py:1184)", Kind::Sequential, MG, MG,
            &fx_f64("mag_det.bin"), &det.iter().copied().collect::<Vec<f64>>());
        let labels = Array2::from_elem((MG, MG), 1i32);
        let cmf = cortical_magnification_factor(&det, &labels);
        g.add("Allen", 81, "mag_cmf", "Magnification factor 1/|det J|",
            "Allen reciprocal CMF", Kind::Sequential, MG, MG,
            &fx_f64("mag_cmf.bin"), &cmf.iter().copied().collect::<Vec<f64>>());
    }

    // ── SNLC / Garrett MATLAB group ─────────────────────────────────────────
    {
        const N: usize = 64;
        let fwd = Complex2::from_phase(phase_tensor(&fx_f64("combine_ang0.bin"), N, N));
        let rev = Complex2::from_phase(phase_tensor(&fx_f64("combine_ang2.bin"), N, N));
        let result = position_phasor_delay_subtracted(&fwd, &rev);
        g.add("SNLC", 40, "kalatsky_combine", "Kalatsky combine — position phase",
            "SNLC Gprocesskret.m kmap (88-99)",
            Kind::Periodic { period: std::f64::consts::TAU, vmin: -std::f64::consts::PI, vmax: std::f64::consts::PI },
            N, N, &fx_f64("combine_kmap.bin"), &flat(result.angle()));
        let delay = delay_map(&fwd, &rev);
        g.add("SNLC", 85, "delay_map", "Hemodynamic delay (0,π]",
            "SNLC Gprocesskret.m delay (88-96)", Kind::Sequential, N, N,
            &fx_f64("combine_delay.bin"), &flat(delay));
    }
    {
        const H: usize = 16;
        const W: usize = 16;
        let fwd = Complex2::new(tensor2(fx_f32("amp_fwd_re.bin"), H, W), tensor2(fx_f32("amp_fwd_im.bin"), H, W));
        let rev = Complex2::new(tensor2(fx_f32("amp_rev_re.bin"), H, W), tensor2(fx_f32("amp_rev_im.bin"), H, W));
        g.add("SNLC", 45, "amplitude", "F1 amplitude ½(|fwd|+|rev|)",
            "SNLC Gprocesskret.m magS", Kind::Sequential, H, W,
            &fx_f64("amp_expected.bin"), &flat(position_amplitude(&fwd, &rev)));
    }
    {
        const M: usize = 48;
        let t = |n: &str| tensor2(fx_f64(n).iter().map(|&v| v as f32).collect(), M, M);
        let (axis, dist) = magnification_anisotropy(
            t("maganiso_dhdx.bin"), t("maganiso_dhdy.bin"),
            t("maganiso_dvdx.bin"), t("maganiso_dvdy.bin"));
        g.add("SNLC", 86, "maganiso_axis", "Anisotropy axis [0,180)°",
            "SNLC getMagFactors.m prefAxisMF",
            Kind::Periodic { period: 180.0, vmin: 0.0, vmax: 180.0 }, M, M,
            &fx_f64("maganiso_axis.bin"), &flat(axis));
        g.add("SNLC", 87, "maganiso_distortion", "Anisotropy distortion |Res|",
            "SNLC getMagFactors.m Distrtion", Kind::Sequential, M, M,
            &fx_f64("maganiso_distortion.bin"), &flat(dist));
    }

    g.finish();
}

/// Decode the shared ΔF/F frame fixture as an `Array3<u16>`.
fn dff_frames<const N: usize, const H: usize, const W: usize>() -> Array3<u16> {
    let bytes = read_bytes("dff_frames.bin");
    let frames_u16: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes(c.try_into().unwrap()))
        .collect();
    Array3::from_shape_fn((N, H, W), |(t, r, c)| frames_u16[t * H * W + r * W + c])
}

// ── r43 path: real-data pipeline recompute vs committed baseline ────────────────

/// One `/results` leaf to visualize on R43: its HDF5 dtype + colormap kind +
/// which group/figure it lands in.
struct R43Leaf {
    name: &'static str,
    group: &'static str,
    order: u32,
    title: &'static str,
    dtype: R43Dtype,
    kind_of: fn() -> Kind,
}

enum R43Dtype {
    F64,
    U8,
    I32,
}

// Non-capturing `kind_of` constructors for the R43 leaf table.
fn k_phase2pi() -> Kind {
    Kind::Periodic { period: std::f64::consts::TAU, vmin: -std::f64::consts::PI, vmax: std::f64::consts::PI }
}
fn k_axis180() -> Kind {
    Kind::Periodic { period: 180.0, vmin: 0.0, vmax: 180.0 }
}
fn k_polar360() -> Kind {
    Kind::Periodic { period: 360.0, vmin: -180.0, vmax: 180.0 }
}
fn k_diverging() -> Kind {
    Kind::Diverging
}
fn k_sequential() -> Kind {
    Kind::Sequential
}
fn k_mask() -> Kind {
    Kind::Mask
}
fn k_labels() -> Kind {
    Kind::Labels
}

fn dump_r43() {
    let fixture = crate_dir().join("tests/fixtures/oisi/R43_smoke.oisi");
    let baseline = crate_dir().join("tests/fixtures/baseline/R43_smoke.baseline.oisi");
    if !fixture.exists() || !baseline.exists() {
        eprintln!(
            "  r43 — skipped (need both {} and {})",
            fixture.display(),
            baseline.display()
        );
        return;
    }

    // Recompute the pipeline on a fresh copy of the fixture (the equivalence /
    // capture_baseline recipe). The "ours" column is this candidate; the
    // reference column is the committed baseline.
    let candidate = out_root().join("r43").join("R43_candidate.oisi");
    let t0 = Instant::now();
    println!("  r43 — recomputing analyze on {} …", fixture.display());
    recompute(&fixture, &candidate);
    println!("  r43 — recompute done in {:.1}s", t0.elapsed().as_secs_f64());

    let mut g = Gallery::new(
        "r43",
        "OpenISI baseline",
        "Real-data REGRESSION — pipeline recompute vs committed OpenISI baseline (self-consistency, \
         NOT an external oracle). True SNLC oracle on R43 is the separate r43_oracle figure.",
    );

    // "For the ones possible": each leaf is emitted only if present in BOTH
    // files. The maps are grouped into two figures (retinotopy maps; discrete
    // segmentation) so neither figure is unreadably tall.
    let leaves: &[R43Leaf] = &[
        // ── group: Maps (continuous retinotopy) ──
        R43Leaf { name: "azi_phase", group: "Maps", order: 10, title: "Azimuth phase (rad)", dtype: R43Dtype::F64, kind_of: k_phase2pi },
        R43Leaf { name: "alt_phase", group: "Maps", order: 11, title: "Altitude phase (rad)", dtype: R43Dtype::F64, kind_of: k_phase2pi },
        R43Leaf { name: "azi_amplitude", group: "Maps", order: 20, title: "Azimuth F1 amplitude", dtype: R43Dtype::F64, kind_of: k_sequential },
        R43Leaf { name: "alt_amplitude", group: "Maps", order: 21, title: "Altitude F1 amplitude", dtype: R43Dtype::F64, kind_of: k_sequential },
        R43Leaf { name: "vfs", group: "Maps", order: 30, title: "Visual field sign (raw)", dtype: R43Dtype::F64, kind_of: k_diverging },
        R43Leaf { name: "vfs_smoothed", group: "Maps", order: 31, title: "Visual field sign (smoothed)", dtype: R43Dtype::F64, kind_of: k_diverging },
        R43Leaf { name: "eccentricity", group: "Maps", order: 40, title: "Eccentricity (deg)", dtype: R43Dtype::F64, kind_of: k_sequential },
        R43Leaf { name: "polar_angle", group: "Maps", order: 41, title: "Polar angle (deg)", dtype: R43Dtype::F64, kind_of: k_polar360 },
        R43Leaf { name: "magnification", group: "Maps", order: 50, title: "Magnification 1/|det J|", dtype: R43Dtype::F64, kind_of: k_sequential },
        R43Leaf { name: "magnification_axis", group: "Maps", order: 51, title: "Anisotropy axis (deg)", dtype: R43Dtype::F64, kind_of: k_axis180 },
        R43Leaf { name: "magnification_distortion", group: "Maps", order: 52, title: "Anisotropy distortion", dtype: R43Dtype::F64, kind_of: k_sequential },
        R43Leaf { name: "azi_delay", group: "Maps", order: 60, title: "Azimuth delay (deg)", dtype: R43Dtype::F64, kind_of: k_sequential },
        R43Leaf { name: "alt_delay", group: "Maps", order: 61, title: "Altitude delay (deg)", dtype: R43Dtype::F64, kind_of: k_sequential },
        // ── group: Segmentation (discrete decisions) ──
        R43Leaf { name: "cortex_mask", group: "Segmentation", order: 10, title: "Cortex mask", dtype: R43Dtype::U8, kind_of: k_mask },
        R43Leaf { name: "vfs_smoothed_thresholded", group: "Segmentation", order: 15, title: "VFS thresholded (NaN off-cortex)", dtype: R43Dtype::F64, kind_of: k_diverging },
        R43Leaf { name: "area_borders", group: "Segmentation", order: 20, title: "Area borders", dtype: R43Dtype::U8, kind_of: k_mask },
        R43Leaf { name: "area_labels", group: "Segmentation", order: 30, title: "Area labels", dtype: R43Dtype::I32, kind_of: k_labels },
    ];

    let ds_path = |name: &str| format!("results/{name}");
    for leaf in leaves {
        let dp = ds_path(leaf.name);
        if !leaf_exists(&candidate, &dp) || !leaf_exists(&baseline, &dp) {
            continue; // not produced for this combine / acquisition mode
        }
        let (oracle, h, w) = read_leaf(&baseline, &dp, &leaf.dtype);
        let (ours, h2, w2) = read_leaf(&candidate, &dp, &leaf.dtype);
        if (h, w) != (h2, w2) {
            eprintln!("  r43 — shape mismatch on {}, skipped", leaf.name);
            continue;
        }
        g.add(
            leaf.group,
            leaf.order,
            leaf.name,
            leaf.title,
            "OpenISI baseline (R43_smoke.baseline.oisi)",
            (leaf.kind_of)(),
            h,
            w,
            &oracle,
            &ours,
        );
    }

    g.finish();

    // Export the inputs the TRUE SNLC oracle consumes (figdata_oracle_state_snlc.m
    // runs the real getMouseAreasX segmentation on these under Octave): R43's
    // azimuth/altitude POSITION maps (degrees) + pixels-per-mm calibration.
    export_snlc_oracle_inputs(&candidate);
}

/// Write R43's azimuth/altitude position maps (degrees) + `pixpermm` so the
/// Octave figure-data generator can run the SNLC segmentation oracle on the real
/// recording. `pixpermm = 1000 / um_per_pixel`, read from the file's
/// `/rig_params` (default 20 µm/px if absent — surfaced, since the SNLC
/// patch-area criteria in split/fuse are scale-dependent).
fn export_snlc_oracle_inputs(candidate: &Path) {
    let inp = out_root().join("r43").join("oracle_in");
    std::fs::create_dir_all(&inp).unwrap();
    let (azi, h, w) = read_leaf(candidate, "results/azi_phase_degrees", &R43Dtype::F64);
    let (alt, ha, wa) = read_leaf(candidate, "results/alt_phase_degrees", &R43Dtype::F64);
    assert_eq!((h, w), (ha, wa), "azi/alt position-map shape mismatch");
    write_f64(&inp.join("kmap_hor.bin"), &azi);
    write_f64(&inp.join("kmap_vert.bin"), &alt);

    let um_per_pixel = isi_analysis::io::read_rig_params(candidate)
        .ok()
        .flatten()
        .and_then(|v| {
            v.get("camera")
                .and_then(|c| c.get("um_per_pixel"))
                .and_then(serde_json::Value::as_f64)
        })
        .unwrap_or(20.0);
    let pixpermm = 1000.0 / um_per_pixel;
    let meta = serde_json::json!({
        "h": h, "w": w, "um_per_pixel": um_per_pixel, "pixpermm": pixpermm,
    });
    std::fs::write(inp.join("meta.json"), serde_json::to_vec_pretty(&meta).unwrap()).unwrap();
    println!(
        "  r43 — exported SNLC oracle inputs ({h}×{w}, um/px={um_per_pixel}, pixpermm={pixpermm:.2})"
    );
}

/// Recompute the full pipeline on a fresh copy of `fixture` into `candidate`,
/// stripping caches and migrating pre-2026 params first (the capture_baseline /
/// equivalence recipe, via the public API).
fn recompute(fixture: &Path, candidate: &Path) {
    if let Some(parent) = candidate.parent() {
        std::fs::create_dir_all(parent).expect("create candidate dir");
    }
    std::fs::copy(fixture, candidate).expect("copy fixture");
    isi_analysis::io::strip_derived_outputs(candidate).expect("strip derived outputs");

    if isi_analysis::io::is_pre_2026_analysis_params(candidate).expect("is_pre_2026") {
        let old = isi_analysis::io::read_analysis_params_attr(candidate)
            .expect("read /analysis_params")
            .expect("pre-2026 said yes but read returned None");
        let new = isi_analysis::migrate::translate_pre_2026_analysis_params(&old)
            .expect("translate pre-2026 params");
        isi_analysis::io::write_analysis_params_attr(candidate, &new).expect("write migrated");
    }

    let params = match isi_analysis::io::read_analysis_params_attr(candidate).expect("read params") {
        Some(tree) => isi_analysis::bridge::analysis_params_from_oisi_tree(&tree)
            .expect("reconstruct AnalysisParams"),
        None => isi_analysis::AnalysisParams::from(&openisi_params::config::AnalysisConfig::default()),
    };
    let cancel = AtomicBool::new(false);
    isi_analysis::analyze(candidate, &params, None, &SilentProgress, &cancel).expect("analyze");
}

fn leaf_exists(path: &Path, ds: &str) -> bool {
    hdf5::File::open(path).map(|f| f.dataset(ds).is_ok()).unwrap_or(false)
}

/// Read a 2D `/results` leaf as row-major `f64` + its `(h, w)`. Integer/byte
/// leaves are widened to f64 (masks → 0/1, labels → label value).
fn read_leaf(path: &Path, ds: &str, dtype: &R43Dtype) -> (Vec<f64>, usize, usize) {
    let file = hdf5::File::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let dataset = file.dataset(ds).unwrap_or_else(|e| panic!("dataset {ds}: {e}"));
    let shape = dataset.shape();
    assert_eq!(shape.len(), 2, "{ds}: expected 2D, got {shape:?}");
    let (h, w) = (shape[0], shape[1]);
    let data: Vec<f64> = match dtype {
        R43Dtype::F64 => dataset.read_dyn::<f64>().expect("read f64").into_iter().collect(),
        R43Dtype::U8 => dataset.read_dyn::<u8>().expect("read u8").iter().map(|&v| v as f64).collect(),
        R43Dtype::I32 => dataset.read_dyn::<i32>().expect("read i32").iter().map(|&v| v as f64).collect(),
    };
    (data, h, w)
}
