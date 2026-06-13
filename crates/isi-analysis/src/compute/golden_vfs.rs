//! Golden cross-validation of the VFS-computation stage against the method it
//! claims equivalence to.
//!
//! `VfsComputationMethod::OpenIsiChainRulePhasorGradient` documents itself as
//! "mathematically equivalent to Allen `visualSignMap`
//! (`RetinotopicMapping.py` L446-478) but more numerically stable near phase
//! wraps." This test holds that claim to account: it feeds identical smooth
//! (no-wrap) phase maps to our Rust path and compares against the **verbatim**
//! Allen output captured in `tests/golden/fixtures/` by
//! `tests/golden/gen_vfs_golden.py`.
//!
//! Expectation: our chain-rule gradient (`(c·∂s − s·∂c)/|z|²`) and Allen's
//! `np.gradient(φ)` agree only to *discretization order* on smooth input (they
//! coincide in the continuum limit, differ at O(Δφ²)), and diverge at wraps.
//! So this asserts agreement to a discretization tolerance, not bit-equality —
//! the divergence-at-wraps half of the claim is a separate (future) fixture.

#[cfg(test)]
mod tests {
    use crate::compute::responsiveness::reliability;
    use crate::compute::{
        amp_weighted_complex_smooth, compute_magnification_jacobian, compute_vfs, device,
        gaussian_smooth, phase_gradients, position_amplitude, position_gaussian_smooth,
        position_phasor_delay_subtracted, real_gradients, tensor_to_array2_f64, Backend, Complex2,
    };
    use crate::methods::patch_threshold::PatchThresholdMethod;
    use crate::test_support::{count_differing, load_f32, load_f64};
    use burn_tensor::{Tensor, TensorData};
    use ndarray::Array2;

    const N: usize = 64;

    fn tensor2(data: Vec<f32>, h: usize, w: usize) -> Tensor<Backend, 2> {
        Tensor::<Backend, 2>::from_data(TensorData::new(data, [h, w]), &device())
    }

    fn phase_tensor(phi: &[f64]) -> Tensor<Backend, 2> {
        let f32s: Vec<f32> = phi.iter().map(|&v| v as f32).collect();
        Tensor::<Backend, 2>::from_data(TensorData::new(f32s, [N, N]), &device())
    }

    #[test]
    fn vfs_matches_allen_visual_sign_map_on_smooth_input() {
        let phi1 = load_f64(include_bytes!("../../tests/golden/fixtures/vfs_smooth_phi1.bin"));
        let phi2 = load_f64(include_bytes!("../../tests/golden/fixtures/vfs_smooth_phi2.bin"));
        let allen = load_f64(include_bytes!("../../tests/golden/fixtures/vfs_smooth_allen.bin"));
        assert_eq!(phi1.len(), N * N);

        // Allen `visualSignMap(phasemap1, phasemap2) = sin(θ₁ − θ₂)` with
        // `θ = atan2(∂/∂col, ∂/∂row)`. Ours is `sin(θ_alt − θ_azi)` with
        // `θ = atan2(∂/∂row, ∂/∂col)` (swapped args). Mapping azi←phasemap1,
        // alt←phasemap2 makes the two coincide; the test reports both the
        // same-sign and flipped-sign residual so the convention is verified,
        // not assumed.
        let z_azi = Complex2::from_phase(phase_tensor(&phi1));
        let z_alt = Complex2::from_phase(phase_tensor(&phi2));
        let (d_azi_dx, d_azi_dy) = phase_gradients(&z_azi);
        let (d_alt_dx, d_alt_dy) = phase_gradients(&z_alt);
        let vfs = compute_vfs(d_azi_dx, d_azi_dy, d_alt_dx, d_alt_dy);
        let ours = tensor_to_array2_f64(vfs).unwrap();

        // Interior only: exclude the 1-px border, where one-sided edge
        // differences (shared by both) plus the gradient-direction
        // ill-conditioning at φ₂'s gradient zero-crossings are not the
        // equivalence we're testing.
        let mut max_same = 0.0f64;
        let mut max_flip = 0.0f64;
        for i in 1..N - 1 {
            for j in 1..N - 1 {
                let o = ours[[i, j]];
                let a = allen[i * N + j];
                max_same = max_same.max((o - a).abs());
                max_flip = max_flip.max((o + a).abs());
            }
        }
        eprintln!(
            "VFS vs Allen (interior): max|ours-allen|={max_same:.3e}  max|ours+allen|={max_flip:.3e}"
        );

        // azi←p1/alt←p2 should be the *same-sign* match; assert it agrees to
        // discretization order and is unambiguously not the flipped mapping.
        assert!(
            max_same < 5e-2,
            "VFS deviates from Allen beyond discretization tolerance: {max_same:.3e} \
             (flipped residual {max_flip:.3e})"
        );
    }

