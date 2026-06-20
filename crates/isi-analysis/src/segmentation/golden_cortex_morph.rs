//! Live cross-validation of the SNLC-cortex binary morphology against the
//! genuine Octave Image Processing Toolbox, computed each run.
//!
//! `SnlcGarrett2014ImBound` is a *faithful-reproduction* claim of
//! `getMouseAreasX.m`, built from `imopen(disk2)`, `imclose(disk10)`,
//! `imfill('holes')`, `imdilate(disk3)`, and the largest 4-connected
//! component. The disk structuring element is bit-identical to
//! `strel('disk',R,0)`; `cortex_morphology_matches_genuine_octave_live` confirms
//! each *operation* — including border padding (erode pads 1, dilate pads 0) and
//! hole-fill semantics — against genuine Octave on a scene that stresses borders,
//! holes, and gap-bridging, and `keep_largest_component_tiebreak_*` covers the
//! largest-CC tie-break. (The former frozen `gen_cortex_morph_golden.m` golden was
//! retired once every op had a live counterpart.)

#[cfg(test)]
mod tests {
    use crate::methods::cortex_source::{CortexResolveContext, CortexSourceExt, CortexSourceMethod};
    use crate::segmentation::connectivity::keep_largest_component;
    // Used only by the `oracle_live`-gated live tests (their frozen counterparts,
    // which also used these in the default build, were retired in the cutover).
    // (label_4conn / binary_{opening,closing}_cross are imported locally inside
    // their live tests, so no module-level gated import is needed for them.)
    #[cfg(feature = "oracle_live")]
    use crate::methods::patch_extraction::raw_patch_map_allen;
    #[cfg(feature = "oracle_live")]
    use crate::segmentation::connectivity::dilation_patches2_allen;
    #[cfg(feature = "oracle_live")]
    use crate::segmentation::morphology::gaussian_smooth_f64;
    use crate::test_support::{count_differing, load_f64, load_i32};
    use ndarray::Array2;

    const N: usize = 96;

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

    // (Cutover, objective 6) The frozen `cortex_morphology_matches_octave_strel_ops`
    // golden + its six `cortex_morph_*.bin` fixtures + BOTH owning generators
    // (gen_cortex_morph_golden.m, which wrote the input mask + five Octave op
    // outputs, and the now-dead gen_patch_morph_golden.py, which read the mask and
    // wrote unconsumed patch_morph_*.bin) were DELETED. Every op it checked is
    // validated LIVE each run: the four disk-strel ops by
    // `cortex_morphology_matches_genuine_octave_live` (genuine Octave
    // imopen/imclose/imdilate/imfill — its scene now includes the top-border blob
    // the frozen golden curated, so edge-padding coverage is preserved), and
    // `keep_largest_component` by `keep_largest_component_tiebreak_matches_snlc_argmax`.

