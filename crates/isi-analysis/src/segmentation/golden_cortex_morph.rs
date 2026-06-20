//! Golden cross-validation of the SNLC-cortex binary morphology against
//! Octave's Image Processing Toolbox.
//!
//! `SnlcGarrett2014ImBound` is a *faithful-reproduction* claim of
//! `getMouseAreasX.m`, built from `imopen(disk2)`, `imclose(disk10)`,
//! `imfill('holes')`, `imdilate(disk3)`, and the largest 4-connected
//! component. The disk structuring element is already confirmed bit-identical
//! to `strel('disk',R,0)` (see `gen_cortex_morph_golden.m` provenance); this
//! test confirms the *operations* — including border padding (erode pads 1,
//! dilate pads 0) and hole-fill / largest-CC semantics — reproduce Octave on a
//! mask that deliberately stresses borders, holes, gap-bridging, and specks.
//!
//! Fixtures are produced by `tests/golden/gen_cortex_morph_golden.m`
//! (uint8, row-major, 96x96). Exact-match expected: binary in, binary out.

#[cfg(test)]
mod tests {
    use crate::methods::cortex_source::{CortexResolveContext, CortexSourceExt, CortexSourceMethod};
    use crate::methods::patch_extraction::raw_patch_map_allen;
    use crate::segmentation::connectivity::{
        dilation_patches2_allen, keep_largest_component, label_4conn,
    };
    use crate::segmentation::morphology::{
        binary_closing_cross, binary_closing_disk, binary_dilation_disk, binary_fill_holes,
        binary_opening_cross, binary_opening_disk, binary_skeletonize_skimage, gaussian_smooth_f64,
    };
    use crate::test_support::{count_differing, load_f64, load_i32};
    use ndarray::Array2;

    const N: usize = 96;

    fn load_mask(bytes: &[u8]) -> Array2<bool> {
        assert_eq!(bytes.len(), N * N, "fixture size mismatch");
        Array2::from_shape_fn((N, N), |(r, c)| bytes[r * N + c] != 0)
    }

    /// `SnlcMagThreshold` (the `overlaymaps.m` response-magnitude ROI gate)
    /// matches the verbatim Octave lines: `mag = magf.^1.1; mag = mag − min; mag
    /// = mag/max; magROI = mag ≥ .12`. Boolean mask ⇒ exact match. Fixtures from
    /// `gen_magroi_golden.m` (40×48).
    #[test]
    fn snlc_mag_threshold_roi_matches_overlaymaps() {
        use crate::methods::cortex_source::snlc_mag_threshold_roi;
        let meta = load_f64(include_bytes!("../../tests/golden/fixtures/magroi_meta.bin"));
        let (h, w) = (meta[0] as usize, meta[1] as usize);
        let (exponent, threshold) = (meta[2], meta[3]);
        let inp = load_f64(include_bytes!("../../tests/golden/fixtures/magroi_in.bin"));
        let magf = Array2::from_shape_fn((h, w), |(r, c)| inp[r * w + c]);
        let expected: &[u8] = include_bytes!("../../tests/golden/fixtures/magroi_out.bin");
        let roi = snlc_mag_threshold_roi(&magf, exponent, threshold);
        let d = count_differing(&roi, expected);
        eprintln!("snlc_mag_threshold_roi vs overlaymaps.m: differing px = {d}");
        assert_eq!(d, 0, "mag-threshold ROI diverged from overlaymaps.m");
    }

    #[test]
    fn cortex_morphology_matches_octave_strel_ops() {
        let input = load_mask(include_bytes!("../../tests/golden/fixtures/cortex_morph_input.bin"));
        let g_open: &[u8] = include_bytes!("../../tests/golden/fixtures/cortex_morph_open.bin");
        let g_close: &[u8] = include_bytes!("../../tests/golden/fixtures/cortex_morph_close.bin");
        let g_fill: &[u8] = include_bytes!("../../tests/golden/fixtures/cortex_morph_fill.bin");
        let g_dilate: &[u8] = include_bytes!("../../tests/golden/fixtures/cortex_morph_dilate.bin");
        let g_largest: &[u8] =
            include_bytes!("../../tests/golden/fixtures/cortex_morph_largestcc.bin");

        let cases: [(&str, Array2<bool>, &[u8]); 5] = [
            ("imopen(disk2)", binary_opening_disk(&input, 2), g_open),
            ("imclose(disk10)", binary_closing_disk(&input, 10), g_close),
            ("imfill(holes)", binary_fill_holes(&input), g_fill),
            ("imdilate(disk3)", binary_dilation_disk(&input, 3), g_dilate),
            ("largest_cc(4conn)", keep_largest_component(&input), g_largest),
        ];

        let mut total = 0usize;
        for (name, ours, golden) in &cases {
            let d = count_differing(ours, golden);
            eprintln!("  {name:20} differing px = {d}");
            total += d;
        }
        assert_eq!(
            total, 0,
            "SNLC cortex morphology diverges from Octave strel ops (see per-op counts above)"
        );
    }

    /// End-to-end: the real `CortexSourceMethod::resolve` for `SnlcGarrett2014ImBound`
    /// (threshold `1.5·std(VFS)·0.5` → imopen2 → imclose10 → fill → imdilate3 →
    /// fill → largest 4-CC) on a |VFS| field, against the same sequence in
    /// Octave. Input has a wide threshold margin so the std N-vs-(N−1)
    /// convention cannot flip a pixel — this validates the orchestration.
    #[test]
    fn snlc_cortex_endtoend_matches_octave() {
        let vfs_flat = load_f64(include_bytes!("../../tests/golden/fixtures/cortex_full_vfs.bin"));
        assert_eq!(vfs_flat.len(), N * N);
        let vfs = Array2::from_shape_fn((N, N), |(r, c)| vfs_flat[r * N + c]);
        let golden: &[u8] = include_bytes!("../../tests/golden/fixtures/cortex_full_golden.bin");

        let ctx = CortexResolveContext {
            shape: (N, N),
            reliability: None,
            vfs_smoothed: Some(&vfs),
            response_magnitude: None,
        };
        let method = CortexSourceMethod::SnlcGarrett2014ImBound {
            k: 1.5,
            close: 10,
            dilate: 3,
        };
        let cortex = method.apply(&ctx).expect("resolve cortex");

        let d = count_differing(&cortex, golden);
        eprintln!("  SNLC cortex end-to-end: differing px = {d}");
        assert_eq!(d, 0, "SNLC cortex orchestration diverges from Octave");
    }