    /// The other half of the equivalence claim: "more numerically stable near
    /// phase wraps." A steep azimuth ramp is stored as its wrapped angle. Our
    /// chain-rule path sees the continuous phasor and recovers the *true*
    /// (unwrapped) VFS; Allen's `np.gradient` of the wrapped scalar spikes at
    /// each 2π jump. So ours must (a) match Allen-on-the-unwrapped-truth and
    /// (b) diverge from Allen-on-the-wrapped-input at the wrap columns.
    #[test]
    fn vfs_stable_across_phase_wraps_where_allen_gradient_spikes() {
        let phi1 = load_f64(include_bytes!("../../tests/golden/fixtures/vfs_wrap_phi1.bin"));
        let phi2 = load_f64(include_bytes!("../../tests/golden/fixtures/vfs_wrap_phi2.bin"));
        let allen_true =
            load_f64(include_bytes!("../../tests/golden/fixtures/vfs_wrap_allen_true.bin"));
        let allen_wrapped =
            load_f64(include_bytes!("../../tests/golden/fixtures/vfs_wrap_allen_wrapped.bin"));

        let z_azi = Complex2::from_phase(phase_tensor(&phi1));
        let z_alt = Complex2::from_phase(phase_tensor(&phi2));
        let (d_azi_dx, d_azi_dy) = phase_gradients(&z_azi);
        let (d_alt_dx, d_alt_dy) = phase_gradients(&z_alt);
        let vfs = compute_vfs(d_azi_dx, d_azi_dy, d_alt_dx, d_alt_dy);
        let ours = tensor_to_array2_f64(vfs).unwrap();

        let mut max_vs_true = 0.0f64;
        let mut max_vs_wrapped = 0.0f64;
        for i in 1..N - 1 {
            for j in 1..N - 1 {
                let o = ours[[i, j]];
                max_vs_true = max_vs_true.max((o - allen_true[i * N + j]).abs());
                max_vs_wrapped = max_vs_wrapped.max((o - allen_wrapped[i * N + j]).abs());
            }
        }
        eprintln!(
            "VFS wrap: max|ours-allen_TRUE|={max_vs_true:.3e}  \
             max|ours-allen_WRAPPED|={max_vs_wrapped:.3e}"
        );
        assert!(
            max_vs_true < 5e-2,
            "ours should recover the unwrapped-truth VFS: {max_vs_true:.3e}"
        );
        assert!(
            max_vs_wrapped > 0.5,
            "ours should diverge from Allen's wrap artifact: {max_vs_wrapped:.3e}"
        );
    }

    /// `position_phasor_delay_subtracted` vs SNLC `Gprocesskret.m` (88-99): the
    /// Kalatsky-Stryker delay-subtracted combine. Compared as phasors
    /// (cos/sin of the reference kmap) so ±2π wrap boundaries can't create
    /// false diffs. Fixtures from `gen_combine_golden.m`.
    #[test]
    fn kalatsky_combine_matches_snlc_gprocesskret() {
        let a0 = load_f64(include_bytes!("../../tests/golden/fixtures/combine_ang0.bin"));
        let a2 = load_f64(include_bytes!("../../tests/golden/fixtures/combine_ang2.bin"));
        let kmap = load_f64(include_bytes!("../../tests/golden/fixtures/combine_kmap.bin"));

        let fwd = Complex2::from_phase(phase_tensor(&a0));
        let rev = Complex2::from_phase(phase_tensor(&a2));
        let result = position_phasor_delay_subtracted(&fwd, &rev);
        let re = tensor_to_array2_f64(result.real()).unwrap();
        let im = tensor_to_array2_f64(result.imag()).unwrap();

        let mut maxd = 0.0f64;
        for i in 0..N {
            for j in 0..N {
                let k = kmap[i * N + j];
                maxd = maxd.max((re[[i, j]] - k.cos()).abs());
                maxd = maxd.max((im[[i, j]] - k.sin()).abs());
            }
        }
        eprintln!("Kalatsky combine vs SNLC Gprocesskret: max phasor diff = {maxd:.3e}");
        assert!(maxd < 1e-5, "combine diverges from Gprocesskret: {maxd:.3e}");
    }