    /// **Live library-primitive oracle, Octave**: our disk-strel morphology
    /// (`binary_opening_disk`, `binary_closing_disk`, `binary_dilation_disk`,
    /// `binary_fill_holes`) vs the GENUINE Octave IPT `imopen`/`imclose`/
    /// `imdilate`(`strel('disk',R,0)`)/`imfill('holes')`, executed live. Octave's
    /// IPT is the oracle; the bridge only calls it. (`keep_largest_component`'s
    /// `max`-first-index tie-break is a language guarantee, not a code oracle — it
    /// stays a regression-lock, excluded here.) `strel('disk',R,0)` is the exact
    /// Euclidean disk (N=0, no approximation). Gated behind `oracle_live`.
    #[cfg(feature = "oracle_live")]
    #[test]
    fn cortex_morphology_matches_genuine_octave_live() {
        use crate::segmentation::morphology::{
            binary_closing_disk, binary_dilation_disk, binary_fill_holes, binary_opening_disk,
        };
        use crate::test_support::oracle;
        use ndarray::Array2;
        const N: usize = 64;
        // A blob with a thin spur (opening trims), a notch + interior hole
        // (closing/fill act), near another blob (dilation bridges).
        let mut f = Array2::<f64>::zeros((N, N));
        for r in 14..44 {
            for c in 14..40 {
                f[[r, c]] = 1.0;
            }
        }
        for r in 26..30 {
            for c in 40..52 {
                f[[r, c]] = 1.0; // spur
            }
        }
        for r in 22..32 {
            for c in 22..26 {
                f[[r, c]] = 0.0; // interior hole
            }
        }
        for r in 48..58 {
            for c in 46..58 {
                f[[r, c]] = 1.0; // second blob
            }
        }
        // Blob straddling the TOP border (row 0) — exercises edge padding for
        // imopen/imclose/imdilate/imfill (the curated stress case the retired
        // frozen `cortex_morphology_matches_octave_strel_ops` golden held; ported
        // here so the live oracle covers it, no blind coverage loss).
        for r in 0..6 {
            for c in 6..18 {
                f[[r, c]] = 1.0;
            }
        }
        let mask = f.mapv(|v| v != 0.0);

        let cases: [(&str, f64, Array2<bool>); 4] = [
            ("imopen_disk", 2.0, binary_opening_disk(&mask, 2)),
            ("imclose_disk", 10.0, binary_closing_disk(&mask, 10)),
            ("imdilate_disk", 3.0, binary_dilation_disk(&mask, 3)),
            ("imfill_holes", 0.0, binary_fill_holes(&mask)),
        ];
        let mut total = 0usize;
        for (fname, radius, ours) in &cases {
            let params: Vec<(&str, f64)> = if *fname == "imfill_holes" {
                vec![]
            } else {
                vec![("radius", *radius)]
            };
            let genuine = oracle::snlc(fname, &[f.clone()], &params).remove(0);
            let mut d = 0usize;
            for r in 0..N {
                for c in 0..N {
                    if (ours[[r, c]] as i32) != genuine[[r, c]].round() as i32 {
                        d += 1;
                    }
                }
            }
            eprintln!("  {fname:14} vs GENUINE Octave (live): differing px = {d}");
            total += d;
        }
        assert_eq!(total, 0, "cortex disk-strel morphology diverges from genuine Octave IPT");
    }

    /// **FROZEN orchestration regression-lock (objective-6 exception, honestly
    /// labelled — NOT a live oracle).** The end-to-end `SnlcGarrett2014ImBound`
    /// SEQUENCE (threshold → imopen2 → imclose10 → fill → imdilate3 → fill →
    /// largest 4-CC) is OpenISI's composition — there is no single SNLC `.m`
    /// defining it (`getMouseAreasX.m` is the GUI pipeline). Each *primitive* in
    /// the sequence IS validated live (`cortex_morphology_matches_genuine_octave_live`
    /// and `keep_largest_component_tiebreak_*`); this frozen golden pins only the
    /// orchestration order, against a one-off Octave run of the same sequence.
    /// End-to-end: the real `CortexSourceMethod::resolve` for `SnlcGarrett2014ImBound`
    /// on a |VFS| field. Input has a wide threshold margin so the std N-vs-(N−1)
    /// convention cannot flip a pixel — this pins the orchestration.
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

    // (Cutover, objective 6) The frozen `allen_cross_morphology_matches_scipy`
    // golden + its patch_morph_open/close.bin fixtures were DELETED: the live
    // `cross_morphology_matches_genuine_scipy_live` recomputes the genuine scipy
    // binary_opening/closing (4-conn cross) each run, and `label_4conn` is
    // validated live by `label4conn_matches_genuine_scipy_live`. (gen_patch_morph
    // was later deleted too — it was dead, only reading cortex_morph_input.bin and
    // writing the unconsumed patch_morph_*.bin; see the cortex_morph cutover note.)

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