    /// Allen patch-extraction morphology primitives vs scipy.ndimage (the
    /// library `RetinotopicMapping.py` uses): `binary_opening_cross` /
    /// `binary_closing_cross` (iterations=3, 4-conn cross, scipy
    /// `border_value=0` so the edge erodes) and `label_4conn` (scipy default
    /// `ni.label` structure). Fixtures from `gen_patch_morph_golden.py`.
    #[test]
    fn allen_cross_morphology_matches_scipy() {
        let input = load_mask(include_bytes!("../../tests/golden/fixtures/cortex_morph_input.bin"));
        let g_open: &[u8] = include_bytes!("../../tests/golden/fixtures/patch_morph_open.bin");
        let g_close: &[u8] = include_bytes!("../../tests/golden/fixtures/patch_morph_close.bin");

        let open = binary_opening_cross(&input, 3);
        let close = binary_closing_cross(&input, 3);
        let d_open = count_differing(&open, g_open);
        let d_close = count_differing(&close, g_close);
        let (_, n) = label_4conn(&input);
        eprintln!("  cross open diff={d_open}  close diff={d_close}  label_4conn n={n}");
        assert_eq!(d_open, 0, "binary_opening_cross diverges from scipy");
        assert_eq!(d_close, 0, "binary_closing_cross diverges from scipy");
        assert_eq!(n, 7, "label_4conn count diverges from scipy ni.label");
    }

    /// **Live library-primitive oracle**: our `binary_opening_cross` /
    /// `binary_closing_cross` (4-conn cross, `border_value=0`) vs the GENUINE
    /// `scipy.ndimage.binary_opening`/`binary_closing` with the same cross
    /// structure, executed live in the uv-locked env. scipy is the oracle; the
    /// bridge only calls it. A blob with a thin spur (opening erodes it) and a
    /// notch (closing fills it) exercises both. Gated behind `oracle_live`.
    #[cfg(feature = "oracle_live")]
    #[test]
    fn cross_morphology_matches_genuine_scipy_live() {
        use crate::segmentation::morphology::{binary_closing_cross, binary_opening_cross};
        use crate::test_support::oracle;
        use ndarray::Array2;
        const M: usize = 48;
        let mut f = Array2::<f64>::zeros((M, M));
        for r in 12..36 {
            for c in 12..36 {
                f[[r, c]] = 1.0; // main blob
            }
        }
        for r in 22..26 {
            for c in 36..44 {
                f[[r, c]] = 1.0; // thin spur — opening erodes it
            }
        }
        for r in 18..30 {
            for c in 22..26 {
                f[[r, c]] = 0.0; // interior notch — closing fills it
            }
        }
        let mask = f.mapv(|v| v != 0.0);

        let mut total = 0usize;
        for (name, fname, ours) in [
            ("opening", "scipy_binary_opening_cross", binary_opening_cross(&mask, 3)),
            ("closing", "scipy_binary_closing_cross", binary_closing_cross(&mask, 3)),
        ] {
            let genuine = oracle::nat_raw(fname, &[f.clone()], &[("iterations", 3.0)])
                .remove(0)
                .bool();
            let d = ndarray::Zip::from(&ours)
                .and(&genuine)
                .fold(0usize, |a, &o, &g| a + (o != g) as usize);
            eprintln!("  cross {name} vs GENUINE scipy (live): differing px = {d}");
            total += d;
        }
        assert_eq!(total, 0, "cross morphology diverges from genuine scipy binary_opening/closing");
    }

    /// Allen `_getRawPatchMap` orchestration (open → label → per-patch close →
    /// recombine) via the extracted production helper `raw_patch_map_allen`,
    /// vs the same composition in scipy. Fixture from
    /// `gen_patch_extraction_golden.py`.
    #[test]
    fn allen_raw_patch_map_matches_scipy() {
        let imseg = load_mask(include_bytes!("../../tests/golden/fixtures/cortex_morph_input.bin"));
        let golden: &[u8] = include_bytes!("../../tests/golden/fixtures/patchext_rawmap.bin");
        let ours = raw_patch_map_allen(&imseg, 3, 3);
        let d = count_differing(&ours, golden);
        eprintln!("Allen _getRawPatchMap vs scipy: differing px = {d}");
        assert_eq!(d, 0, "raw_patch_map_allen diverges from scipy _getRawPatchMap");
    }

    /// **Live genuine-oracle, CLASS METHOD**: drives the real
    /// `RetinotopicMappingTrial._getRawPatchMap` (constructed in the bridge with
    /// `signMapThr=0.5` so its threshold reproduces our binary input) and compares
    /// our `raw_patch_map_allen`. This validates the orchestration against Allen's
    /// actual method, not a scipy transcription of it. Gated behind `oracle_live`.
    #[cfg(feature = "oracle_live")]
    #[test]
    fn raw_patch_map_matches_genuine_nat_live() {
        use crate::test_support::oracle;
        use ndarray::Array2;
        const M: usize = 96;
        // A few patches large enough to survive opening(iter=3), built in Rust.
        let mut imseg_f = Array2::<f64>::zeros((M, M));
        for (r0, r1, c0, c1) in [(10, 30, 10, 30), (50, 80, 40, 75), (20, 35, 60, 78)] {
            for r in r0..r1 {
                for c in c0..c1 {
                    imseg_f[[r, c]] = 1.0;
                }
            }
        }
        let imseg = imseg_f.mapv(|v| v != 0.0);

        let genuine = oracle::nat_raw(
            "getRawPatchMap",
            &[imseg_f],
            &[("signMapThr", 0.5), ("openIter", 3.0), ("closeIter", 3.0)],
        )
        .remove(0)
        .bool();
        let ours = raw_patch_map_allen(&imseg, 3, 3);

        let d = ndarray::Zip::from(&ours)
            .and(&genuine)
            .fold(0usize, |a, &o, &g| a + (o != g) as usize);
        eprintln!("_getRawPatchMap vs GENUINE NAT method (live): differing px = {d}");
        assert_eq!(d, 0, "raw_patch_map_allen diverges from genuine NAT _getRawPatchMap");
    }