    /// `math::eccentricity_pixel_deg` (the core of
    /// `EccentricityMethod::OpenIsiWholeCortexV1`) vs Allen
    /// `eccentricityMap` (RetinotopicMapping.py:729-760). Pure f64 on both
    /// sides → machine-precision match. Fixtures from `gen_ecc_golden.py`.
    #[test]
    fn garrett_eccentricity_matches_allen_eccentricitymap() {
        let alt = load_f64(include_bytes!("../../tests/golden/fixtures/ecc_alt.bin"));
        let azi = load_f64(include_bytes!("../../tests/golden/fixtures/ecc_azi.bin"));
        let golden = load_f64(include_bytes!("../../tests/golden/fixtures/ecc_golden.bin"));
        const ALT_C: f64 = 5.0;
        const AZI_C: f64 = 10.0;

        let mut maxd = 0.0f64;
        for i in 0..N * N {
            let e = crate::math::eccentricity_pixel_deg(alt[i], azi[i], ALT_C, AZI_C);
            maxd = maxd.max((e - golden[i]).abs());
        }
        eprintln!("Garrett eccentricity vs Allen eccentricityMap: max diff = {maxd:.3e} deg");
        assert!(maxd < 1e-9, "eccentricity diverges from Allen: {maxd:.3e}");
    }

    /// `compute_magnification_jacobian` (our `magnification_raw`, |det J|) vs Allen
    /// `RetinotopicMapping._getDeterminantMap` (L1184), plus the inverted
    /// `magnification` leaf (cortical magnification factor = 1/max(|det J|, eps)).
    /// `np.gradient` == our `real_gradients`, and Allen's `np.linalg.det` of
    /// `[[∇alt],[∇azi]]` is the same two product terms as our determinant, so this
    /// is an f32-precision match. Fixtures from `gen_magnification_golden.py`.
    #[test]
    fn magnification_jacobian_matches_allen_determinant_map() {
        const MG: usize = 48;
        let alt = load_f64(include_bytes!("../../tests/golden/fixtures/mag_alt.bin"));
        let azi = load_f64(include_bytes!("../../tests/golden/fixtures/mag_azi.bin"));
        let det_g = load_f64(include_bytes!("../../tests/golden/fixtures/mag_det.bin"));
        let cmf_g = load_f64(include_bytes!("../../tests/golden/fixtures/mag_cmf.bin"));

        let alt_t = tensor2(alt.iter().map(|&v| v as f32).collect(), MG, MG);
        let azi_t = tensor2(azi.iter().map(|&v| v as f32).collect(), MG, MG);
        // real_gradients returns (d/d_col, d/d_row) = (dx, dy).
        let (d_alt_dx, d_alt_dy) = real_gradients(alt_t);
        let (d_azi_dx, d_azi_dy) = real_gradients(azi_t);
        // Maps already in degrees → unit scale (Allen applies none).
        let mag = compute_magnification_jacobian(d_azi_dx, d_azi_dy, d_alt_dx, d_alt_dy, 1.0, 1.0);
        let got = tensor_to_array2_f64(mag).unwrap();

        let mut maxd = 0.0f64;
        for r in 0..MG {
            for c in 0..MG {
                maxd = maxd.max((got[[r, c]] - det_g[r * MG + c]).abs());
            }
        }
        eprintln!("|det J| vs Allen _getDeterminantMap: max diff = {maxd:.3e}");
        assert!(maxd < 1e-3, "magnification Jacobian diverges from Allen: {maxd:.3e}");

        // Inversion check: the `magnification` leaf is the reciprocal CMF.
        let labels = Array2::from_elem((MG, MG), 1i32); // all in-ROI
        let cmf = crate::math::cortical_magnification_factor(&got, &labels);
        let mut maxc = 0.0f64;
        for r in 0..MG {
            for c in 0..MG {
                maxc = maxc.max((cmf[[r, c]] - cmf_g[r * MG + c]).abs());
            }
        }
        eprintln!("CMF (1/|det J|) vs golden: max diff = {maxc:.3e}");
        assert!(maxc < 1e-2, "cortical magnification factor diverges: {maxc:.3e}");
    }