    // (Cutover, objective 1) The frozen `allen_raw_patch_map_matches_scipy` golden +
    // its exclusive patchext_rawmap.bin fixture + gen_patch_extraction_golden.py were
    // DELETED. gen_patch_extraction was a TRANSCRIPTION (a scipy composition mimicking
    // `_getRawPatchMap`'s orchestration). The live `raw_patch_map_matches_genuine_nat_live`
    // drives the GENUINE `RetinotopicMappingTrial._getRawPatchMap` live.

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
        let paint = |r0: usize, r1: usize, c0: usize, c1: usize, s: f64, im: &mut Array2<f64>, sgn: &mut Array2<f64>| {
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
        // A 4th patch with EXACTLY zero mean (half +0.5, half −0.5) — the documented
        // deviation surface: genuine MATLAB `sign(0)=0` (undefined) vs our `+1`
        // tie-break. Compared specially below (excluded from the agreement loop).
        for r in 22..28 {
            for c in 20..26 {
                im[[r, c]] = 1.0;
                sgn[[r, c]] = if c < 23 { 0.5 } else { -0.5 };
            }
        }

        // Genuine getPatchSign returns a per-pixel map: patch pixels = sign+1.1
        // (label-INVARIANT — sidesteps our row-major vs MATLAB column-major
        // bwlabel ordering, which is not a divergence).
        let patch_sign_map = oracle::snlc("getPatchSign", &[im.clone(), sgn.clone()], &[]).remove(0);
        let (labels, n) = label_4conn(&im.mapv(|v| v != 0.0));
        let patches = patches_from_labels_majority_sign(&labels, n, &sgn);

        let mut mismatch = 0usize;
        let mut saw_zero_mean = false;
        for p in &patches {
            // The genuine per-pixel sign for this patch (constant over its pixels).
            let (pr, pc) = (0..M)
                .flat_map(|r| (0..M).map(move |c| (r, c)))
                .find(|&(r, c)| p.mask[[r, c]])
                .unwrap();
            let genuine_sign = (patch_sign_map[[pr, pc]] - 1.1).round() as i8;
            if genuine_sign == 0 {
                // Documented deviation (regression-lock): genuine MATLAB sign(0)=0;
                // ours forces a deterministic +1 (a patch must get a sign).
                saw_zero_mean = true;
                assert_eq!(p.sign, 1, "zero-mean patch: ours must force +1 (genuine sign(0)=0)");
                continue;
            }
            for r in 0..M {
                for c in 0..M {
                    if p.mask[[r, c]] && (patch_sign_map[[r, c]] - 1.1).round() as i8 != p.sign {
                        mismatch += 1;
                    }
                }
            }
        }
        eprintln!("getPatchSign (live, region-wise): mismatching px = {mismatch}, saw_zero_mean={saw_zero_mean}");
        assert_eq!(mismatch, 0, "patch signs diverge from genuine SNLC getPatchSign (non-zero-mean)");
        assert!(saw_zero_mean, "the zero-mean deviation case did not materialize");
    }

    // (Cutover, objective 1) The frozen `gaussian_smooth_matches_scipy_gaussian_filter`
    // (f64) + `tensor_gaussian_smooth_matches_scipy` (f32) goldens + their shared
    // gauss_*.bin fixtures + gen_gaussian_golden.py were DELETED: the live
    // `gaussian_smooth_matches_genuine_scipy_live` (below, f64) and
    // `tensor_gaussian_smooth_matches_genuine_scipy_live` (golden_vfs, f32) compute the
    // genuine scipy.ndimage.gaussian_filter live on fresh fields.

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

    // (Cutover, objective 1) The frozen `skeletonize_matches_skimage` golden +
    // its `skel_*` fixtures + `gen_skeletonize_golden.py` were DELETED: the live
    // `skeletonize_matches_genuine_skimage_live` (below) computes the genuine
    // skimage `skeletonize` oracle on every run, fully superseding the frozen
    // transcription-era fixture (no committed fixture can silently drift).

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
        // Partition-equivalence check: a connected-component labeling is defined
        // only up to relabeling, so we require the two induce the SAME partition
        // (same background + a consistent bijection ours↔genuine over foreground),
        // not bit-identical label integers.
        let partition_mismatches = |f: &Array2<f64>| -> usize {
            let mask = f.mapv(|v| v != 0.0);
            let genuine = oracle::nat_raw("scipy_label", &[f.clone()], &[]).remove(0).i32();
            let (ours, _n) = label_4conn(&mask);
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
            mismatches
        };

