//! Stage 8 — Patch refinement (split + merge).
//!
//! Given raw patches from the extraction stage, optionally apply split
//! and merge passes to refine multi-area blobs into canonical visual
//! areas (~7 for typical mouse retinotopy).

use std::sync::atomic::AtomicBool;

use ndarray::Array2;

use crate::segmentation::Patch;
use crate::AnalysisError;

/// Method choice for patch refinement.
///
/// Canonical type: [`openisi_params::config::analysis::PatchRefinement`] (UNIFY);
/// compute behavior is attached via [`PatchRefinementExt`].
pub use openisi_params::config::analysis::PatchRefinement as PatchRefinementMethod;

/// Compute behavior for the patch-refinement stage (extension trait).
pub trait PatchRefinementExt {
    /// Apply the refinement. `determinant_map` is `|det(grad)|` of the
    /// position-in-visual-angle maps (= our `magnification_raw`).
    /// `azi_position_deg` / `alt_position_deg` are positions in visual-angle
    /// degrees (= our `azi_phase_degrees` / `alt_phase_degrees`, which
    /// `phase_to_degrees` produces in visual-angle units).
    fn apply(
        &self,
        patches: Vec<Patch>,
        azi_position_deg: &Array2<f64>,
        alt_position_deg: &Array2<f64>,
        determinant_map: &Array2<f64>,
        cancel: &AtomicBool,
    ) -> Result<Vec<Patch>, AnalysisError>;
}

impl PatchRefinementExt for PatchRefinementMethod {
    fn apply(
        &self,
        patches: Vec<Patch>,
        azi_position_deg: &Array2<f64>,
        alt_position_deg: &Array2<f64>,
        determinant_map: &Array2<f64>,
        cancel: &AtomicBool,
    ) -> Result<Vec<Patch>, AnalysisError> {
        match self {
            Self::None => Ok(patches),
            Self::AllenZhuang2017SplitMerge {
                split_overlap_thr,
                split_local_min_cut_step,
                merge_overlap_thr,
                visual_space_pixel_size,
                visual_space_close_iter,
                ecc_map_filter_sigma,
                border_width,
                small_patch_thr,
            } => {
                // Split/merge does O(N²) pair work on patches; with
                // noise-dominated K=1 input the patch count can be in
                // the hundreds, the split+merge result is meaningless
                // regardless, and the loop will hang for minutes. Skip
                // when N is in noise territory — same threshold and
                // reason as the patch_extract adjacency filter.
                const REFINEMENT_MAX_PATCHES: usize = 100;
                if patches.len() > REFINEMENT_MAX_PATCHES {
                    tracing::warn!(
                        patches = patches.len(),
                        threshold = REFINEMENT_MAX_PATCHES,
                        "skipping split/merge — patch count over threshold \
                         (input VFS is noise-dominated; acquire more cycles for better SNR)",
                    );
                    return Ok(patches);
                }
                allen::run_split_merge(
                    patches,
                    azi_position_deg,
                    alt_position_deg,
                    determinant_map,
                    allen::Params {
                        split_overlap_thr: *split_overlap_thr,
                        split_local_min_cut_step: *split_local_min_cut_step,
                        merge_overlap_thr: *merge_overlap_thr,
                        visual_space_pixel_size: *visual_space_pixel_size,
                        visual_space_close_iter: *visual_space_close_iter,
                        ecc_map_filter_sigma: *ecc_map_filter_sigma,
                        border_width: *border_width,
                        small_patch_thr: *small_patch_thr,
                    },
                    cancel,
                )
            }
        }
    }
}

// =============================================================================
// Allen split/merge implementation
// =============================================================================

mod allen {
    use std::sync::atomic::{AtomicBool, Ordering};

    use ndarray::Array2;
    use rayon::prelude::*;

    use crate::segmentation::connectivity::{is_adjacent, label_4conn};
    use crate::segmentation::morphology::{binary_closing_cross, binary_skeletonize_skimage};
    use crate::segmentation::Patch;
    use crate::AnalysisError;

    pub(super) struct Params {
        pub split_overlap_thr: f64,
        pub split_local_min_cut_step: f64,
        pub merge_overlap_thr: f64,
        pub visual_space_pixel_size: f64,
        pub visual_space_close_iter: i32,
        pub ecc_map_filter_sigma: i32,
        pub border_width: i32,
        pub small_patch_thr: usize,
    }

    pub(super) fn run_split_merge(
        patches: Vec<Patch>,
        azi: &Array2<f64>,
        alt: &Array2<f64>,
        det_map: &Array2<f64>,
        p: Params,
        cancel: &AtomicBool,
    ) -> Result<Vec<Patch>, AnalysisError> {
        // Derive visual-space grid extents from the data.
        let grid = derive_visual_grid(alt, azi, p.visual_space_pixel_size);

        // Split/merge is the pipeline's hotspot (hundreds of ms); a mid-stage
        // cancellation must not wait it out. Check before the split pass and at
        // the top of every merge round — the coarse units of work.
        if cancel.load(Ordering::Relaxed) {
            return Err(AnalysisError::Cancelled);
        }

        // -------- SPLIT --------
        // Each patch's split decision is independent (reads only shared
        // immutable maps), so the loop parallelizes. `flat_map_iter` + ordered
        // `collect` preserves input order, so the resulting patch sequence — and
        // every downstream index-sensitive merge decision — is identical to the
        // serial path.
        let after_split: Vec<Patch> = patches
            .into_par_iter()
            .flat_map_iter(|patch| {
                let (_vs, au) =
                    patch_visual_space(&patch.mask, azi, alt, &grid, p.visual_space_close_iter);
                let as_area = sigma_area(&patch.mask, det_map);
                let out: Vec<Patch> = if au > 1e-9 && as_area / au >= p.split_overlap_thr {
                    let split_into = split_patch(
                        &patch,
                        azi,
                        alt,
                        p.split_local_min_cut_step,
                        p.ecc_map_filter_sigma,
                        p.border_width,
                    );
                    if split_into.len() >= 2 {
                        split_into
                    } else {
                        vec![patch]
                    }
                } else {
                    vec![patch]
                };
                out.into_iter()
            })
            .collect();

        // -------- MERGE --------
        let mut current = after_split;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(AnalysisError::Cancelled);
            }
            let n = current.len();
            // Find adjacent same-sign pairs (Allen calls with
            // borderWidth+1 — i.e. 1-pixel dilation of each).
            let adj_width = p.border_width + 1;
            // Each patch's visual-space projection (with its `binary_closing`)
            // is constant within a round but was recomputed for every O(N²)
            // pair — once as `i`, N−1 times as `j`. Project each patch ONCE per
            // round and reuse; identical values, just not 2·N² times. The
            // per-pair *merged* projection stays per-pair (its mask is unique).
            let vs_au: Vec<(Array2<bool>, f64)> = current
                .iter()
                .map(|patch| {
                    patch_visual_space(&patch.mask, azi, alt, &grid, p.visual_space_close_iter)
                })
                .collect();
            // Candidate evaluation is pure (reads only shared immutable
            // `current`/`vs_au`/maps), so the O(N²) pair scan parallelizes.
            // `flat_map_iter` + ordered `collect` yields candidates in the same
            // (i, j) lexicographic order as the serial loop, so the stable sort
            // below — and thus the greedy merge result — is bit-identical.
            // Shared references (Copy) + scalar params, so both closures
            // capture by value without moving the owners (`current`/`vs_au`/
            // `grid` are reused across rounds and after the loop).
            let cur = &current;
            let vs = &vs_au;
            let gr = &grid;
            let border_width = p.border_width;
            let close_iter = p.visual_space_close_iter;
            let pixel_size = p.visual_space_pixel_size;
            let merge_overlap_thr = p.merge_overlap_thr;
            let mut candidates: Vec<MergeCandidate> = (0..n)
                .into_par_iter()
                .flat_map_iter(move |i| {
                    ((i + 1)..n).filter_map(move |j| {
                        if cur[i].sign != cur[j].sign {
                            return None;
                        }
                        if !is_adjacent(&cur[i].mask, &cur[j].mask, adj_width) {
                            return None;
                        }
                        // too far apart even with closing → no candidate
                        let merged_mask = merge_two(&cur[i].mask, &cur[j].mask, border_width)?;
                        let (vs1, au1) = (&vs[i].0, vs[i].1);
                        let (vs2, au2) = (&vs[j].0, vs[j].1);
                        let (_vsm, au_m) =
                            patch_visual_space(&merged_mask, azi, alt, gr, close_iter);
                        if au1 < 1e-9 || au2 < 1e-9 {
                            return None;
                        }
                        let a_overlap = visual_space_overlap(vs1, vs2, pixel_size);
                        let r1 = a_overlap / au1;
                        let r2 = a_overlap / au2;
                        if r1 <= merge_overlap_thr && r2 <= merge_overlap_thr {
                            Some(MergeCandidate {
                                i,
                                j,
                                merged_mask,
                                sign: cur[i].sign,
                                max_ratio: r1.max(r2),
                                neg_au: -au_m,
                            })
                        } else {
                            None
                        }
                    })
                })
                .collect();
            if candidates.is_empty() {
                break;
            }