    /// patch_threshold: `AllenZhuang2017FixedSignMapThr` (|signMapf|≥0.35) and
    /// `Garrett2014SigmaScaled` (k·std·0.5, MATLAB N-1 std) vs reference
    /// thresholds. cortex_mask=all-true to isolate the threshold itself.
    /// Fixtures from `gen_patch_threshold_golden.py`.
    #[test]
    fn patch_threshold_matches_reference() {
        let vfs_flat = load_f64(include_bytes!("../../tests/golden/fixtures/pthr_vfs.bin"));
        let g_allen: &[u8] = include_bytes!("../../tests/golden/fixtures/pthr_allen.bin");
        let g_garrett: &[u8] = include_bytes!("../../tests/golden/fixtures/pthr_garrett.bin");
        let vfs = Array2::from_shape_fn((N, N), |(r, c)| vfs_flat[r * N + c]);
        let all_cortex = Array2::from_elem((N, N), true);

        let allen = PatchThresholdMethod::AllenZhuang2017FixedSignMapThr { value: 0.35 }
            .apply(&vfs, &all_cortex)
            .imseg;
        let garrett = PatchThresholdMethod::Garrett2014SigmaScaled { k: 1.5 }
            .apply(&vfs, &all_cortex)
            .imseg;

        let d_allen = count_differing(&allen, g_allen);
        let d_garrett = count_differing(&garrett, g_garrett);
        eprintln!("patch_threshold: allen diff={d_allen}  garrett diff={d_garrett}");
        assert_eq!(d_allen, 0, "Allen fixed threshold diverges from reference");
        assert_eq!(d_garrett, 0, "Garrett sigma-scaled threshold diverges from reference");
    }

    /// The TENSOR (f32) `gaussian_smooth` vs scipy `gaussian_filter` — the same
    /// canonical convention (4σ truncation, scipy `reflect` border) as the f64
    /// `gaussian_smooth_f64` golden, now validated at f32 precision too. Same
    /// fixture (`gen_gaussian_golden.py`), f32 tolerance.
    #[test]
    fn tensor_gaussian_smooth_matches_scipy() {
        const G: usize = 96;
        let inp = load_f64(include_bytes!("../../tests/golden/fixtures/gauss_input.bin"));
        let golden = load_f64(include_bytes!("../../tests/golden/fixtures/gauss_sigma4.bin"));
        let f32s: Vec<f32> = inp.iter().map(|&v| v as f32).collect();
        let t = Tensor::<Backend, 2>::from_data(TensorData::new(f32s, [G, G]), &device());
        let out = gaussian_smooth(t, 4.0);
        let ours = tensor_to_array2_f64(out).unwrap();
        let mut maxd = 0.0f64;
        for r in 0..G {
            for c in 0..G {
                maxd = maxd.max((ours[[r, c]] - golden[r * G + c]).abs());
            }
        }
        eprintln!("tensor gaussian_smooth vs scipy (f32, sigma=4): max diff = {maxd:.3e}");
        assert!(maxd < 1e-4, "tensor gaussian diverges from scipy: {maxd:.3e}");
    }