    /// **Live genuine-oracle, SNLC/Octave**: our per-patch sign assignment vs the
    /// GENUINE SNLC `getPatchSign` (`sign(mean(imsign over patch))`), executed live
    /// via Octave. Tested on non-zero-mean patches (where they agree). The
    /// zero-mean case is a documented deviation — MATLAB `sign(0)=0` (undefined
    /// patch sign) vs our deterministic `+1` tie-break — recorded in the ledger,
    /// not silently reconciled. Gated behind `oracle_live`.
    #[cfg(feature = "oracle_live")]
    #[test]
    fn patch_sign_matches_genuine_snlc_getpatchsign_live() {
        use crate::segmentation::connectivity::{label_4conn, patches_from_labels_majority_sign};
        use crate::test_support::oracle;
        use ndarray::Array2;
        const M: usize = 32;
        let mut im = Array2::<f64>::zeros((M, M));
        let mut sgn = Array2::<f64>::zeros((M, M));
        let mut paint = |r0: usize, r1: usize, c0: usize, c1: usize, s: f64, im: &mut Array2<f64>, sgn: &mut Array2<f64>| {
            for r in r0..r1 {
                for c in c0..c1 {
                    im[[r, c]] = 1.0;
                    sgn[[r, c]] = s;
                }
            }
        };
        // 3 patches, clearly non-zero mean → unambiguous signs (+1, -1, +1).
        paint(3, 9, 3, 9, 0.7, &mut im, &mut sgn);
        paint(12, 18, 14, 20, -0.5, &mut im, &mut sgn);
        paint(22, 28, 5, 12, 0.3, &mut im, &mut sgn);

        // Genuine getPatchSign returns a per-pixel map: patch pixels = sign+1.1
        // (label-INVARIANT — sidesteps our row-major vs MATLAB column-major
        // bwlabel ordering, which is not a divergence).
        let patch_sign_map = oracle::snlc("getPatchSign", &[im.clone(), sgn.clone()], &[]).remove(0);
        let (labels, n) = label_4conn(&im.mapv(|v| v != 0.0));
        let patches = patches_from_labels_majority_sign(&labels, n, &sgn);

        let mut mismatch = 0usize;
        for p in &patches {
            for r in 0..M {
                for c in 0..M {
                    if p.mask[[r, c]] {
                        let genuine_sign = (patch_sign_map[[r, c]] - 1.1).round() as i8;
                        if genuine_sign != p.sign {
                            mismatch += 1;
                        }
                    }
                }
            }
        }
        eprintln!("getPatchSign (live, region-wise): mismatching px = {mismatch}");
        assert_eq!(mismatch, 0, "patch signs diverge from genuine SNLC getPatchSign (non-zero-mean)");
    }

    /// `gaussian_smooth_f64` vs scipy `ni.gaussian_filter` (what Allen's
    /// `_getSignMap` / `phaseFilter` call): scipy defaults `truncate=4.0`,
    /// `mode='reflect'`. Fixture from `gen_gaussian_golden.py`.
    #[test]
    fn gaussian_smooth_matches_scipy_gaussian_filter() {
        let inp = load_f64(include_bytes!("../../tests/golden/fixtures/gauss_input.bin"));
        let golden = load_f64(include_bytes!("../../tests/golden/fixtures/gauss_sigma4.bin"));
        let input = Array2::from_shape_fn((N, N), |(r, c)| inp[r * N + c]);

        let out = gaussian_smooth_f64(&input, 4.0);
        let mut maxd = 0.0f64;
        for r in 0..N {
            for c in 0..N {
                maxd = maxd.max((out[[r, c]] - golden[r * N + c]).abs());
            }
        }
        eprintln!("gaussian_smooth vs scipy (sigma=4): max diff = {maxd:.3e}");
        assert!(
            maxd < 1e-6,
            "gaussian_smooth diverges from scipy gaussian_filter: {maxd:.3e}"
        );
    }

    /// **Live library-primitive oracle**: our `gaussian_smooth_f64` vs
    /// `scipy.ndimage.gaussian_filter` (reflect, truncate=4) computed LIVE in the
    /// NAT env's pinned scipy 1.9.3 — the library is the genuine oracle, and it is
    /// computed each run (no frozen fixture to drift; condition 6). Gated `oracle_live`.
    #[cfg(feature = "oracle_live")]
    #[test]
    fn gaussian_smooth_matches_genuine_scipy_live() {
        use crate::test_support::oracle;
        use ndarray::Array2;
        const G: usize = 96;
        let input = Array2::from_shape_fn((G, G), |(r, c)| {
            let (x, y) = (c as f64 / G as f64, r as f64 / G as f64);
            x + 0.5 * y
                + (-(((r as f64 - 30.0).powi(2) + (c as f64 - 40.0).powi(2)) / 100.0)).exp()
        });
        let genuine = oracle::nat("scipy_gaussian_filter", &[input.clone()], &[("sigma", 4.0)]).remove(0);
        let ours = gaussian_smooth_f64(&input, 4.0);
        let mut maxd = 0.0f64;
        for r in 0..G {
            for c in 0..G {
                maxd = maxd.max((ours[[r, c]] - genuine[[r, c]]).abs());
            }
        }
        eprintln!("gaussian_smooth vs GENUINE scipy (live): max diff = {maxd:.3e}");
        assert!(maxd < 1e-6, "gaussian_smooth diverges from live scipy gaussian_filter: {maxd:.3e}");
    }

