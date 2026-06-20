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
        amp_weighted_complex_smooth, compute_magnification_jacobian, compute_vfs, delay_map,
        device, gaussian_smooth, magnification_anisotropy, phase_gradients, position_amplitude,
        position_gaussian_smooth, position_phasor_delay_subtracted, real_gradients,
        tensor_to_array2_f64, Backend, Complex2,
    };
    use crate::methods::patch_threshold::{PatchThresholdExt, PatchThresholdMethod};
    use crate::test_support::{count_differing, load_f32, load_f64};
    use agreement::{Eps, Tol};
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

    /// **Live genuine-oracle version** of `vfs_matches_allen_visual_sign_map_on_smooth_input`:
    /// builds the same smooth phase maps in Rust and compares against the GENUINE
    /// NeuroAnalysisTools `visualSignMap`, executed live in its own uv-locked
    /// period-correct env (`tests/oracle/nat/`) — no transcription, no fixture.
    /// Gated behind `oracle_live`; run with `--features oracle_live` (needs `uv`).
    #[cfg(feature = "oracle_live")]
    #[test]
    fn vfs_matches_genuine_nat_visual_sign_map_live() {
        use crate::test_support::oracle;
        use std::f64::consts::{PI, TAU};

        // Smooth (no-wrap) phase maps — the INPUT, built in Rust, handed to the
        // genuine oracle live. `meshgrid(xs, ys)`: X varies along cols, Y rows.
        let lin = |i: usize| i as f64 / (N - 1) as f64;
        let mut phi1 = Array2::<f64>::zeros((N, N));
        let mut phi2 = Array2::<f64>::zeros((N, N));
        for r in 0..N {
            for c in 0..N {
                let (x, y) = (lin(c), lin(r));
                phi1[[r, c]] = 0.8 * x + 0.15 * y;
                phi2[[r, c]] = 0.30 * (TAU * y).sin() + 0.15 * x;
            }
        }
        assert!(
            phi1.iter().chain(phi2.iter()).all(|v| v.abs() < PI),
            "inputs would wrap"
        );

        // GENUINE oracle: real NAT visualSignMap, live, in its locked env.
        let allen = oracle::nat("visualSignMap", &[phi1.clone(), phi2.clone()], &[]).remove(0);

        // Ours (same crate-internal path as the fixture test).
        let p1: Vec<f64> = phi1.iter().copied().collect();
        let p2: Vec<f64> = phi2.iter().copied().collect();
        let z_azi = Complex2::from_phase(phase_tensor(&p1));
        let z_alt = Complex2::from_phase(phase_tensor(&p2));
        let (d_azi_dx, d_azi_dy) = phase_gradients(&z_azi);
        let (d_alt_dx, d_alt_dy) = phase_gradients(&z_alt);
        let ours = tensor_to_array2_f64(compute_vfs(d_azi_dx, d_azi_dy, d_alt_dx, d_alt_dy)).unwrap();

        let mut max_same = 0.0f64;
        for i in 1..N - 1 {
            for j in 1..N - 1 {
                max_same = max_same.max((ours[[i, j]] - allen[[i, j]]).abs());
            }
        }
        eprintln!("VFS vs GENUINE NAT (live, interior): max|ours-allen|={max_same:.3e}");
        assert!(
            max_same < 5e-2,
            "VFS deviates from genuine NAT beyond discretization tolerance: {max_same:.3e}"
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

        // Compare the phasor (re, im) against cos/sin of the oracle kmap — the
        // phasor form sidesteps ±2π wrap false-diffs. f32 backend, observed
        // ≈ 2.1·ε_f32 → K=4.
        let cos_ref: Vec<f64> = kmap.iter().map(|k| k.cos()).collect();
        let sin_ref: Vec<f64> = kmap.iter().map(|k| k.sin()).collect();
        Tol::abs(4, Eps::F32).assert("kalatsky combine re", re.as_slice().expect("contiguous"), &cos_ref);
        Tol::abs(4, Eps::F32).assert("kalatsky combine im", im.as_slice().expect("contiguous"), &sin_ref);
    }

    /// `delay_map` vs SNLC `Gprocesskret.m:88-96` `delay_hor`/`delay_vert`: the
    /// hemodynamic delay (the symmetric fwd+rev component), forced into (0, π].
    /// Compared directly in radians — delay is single-valued in (0, π], so no
    /// wrap ambiguity. Fixture `combine_delay.bin` from `gen_combine_golden.m`
    /// (same ang0/ang2 inputs as the kmap golden, so the two are consistent).
    #[test]
    fn delay_map_matches_snlc_gprocesskret() {
        let a0 = load_f64(include_bytes!("../../tests/golden/fixtures/combine_ang0.bin"));
        let a2 = load_f64(include_bytes!("../../tests/golden/fixtures/combine_ang2.bin"));
        let delay = load_f64(include_bytes!("../../tests/golden/fixtures/combine_delay.bin"));

        let fwd = Complex2::from_phase(phase_tensor(&a0));
        let rev = Complex2::from_phase(phase_tensor(&a2));
        let ours = tensor_to_array2_f64(delay_map(&fwd, &rev)).unwrap();

        // Delay is an absolute angular magnitude in (0, π] (no wrap). f32 atan2 +
        // the (0,π] sign correction drift most near the flip region (here ~half
        // the product grid crosses it); observed ≈ 4.79e-5 ≈ 402·ε_f32 → K=512
        // (the phase-map class in tolerances.toml).
        Tol::abs(512, Eps::F32).assert(
            "delay_map vs SNLC Gprocesskret",
            ours.as_slice().expect("contiguous"),
            &delay,
        );
    }

    /// `magnification_anisotropy` vs SNLC `getMagFactors.m` (`prefAxisMF` +
    /// `Distrtion`): the doubled-angle anisotropy of the visual-field Jacobian.
    /// The axis is compared **circularly** (period 180°) so a pixel near the
    /// 0/180 wrap can't create a false diff; distortion is a bounded `[0,1]`
    /// magnitude → absolute diff. Fixtures from `gen_maganiso_golden.py` (the
    /// same four gradients the op consumes).
    #[test]
    fn magnification_anisotropy_matches_snlc_getmagfactors() {
        const M: usize = 48;
        let g = |b: &[u8]| tensor2(load_f64(b).iter().map(|&v| v as f32).collect(), M, M);
        let dhdx = g(include_bytes!("../../tests/golden/fixtures/maganiso_dhdx.bin"));
        let dhdy = g(include_bytes!("../../tests/golden/fixtures/maganiso_dhdy.bin"));
        let dvdx = g(include_bytes!("../../tests/golden/fixtures/maganiso_dvdx.bin"));
        let dvdy = g(include_bytes!("../../tests/golden/fixtures/maganiso_dvdy.bin"));
        let axis_gold = load_f64(include_bytes!("../../tests/golden/fixtures/maganiso_axis.bin"));
        let dist_gold =
            load_f64(include_bytes!("../../tests/golden/fixtures/maganiso_distortion.bin"));

        let (axis_t, dist_t) = magnification_anisotropy(dhdx, dhdy, dvdx, dvdy);
        let axis = tensor_to_array2_f64(axis_t).unwrap();
        let dist = tensor_to_array2_f64(dist_t).unwrap();

        // Grounded via the shared `agreement` comparator — no hand-rolled loop,
        // no magic numbers. `distortion = |Res|` is well-conditioned (modulus,
        // κ≈1) → absolute op-count K=16·ε_f32. `axis = ∠Res/2` is the argument of
        // a complex number, whose analytic condition number is κ = 1/|Res| =
        // 1/distortion (ill-conditioned where isotropic) → wrap-180 (an axis),
        // ×90/π (∠Res/2 → deg), scaled by the MEASURED κ_max from this fixture.
        let kappa_max = 1.0 / dist_gold.iter().copied().fold(f64::INFINITY, f64::min);
        Tol::abs(16, Eps::F32).assert(
            "maganiso distortion",
            dist.as_slice().expect("contiguous"),
            &dist_gold,
        );
        Tol::wrap(180.0, 2, Eps::F32, 90.0 / std::f64::consts::PI)
            .with_kappa(kappa_max)
            .assert("maganiso axis", axis.as_slice().expect("contiguous"), &axis_gold);
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

        // Pure f64 on both sides (our formula == Allen's) → machine precision;
        // observed ≈ 2.1e-14 ≈ 96·ε_f64 → K=128, F64 base. (Was a magic 1e-9 —
        // ~5 orders too loose to catch a real f64 regression.)
        let ours: Vec<f64> = (0..N * N)
            .map(|i| crate::math::eccentricity_pixel_deg(alt[i], azi[i], ALT_C, AZI_C))
            .collect();
        Tol::abs(128, Eps::F64).assert("garrett eccentricity vs Allen", &ours, &golden);
    }

    /// **Live genuine-oracle version**: builds altitude/azimuth degree maps in
    /// Rust and compares our `eccentricity_pixel_deg` against the GENUINE
    /// NeuroAnalysisTools `eccentricityMap`, executed live. Gated `oracle_live`.
    #[cfg(feature = "oracle_live")]
    #[test]
    fn eccentricity_matches_genuine_nat_eccentricitymap_live() {
        use crate::test_support::oracle;
        const ALT_C: f64 = 5.0;
        const AZI_C: f64 = 10.0;
        // Smooth, non-degenerate degree ramps (the formula is per-pixel; any
        // non-trivial maps exercise it).
        let lin = |i: usize, lo: f64, hi: f64| lo + (hi - lo) * i as f64 / (N - 1) as f64;
        let alt = Array2::from_shape_fn((N, N), |(r, _)| lin(r, -20.0, 20.0));
        let azi = Array2::from_shape_fn((N, N), |(_, c)| lin(c, -30.0, 30.0));

        let genuine = oracle::nat(
            "eccentricityMap",
            &[alt.clone(), azi.clone()],
            &[("altCenter", ALT_C), ("aziCenter", AZI_C)],
        )
        .remove(0);
        let ours: Vec<f64> = (0..N * N)
            .map(|i| crate::math::eccentricity_pixel_deg(alt.as_slice().unwrap()[i], azi.as_slice().unwrap()[i], ALT_C, AZI_C))
            .collect();
        Tol::abs(128, Eps::F64).assert(
            "eccentricity vs GENUINE NAT eccentricityMap (live)",
            &ours,
            genuine.as_slice().unwrap(),
        );
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

        // f32 gradients + a cancellation difference in det; observed ≈ 4.7e-6 ≈
        // 40·ε_f32 → K=64. (Was a magic 1e-3.)
        Tol::abs(64, Eps::F32).assert("det J vs Allen", got.as_slice().expect("contiguous"), &det_g);

        // Inversion check: the `magnification` leaf is the reciprocal CMF. The
        // 1/det amplifies f32 error; observed ≈ 1.55e-5 ≈ 130·ε_f32 → K=256 (on
        // this smooth synthetic det stays away from zero, so a flat K suffices;
        // the production map is κ-grounded in tolerances.toml). Was a magic 1e-2.
        let labels = Array2::from_elem((MG, MG), 1i32); // all in-ROI
        let cmf = crate::math::cortical_magnification_factor(&got, &labels);
        Tol::abs(256, Eps::F32).assert("CMF (1/|det J|)", cmf.as_slice().expect("contiguous"), &cmf_g);
    }

    /// **Live genuine-oracle, CLASS METHOD**: our `compute_magnification_jacobian`
    /// (|det J|) vs the GENUINE `RetinotopicMappingTrial._getDeterminantMap`,
    /// driven on non-affine position maps built in Rust. Gated `oracle_live`.
    #[cfg(feature = "oracle_live")]
    #[test]
    fn magnification_matches_genuine_nat_determinant_map_live() {
        use crate::test_support::oracle;
        const MG: usize = 48;
        // Non-affine alt/azi degree maps → spatially-varying Jacobian.
        let pos = |r: usize, c: usize| {
            let (x, y) = (c as f64 / (MG - 1) as f64, r as f64 / (MG - 1) as f64);
            (10.0 * x + 5.0 * y * y + 3.0 * x * y, 20.0 * y - 4.0 * x * x + 2.0 * x * y)
        };
        let alt = Array2::from_shape_fn((MG, MG), |(r, c)| pos(r, c).0);
        let azi = Array2::from_shape_fn((MG, MG), |(r, c)| pos(r, c).1);

        let genuine = oracle::nat("getDeterminantMap", &[alt.clone(), azi.clone()], &[]).remove(0);

        let alt_t = tensor2(alt.iter().map(|&v| v as f32).collect(), MG, MG);
        let azi_t = tensor2(azi.iter().map(|&v| v as f32).collect(), MG, MG);
        let (d_alt_dx, d_alt_dy) = real_gradients(alt_t);
        let (d_azi_dx, d_azi_dy) = real_gradients(azi_t);
        let mag = compute_magnification_jacobian(d_azi_dx, d_azi_dy, d_alt_dx, d_alt_dy, 1.0, 1.0);
        let got = tensor_to_array2_f64(mag).unwrap();

        // f32 gradients + det cancellation vs genuine f64 (same as the fixture test, K=64).
        Tol::abs(64, Eps::F32).assert(
            "det J vs GENUINE NAT _getDeterminantMap (live)",
            got.as_slice().expect("contiguous"),
            genuine.as_slice().expect("contiguous"),
        );
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
        // f32 separable convolution vs scipy f64; observed ≈ 5.9e-7 ≈ 5·ε_f32 →
        // K=8. (Was a magic 1e-4.)
        Tol::abs(8, Eps::F32).assert(
            "tensor gaussian_smooth vs scipy",
            ours.as_slice().expect("contiguous"),
            &golden,
        );
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

        // f32 length-24 DFT reduction vs numpy f64; observed ≈ 8.4e-6 ≈ 70·ε_f32
        // → K=128. (Was a magic 1e-3.)
        Tol::abs(128, Eps::F32).assert("F1 DFT re vs numpy", re.as_slice().expect("contiguous"), &f1_re);
        Tol::abs(128, Eps::F32).assert("F1 DFT im vs numpy", im.as_slice().expect("contiguous"), &f1_im);
    }

    /// **Live library-primitive oracle**: stage-0 single-bin F1 DFT
    /// (`dft_projection_at_freq`) vs the GENUINE `numpy.fft.fft(...)[1]`, executed
    /// live in the uv-locked env. numpy's FFT *is* the oracle (the bridge only
    /// calls it). The movie is built fresh in Rust as f32 and the SAME f32 values
    /// (widened to f64) are handed to numpy, so the only difference is the
    /// length-24 reduction arithmetic — not an f32-vs-f64 input gap. A constant DC
    /// offset confirms bin-1 rejects DC. Gated behind `oracle_live`.
    #[cfg(feature = "oracle_live")]
    #[test]
    fn dft_projection_matches_genuine_numpy_fft_live() {
        use crate::test_support::oracle;
        const NF: usize = 24;
        const HW: usize = 16;
        // Per-pixel sinusoid DC + A·cos(2π t/n + φ); A varies along x, φ over the
        // full circle along y (covers amplitude + phase), DC tests bin-1 rejection.
        let mut movie_f32 = vec![0.0f32; NF * HW * HW];
        for t in 0..NF {
            for r in 0..HW {
                for c in 0..HW {
                    let a = 1.0 + 0.5 * (c as f64 / HW as f64);
                    let phi = 2.0 * std::f64::consts::PI * (r as f64 / HW as f64) - std::f64::consts::PI;
                    let v = 5.0 + a * (2.0 * std::f64::consts::PI * t as f64 / NF as f64 + phi).cos();
                    movie_f32[t * HW * HW + r * HW + c] = v as f32;
                }
            }
        }
        // numpy gets the exact f32 values widened to f64 (isolate the reduction).
        let movie_f64: Vec<f64> = movie_f32.iter().map(|&v| v as f64).collect();
        let genuine = oracle::nat_raw_nd(
            "numpy_fft_bin",
            &[(movie_f64.as_slice(), &[NF, HW, HW])],
            &[("bin", 1.0)],
        );
        let g_re = genuine[0].f64();
        let g_im = genuine[1].f64();

        let movie =
            Tensor::<Backend, 3>::from_data(TensorData::new(movie_f32, [NF, HW, HW]), &device());
        let f1 = crate::compute::dft_projection_at_freq(movie, 1.0, 1.0 / NF as f64);
        let re = tensor_to_array2_f64(f1.real()).unwrap();
        let im = tensor_to_array2_f64(f1.imag()).unwrap();

        let mut maxd = 0.0f64;
        for r in 0..HW {
            for c in 0..HW {
                maxd = maxd.max((re[[r, c]] - g_re[[r, c]]).abs());
                maxd = maxd.max((im[[r, c]] - g_im[[r, c]]).abs());
            }
        }
        eprintln!("F1 DFT vs GENUINE numpy.fft (live): max diff = {maxd:.3e}");
        // f32 length-24 reduction vs numpy f64; same K=128·ε_f32 budget as the
        // frozen-fixture test above (observed ≈ 8e-6).
        assert!(
            maxd < 128.0 * f32::EPSILON as f64,
            "dft_projection diverges from genuine numpy.fft: {maxd:.3e}"
        );
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
        // f32 coherence vs numpy f64; observed ≈ 1.9e-7 ≈ 1.6·ε_f32 → K=4.
        Tol::abs(4, Eps::F32).assert(
            "reliability vs coherence formula",
            rel.as_slice().expect("contiguous"),
            &exp,
        );
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
        // f32 magnitude vs numpy f64; observed ≈ 1.2e-7 ≈ 1·ε_f32 → K=2.
        Tol::abs(2, Eps::F32).assert(
            "position_amplitude vs SNLC magS",
            amp.as_slice().expect("contiguous"),
            &exp,
        );
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

        // (1) Baseline F0 = Allen np.mean(movie, axis=0). Positive magnitude,
        // f64 — relative, K covers the 20-frame reduction. Observed 0.
        let baseline = temporal_mean_baseline(&frames);
        Tol::rel(64, Eps::F64, 64).assert(
            "baseline F0 vs Allen mean",
            baseline.as_slice().expect("contiguous"),
            &f0_exp,
        );

        // (2) dF/F (faithful path, floor=0) = Allen (F − F0)/F0, in f32; observed
        // 0 → K=16 covers the f32 (sub, div) forward error.
        let idx: Vec<usize> = (0..N).collect();
        let dff_t = frames_u16_subset_to_dff_tensor(&frames, &idx, &baseline, 0.0, true);
        let got: Vec<f64> = dff_t
            .into_data()
            .to_vec::<f32>()
            .expect("dff to vec")
            .iter()
            .map(|&v| f64::from(v))
            .collect();
        let dff_ref: Vec<f64> = dff_exp.iter().map(|&v| f64::from(v)).collect();
        Tol::abs(16, Eps::F32).assert("dF/F vs Allen normalizeMovie", &got, &dff_ref);
    }

    /// **Response-normalization equivalence (item 4).** The oracle-faithful
    /// absolute-ΔF F1 (`OracleAbsoluteDeltaF`, `F − F0`, SNLC `Gf1image.m` /
    /// Allen `generatePhaseMap2`) and OpenISI's fractional ΔF/F F1
    /// (`OpenIsiFractionalDff`, `(F − F0)/F0`) are related by a **positive-real
    /// per-pixel scale** `1/F0`. So the bin-1 DFT obeys, per pixel:
    ///
    /// ```text
    /// F1_fractional · F0  ==  F1_absolute      (complex)
    /// ```
    ///
    /// which proves BOTH halves of the audit finding at once: identical **phase**
    /// (the `1/F0` factor is invisible to `arg`), and the exact **amplitude**
    /// divergence `|F1_fractional| = |F1_absolute| / F0` that the oracles don't
    /// carry. Reuses the `gen_dff_golden.py` movie (all F0 > 0); floor 0 so the
    /// fractional denominator is exactly `F0`.
    #[test]
    fn response_normalization_absolute_vs_fractional_phase_equivalence() {
        use crate::compute::{
            dft_projection_at_freq, frames_u16_subset_to_dff_tensor, temporal_mean_baseline,
        };
        use ndarray::Array3;
        const N: usize = 20;
        const H: usize = 16;
        const W: usize = 16;
        let fb: &[u8] = include_bytes!("../../tests/golden/fixtures/dff_frames.bin");
        let frames_u16: Vec<u16> = fb
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes(c.try_into().unwrap()))
            .collect();
        let frames = Array3::from_shape_fn((N, H, W), |(t, r, c)| frames_u16[t * H * W + r * W + c]);
        let baseline = temporal_mean_baseline(&frames);
        let idx: Vec<usize> = (0..N).collect();

        // Fractional ΔF/F (divide=true, floor 0 ⇒ denom = F0) and absolute ΔF
        // (divide=false, denom = 1) — the two ResponseNormalization variants.
        let frac = frames_u16_subset_to_dff_tensor(&frames, &idx, &baseline, 0.0, true);
        let absolute = frames_u16_subset_to_dff_tensor(&frames, &idx, &baseline, 0.0, false);

        // Bin-1 DFT (any dt/freq works — the identity holds for the linear DFT at
        // any single frequency; the per-pixel 1/F0 factors straight through).
        let (dt, freq) = (1.0_f64, 1.0 / N as f64);
        let cm_frac = dft_projection_at_freq(frac, dt, freq);
        let cm_abs = dft_projection_at_freq(absolute, dt, freq);
        let fr_re = tensor_to_array2_f64(cm_frac.real()).unwrap();
        let fr_im = tensor_to_array2_f64(cm_frac.imag()).unwrap();
        let ab_re = tensor_to_array2_f64(cm_abs.real()).unwrap();
        let ab_im = tensor_to_array2_f64(cm_abs.imag()).unwrap();

        let f0 = baseline.as_slice().expect("contiguous");
        let pred_re: Vec<f64> = fr_re.iter().zip(f0).map(|(&v, &b)| v * b).collect();
        let pred_im: Vec<f64> = fr_im.iter().zip(f0).map(|(&v, &b)| v * b).collect();
        let ab_re_v: Vec<f64> = ab_re.iter().copied().collect();
        let ab_im_v: Vec<f64> = ab_im.iter().copied().collect();

        // K grounded to MEASURED drift: the round-trip is f32 DFT, then a divide
        // (by F0) reintroduced by a multiply, over a ~10³-count F1 magnitude.
        // Observed max rel ≈ 30·ε_f32 → K = 64 bounds it (power-of-two cover).
        Tol::rel(64, Eps::F32, 64).assert("Re: F1_fractional·F0 vs F1_absolute", &pred_re, &ab_re_v);
        Tol::rel(64, Eps::F32, 64).assert("Im: F1_fractional·F0 vs F1_absolute", &pred_im, &ab_im_v);
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

        // Median is a selection + (even N) average of two middles — essentially
        // exact f64; observed 0 → K=8 covers the one averaging op.
        let med = temporal_median_baseline(&frames);
        Tol::abs(8, Eps::F64).assert(
            "median baseline vs np.median",
            med.as_slice().expect("contiguous"),
            &exp,
        );
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

        // Phase is angular (radians, period 2π) → wrap-aware; observed ≈ 2.4e-7
        // ≈ 2·ε_f32 → K=4.
        Tol::wrap(std::f64::consts::TAU, 4, Eps::F32, 1.0).assert(
            "amp-weighted vs SNLC complex-smoothing phase",
            ours.as_slice().expect("contiguous"),
            snlc.as_slice().expect("contiguous"),
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

        // Same f32 Gaussian both ways on a non-wrapping ramp; observed ≈ 1.2e-7
        // ≈ 1·ε_f32 → K=2.
        Tol::abs(2, Eps::F32).assert(
            "Allen position-Gaussian vs scalar gaussian on phase",
            allen.as_slice().expect("contiguous"),
            scalar.as_slice().expect("contiguous"),
        );
    }
}