    /// Stage-0 single-bin F1 DFT (`dft_projection_at_freq`) vs numpy
    /// `np.fft.fft(...)[1]`. `dt=1, freq=1/n` makes our kernel
    /// `exp(-2πi·t/n)` exactly numpy's bin 1; the DC offset in the fixture
    /// confirms bin-1 rejects DC. Fixture from `gen_dft_golden.py`.
    #[test]
    fn dft_projection_matches_numpy_fft_bin1() {
        const NF: usize = 24;
        const HW: usize = 16;
        let movie_f32: Vec<f32> = include_bytes!("../../tests/golden/fixtures/dft_movie.bin")
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        let f1_re = load_f64(include_bytes!("../../tests/golden/fixtures/dft_f1_re.bin"));
        let f1_im = load_f64(include_bytes!("../../tests/golden/fixtures/dft_f1_im.bin"));
        assert_eq!(movie_f32.len(), NF * HW * HW);

        let movie =
            Tensor::<Backend, 3>::from_data(TensorData::new(movie_f32, [NF, HW, HW]), &device());
        let f1 = crate::compute::dft_projection_at_freq(movie, 1.0, 1.0 / NF as f64);
        let re = tensor_to_array2_f64(f1.real()).unwrap();
        let im = tensor_to_array2_f64(f1.imag()).unwrap();

        let mut maxd = 0.0f64;
        for r in 0..HW {
            for c in 0..HW {
                maxd = maxd.max((re[[r, c]] - f1_re[r * HW + c]).abs());
                maxd = maxd.max((im[[r, c]] - f1_im[r * HW + c]).abs());
            }
        }
        eprintln!("F1 DFT vs numpy fft bin1: max diff = {maxd:.3e}");
        assert!(maxd < 1e-3, "DFT diverges from numpy fft bin 1: {maxd:.3e}");
    }

    /// `responsiveness::reliability` (cross-cycle coherence `|ΣZ_k|/Σ|Z_k|`, the
    /// metric the reliability signal-quality criterion thresholds) vs a
    /// verbatim numpy transcription of the Engel/Zhuang coherence formula.
    /// Coherent + incoherent synthetic regions; f32 device path → tolerance.
    /// Fixtures from `gen_reliability_golden.py` (K=5, 8×8).
    #[test]
    fn reliability_matches_coherence_formula() {
        const K: usize = 5;
        const H: usize = 8;
        const W: usize = 8;
        let re = load_f32(include_bytes!("../../tests/golden/fixtures/rel_z_re.bin"));
        let im = load_f32(include_bytes!("../../tests/golden/fixtures/rel_z_im.bin"));
        let exp = load_f64(include_bytes!("../../tests/golden/fixtures/rel_expected.bin"));

        let cycles: Vec<Complex2> = (0..K)
            .map(|k| {
                let re_k = re[k * H * W..(k + 1) * H * W].to_vec();
                let im_k = im[k * H * W..(k + 1) * H * W].to_vec();
                Complex2::new(tensor2(re_k, H, W), tensor2(im_k, H, W))
            })
            .collect();
        let rel = tensor_to_array2_f64(reliability(&cycles)).expect("reliability");
        let mut md = 0.0f64;
        for r in 0..H {
            for c in 0..W {
                md = md.max((rel[[r, c]] - exp[r * W + c]).abs());
            }
        }
        eprintln!("reliability vs coherence formula: max diff = {md:.2e}");
        assert!(md < 1e-5, "reliability diverges from coherence formula: {md:.2e}");
    }

    /// `position_amplitude` (`0.5·(|fwd|+|rev|)`, the F1 magnitude = SNLC
    /// `Gprocesskret.m` `magS`, the metric the SnlcF1Amplitude mask thresholds)
    /// vs verbatim numpy. f32 device path → tolerance. Fixtures from
    /// `gen_amplitude_golden.py` (16×16).
    #[test]
    fn position_amplitude_matches_snlc_mags() {
        const H: usize = 16;
        const W: usize = 16;
        let fr = load_f32(include_bytes!("../../tests/golden/fixtures/amp_fwd_re.bin"));
        let fi = load_f32(include_bytes!("../../tests/golden/fixtures/amp_fwd_im.bin"));
        let rr = load_f32(include_bytes!("../../tests/golden/fixtures/amp_rev_re.bin"));
        let ri = load_f32(include_bytes!("../../tests/golden/fixtures/amp_rev_im.bin"));
        let exp = load_f64(include_bytes!("../../tests/golden/fixtures/amp_expected.bin"));

        let fwd = Complex2::new(tensor2(fr, H, W), tensor2(fi, H, W));
        let rev = Complex2::new(tensor2(rr, H, W), tensor2(ri, H, W));
        let amp = tensor_to_array2_f64(position_amplitude(&fwd, &rev)).expect("amplitude");
        let mut md = 0.0f64;
        for r in 0..H {
            for c in 0..W {
                md = md.max((amp[[r, c]] - exp[r * W + c]).abs());
            }
        }
        eprintln!("position_amplitude vs SNLC magS: max diff = {md:.2e}");
        assert!(md < 1e-5, "position_amplitude diverges from SNLC magS: {md:.2e}");
    }