    /// Load a square uint8 fixture of explicit side `n` as a bool mask.
    fn load_mask_n(bytes: &[u8], n: usize) -> Array2<bool> {
        assert_eq!(bytes.len(), n * n, "fixture size mismatch");
        Array2::from_shape_fn((n, n), |(r, c)| bytes[r * n + c] != 0)
    }

    /// `binary_skeletonize_skimage` vs skimage `skeletonize` — the exact
    /// function Allen `dilationPatches2` (`RetinotopicMapping.py` L201) calls.
    /// Our Rust ports skimage's `_fast_skeletonize` 256-entry LUT verbatim
    /// (skimage's variant differs from a textbook Zhang-Suen by a few px on
    /// thin features, so faithfulness to Allen requires matching skimage, not
    /// the textbook). Fixtures from `gen_skeletonize_golden.py`.
    #[test]
    fn skeletonize_matches_skimage() {
        const M: usize = 64;
        let cases: [(&str, &[u8], &[u8]); 3] = [
            (
                "block",
                include_bytes!("../../tests/golden/fixtures/skel_block_in.bin"),
                include_bytes!("../../tests/golden/fixtures/skel_block_out.bin"),
            ),
            (
                "halo",
                include_bytes!("../../tests/golden/fixtures/skel_halo_in.bin"),
                include_bytes!("../../tests/golden/fixtures/skel_halo_out.bin"),
            ),
            (
                "bridge",
                include_bytes!("../../tests/golden/fixtures/skel_bridge_in.bin"),
                include_bytes!("../../tests/golden/fixtures/skel_bridge_out.bin"),
            ),
        ];
        let mut total = 0usize;
        for (name, inp, golden) in &cases {
            let ours = binary_skeletonize_skimage(&load_mask_n(inp, M));
            let d = count_differing(&ours, golden);
            eprintln!("  skeletonize {name:8} differing px = {d}");
            total += d;
        }
        assert_eq!(
            total, 0,
            "binary_skeletonize_zs diverges from skimage skeletonize (per-case counts above)"
        );
    }

    /// **Live library-primitive oracle**: our `label_4conn` vs the GENUINE
    /// `scipy.ndimage.label` (4-conn cross structure), executed live in the
    /// uv-locked env. The library *is* the oracle here (no authored logic in the
    /// bridge). Compared label-invariantly: a connected-component labeling is
    /// only defined up to a relabeling, so we assert the two induce the SAME
    /// partition (same background, and a consistent bijection ours↔genuine over
    /// foreground), not bit-identical label integers. Gated behind `oracle_live`.
    #[cfg(feature = "oracle_live")]
    #[test]
    fn label4conn_matches_genuine_scipy_live() {
        use crate::segmentation::connectivity::label_4conn;
        use crate::test_support::oracle;
        use ndarray::Array2;
        use std::collections::HashMap;
        const M: usize = 48;
        // Several disjoint blobs + a diagonal-only contact (must NOT merge under
        // 4-connectivity) to exercise the structure choice.
        let mut f = Array2::<f64>::zeros((M, M));
        let mut paint = |r0: usize, r1: usize, c0: usize, c1: usize| {
            for r in r0..r1 {
                for c in c0..c1 {
                    f[[r, c]] = 1.0;
                }
            }
        };
        paint(3, 9, 3, 9); // blob A
        paint(3, 9, 20, 27); // blob B
        paint(20, 30, 5, 16); // blob C
        // D and E touch only diagonally at (33,33)/(34,34): stay separate (4-conn).
        paint(30, 34, 30, 34);
        paint(34, 38, 34, 38);
        let mask = f.mapv(|v| v != 0.0);

        let genuine = oracle::nat_raw("scipy_label", &[f.clone()], &[]).remove(0).i32();
        let (ours, _n) = label_4conn(&mask);

        // Partition equivalence: background agrees pixel-wise, and the foreground
        // label correspondence is a consistent bijection in both directions.
        let mut o2g: HashMap<i32, i32> = HashMap::new();
        let mut g2o: HashMap<i32, i32> = HashMap::new();
        let mut mismatches = 0usize;
        for ((r, c), &ol) in ours.indexed_iter() {
            let gl = genuine[[r, c]];
            if (ol == 0) != (gl == 0) {
                mismatches += 1;
                continue;
            }
            if ol == 0 {
                continue;
            }
            if *o2g.entry(ol).or_insert(gl) != gl || *g2o.entry(gl).or_insert(ol) != ol {
                mismatches += 1;
            }
        }
        eprintln!(
            "label_4conn vs GENUINE scipy.ndimage.label (live): partition mismatches = {mismatches}"
        );
        assert_eq!(mismatches, 0, "label_4conn diverges from genuine scipy.ndimage.label");
    }