            // Sort: max_ratio ascending, then neg_au ascending
            // (= au descending → bigger merges first when ratios tie).
            candidates.sort_by(|a, b| {
                a.max_ratio
                    .partial_cmp(&b.max_ratio)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(
                        a.neg_au
                            .partial_cmp(&b.neg_au)
                            .unwrap_or(std::cmp::Ordering::Equal),
                    )
            });

            // Greedy apply — skip any candidate whose indices have
            // already been consumed this iteration.
            let mut consumed = vec![false; n];
            let mut next_patches: Vec<Patch> = Vec::with_capacity(n);
            for cand in &candidates {
                if consumed[cand.i] || consumed[cand.j] {
                    continue;
                }
                consumed[cand.i] = true;
                consumed[cand.j] = true;
                next_patches.push(Patch {
                    mask: cand.merged_mask.clone(),
                    sign: cand.sign,
                });
            }
            // Carry through patches that weren't merged this round.
            for (idx, patch) in current.into_iter().enumerate() {
                if !consumed[idx] {
                    next_patches.push(patch);
                }
            }
            current = next_patches;
        }

        // Final small-patch cull.
        current.retain(|p2| p2.area() >= p.small_patch_thr);
        Ok(current)
    }

    struct MergeCandidate {
        i: usize,
        j: usize,
        merged_mask: Array2<bool>,
        sign: i8,
        max_ratio: f64,
        neg_au: f64,
    }

    // -------------------------------------------------------------------------
    // Visual-space projection (`getVisualSpace`)
    // -------------------------------------------------------------------------

    pub(super) struct VisualGrid {
        pub alt_min: f64,
        pub azi_min: f64,
        pub pixel_size: f64,
        pub h: usize,
        pub w: usize,
    }

    fn derive_visual_grid(alt: &Array2<f64>, azi: &Array2<f64>, pixel_size: f64) -> VisualGrid {
        // Tight bounding box of finite alt/azi values, with a one-pixel
        // pad. Mirrors Allen's hardcoded ranges in concept (they fix
        // alt ∈ [-40, 60], azi ∈ [-20, 120]) but adapts to the actual
        // rig's stimulus extent.
        let mut alt_min = f64::INFINITY;
        let mut alt_max = f64::NEG_INFINITY;
        let mut azi_min = f64::INFINITY;
        let mut azi_max = f64::NEG_INFINITY;
        let (h, w) = alt.dim();
        for r in 0..h {
            for c in 0..w {
                let a = alt[[r, c]];
                let z = azi[[r, c]];
                if a.is_finite() && z.is_finite() {
                    if a < alt_min {
                        alt_min = a;
                    }
                    if a > alt_max {
                        alt_max = a;
                    }
                    if z < azi_min {
                        azi_min = z;
                    }
                    if z > azi_max {
                        azi_max = z;
                    }
                }
            }
        }
        if !alt_min.is_finite() {
            return VisualGrid {
                alt_min: 0.0,
                azi_min: 0.0,
                pixel_size,
                h: 1,
                w: 1,
            };
        }
        let pad = pixel_size;
        let alt_lo = alt_min - pad;
        let alt_hi = alt_max + pad;
        let azi_lo = azi_min - pad;
        let azi_hi = azi_max + pad;
        let h_v = ((alt_hi - alt_lo) / pixel_size).ceil().max(1.0) as usize;
        let w_v = ((azi_hi - azi_lo) / pixel_size).ceil().max(1.0) as usize;
        VisualGrid {
            alt_min: alt_lo,
            azi_min: azi_lo,
            pixel_size,
            h: h_v,
            w: w_v,
        }
    }

    /// Project `patch_mask` into visual space. Returns `(mask, unique_area)`
    /// — `mask` is the visual-space binary mask (after binary_closing),
    /// `unique_area` is `count(mask) · pixel_size²`.
    pub(super) fn patch_visual_space(
        patch_mask: &Array2<bool>,
        azi: &Array2<f64>,
        alt: &Array2<f64>,
        grid: &VisualGrid,
        close_iter: i32,
    ) -> (Array2<bool>, f64) {
        let (h, w) = patch_mask.dim();
        let mut vs = Array2::<bool>::from_elem((grid.h, grid.w), false);
        for r in 0..h {
            for c in 0..w {
                if !patch_mask[[r, c]] {
                    continue;
                }
                let a = alt[[r, c]];
                let z = azi[[r, c]];
                if !a.is_finite() || !z.is_finite() {
                    continue;
                }
                let i_a = ((a - grid.alt_min) / grid.pixel_size).floor();
                let i_z = ((z - grid.azi_min) / grid.pixel_size).floor();
                if i_a < 0.0 || i_z < 0.0 {
                    continue;
                }
                let i_a = i_a as usize;
                let i_z = i_z as usize;
                if i_a < grid.h && i_z < grid.w {
                    vs[[i_a, i_z]] = true;
                }
            }
        }
        if close_iter > 0 {
            vs = binary_closing_cross(&vs, close_iter);
        }
        let count = vs.iter().filter(|&&b| b).count();
        let au = count as f64 * grid.pixel_size * grid.pixel_size;
        (vs, au)
    }

    pub(super) fn visual_space_overlap(a: &Array2<bool>, b: &Array2<bool>, pixel_size: f64) -> f64 {
        let (h, w) = a.dim();
        let mut n = 0usize;
        for r in 0..h {
            for c in 0..w {
                if a[[r, c]] && b[[r, c]] {
                    n += 1;
                }
            }
        }
        n as f64 * pixel_size * pixel_size
    }

    // -------------------------------------------------------------------------
    // Sigma area (`getSigmaArea`)
    // -------------------------------------------------------------------------

    pub(super) fn sigma_area(patch_mask: &Array2<bool>, det_map: &Array2<f64>) -> f64 {
        // Allen `getSigmaArea` = `np.sum(int_mask * detMap)` over the WHOLE
        // image. `0.0 * NaN == NaN` in both NumPy and Rust, so any NaN in
        // `det_map` (inside OR outside the patch) propagates to NaN — matching
        // the oracle. (The previous masked, finite-only sum silently returned a
        // finite value where Allen returns NaN.)
        let (h, w) = patch_mask.dim();
        let mut s = 0.0_f64;
        for r in 0..h {
            for c in 0..w {
                let m = if patch_mask[[r, c]] { 1.0 } else { 0.0 };
                s += m * det_map[[r, c]];
            }
        }
        s
    }

    // -------------------------------------------------------------------------
    // Per-patch eccentricity (`Patch.eccentricityMap`, RetinotopicMapping.py L2818)
    // -------------------------------------------------------------------------

    /// Great-circle distance on the visual sphere from `(alt_c, azi_c)`
    /// in degrees. Allen formula
    /// `arctan(sqrt(tan²(alt-altC) + tan²(azi-aziC)/cos²(alt-altC)))`.
    /// Computed over the **full image** (not masked to the patch), so
    /// the subsequent `uniform_filter_finite` doesn't bleed across the
    /// patch boundary. Mask back to the patch after filtering via
    /// `mask_to_patch`.
    pub(super) fn eccentricity_full_image(
        azi: &Array2<f64>,
        alt: &Array2<f64>,
        alt_c: f64,
        azi_c: f64,
    ) -> Array2<f64> {
        let (h, w) = azi.dim();
        let to_rad = std::f64::consts::PI / 180.0;
        let alt_c_r = alt_c * to_rad;
        let azi_c_r = azi_c * to_rad;
        let mut ecc = Array2::<f64>::from_elem((h, w), f64::NAN);
        for r in 0..h {
            for c in 0..w {
                let a = alt[[r, c]];
                let z = azi[[r, c]];
                if !a.is_finite() || !z.is_finite() {
                    continue;
                }
                let dalt = a * to_rad - alt_c_r;
                let dazi = z * to_rad - azi_c_r;
                let cos_dalt = dalt.cos();
                let term = (dalt.tan().powi(2) + dazi.tan().powi(2) / (cos_dalt * cos_dalt))
                    .sqrt()
                    .atan();
                ecc[[r, c]] = term * (180.0 / std::f64::consts::PI);
            }
        }
        ecc
    }

    /// Set non-patch pixels to NaN. Used after `uniform_filter_finite`
    /// to restrict markers and watershed to within the patch.
    fn mask_to_patch(arr: &Array2<f64>, patch_mask: &Array2<bool>) -> Array2<f64> {
        let (h, w) = arr.dim();
        Array2::from_shape_fn((h, w), |(r, c)| {
            if patch_mask[[r, c]] {
                arr[[r, c]]
            } else {
                f64::NAN
            }
        })
    }

    /// Allen `Patch.getPixelVisualCenter` (`RetinotopicMapping.py`
    /// L2805-2816): the mean of each visual coordinate over patch pixels where
    /// that coordinate is `!= 0`, computed INDEPENDENTLY per coordinate (the
    /// alt centre and azi centre may average over different pixel subsets —
    /// `array*altMap` then `mean(.[. != 0])`, likewise for azi). Returns
    /// `(alt_center, azi_center)` in degrees.
    ///
    /// The previous version averaged alt and azi over the SAME finite subset
    /// and never excluded exact-zero pixels — both wrong, shifting the centre
    /// (and hence every downstream eccentricity value).
    fn patch_visual_center(
        patch_mask: &Array2<bool>,
        azi: &Array2<f64>,
        alt: &Array2<f64>,
    ) -> (f64, f64) {
        let (h, w) = patch_mask.dim();
        let (mut sum_alt, mut n_alt) = (0.0_f64, 0usize);
        let (mut sum_azi, mut n_azi) = (0.0_f64, 0usize);
        for r in 0..h {
            for c in 0..w {
                if !patch_mask[[r, c]] {
                    continue;
                }
                let a = alt[[r, c]];
                if a.is_finite() && a != 0.0 {
                    sum_alt += a;
                    n_alt += 1;
                }
                let z = azi[[r, c]];
                if z.is_finite() && z != 0.0 {
                    sum_azi += z;
                    n_azi += 1;
                }
            }
        }
        let alt_c = if n_alt > 0 { sum_alt / n_alt as f64 } else { 0.0 };
        let azi_c = if n_azi > 0 { sum_azi / n_azi as f64 } else { 0.0 };
        (alt_c, azi_c)
    }

    /// Faithful port of `scipy.ndimage.uniform_filter(arr, size)` (mode
    /// `'reflect'`, `origin=0`), the call Allen makes on the eccentricity
    /// map before `localMin` (`RetinotopicMapping.py` L1230).
    ///
    /// Separable: a 1-D box of the FULL width `size` along each axis, where
    /// the window for output index `i` is `arr[i - size/2 + k]`, `k ∈ 0..size`
    /// — asymmetric for even `size` (scipy's convention), not the previous
    /// symmetric `2·radius+1`. Borders use scipy `'reflect'` (half-sample,
    /// edge duplicated). Each pass divides by the full `size`, and NaN
    /// PROPAGATES (scipy averages the raw window including NaN). The previous
    /// implementation averaged only the in-bounds, finite pixels — neither the
    /// scipy window, the scipy border, nor the scipy NaN behaviour.
    fn uniform_filter_finite(arr: &Array2<f64>, size: i32) -> Array2<f64> {
        if size <= 1 {
            return arr.clone();
        }
        let size = size as usize;
        let lo = (size / 2) as i32;

        // scipy `'reflect'` index fold: edge sample duplicated
        // (`…c b a | a b c d | d c b…`).
        fn reflect(mut i: i32, n: i32) -> usize {
            if n == 1 {
                return 0;
            }
            loop {
                if i < 0 {
                    i = -i - 1;
                } else if i >= n {
                    i = 2 * n - 1 - i;
                } else {
                    return i as usize;
                }
            }
        }

        let (h, w) = arr.dim();
        // Pass 1 — along the width (axis 1).
        let mut tmp = Array2::<f64>::zeros((h, w));
        for r in 0..h {
            for c in 0..w {
                let mut s = 0.0;
                for k in 0..size {
                    let j = c as i32 - lo + k as i32;
                    s += arr[[r, reflect(j, w as i32)]];
                }
                tmp[[r, c]] = s / size as f64;
            }
        }
        // Pass 2 — along the height (axis 0).
        let mut out = Array2::<f64>::zeros((h, w));
        for c in 0..w {
            for r in 0..h {
                let mut s = 0.0;
                for k in 0..size {
                    let i = r as i32 - lo + k as i32;
                    s += tmp[[reflect(i, h as i32), c]];
                }
                out[[r, c]] = s / size as f64;
            }
        }
        out
    }

    // -------------------------------------------------------------------------
    // `localMin` — Allen RetinotopicMapping.py L382
    // -------------------------------------------------------------------------

    /// Progressive thresholding of `ecc_map`: increase the cut from
    /// `min(ecc) - bin_size` upward by `bin_size` steps. At each cut,
    /// label CCs of `ecc <= cut`. Stop at the first cut yielding ≥ 2
    /// CCs. Returns the label map at that point (0 = no marker).
    fn local_min_markers(ecc_map: &Array2<f64>, bin_size: f64) -> Array2<i32> {
        let (h, w) = ecc_map.dim();
        let mut vmin = f64::INFINITY;
        let mut vmax = f64::NEG_INFINITY;
        for r in 0..h {
            for c in 0..w {
                let v = ecc_map[[r, c]];
                if v.is_finite() {
                    if v < vmin {
                        vmin = v;
                    }
                    if v > vmax {
                        vmax = v;
                    }
                }
            }
        }
        if !vmin.is_finite() {
            return Array2::<i32>::zeros((h, w));
        }
        let mut cut = vmin - bin_size;
        let cut_max = vmax + bin_size;
        let mut last_marker: Array2<i32> = Array2::zeros((h, w));
        while cut <= cut_max {
            let marker = Array2::from_shape_fn((h, w), |(r, c)| {
                let v = ecc_map[[r, c]];
                v.is_finite() && v <= cut
            });
            let (labels, n) = label_4conn(&marker);
            last_marker = labels;
            if n >= 2 {
                break;
            }
            cut += bin_size;
        }
        last_marker
    }

    // -------------------------------------------------------------------------
    // Watershed (marker-based, 8-conn)
    // -------------------------------------------------------------------------

    /// Marker-based watershed by immersion — a faithful port of
    /// `skimage.segmentation.watershed(elevation, markers,
    /// connectivity=ones((3,3)), mask=…, watershed_line=False)`, the call
    /// Allen `Patch.split2` makes (`RetinotopicMapping.py` L3540).
    ///
    /// skimage floods from the markers using a priority queue keyed by
    /// `(elevation, age)`, where `age` is a monotonic push counter that
    /// breaks ties in FIFO order (the nearer flood front, by entry time,
    /// wins a plateau). Every in-mask pixel is claimed by the FIRST front
    /// to reach it, so with `watershed_line=False` there are NO unlabelled
    /// in-mask pixels.
    ///
    /// The previous implementation was a different algorithm: it kept a
    /// pixel at 0 when its labelled neighbours carried ≥2 distinct labels
    /// (that is the `watershed_line=True` / cv2 behaviour, not skimage's
    /// default) and apportioned plateaus by an elevation-bucket fixpoint
    /// sweep — both wrong. 8-connected; out-of-mask and non-finite pixels
    /// are never flooded (skimage masks markers and the basin to `mask`).
    ///
    /// Validated bit-for-bit against skimage by
    /// `watershed_from_markers_matches_skimage`.
    fn watershed_from_markers(
        elevation: &Array2<f64>,
        markers: &Array2<i32>,
        mask: &Array2<bool>,
    ) -> Array2<i32> {
        use std::cmp::Ordering;
        use std::collections::BinaryHeap;

        let (h, w) = elevation.dim();
        let mut labels = markers.clone();

        // MIN-heap on (value, age): `BinaryHeap` is a max-heap, so `Ord` is
        // inverted. `age` = push order, skimage's FIFO tie-break.
        struct Item {
            value: f64,
            age: u64,
            r: usize,
            c: usize,
        }
        impl PartialEq for Item {
            fn eq(&self, o: &Self) -> bool {
                self.value == o.value && self.age == o.age
            }
        }
        impl Eq for Item {}
        impl Ord for Item {
            fn cmp(&self, o: &Self) -> Ordering {
                // Smaller value first, then smaller age first — inverted for
                // the max-heap so `pop` yields the global minimum.
                o.value
                    .partial_cmp(&self.value)
                    .unwrap_or(Ordering::Equal)
                    .then_with(|| o.age.cmp(&self.age))
            }
        }
        impl PartialOrd for Item {
            fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
                Some(self.cmp(o))
            }
        }

        let mut heap: BinaryHeap<Item> = BinaryHeap::new();
        // skimage seeds every in-mask marker pixel with `age = 0` (raster
        // order); the flood counter starts at 1. Marker pixels outside the
        // mask are dropped.
        for r in 0..h {
            for c in 0..w {
                if mask[[r, c]] && markers[[r, c]] > 0 {
                    heap.push(Item {
                        value: elevation[[r, c]],
                        age: 0,
                        r,
                        c,
                    });
                }
            }
        }
        let mut age: u64 = 1;

        while let Some(it) = heap.pop() {
            let lab = labels[[it.r, it.c]];
            for dr in -1i32..=1 {
                for dc in -1i32..=1 {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let rr = it.r as i32 + dr;
                    let cc = it.c as i32 + dc;
                    if rr < 0 || rr >= h as i32 || cc < 0 || cc >= w as i32 {
                        continue;
                    }
                    let (rr, cc) = (rr as usize, cc as usize);
                    if !mask[[rr, cc]] || labels[[rr, cc]] > 0 {
                        continue;
                    }
                    let mut v = elevation[[rr, cc]];
                    if !v.is_finite() {
                        continue;
                    }
                    // Flood level: a pixel cannot be entered below the level
                    // of the front that reached it (skimage's
                    // `if new.value < elem.value: new.value = elem.value`).
                    if v < it.value {
                        v = it.value;
                    }
                    // Claim at enqueue time (skimage sets output[neighbour]
                    // when pushing), so a pixel is taken by the first front.
                    labels[[rr, cc]] = lab;
                    heap.push(Item {
                        value: v,
                        age,
                        r: rr,
                        c: cc,
                    });
                    age += 1;
                }
            }
        }
        labels
    }

    // -------------------------------------------------------------------------
    // `Patch.split2` — RetinotopicMapping.py L2853
    // -------------------------------------------------------------------------

    fn split_patch(
        patch: &Patch,
        azi: &Array2<f64>,
        alt: &Array2<f64>,
        cut_step: f64,
        ecc_filter_sigma: i32,
        border_width: i32,
    ) -> Vec<Patch> {
        let (alt_c, azi_c) = patch_visual_center(&patch.mask, azi, alt);
        // Allen `_getEccentricityMap` (RetinotopicMapping.py L1212):
        // computes ecc over the FULL image, uniform-filters it, then
        // **assigns only patch pixels** (others NaN). The post-filter
        // re-mask is critical — without it, the filter bleeds finite
        // values outward across the patch boundary and `local_min`
        // finds markers OUTSIDE the patch.
        let ecc_full = eccentricity_full_image(azi, alt, alt_c, azi_c);
        let ecc_full_f = uniform_filter_finite(&ecc_full, ecc_filter_sigma);
        let ecc_f = mask_to_patch(&ecc_full_f, &patch.mask);
        split_patch_from_ecc(patch, &ecc_f, cut_step, border_width)
    }

    /// The `split2` composition downstream of the eccentricity map: localMin
    /// → watershed → (whole-patch border ∪ per-region borders) → skeletonize
    /// → dilate → cut from the dilated patch → label → AND patch. Split out
    /// so it can be golden-tested against Allen `split2` with a hand-built
    /// `ecc_f` (eccMap, NaN outside the patch); the eccentricity-map
    /// construction (azi/alt → ecc) is validated separately.
    fn split_patch_from_ecc(
        patch: &Patch,
        ecc_f: &Array2<f64>,
        cut_step: f64,
        border_width: i32,
    ) -> Vec<Patch> {
        let markers = local_min_markers(ecc_f, cut_step);
        let n_min = markers.iter().copied().max().unwrap_or(0);
        if n_min < 2 {
            return vec![patch.clone()];
        }
        // Watershed within the patch mask.
        let watershed = watershed_from_markers(ecc_f, &markers, &patch.mask);
        // Build per-region masks from the watershed labels and
        // include the watershed-boundary subtraction: Allen's split2
        // builds borders by skeletonizing each labelled region's
        // boundary; if border_width > 1, dilate. Then subtract those
        // borders from the patch mask.
        let (h, w) = patch.mask.dim();
        // Allen split2 seeds the border with the WHOLE-PATCH outer border
        // `dilate(self.array) − self.array`, then unions the per-region
        // watershed borders. Omitting it left the outer ring uncut, so the
        // split collapsed back to a single patch.
        let dil_full = crate::segmentation::morphology::binary_dilation_cross(&patch.mask, 1);
        let mut all_borders =
            Array2::from_shape_fn((h, w), |(r, c)| dil_full[[r, c]] && !patch.mask[[r, c]]);
        for k in 1..=n_min {
            let region = Array2::from_shape_fn((h, w), |(r, c)| watershed[[r, c]] == k);
            // Boundary = dilate(region) - region.
            let dilated = crate::segmentation::morphology::binary_dilation_cross(&region, 1);
            for r in 0..h {
                for c in 0..w {
                    if dilated[[r, c]] && !region[[r, c]] {
                        all_borders[[r, c]] = true;
                    }
                }
            }
        }
        let mut border = binary_skeletonize_skimage(&all_borders);
        if border_width > 1 {
            border =
                crate::segmentation::morphology::binary_dilation_cross(&border, border_width - 1);
        }
        // new_patches = dilate(patch.mask, 1) AND NOT border
        // — Allen does `binary_dilation(self.array)`; we mirror that.
        let dil_patch = crate::segmentation::morphology::binary_dilation_cross(&patch.mask, 1);
        let new_patches_bin =
            Array2::from_shape_fn((h, w), |(r, c)| dil_patch[[r, c]] && !border[[r, c]]);
        let (labeled, n) = label_4conn(&new_patches_bin);
        let mut out: Vec<Patch> = Vec::with_capacity(n);
        for k in 1..=n as i32 {
            let curr =
                Array2::from_shape_fn((h, w), |(r, c)| labeled[[r, c]] == k && patch.mask[[r, c]]);
            if curr.iter().any(|&b| b) {
                out.push(Patch {
                    mask: curr,
                    sign: patch.sign,
                });
            }
        }
        if out.is_empty() {
            vec![patch.clone()]
        } else {
            out
        }
    }

    // -------------------------------------------------------------------------
    // `mergePatches` (binary, module-level) — RetinotopicMapping.py L435
    // -------------------------------------------------------------------------

    /// Allen `mergePatches(a, b, borderWidth)`: union the two binary
    /// patches and apply `binary_closing(iterations=border_width)`. If
    /// the result is a single CC, return it; otherwise the patches are
    /// too far apart to merge.
    fn merge_two(a: &Array2<bool>, b: &Array2<bool>, border_width: i32) -> Option<Array2<bool>> {
        let (h, w) = a.dim();
        let union = Array2::from_shape_fn((h, w), |(r, c)| a[[r, c]] || b[[r, c]]);
        let merged = binary_closing_cross(&union, border_width.max(1));
        let (_, n) = label_4conn(&merged);
        if n == 1 {
            Some(merged)
        } else {
            None
        }
    }

    // ── Golden cross-validation of the Allen split/merge sub-components
    //    against their real oracles (skimage / scipy). Nested inside
    //    `mod allen` so the tests reach the private fns without widening
    //    their visibility. Fixtures in tests/golden/fixtures/.
    #[cfg(test)]
    mod golden {
        use super::*;
        use crate::test_support::{load_f64, load_i32};

        /// `watershed_from_markers` vs `skimage.segmentation.watershed`
        /// (`connectivity=ones((3,3))`, `watershed_line=False`), the call
        /// Allen `Patch.split2` makes. Stress scene: colliding basins on a
        /// flat plateau, a border-touching marker, a thin isthmus, mask
        /// holes. Fixtures from `gen_watershed_markers_golden.py`.
        #[test]
        fn watershed_from_markers_matches_skimage() {
            const N: usize = 24;
            let elev = load_f64(include_bytes!("../../tests/golden/fixtures/ws_elev.bin"));
            let mk = load_i32(include_bytes!("../../tests/golden/fixtures/ws_markers.bin"));
            let mask_b: &[u8] = include_bytes!("../../tests/golden/fixtures/ws_mask.bin");
            let exp = load_i32(include_bytes!("../../tests/golden/fixtures/ws_out.bin"));

            let elevation = Array2::from_shape_fn((N, N), |(r, c)| elev[r * N + c]);
            let markers = Array2::from_shape_fn((N, N), |(r, c)| mk[r * N + c]);
            let mask = Array2::from_shape_fn((N, N), |(r, c)| mask_b[r * N + c] != 0);

            let out = watershed_from_markers(&elevation, &markers, &mask);
            let mut diff = 0usize;
            let mut unlabelled_in_mask = 0usize;
            for r in 0..N {
                for c in 0..N {
                    if out[[r, c]] != exp[r * N + c] {
                        diff += 1;
                    }
                    if mask[[r, c]] && out[[r, c]] == 0 {
                        unlabelled_in_mask += 1;
                    }
                }
            }
            eprintln!(
                "watershed vs skimage: differing px = {diff}, unlabelled-in-mask = {unlabelled_in_mask}"
            );
            assert_eq!(diff, 0, "watershed_from_markers diverges from skimage");
        }

        /// `split_patch_from_ecc` (the `split2` composition) vs a verbatim
        /// transcription of Allen `Patch.split2` (the `sm.watershed` branch,
        /// `RetinotopicMapping.py` L2853-2909) run on scipy + skimage. A
        /// plateau eccentricity field with two wells forces the split; we
        /// compare the patch COUNT and the order-free UNION of output masks.
        /// Fixtures from `gen_splitpatch_golden.py` (N=48, cut_step=1,
        /// border_width=2).
        #[test]
        fn split2_matches_allen_watershed_branch() {
            const N: usize = 48;
            let mask_b: &[u8] = include_bytes!("../../tests/golden/fixtures/splitpatch_mask.bin");
            let ecc_v = load_f64(include_bytes!("../../tests/golden/fixtures/splitpatch_ecc.bin"));
            let n_exp =
                load_i32(include_bytes!("../../tests/golden/fixtures/splitpatch_nlabels.bin"))[0];
            let union_b: &[u8] =
                include_bytes!("../../tests/golden/fixtures/splitpatch_union.bin");

            let mask = Array2::from_shape_fn((N, N), |(r, c)| mask_b[r * N + c] != 0);
            let ecc = Array2::from_shape_fn((N, N), |(r, c)| ecc_v[r * N + c]);
            let patch = Patch { mask, sign: 1 };

            let out = split_patch_from_ecc(&patch, &ecc, 1.0, 2);

            // Order-free union of the returned patch masks.
            let mut union = Array2::<bool>::from_elem((N, N), false);
            for p in &out {
                for r in 0..N {
                    for c in 0..N {
                        if p.mask[[r, c]] {
                            union[[r, c]] = true;
                        }
                    }
                }
            }
            let mut udiff = 0usize;
            for r in 0..N {
                for c in 0..N {
                    if (union[[r, c]] as u8) != union_b[r * N + c] {
                        udiff += 1;
                    }
                }
            }
            eprintln!(
                "split2 vs Allen: n_patches = {} (expected {n_exp}), union diff = {udiff}",
                out.len()
            );
            assert_eq!(out.len() as i32, n_exp, "split2 patch count diverges from Allen");
            assert_eq!(udiff, 0, "split2 union mask diverges from Allen");
        }

        /// `uniform_filter_finite` vs `scipy.ndimage.uniform_filter`
        /// (`mode='reflect'`): odd size 15 and even size 10 on a 48×48 field,
        /// size 5 on 11×11. Even sizes are the asymmetric-window stress.
        /// Fixtures from `gen_uniform_filter_ecc_golden.py`.
        #[test]
        fn uniform_filter_matches_scipy_reflect() {
            fn maxdiff(out: &Array2<f64>, golden: &[f64], h: usize, w: usize) -> f64 {
                let mut m = 0.0f64;
                for r in 0..h {
                    for c in 0..w {
                        let (o, g) = (out[[r, c]], golden[r * w + c]);
                        if o.is_nan() || g.is_nan() {
                            assert_eq!(o.is_nan(), g.is_nan(), "NaN mismatch at {r},{c}");
                        } else {
                            m = m.max((o - g).abs());
                        }
                    }
                }
                m
            }
            let ia = load_f64(include_bytes!("../../tests/golden/fixtures/uf_in_a.bin"));
            let a = Array2::from_shape_fn((48, 48), |(r, c)| ia[r * 48 + c]);
            let d15 = maxdiff(
                &uniform_filter_finite(&a, 15),
                &load_f64(include_bytes!("../../tests/golden/fixtures/uf_out_a15.bin")),
                48,
                48,
            );
            let d10 = maxdiff(
                &uniform_filter_finite(&a, 10),
                &load_f64(include_bytes!("../../tests/golden/fixtures/uf_out_a10.bin")),
                48,
                48,
            );
            let ib = load_f64(include_bytes!("../../tests/golden/fixtures/uf_in_b.bin"));
            let b = Array2::from_shape_fn((11, 11), |(r, c)| ib[r * 11 + c]);
            let d5 = maxdiff(
                &uniform_filter_finite(&b, 5),
                &load_f64(include_bytes!("../../tests/golden/fixtures/uf_out_b5.bin")),
                11,
                11,
            );
            eprintln!("uniform_filter vs scipy: d15={d15:.2e} d10={d10:.2e} d5={d5:.2e}");
            assert!(
                d15 < 1e-9 && d10 < 1e-9 && d5 < 1e-9,
                "uniform_filter_finite diverges from scipy uniform_filter (reflect)"
            );
        }

        /// `sigma_area` vs Allen `getSigmaArea` = `np.sum(int_mask · detMap)`.
        /// Five cases incl. NaN inside / outside the patch (where `0·NaN=NaN`
        /// must propagate) and a negative determinant. Fixtures from
        /// `gen_sigmaarea_golden.py` (mask/det are H·W = 768 px; the sum is
        /// orientation-free).
        #[test]
        fn sigma_area_matches_allen_get_sigma_area() {
            fn run(name: &str, mask_b: &[u8], det_b: &[u8], exp_b: &[u8]) {
                const H: usize = 24;
                const W: usize = 32;
                let mask = Array2::from_shape_fn((H, W), |(r, c)| mask_b[r * W + c] != 0);
                let dv = load_f64(det_b);
                let det = Array2::from_shape_fn((H, W), |(r, c)| dv[r * W + c]);
                let exp = f64::from_le_bytes(exp_b[0..8].try_into().unwrap());
                let got = sigma_area(&mask, &det);
                eprintln!("  sigma_area {name:10} got={got} exp={exp}");
                if exp.is_nan() {
                    assert!(got.is_nan(), "{name}: expected NaN, got {got}");
                } else {
                    assert!((got - exp).abs() < 1e-6, "{name}: {got} != {exp}");
                }
            }
            run(
                "finiteall",
                include_bytes!("../../tests/golden/fixtures/sigarea_finiteall_mask.bin"),
                include_bytes!("../../tests/golden/fixtures/sigarea_finiteall_det.bin"),
                include_bytes!("../../tests/golden/fixtures/sigarea_finiteall_exp.bin"),
            );
            run(
                "nanin",
                include_bytes!("../../tests/golden/fixtures/sigarea_nanin_mask.bin"),
                include_bytes!("../../tests/golden/fixtures/sigarea_nanin_det.bin"),
                include_bytes!("../../tests/golden/fixtures/sigarea_nanin_exp.bin"),
            );
            run(
                "nanout",
                include_bytes!("../../tests/golden/fixtures/sigarea_nanout_mask.bin"),
                include_bytes!("../../tests/golden/fixtures/sigarea_nanout_det.bin"),
                include_bytes!("../../tests/golden/fixtures/sigarea_nanout_exp.bin"),
            );
            run(
                "multicomp",
                include_bytes!("../../tests/golden/fixtures/sigarea_multicomp_mask.bin"),
                include_bytes!("../../tests/golden/fixtures/sigarea_multicomp_det.bin"),
                include_bytes!("../../tests/golden/fixtures/sigarea_multicomp_exp.bin"),
            );
            run(
                "negzero",
                include_bytes!("../../tests/golden/fixtures/sigarea_negzero_mask.bin"),
                include_bytes!("../../tests/golden/fixtures/sigarea_negzero_det.bin"),
                include_bytes!("../../tests/golden/fixtures/sigarea_negzero_exp.bin"),
            );
        }

        /// `eccentricity_full_image` (great-circle formula + NaN propagation)
        /// and `patch_visual_center` (the `!= 0`, per-coordinate centre) vs
        /// verbatim Allen `eccentricityMap` + `getPixelVisualCenter`
        /// (`RetinotopicMapping.py` L450/L2805). Stress: a patch pixel with
        /// `alt==0` and one with `azi==0`, NaN background. Fixtures from
        /// `gen_eccfull_golden.py` (`eccfull_center.bin` = `[altC, aziC]`).
        #[test]
        fn eccentricity_full_image_and_center_match_allen() {
            const N: usize = 24;
            let av = load_f64(include_bytes!("../../tests/golden/fixtures/eccfull_alt.bin"));
            let zv = load_f64(include_bytes!("../../tests/golden/fixtures/eccfull_azi.bin"));
            let ev = load_f64(include_bytes!("../../tests/golden/fixtures/eccfull_ecc.bin"));
            let mask_b: &[u8] = include_bytes!("../../tests/golden/fixtures/eccfull_mask.bin");
            let center_b: &[u8] = include_bytes!("../../tests/golden/fixtures/eccfull_center.bin");

            let alt = Array2::from_shape_fn((N, N), |(r, c)| av[r * N + c]);
            let azi = Array2::from_shape_fn((N, N), |(r, c)| zv[r * N + c]);
            let mask = Array2::from_shape_fn((N, N), |(r, c)| mask_b[r * N + c] != 0);
            let exp_alt_c = f64::from_le_bytes(center_b[0..8].try_into().unwrap());
            let exp_azi_c = f64::from_le_bytes(center_b[8..16].try_into().unwrap());

            // (b) centre.
            let (alt_c, azi_c) = patch_visual_center(&mask, &azi, &alt);
            eprintln!("center: alt_c={alt_c} (exp {exp_alt_c}), azi_c={azi_c} (exp {exp_azi_c})");
            assert!(
                (alt_c - exp_alt_c).abs() < 1e-9 && (azi_c - exp_azi_c).abs() < 1e-9,
                "patch_visual_center diverges from Allen getPixelVisualCenter"
            );

            // (a) full-image great-circle formula + NaN propagation, at the
            // oracle centre.
            let ecc = eccentricity_full_image(&azi, &alt, exp_alt_c, exp_azi_c);
            let mut md = 0.0f64;
            for r in 0..N {
                for c in 0..N {
                    let (o, g) = (ecc[[r, c]], ev[r * N + c]);
                    if o.is_nan() || g.is_nan() {
                        assert_eq!(o.is_nan(), g.is_nan(), "NaN mismatch at {r},{c}");
                    } else {
                        md = md.max((o - g).abs());
                    }
                }
            }
            eprintln!("eccentricity_full_image max diff = {md:.2e}");
            assert!(md < 1e-9, "eccentricity_full_image diverges from Allen: {md:.2e}");
        }

        /// `local_min_markers` vs verbatim Allen `localMin`
        /// (`RetinotopicMapping.py` L382-414), `bin_size=0.5`. Pins the
        /// progressive-threshold marker generation incl. label numbering, the
        /// ≥2-CC stop, NaN handling, and the single-min exhaustion branch.
        /// Fixtures from `gen_local_min_golden.py` (32×32). Predicted-match.
        #[test]
        fn local_min_matches_allen_localmin() {
            const N: usize = 32;
            fn check(name: &str, ecc_b: &[u8], mk_b: &[u8]) -> usize {
                let ev = ecc_b
                    .chunks_exact(8)
                    .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
                    .collect::<Vec<_>>();
                let exp = mk_b
                    .chunks_exact(4)
                    .map(|c| i32::from_le_bytes(c.try_into().unwrap()))
                    .collect::<Vec<_>>();
                let ecc = Array2::from_shape_fn((N, N), |(r, c)| ev[r * N + c]);
                let m = local_min_markers(&ecc, 0.5);
                let mut d = 0usize;
                for r in 0..N {
                    for c in 0..N {
                        if m[[r, c]] != exp[r * N + c] {
                            d += 1;
                        }
                    }
                }
                eprintln!("  local_min {name:11} differing = {d}");
                d
            }
            let mut total = 0;
            total += check(
                "two_basins",
                include_bytes!("../../tests/golden/fixtures/lmin_two_basins_ecc.bin"),
                include_bytes!("../../tests/golden/fixtures/lmin_two_basins_marker.bin"),
            );
            total += check(
                "border_min",
                include_bytes!("../../tests/golden/fixtures/lmin_border_min_ecc.bin"),
                include_bytes!("../../tests/golden/fixtures/lmin_border_min_marker.bin"),
            );
            total += check(
                "tie_step",
                include_bytes!("../../tests/golden/fixtures/lmin_tie_step_ecc.bin"),
                include_bytes!("../../tests/golden/fixtures/lmin_tie_step_marker.bin"),
            );
            total += check(
                "single_min",
                include_bytes!("../../tests/golden/fixtures/lmin_single_min_ecc.bin"),
                include_bytes!("../../tests/golden/fixtures/lmin_single_min_marker.bin"),
            );
            assert_eq!(total, 0, "local_min_markers diverges from Allen localMin");
        }

        /// `merge_two` vs verbatim Allen `mergePatches`
        /// (`RetinotopicMapping.py` L435-447), `border_width=2`. `flag=1` →
        /// `Some(spc)`; `flag=0` → `None` (Allen raises "too far apart").
        /// Fixtures from `gen_merge_two_golden.py` (32×32). Predicted-match.
        #[test]
        fn merge_two_matches_allen_mergepatches() {
            const N: usize = 32;
            fn check(name: &str, a_b: &[u8], b_b: &[u8], out_b: &[u8], flag_b: &[u8]) -> usize {
                let a = Array2::from_shape_fn((N, N), |(r, c)| a_b[r * N + c] != 0);
                let b = Array2::from_shape_fn((N, N), |(r, c)| b_b[r * N + c] != 0);
                let mergeable = flag_b[0] != 0;
                let got = merge_two(&a, &b, 2);
                let mut d = 0usize;
                match (got, mergeable) {
                    (Some(m), true) => {
                        for r in 0..N {
                            for c in 0..N {
                                if (m[[r, c]] as u8) != out_b[r * N + c] {
                                    d += 1;
                                }
                            }
                        }
                    }
                    (None, false) => {}
                    (Some(_), false) => d = 1,
                    (None, true) => d = 1,
                }
                eprintln!("  merge_two {name:13} mergeable={mergeable} diff={d}");
                d
            }
            let mut total = 0;
            for name in [
                "touch_border",
                "gap_eq_bw",
                "gap_too_far",
                "diag_only",
                "thin_bridge",
            ] {
                // include_bytes! needs literal paths; match on name.
                let (a, b, o, f): (&[u8], &[u8], &[u8], &[u8]) = match name {
                    "touch_border" => (
                        include_bytes!("../../tests/golden/fixtures/mt_touch_border_a.bin"),
                        include_bytes!("../../tests/golden/fixtures/mt_touch_border_b.bin"),
                        include_bytes!("../../tests/golden/fixtures/mt_touch_border_out.bin"),
                        include_bytes!("../../tests/golden/fixtures/mt_touch_border_flag.bin"),
                    ),
                    "gap_eq_bw" => (
                        include_bytes!("../../tests/golden/fixtures/mt_gap_eq_bw_a.bin"),
                        include_bytes!("../../tests/golden/fixtures/mt_gap_eq_bw_b.bin"),
                        include_bytes!("../../tests/golden/fixtures/mt_gap_eq_bw_out.bin"),
                        include_bytes!("../../tests/golden/fixtures/mt_gap_eq_bw_flag.bin"),
                    ),
                    "gap_too_far" => (
                        include_bytes!("../../tests/golden/fixtures/mt_gap_too_far_a.bin"),
                        include_bytes!("../../tests/golden/fixtures/mt_gap_too_far_b.bin"),
                        include_bytes!("../../tests/golden/fixtures/mt_gap_too_far_out.bin"),
                        include_bytes!("../../tests/golden/fixtures/mt_gap_too_far_flag.bin"),
                    ),
                    "diag_only" => (
                        include_bytes!("../../tests/golden/fixtures/mt_diag_only_a.bin"),
                        include_bytes!("../../tests/golden/fixtures/mt_diag_only_b.bin"),
                        include_bytes!("../../tests/golden/fixtures/mt_diag_only_out.bin"),
                        include_bytes!("../../tests/golden/fixtures/mt_diag_only_flag.bin"),
                    ),
                    _ => (
                        include_bytes!("../../tests/golden/fixtures/mt_thin_bridge_a.bin"),
                        include_bytes!("../../tests/golden/fixtures/mt_thin_bridge_b.bin"),
                        include_bytes!("../../tests/golden/fixtures/mt_thin_bridge_out.bin"),
                        include_bytes!("../../tests/golden/fixtures/mt_thin_bridge_flag.bin"),
                    ),
                };
                total += check(name, a, b, o, f);
            }
            assert_eq!(total, 0, "merge_two diverges from Allen mergePatches");
        }

        /// `patch_visual_space` vs verbatim Allen `Patch.getVisualSpace`
        /// (`RetinotopicMapping.py` L2745-2797): the scatter-into-grid (floor
        /// division), `binary_closing` (cross SE, border 0), and `uniqueArea =
        /// count·pixelSize²`. The grid is built to Allen's hardcoded ranges (the
        /// meta fixture carries `alt_min, azi_min, pixel_size, vs_h, vs_w`) so the
        /// projection/closing math is isolated from the (separately divergent)
        /// `derive_visual_grid` bounding box. Four cases (in/out-of-range gating,
        /// floor-division boundaries, NaN skip, border closing). Fixtures from
        /// `gen_patchvs_golden.py` (cortex grid 40×40).
        #[test]
        fn patch_visual_space_matches_allen_get_visual_space() {
            const MH: usize = 40;
            const MW: usize = 40;
            fn check(
                name: &str,
                mask_b: &[u8],
                alt_b: &[u8],
                azi_b: &[u8],
                out_b: &[u8],
                meta_b: &[u8],
                close_iter: i32,
            ) {
                let meta = meta_b
                    .chunks_exact(8)
                    .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
                    .collect::<Vec<_>>();
                let (alt_min, azi_min, ps) = (meta[0], meta[1], meta[2]);
                let (vs_h, vs_w) = (meta[3] as usize, meta[4] as usize);
                let ua_exp = meta[5];

                let alt_v = alt_b
                    .chunks_exact(8)
                    .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
                    .collect::<Vec<_>>();
                let azi_v = azi_b
                    .chunks_exact(8)
                    .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
                    .collect::<Vec<_>>();
                let mask = Array2::from_shape_fn((MH, MW), |(r, c)| mask_b[r * MW + c] != 0);
                let alt = Array2::from_shape_fn((MH, MW), |(r, c)| alt_v[r * MW + c]);
                let azi = Array2::from_shape_fn((MH, MW), |(r, c)| azi_v[r * MW + c]);

                let grid = VisualGrid {
                    alt_min,
                    azi_min,
                    pixel_size: ps,
                    h: vs_h,
                    w: vs_w,
                };
                let (vs, ua) = patch_visual_space(&mask, &azi, &alt, &grid, close_iter);
                assert_eq!((vs.nrows(), vs.ncols()), (vs_h, vs_w), "{name}: vs shape");
                let mut diff = 0usize;
                for r in 0..vs_h {
                    for c in 0..vs_w {
                        if (vs[[r, c]] as u8) != out_b[r * vs_w + c] {
                            diff += 1;
                        }
                    }
                }
                eprintln!("  patchvs {name:8} vs_diff={diff}  area got={ua} exp={ua_exp}");
                assert_eq!(diff, 0, "{name}: visual-space mask diverges from Allen");
                assert!(
                    (ua - ua_exp).abs() < 1e-9,
                    "{name}: uniqueArea {ua} != {ua_exp}"
                );
            }
            check(
                "basic",
                include_bytes!("../../tests/golden/fixtures/patchvs_mask_basic.bin"),
                include_bytes!("../../tests/golden/fixtures/patchvs_alt_basic.bin"),
                include_bytes!("../../tests/golden/fixtures/patchvs_azi_basic.bin"),
                include_bytes!("../../tests/golden/fixtures/patchvs_out_basic.bin"),
                include_bytes!("../../tests/golden/fixtures/patchvs_meta_basic.bin"),
                2,
            );
            check(
                "exact",
                include_bytes!("../../tests/golden/fixtures/patchvs_mask_exact.bin"),
                include_bytes!("../../tests/golden/fixtures/patchvs_alt_exact.bin"),
                include_bytes!("../../tests/golden/fixtures/patchvs_azi_exact.bin"),
                include_bytes!("../../tests/golden/fixtures/patchvs_out_exact.bin"),
                include_bytes!("../../tests/golden/fixtures/patchvs_meta_exact.bin"),
                1,
            );
            check(
                "border",
                include_bytes!("../../tests/golden/fixtures/patchvs_mask_border.bin"),
                include_bytes!("../../tests/golden/fixtures/patchvs_alt_border.bin"),
                include_bytes!("../../tests/golden/fixtures/patchvs_azi_border.bin"),
                include_bytes!("../../tests/golden/fixtures/patchvs_out_border.bin"),
                include_bytes!("../../tests/golden/fixtures/patchvs_meta_border.bin"),
                3,
            );
            check(
                "random",
                include_bytes!("../../tests/golden/fixtures/patchvs_mask_random.bin"),
                include_bytes!("../../tests/golden/fixtures/patchvs_alt_random.bin"),
                include_bytes!("../../tests/golden/fixtures/patchvs_azi_random.bin"),
                include_bytes!("../../tests/golden/fixtures/patchvs_out_random.bin"),
                include_bytes!("../../tests/golden/fixtures/patchvs_meta_random.bin"),
                1,
            );
        }

        /// `visual_space_overlap` = `count(a ∧ b) · pixel_size²` — Allen's
        /// `Patch.getOverlap` numerator (the shared visual-space area used by the
        /// merge criterion). Deterministic hand-built masks: rows {0,1} ∧ cols
        /// {1,2,3} = a 2×3 = 6-cell intersection.
        #[test]
        fn visual_space_overlap_counts_intersection_area() {
            let a = Array2::from_shape_fn((4, 5), |(r, _)| r < 2);
            let b = Array2::from_shape_fn((4, 5), |(_, c)| (1..=3).contains(&c));
            let ps = 0.5;
            let ov = visual_space_overlap(&a, &b, ps);
            assert_eq!(ov, 6.0 * ps * ps, "overlap area = 6 cells · pixel_size²");
            // Disjoint masks → zero overlap.
            let c = Array2::from_shape_fn((4, 5), |(r, _)| r >= 2);
            assert_eq!(visual_space_overlap(&a, &c, ps), 0.0);
        }

        /// `derive_visual_grid` is an **OpenISI** choice (regression-lock, NOT an
        /// oracle match): a tight finite-value bounding box with a one-pixel pad.
        /// Allen `getVisualSpace` instead hardcodes `alt ∈ [-40,60], azi ∈
        /// [-20,120]` (see `gen_visualgrid_golden.py`), so the two grids diverge by
        /// design — this pins our adaptive grid, NaN-skipping included.
        #[test]
        fn derive_visual_grid_is_openisi_data_bbox_not_allen_fixed_range() {
            // Finite alt ∈ [0,10], azi ∈ [0,14]; NaN cells ignored.
            let alt = Array2::from_shape_fn((6, 8), |(r, c)| {
                if r == 5 && c == 7 {
                    f64::NAN
                } else if r == 0 && c == 0 {
                    0.0
                } else if r == 4 && c == 4 {
                    10.0
                } else {
                    5.0
                }
            });
            let azi = Array2::from_shape_fn((6, 8), |(r, c)| {
                if r == 5 && c == 7 {
                    f64::NAN
                } else if r == 0 && c == 0 {
                    0.0
                } else if r == 4 && c == 4 {
                    14.0
                } else {
                    7.0
                }
            });
            let g = derive_visual_grid(&alt, &azi, 2.0);
            assert_eq!(g.pixel_size, 2.0);
            assert_eq!(g.alt_min, -2.0, "alt_min = data_min(0) − pad(2)");
            assert_eq!(g.azi_min, -2.0, "azi_min = data_min(0) − pad(2)");
            // h = ceil(((10+2) − (0−2)) / 2) = ceil(14/2) = 7
            assert_eq!(g.h, 7, "alt extent ceil(14/2)");
            // w = ceil(((14+2) − (0−2)) / 2) = ceil(18/2) = 9
            assert_eq!(g.w, 9, "azi extent ceil(18/2)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_passes_through_unchanged() {
        let v = Array2::<f64>::zeros((4, 4));
        let m = PatchRefinementMethod::None;
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let out = m.apply(vec![], &v, &v, &v, &cancel).unwrap();
        assert_eq!(out.len(), 0);
    }
}