    /// ΔF/F (`temporal_mean_baseline` + the dF/F formula) vs a verbatim
    /// transcription of Allen `ImageAnalysis.normalizeMovie` (`baselineType=
    /// 'mean'`): `F0 = mean(movie, axis=0)`, `dFoverF = (F − F0)/F0`. The dF/F
    /// is run with `denom_floor = 0` — the faithful Allen path (the `0.5·median`
    /// production floor is our documented robustness deviation, not Allen).
    /// Fixtures from `gen_dff_golden.py` (n=20, 16×16, all F0 > 0).
    #[test]
    fn dff_matches_allen_normalize_movie_mean() {
        use crate::compute::{frames_u16_subset_to_dff_tensor, temporal_mean_baseline};
        use ndarray::Array3;
        const N: usize = 20;
        const H: usize = 16;
        const W: usize = 16;
        let fb: &[u8] = include_bytes!("../../tests/golden/fixtures/dff_frames.bin");
        let frames_u16: Vec<u16> = fb
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes(c.try_into().unwrap()))
            .collect();
        let f0_exp = load_f64(include_bytes!("../../tests/golden/fixtures/dff_f0.bin"));
        let dff_exp = load_f32(include_bytes!("../../tests/golden/fixtures/dff_dff.bin"));
        let frames = Array3::from_shape_fn((N, H, W), |(t, r, c)| frames_u16[t * H * W + r * W + c]);

        // (1) Baseline F0 = Allen np.mean(movie, axis=0).
        let baseline = temporal_mean_baseline(&frames);
        let mut f0d = 0.0f64;
        for r in 0..H {
            for c in 0..W {
                f0d = f0d.max((baseline[[r, c]] - f0_exp[r * W + c]).abs());
            }
        }
        assert!(f0d < 1e-9, "baseline F0 diverges from Allen mean: {f0d:.2e}");