        // The scene classes the retired frozen golden held.
        let mut blobs = Array2::<f64>::zeros((M, M)); // disjoint blobs + diagonal-only contact
        for (r0, r1, c0, c1) in [(3, 9, 3, 9), (3, 9, 20, 27), (20, 30, 5, 16), (30, 34, 30, 34), (34, 38, 34, 38)] {
            for r in r0..r1 {
                for c in c0..c1 {
                    blobs[[r, c]] = 1.0;
                }
            }
        }
        let empty = Array2::<f64>::zeros((M, M));
        let full = Array2::<f64>::ones((M, M));
        let singletons = Array2::from_shape_fn((M, M), |(r, c)| if (r % 3 == 0) && (c % 3 == 0) { 1.0 } else { 0.0 });
        let serpent = Array2::from_shape_fn((M, M), |(r, c)| {
            // connected boustrophedon: full rows on even bands joined at alternating ends
            let band = r / 4;
            let on_row = r % 4 == 0;
            let joiner = if band % 2 == 0 { c == M - 1 } else { c == 0 };
            if on_row || joiner { 1.0 } else { 0.0 }
        });
        let thin = Array2::from_shape_fn((M, M), |(r, c)| if r == c || r + c == M - 1 { 1.0 } else { 0.0 });
        let borders = Array2::from_shape_fn((M, M), |(r, c)| {
            if r == 0 || c == 0 || r == M - 1 || c == M - 1 { 1.0 } else { 0.0 }
        });
        let rand = Array2::from_shape_fn((M, M), |(r, c)| {
            // deterministic pseudo-random ~45% fill
            if ((r * 73 + c * 151 + 17) % 100) < 45 { 1.0 } else { 0.0 }
        });
        let scenes = [
            ("blobs", blobs), ("empty", empty), ("full", full), ("singletons", singletons),
            ("serpent", serpent), ("thin", thin), ("borders", borders), ("rand", rand),
        ];
        let mut total = 0usize;
        for (name, f) in &scenes {
            let m = partition_mismatches(f);
            eprintln!("  label_4conn {name:10} vs GENUINE scipy.ndimage.label (live): partition mismatches = {m}");
            total += m;
        }
        assert_eq!(total, 0, "label_4conn diverges from genuine scipy.ndimage.label");
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