    /// **Live library-primitive oracle**: our `binary_skeletonize_skimage` vs the
    /// GENUINE `skimage.morphology.skeletonize`, executed live in the uv-locked
    /// env (skimage 0.19.3 — the version `dilationPatches2` depends on). The
    /// library is the oracle; the bridge only calls it. Gated behind
    /// `oracle_live`.
    #[cfg(feature = "oracle_live")]
    #[test]
    fn skeletonize_matches_genuine_skimage_live() {
        use crate::segmentation::morphology::binary_skeletonize_skimage;
        use crate::test_support::oracle;
        use ndarray::Array2;
        const M: usize = 64;
        // A solid block, a thick ring (halo), and a barbell — the shapes whose
        // medial axis exercises the LUT's thinning corners.
        let mut block = Array2::<f64>::zeros((M, M));
        for r in 18..46 {
            for c in 18..46 {
                block[[r, c]] = 1.0;
            }
        }
        let mut halo = Array2::<f64>::zeros((M, M));
        for r in 10..54 {
            for c in 10..54 {
                let on_border = r < 18 || r >= 46 || c < 18 || c >= 46;
                if on_border {
                    halo[[r, c]] = 1.0;
                }
            }
        }
        let mut barbell = Array2::<f64>::zeros((M, M));
        for r in 24..40 {
            for c in 8..22 {
                barbell[[r, c]] = 1.0; // left bell
            }
            for c in 42..56 {
                barbell[[r, c]] = 1.0; // right bell
            }
            for c in 22..42 {
                if (30..34).contains(&r) {
                    barbell[[r, c]] = 1.0; // connecting bar
                }
            }
        }
        let cases = [("block", block), ("halo", halo), ("barbell", barbell)];
        let mut total = 0usize;
        for (name, f) in &cases {
            let genuine = oracle::nat_raw("skimage_skeletonize", &[f.clone()], &[])
                .remove(0)
                .bool();
            let ours = binary_skeletonize_skimage(&f.mapv(|v| v != 0.0));
            let d = ndarray::Zip::from(&ours)
                .and(&genuine)
                .fold(0usize, |a, &o, &g| a + (o != g) as usize);
            eprintln!("  skeletonize {name:8} vs GENUINE skimage (live): differing px = {d}");
            total += d;
        }
        assert_eq!(total, 0, "binary_skeletonize_skimage diverges from genuine skimage skeletonize");
    }

    /// `dilation_patches2_allen` vs a VERBATIM transcription of Allen
    /// `dilationPatches2` (`RetinotopicMapping.py` L190-225) run on scipy +
    /// skimage. Two seed patches placed to collide under dilation, forcing the
    /// separating skeleton — the case the algorithm exists for. Fixtures from
    /// `gen_dilation_patches2_golden.py` (dilation_iter=8, border_width=1).
    #[test]
    fn dilation_patches2_matches_allen() {
        const M: usize = 64;
        let raw = load_mask_n(
            include_bytes!("../../tests/golden/fixtures/dilpatch_raw.bin"),
            M,
        );
        let golden: &[u8] = include_bytes!("../../tests/golden/fixtures/dilpatch_out.bin");
        let ours = dilation_patches2_allen(&raw, 8, 1);
        let d = count_differing(&ours, golden);
        eprintln!("Allen dilationPatches2 vs scipy+skimage: differing px = {d}");
        assert_eq!(d, 0, "dilation_patches2_allen diverges from Allen dilationPatches2");
    }

    /// **Live genuine-oracle version**: builds the seed mask in Rust and compares
    /// our `dilation_patches2_allen` against the GENUINE NeuroAnalysisTools
    /// `dilationPatches2`, executed live in its uv-locked env. Binary output →
    /// exercises the typed (`bool`) bridge path. Gated behind `oracle_live`.
    #[cfg(feature = "oracle_live")]
    #[test]
    fn dilation_patches2_matches_genuine_nat_live() {
        use crate::test_support::oracle;
        use ndarray::Array2;
        const M: usize = 64;
        let mut raw_f64 = Array2::<f64>::zeros((M, M));
        for r in 16..30 {
            for c in 14..28 {
                raw_f64[[r, c]] = 1.0; // patch A
            }
        }
        for r in 34..50 {
            for c in 36..52 {
                raw_f64[[r, c]] = 1.0; // patch B (collides under dilation)
            }
        }
        let raw_bool = raw_f64.mapv(|v| v != 0.0);

        let genuine = oracle::nat_raw(
            "dilationPatches2",
            &[raw_f64],
            &[("dilationIter", 8.0), ("borderWidth", 1.0)],
        )
        .remove(0)
        .bool();
        let ours = dilation_patches2_allen(&raw_bool, 8, 1);

        let d = ndarray::Zip::from(&ours)
            .and(&genuine)
            .fold(0usize, |a, &o, &g| a + (o != g) as usize);
        eprintln!("dilationPatches2 vs GENUINE NAT (live): differing px = {d}");
        assert_eq!(d, 0, "dilation_patches2_allen diverges from genuine NAT dilationPatches2");
    }

    /// **Live genuine-oracle version**: our `is_adjacent` vs the GENUINE
    /// `core.ImageAnalysis.is_adjacent`, on fresh Rust-built patch pairs across
    /// border widths. Gated behind `oracle_live`.
    #[cfg(feature = "oracle_live")]
    #[test]
    fn is_adjacent_matches_genuine_nat_live() {
        use crate::segmentation::connectivity::is_adjacent;
        use crate::test_support::oracle;
        use ndarray::Array2;
        const M: usize = 32;
        let sq = |r0: usize, r1: usize, c0: usize, c1: usize| {
            let mut a = Array2::<f64>::zeros((M, M));
            for r in r0..r1 {
                for c in c0..c1 {
                    a[[r, c]] = 1.0;
                }
            }
            a
        };
        let cases = [
            ("overlap", sq(5, 12, 5, 12), sq(10, 17, 10, 17)),
            ("gap2", sq(5, 12, 5, 10), sq(5, 12, 12, 17)),
            ("far", sq(5, 12, 2, 7), sq(5, 12, 24, 29)),
        ];
        let mut mismatches = 0usize;
        for (name, a, b) in &cases {
            let (ab, bb) = (a.mapv(|v| v != 0.0), b.mapv(|v| v != 0.0));
            for bw in [1.0_f64, 2.0, 3.0] {
                let genuine = oracle::nat_raw("is_adjacent", &[a.clone(), b.clone()], &[("borderWidth", bw)])
                    .remove(0)
                    .bool()[[0, 0]];
                let ours = is_adjacent(&ab, &bb, bw as i32);
                if ours != genuine {
                    mismatches += 1;
                    eprintln!("  is_adjacent {name} bw={bw}: ours={ours} genuine={genuine}");
                }
            }
        }
        assert_eq!(mismatches, 0, "is_adjacent diverges from genuine NAT is_adjacent");
    }