        // (2) dF/F (faithful path, floor=0) = Allen (F − F0)/F0.
        let idx: Vec<usize> = (0..N).collect();
        let dff_t = frames_u16_subset_to_dff_tensor(&frames, &idx, &baseline, 0.0);
        let got: Vec<f32> = dff_t.into_data().to_vec::<f32>().expect("dff to vec");
        let mut md = 0.0f32;
        for i in 0..N * H * W {
            md = md.max((got[i] - dff_exp[i]).abs());
        }
        eprintln!("dF/F vs Allen normalizeMovie: F0 diff={f0d:.2e}, dF/F diff={md:.2e}");
        assert!(md < 1e-5, "dF/F diverges from Allen normalizeMovie: {md:.2e}");
    }

    /// `temporal_median_baseline` vs Allen `normalizeMovie(baselineType=
    /// 'median')` = `np.median(movie, axis=0)`. N=20 (even) exercises numpy's
    /// average-of-two-middle convention. Fixtures from `gen_dff_golden.py`.
    #[test]
    fn median_baseline_matches_numpy() {
        use crate::compute::temporal_median_baseline;
        use ndarray::Array3;
        const N: usize = 20;
        const H: usize = 16;
        const W: usize = 16;
        let fb: &[u8] = include_bytes!("../../tests/golden/fixtures/dff_frames.bin");
        let frames_u16: Vec<u16> = fb
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes(c.try_into().unwrap()))
            .collect();
        let exp = load_f64(include_bytes!("../../tests/golden/fixtures/dff_f0_median.bin"));
        let frames = Array3::from_shape_fn((N, H, W), |(t, r, c)| frames_u16[t * H * W + r * W + c]);

        let med = temporal_median_baseline(&frames);
        let mut d = 0.0f64;
        for r in 0..H {
            for c in 0..W {
                d = d.max((med[[r, c]] - exp[r * W + c]).abs());
            }
        }
        eprintln!("median baseline vs np.median: max diff = {d:.2e}");
        assert!(d < 1e-9, "temporal_median_baseline diverges from np.median: {d:.2e}");
    }

    /// Provenance pin: the SNLC amplitude-weighted phasor smoothing's PHASE is
    /// identical to SNLC `Gprocesskret.m`, which smooths the complex F1
    /// (`amp·e^{iφ}`) directly. Our normalized convolution
    /// `smooth(amp·z) / smooth(amp)` divides by the positive-real `smooth(amp)`,
    /// which cannot change the angle — so `angle(ours) == angle(smooth(amp·z))`.
    /// Self-contained math identity (no fixture). This is what justifies crediting
    /// the method to SNLC, not OpenISI.
    #[test]
    fn amp_weighted_phase_equals_snlc_complex_smoothing() {
        const M: usize = 16;
        let sigma = 2.0;
        let mut re = vec![0f32; M * M];
        let mut im = vec![0f32; M * M];
        let mut amp = vec![0f32; M * M];
        for r in 0..M {
            for c in 0..M {
                // Smooth, spatially-varied phase ramp (no wrap) + varied amplitude.
                let phi = 0.3 * r as f32 - 0.2 * c as f32 + 0.02 * (r * c) as f32;
                re[r * M + c] = phi.cos();
                im[r * M + c] = phi.sin();
                amp[r * M + c] = 0.2 + 0.8 * ((r + 2 * c) as f32 / (3.0 * M as f32));
            }
        }
        let z = Complex2::new(tensor2(re, M, M), tensor2(im, M, M));
        let amp_t = tensor2(amp, M, M);

        // Ours: normalized amplitude-weighted convolution.
        let ours = tensor_to_array2_f64(amp_weighted_complex_smooth(&z, amp_t.clone(), sigma).angle())
            .unwrap();
        // SNLC: smooth the complex F1 (amp·z) directly, unnormalized, then angle().
        let snlc_re = gaussian_smooth(amp_t.clone() * z.real(), sigma);
        let snlc_im = gaussian_smooth(amp_t * z.imag(), sigma);
        let snlc = tensor_to_array2_f64(Complex2::new(snlc_re, snlc_im).angle()).unwrap();

        let mut maxd = 0.0f64;
        for r in 0..M {
            for c in 0..M {
                // Wrap-aware angular difference.
                let dphi = ours[[r, c]] - snlc[[r, c]];
                maxd = maxd.max(dphi.sin().atan2(dphi.cos()).abs());
            }
        }
        eprintln!("amp-weighted vs SNLC complex-smoothing phase: max diff = {maxd:.2e}");
        assert!(
            maxd < 1e-5,
            "normalized amp-weighting must be phase-identical to SNLC: {maxd:.2e}"
        );
    }

    /// The Allen `_getSignMap` phase-smoothing variant (`position_gaussian_smooth`)
    /// applies the *scipy-validated* Gaussian (`tensor_gaussian_smooth_matches_scipy`)
    /// to the phase: its output phase equals `gaussian_smooth(angle)` directly on a
    /// non-wrapping ramp (where rebuilding the unit phasor preserves the smoothed
    /// angle). With the Gaussian already golden-tested vs scipy, this transitively
    /// pins the Allen variant to Allen's `gaussian_filter(positionMap, sigma)`.
    #[test]
    fn allen_position_gaussian_matches_scalar_gaussian_on_phase() {
        const M: usize = 16;
        let sigma = 2.0;
        let mut re = vec![0f32; M * M];
        let mut im = vec![0f32; M * M];
        let mut phase = vec![0f32; M * M];
        for r in 0..M {
            for c in 0..M {
                // Non-wrapping ramp in ~[0.1, 2.5] rad (never crosses ±π).
                let phi = 0.1 + 0.08 * r as f32 + 0.05 * c as f32;
                phase[r * M + c] = phi;
                re[r * M + c] = phi.cos();
                im[r * M + c] = phi.sin();
            }
        }
        let z = Complex2::new(tensor2(re, M, M), tensor2(im, M, M));

        let allen = tensor_to_array2_f64(position_gaussian_smooth(&z, sigma).angle()).unwrap();
        let scalar =
            tensor_to_array2_f64(gaussian_smooth(tensor2(phase, M, M), sigma)).unwrap();

        let mut maxd = 0.0f64;
        for r in 0..M {
            for c in 0..M {
                maxd = maxd.max((allen[[r, c]] - scalar[[r, c]]).abs());
            }
        }
        eprintln!("Allen position-Gaussian vs scalar gaussian_smooth(phase): max diff = {maxd:.2e}");
        assert!(
            maxd < 1e-5,
            "Allen position-Gaussian must equal the scipy-validated scalar Gaussian on phase: {maxd:.2e}"
        );
    }
}