    // (Cutover, objective 1) The frozen `dilation_patches2_matches_allen` golden
    // + its dilpatch_*.bin fixtures + gen_dilation_patches2_golden.py (which
    // imported the `_allen_oracle` SHIM) were DELETED: the live
    // `dilation_patches2_matches_genuine_nat_live` below covers the identical
    // collision scene (two seed patches that collide under dilation → the
    // separating skeleton, the case the algorithm exists for; iter=8, bw=1)
    // against genuine NAT `dilationPatches2` in the shim-free uv env.

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
        // Covers every semantic boundary the retired frozen golden exercised:
        // overlap (adjacent at all bw), edge-touch (gap 0), and gaps of 2/4/wide
        // pixels — so the dilation gap-closing threshold flips at the right bw —
        // across bw 1..=4. bw=1 is the critical `iterations=0` converge-to-fill
        // case (genuine declares every non-empty pair adjacent; ours matches).
        let cases = [
            ("overlap", sq(5, 12, 5, 12), sq(10, 17, 10, 17)),
            ("touch", sq(5, 12, 5, 10), sq(5, 12, 10, 15)),
            ("gap2", sq(5, 12, 5, 10), sq(5, 12, 12, 17)),
            ("gap4", sq(5, 12, 5, 10), sq(5, 12, 14, 19)),
            ("far", sq(5, 12, 2, 7), sq(5, 12, 24, 29)),
            // diagonal-only corner contact — exercises the 4-conn cross structure
            // (NOT 8-conn): the two squares meet only at the (8,8)/(9,9) corner.
            ("diag", sq(5, 9, 5, 9), sq(9, 13, 9, 13)),
            // one patch empty — the predicate `amax(p1d+p2d)>1` can never hold, so
            // genuine returns not-adjacent at every bw; ours must match.
            ("empty", sq(5, 12, 5, 12), sq(0, 0, 0, 0)),
        ];
        let mut mismatches = 0usize;
        for (name, a, b) in &cases {
            let (ab, bb) = (a.mapv(|v| v != 0.0), b.mapv(|v| v != 0.0));
            for bw in [1.0_f64, 2.0, 3.0, 4.0] {
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

    // (Cutover, objective 1) The frozen `is_adjacent_matches_allen` golden + its
    // 21 isadj_*.bin fixtures + gen_is_adjacent_golden.py (which imported the
    // `_allen_oracle` SHIM) were DELETED: the live
    // `is_adjacent_matches_genuine_nat_live` above was enriched to cover every
    // semantic case the frozen golden held (overlap/touch/gap2/gap4/far/diag/empty
    // × border-width 1..=4, incl. the bw=1 converge-to-fill semantic) and computes
    // the genuine NAT `is_adjacent` live in the shim-free uv-locked env.

    // (Cutover, objective 1) The frozen `segment_threshold_only_opening_matches_allen`
    // golden + its thronly_*.bin fixtures + gen_thronly_golden.py were DELETED: the
    // post-threshold opening it pinned is `binary_opening_cross(·, 3)`, which the live
    // `cross_morphology_matches_genuine_scipy_live` validates against the genuine
    // `scipy.ndimage.binary_opening` (4-conn cross, iterations=3) live.

    // (Cutover, objective 1) The frozen `label_4conn_matches_scipy_ndimage_label`
    // golden + its label4conn_*.bin fixtures + gen_label4conn_golden.py were DELETED:
    // the live `label4conn_matches_genuine_scipy_live` above was refactored to cover
    // the same eight scene classes (borders, diag, serpent, thin, singletons, rand,
    // empty, full) against the genuine `scipy.ndimage.label` live. (The frozen golden
    // pinned EXACT label integers — an implementation coincidence, both raster-scan;
    // the live test asserts the semantically-correct invariant: the partition, up to
    // relabeling, which is all the downstream sign assignment depends on.)

    // ── Decision-gated items: regression-lock tests that pin CURRENT behaviour
    //    and record the divergence from a reference as an executable fact. The
    //    canonical choice is a method decision deferred to review (see
    //    docs/VALIDATION_SCORECARD.md "Open items").

    // (Cutover, objective 1) The frozen `patch_sign_majority_matches_snlc_except_
    // zero_mean` golden + its psign_*.bin fixtures + gen_patchsign_majority_golden.py
    // (a transcription) were DELETED, along with the DEAD gen_patchsign_golden.m
    // (its patchsign_*.bin outputs were read by no test). The live
    // `patch_sign_matches_genuine_snlc_getpatchsign_live` above now carries BOTH the
    // genuine ±1 agreement (region-wise vs Octave getPatchSign) AND the documented
    // zero-mean tie-break deviation as a regression-lock (genuine sign(0)=0, ours +1).

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

    /// **FORMULA-PIN / faithful-reproduction** (honest label, NOT a live code
    /// oracle). `math::compute_eccentricity_snlc`
    /// (`EccentricityMethod::SnlcGetAreaBordersV1Center`) reproduces SNLC's
    /// V1-center selection (`imopen(disk-10)` → single-pixel pixel-space-centroid
    /// sample → cos-on-azimuth) from `getAreaBorders.m` L211-224. **Irreducible
    /// gap:** `getAreaBorders.m` is a 44-plot GUI pipeline taking animal-name
    /// strings (loads data, plots) — it is **not headless-runnable and the
    /// V1-selection block is not a separable function**, so there is no runnable
    /// reference to call (`getV1id`/`getPatchCoM` are runnable, but they are only
    /// intermediates of the non-separable selection). So this pins the transcribed
    /// SNLC convention against our variant, labelled as a formula-pin with the gap
    /// stated, not dressed as a live oracle. Pure f64 → machine-precision.
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