    /// `keep_largest_component` tie-break vs SNLC `getMouseAreasX.m`
    /// `[~,id]=max(S)` / numpy `argmax` (FIRST maximum → lowest label). The
    /// `tie` case has two equal-size squares; the oracle keeps the LEFT one
    /// (label 1). The `clear` case (one dominant component) confirms the
    /// non-tie path. Fixtures from `gen_largestcc_tie_golden.py`.
    #[test]
    fn keep_largest_component_tiebreak_matches_snlc_argmax() {
        const M: usize = 16;
        let cases: [(&str, &[u8], &[u8]); 2] = [
            (
                "tie",
                include_bytes!("../../tests/golden/fixtures/largestcc_tie_input.bin"),
                include_bytes!("../../tests/golden/fixtures/largestcc_tie_out.bin"),
            ),
            (
                "clear",
                include_bytes!("../../tests/golden/fixtures/largestcc_clear_input.bin"),
                include_bytes!("../../tests/golden/fixtures/largestcc_clear_out.bin"),
            ),
        ];
        let mut total = 0usize;
        for (name, inp, golden) in &cases {
            let ours = keep_largest_component(&load_mask_n(inp, M));
            let d = count_differing(&ours, golden);
            eprintln!("  keep_largest_component {name:6} differing px = {d}");
            total += d;
        }
        assert_eq!(
            total, 0,
            "keep_largest_component tie-break diverges from SNLC/argmax first-max"
        );
    }

    /// `is_adjacent` vs a verbatim Allen `tools.ImageAnalysis.is_adjacent`
    /// (`scipy.ndimage.binary_dilation(iterations=bw-1)` overlap). 10 pairs ×
    /// 4 border-widths; `bw==1` is the `iterations=0` dilate-to-convergence
    /// case (any two non-empty patches adjacent). Fixtures from
    /// `gen_is_adjacent_golden.py` (case order is load-bearing).
    #[test]
    fn is_adjacent_matches_allen() {
        use crate::segmentation::connectivity::is_adjacent;
        const M: usize = 32;
        let pairs: [(&[u8], &[u8]); 10] = [
            (
                include_bytes!("../../tests/golden/fixtures/isadj_overlap_a.bin"),
                include_bytes!("../../tests/golden/fixtures/isadj_overlap_b.bin"),
            ),
            (
                include_bytes!("../../tests/golden/fixtures/isadj_touch_a.bin"),
                include_bytes!("../../tests/golden/fixtures/isadj_touch_b.bin"),
            ),
            (
                include_bytes!("../../tests/golden/fixtures/isadj_gap1_a.bin"),
                include_bytes!("../../tests/golden/fixtures/isadj_gap1_b.bin"),
            ),
            (
                include_bytes!("../../tests/golden/fixtures/isadj_gap2_a.bin"),
                include_bytes!("../../tests/golden/fixtures/isadj_gap2_b.bin"),
            ),
            (
                include_bytes!("../../tests/golden/fixtures/isadj_gap3_a.bin"),
                include_bytes!("../../tests/golden/fixtures/isadj_gap3_b.bin"),
            ),
            (
                include_bytes!("../../tests/golden/fixtures/isadj_diag_a.bin"),
                include_bytes!("../../tests/golden/fixtures/isadj_diag_b.bin"),
            ),
            (
                include_bytes!("../../tests/golden/fixtures/isadj_far_a.bin"),
                include_bytes!("../../tests/golden/fixtures/isadj_far_b.bin"),
            ),
            (
                include_bytes!("../../tests/golden/fixtures/isadj_thin_gap1_a.bin"),
                include_bytes!("../../tests/golden/fixtures/isadj_thin_gap1_b.bin"),
            ),
            (
                include_bytes!("../../tests/golden/fixtures/isadj_b_empty_a.bin"),
                include_bytes!("../../tests/golden/fixtures/isadj_b_empty_b.bin"),
            ),
            (
                include_bytes!("../../tests/golden/fixtures/isadj_edge_gap2_a.bin"),
                include_bytes!("../../tests/golden/fixtures/isadj_edge_gap2_b.bin"),
            ),
        ];
        let expected: &[u8] = include_bytes!("../../tests/golden/fixtures/isadj_expected.bin");
        let bws = [1i32, 2, 3, 4];
        let mut idx = 0usize;
        let mut wrong = 0usize;
        for (a_b, b_b) in &pairs {
            let a = load_mask_n(a_b, M);
            let b = load_mask_n(b_b, M);
            for &bw in &bws {
                let got = is_adjacent(&a, &b, bw);
                let exp = expected[idx] != 0;
                if got != exp {
                    eprintln!("  is_adjacent mismatch at idx {idx} (bw={bw}): got {got} exp {exp}");
                    wrong += 1;
                }
                idx += 1;
            }
        }
        assert_eq!(wrong, 0, "is_adjacent diverges from Allen is_adjacent");
    }

    /// `segment_threshold_only`'s opening must be Allen's
    /// `ni.binary_opening(iterations=3)` (4-conn cross diamond, border_value=0)
    /// — NOT a Euclidean disk-3. Pins the post-threshold opening against the
    /// scipy oracle. Fixtures from `gen_thronly_golden.py`.
    #[test]
    fn segment_threshold_only_opening_matches_allen() {
        const M: usize = 64;
        let thr = load_mask_n(
            include_bytes!("../../tests/golden/fixtures/thronly_thr_mask.bin"),
            M,
        );
        let g_allen: &[u8] = include_bytes!("../../tests/golden/fixtures/thronly_open_allen.bin");
        let ours = binary_opening_cross(&thr, 3);
        let d = count_differing(&ours, g_allen);
        eprintln!("threshold-only opening (cross-3) vs Allen scipy: differing px = {d}");
        assert_eq!(d, 0, "segment_threshold_only opening diverges from Allen");
    }

    /// `label_4conn` vs `scipy.ndimage.label` (default 4-conn cross). Pins the
    /// full label MAP including label VALUES (raster first-pixel order), which
    /// is load-bearing because downstream sign assignment preserves IDs.
    /// Varied shapes: borders/order, diagonal-only (must stay split), a
    /// serpentine U, thin lines, singletons, dense random, empty, full.
    /// Fixtures from `gen_label4conn_golden.py`. Predicted-match.
    #[test]
    fn label_4conn_matches_scipy_ndimage_label() {
        fn check(name: &str, in_b: &[u8], lab_b: &[u8], h: usize, w: usize) -> usize {
            let mask = Array2::from_shape_fn((h, w), |(r, c)| in_b[r * w + c] != 0);
            let exp = lab_b
                .chunks_exact(4)
                .map(|c| i32::from_le_bytes(c.try_into().unwrap()))
                .collect::<Vec<_>>();
            let (labels, _n) = label_4conn(&mask);
            let mut d = 0usize;
            for r in 0..h {
                for c in 0..w {
                    if labels[[r, c]] != exp[r * w + c] {
                        d += 1;
                    }
                }
            }
            eprintln!("  label_4conn {name:11} differing = {d}");
            d
        }
        let mut total = 0;
        total += check(
            "borders",
            include_bytes!("../../tests/golden/fixtures/label4conn_in_borders.bin"),
            include_bytes!("../../tests/golden/fixtures/label4conn_lab_borders.bin"),
            6,
            8,
        );
        total += check(
            "diag",
            include_bytes!("../../tests/golden/fixtures/label4conn_in_diag.bin"),
            include_bytes!("../../tests/golden/fixtures/label4conn_lab_diag.bin"),
            5,
            5,
        );
        total += check(
            "serpent",
            include_bytes!("../../tests/golden/fixtures/label4conn_in_serpent.bin"),
            include_bytes!("../../tests/golden/fixtures/label4conn_lab_serpent.bin"),
            7,
            7,
        );
        total += check(
            "thin",
            include_bytes!("../../tests/golden/fixtures/label4conn_in_thin.bin"),
            include_bytes!("../../tests/golden/fixtures/label4conn_lab_thin.bin"),
            6,
            6,
        );
        total += check(
            "singletons",
            include_bytes!("../../tests/golden/fixtures/label4conn_in_singletons.bin"),
            include_bytes!("../../tests/golden/fixtures/label4conn_lab_singletons.bin"),
            5,
            5,
        );
        total += check(
            "rand",
            include_bytes!("../../tests/golden/fixtures/label4conn_in_rand.bin"),
            include_bytes!("../../tests/golden/fixtures/label4conn_lab_rand.bin"),
            24,
            24,
        );
        total += check(
            "empty",
            include_bytes!("../../tests/golden/fixtures/label4conn_in_empty.bin"),
            include_bytes!("../../tests/golden/fixtures/label4conn_lab_empty.bin"),
            4,
            4,
        );
        total += check(
            "full",
            include_bytes!("../../tests/golden/fixtures/label4conn_in_full.bin"),
            include_bytes!("../../tests/golden/fixtures/label4conn_lab_full.bin"),
            4,
            4,
        );
        assert_eq!(total, 0, "label_4conn diverges from scipy.ndimage.label");
    }

    // ── Decision-gated items: regression-lock tests that pin CURRENT behaviour
    //    and record the divergence from a reference as an executable fact. The
    //    canonical choice is a method decision deferred to review (see
    //    docs/VALIDATION_SCORECARD.md "Open items").

    /// GATED (item 11) — `patches_from_labels_majority_sign` vs SNLC
    /// `getPatchSign.m` (`sign(mean(VFS))`). Matches MATLAB on every non-zero-
    /// mean component; at an EXACT-zero-mean component MATLAB `sign` gives 0
    /// while our `i8` sign forces +1 (`sum >= 0`). This pins both: the ±1
    /// agreement and the documented zero-mean divergence (measure-zero on real
    /// smoothed VFS). Fixtures from `gen_patchsign_majority_golden.py`.
    #[test]
    fn patch_sign_majority_matches_snlc_except_zero_mean() {
        use crate::segmentation::connectivity::patches_from_labels_majority_sign;
        const M: usize = 32;
        let lab_v = load_i32(include_bytes!("../../tests/golden/fixtures/psign_labels.bin"));
        let sig_v = load_f64(include_bytes!("../../tests/golden/fixtures/psign_signal.bin"));
        let n = load_i32(include_bytes!("../../tests/golden/fixtures/psign_n.bin"))[0] as usize;
        let exp = load_i32(include_bytes!("../../tests/golden/fixtures/psign_expsign.bin"));

        let labels = Array2::from_shape_fn((M, M), |(r, c)| lab_v[r * M + c]);
        let signal = Array2::from_shape_fn((M, M), |(r, c)| sig_v[r * M + c]);
        let patches = patches_from_labels_majority_sign(&labels, n, &signal);

        for p in &patches {
            // Recover this patch's label from its first set pixel (robust to order).
            let mut lab = 0i32;
            'find: for r in 0..M {
                for c in 0..M {
                    if p.mask[[r, c]] {
                        lab = labels[[r, c]];
                        break 'find;
                    }
                }
            }
            let ours = p.sign as i32;
            let matlab = exp[(lab - 1) as usize];
            if matlab == 0 {
                // Documented divergence: MATLAB sign(0)=0; our i8 sign forces +1.
                assert_eq!(ours, 1, "zero-mean label {lab}: ours should force +1");
            } else {
                assert_eq!(ours, matlab, "label {lab} sign vs SNLC getPatchSign");
            }
        }
    }

    /// UNVALIDATED (regression-lock only). The cross-cycle reliability
    /// *coherence* `|Σ Z_k| / Σ|Z_k|` is Engel 1994 / Zhuang 2017, but the
    /// specific cortex-MASK derivation here (min-over-directions threshold →
    /// largest-CC → fill-holes) has NO oracle: Zhuang's `RetinotopicMapping.py`
    /// uses no power/coherence ROI mask and segments full-frame (verified from
    /// source). So this test only PINS our own behaviour (the `min >= threshold`
    /// and `is_finite` rule); it does not establish faithfulness to any external
    /// method. The `>=` (inclusive) follows the reference's threshold convention
    /// (`signMapf >= signMapThr`). Primary (no-tie) case only. Fixtures from
    /// `gen_cortexrel_golden.py`.
    #[test]
    fn cortex_from_reliability_pins_current_threshold_rule() {
        const M: usize = 48;
        let ld = |b: &[u8]| -> Array2<f64> {
            let v = load_f64(b);
            Array2::from_shape_fn((M, M), |(r, c)| v[r * M + c])
        };
        let af = ld(include_bytes!("../../tests/golden/fixtures/cortexrel_azi_fwd.bin"));
        let ar = ld(include_bytes!("../../tests/golden/fixtures/cortexrel_azi_rev.bin"));
        let lf = ld(include_bytes!("../../tests/golden/fixtures/cortexrel_alt_fwd.bin"));
        let lr = ld(include_bytes!("../../tests/golden/fixtures/cortexrel_alt_rev.bin"));
        let exp: &[u8] = include_bytes!("../../tests/golden/fixtures/cortexrel_expected.bin");

        let got = crate::segmentation::cortex_from_reliability(&af, &ar, &lf, &lr, 0.5);
        let d = count_differing(&got, exp);
        eprintln!("cortex_from_reliability (regression-lock, `>=` rule): differing px = {d}");
        assert_eq!(
            d, 0,
            "cortex_from_reliability changed (`>=` rule; OpenISI method, no oracle for the mask)"
        );
    }

    /// GATED (item 13) — `compute_eccentricity` V1-center regression-lock. Our
    /// V1 reference point is the visual-field center-of-mass over the largest
    /// area using the Allen-convention great-circle formula. SNLC
    /// `getAreaBorders.m` differs (imopen disk-10 → pixel-space single-pixel
    /// sample → cos-on-azimuth). The two references CONFLICT and matching SNLC
    /// would break the existing Allen ecc golden, so this pins current
    /// behaviour; the canonical convention is the open decision. Fixtures
    /// (`v1ecc_*`) encode our current map.
    #[test]
    fn compute_eccentricity_v1_center_pins_current_allen_convention() {
        const M: usize = 64;
        let av = load_f64(include_bytes!("../../tests/golden/fixtures/v1ecc_azi.bin"));
        let lv = load_f64(include_bytes!("../../tests/golden/fixtures/v1ecc_alt.bin"));
        let labv = load_i32(include_bytes!("../../tests/golden/fixtures/v1ecc_labels.bin"));
        let mapv = load_f64(include_bytes!("../../tests/golden/fixtures/v1ecc_rust_map.bin"));

        let azi = Array2::from_shape_fn((M, M), |(r, c)| av[r * M + c]);
        let alt = Array2::from_shape_fn((M, M), |(r, c)| lv[r * M + c]);
        let labels = Array2::from_shape_fn((M, M), |(r, c)| labv[r * M + c]);

        let got = crate::math::compute_eccentricity(&azi, &alt, &labels);
        let mut md = 0.0f64;
        for r in 0..M {
            for c in 0..M {
                let (o, g) = (got[[r, c]], mapv[r * M + c]);
                if o.is_nan() || g.is_nan() {
                    assert_eq!(o.is_nan(), g.is_nan(), "NaN mismatch at {r},{c}");
                } else {
                    md = md.max((o - g).abs());
                }
            }
        }
        eprintln!("compute_eccentricity V1-center (regression-lock): max diff = {md:.2e}");
        assert!(
            md < 1e-9,
            "compute_eccentricity changed (NB: Allen vs SNLC center convention is an open decision)"
        );
    }

    /// FAITHFUL — `math::compute_eccentricity_snlc`
    /// (`EccentricityMethod::SnlcGetAreaBordersV1Center`) vs the SNLC oracle
    /// `getAreaBorders.m` + `getV1id.m` + `getPatchCoM.m`, transcribed verbatim
    /// in `gen_v1ecc_golden.py`. This exercises the three traps the OpenISI
    /// variant skips: `imopen(disk-10)` before component selection, the
    /// single-pixel sample at the pixel-space centroid, and the cos-on-azimuth
    /// formula. Pure f64 on both sides → machine-precision match expected.
    #[test]
    fn compute_eccentricity_snlc_matches_get_area_borders() {
        const M: usize = 64;
        let av = load_f64(include_bytes!("../../tests/golden/fixtures/v1ecc_azi.bin"));
        let lv = load_f64(include_bytes!("../../tests/golden/fixtures/v1ecc_alt.bin"));
        let labv = load_i32(include_bytes!("../../tests/golden/fixtures/v1ecc_labels.bin"));
        let mapv = load_f64(include_bytes!("../../tests/golden/fixtures/v1ecc_snlc_map.bin"));
        let cen = load_f64(include_bytes!("../../tests/golden/fixtures/v1ecc_snlc_center.bin"));

        let azi = Array2::from_shape_fn((M, M), |(r, c)| av[r * M + c]);
        let alt = Array2::from_shape_fn((M, M), |(r, c)| lv[r * M + c]);
        let labels = Array2::from_shape_fn((M, M), |(r, c)| labv[r * M + c]);

        let got = crate::math::compute_eccentricity_snlc(&azi, &alt, &labels);
        let mut md = 0.0f64;
        for r in 0..M {
            for c in 0..M {
                let (o, g) = (got[[r, c]], mapv[r * M + c]);
                assert_eq!(o.is_nan(), g.is_nan(), "NaN mismatch at {r},{c}");
                if !o.is_nan() {
                    md = md.max((o - g).abs());
                }
            }
        }
        // The oracle center (altC, aziC) is logged for provenance; the map match
        // is the binding assertion (it folds in the same center selection).
        eprintln!(
            "compute_eccentricity_snlc vs getAreaBorders: max map diff = {md:.2e}  \
             (oracle center altC={:.4} aziC={:.4})",
            cen[0], cen[1]
        );
        assert!(
            md < 1e-9,
            "compute_eccentricity_snlc diverges from SNLC getAreaBorders: {md:.3e}"
        );
    }
}
