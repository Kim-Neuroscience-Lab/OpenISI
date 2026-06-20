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
        um_per_pixel: f64,
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
        um_per_pixel: f64,
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
            Self::Garrett2014SplitFuse => {
                // Same noise-territory guard as the Allen path: split/fuse is
                // O(N²) per pass and meaningless on hundreds of noise patches.
                const REFINEMENT_MAX_PATCHES: usize = 100;
                if patches.len() > REFINEMENT_MAX_PATCHES {
                    tracing::warn!(
                        patches = patches.len(),
                        threshold = REFINEMENT_MAX_PATCHES,
                        "skipping split/fuse — patch count over threshold \
                         (input VFS is noise-dominated; acquire more cycles for better SNR)",
                    );
                    return Ok(patches);
                }
                let _ = determinant_map; // SNLC computes its own Jacobian internally
                garrett::run_split_fuse(
                    patches,
                    azi_position_deg,
                    alt_position_deg,
                    um_per_pixel,
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
        // Used only by the `oracle_live`-gated live tests after the cutover retired
        // their frozen counterparts (which used these in the default build).
        #[cfg(feature = "oracle_live")]
        use agreement::{Eps, Tol};
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

        // (Cutover, objective 1) The frozen `split2_matches_allen_watershed_branch`
        // golden + its splitpatch_*.bin fixtures + gen_splitpatch_golden.py (a scipy/
        // skimage TRANSCRIPTION of Patch.split2's watershed branch) were DELETED: the
        // live `split2_matches_genuine_nat_live` below drives the GENUINE Patch.split2
        // (the same two-well plateau split; patch count + order-free union) live.

        /// **Live genuine-oracle, CLASS METHOD**: our `split_patch_from_ecc` vs the
        /// GENUINE `Patch.split2` (the watershed branch), constructed and driven in
        /// the bridge as the real method, executed live in the uv-locked env. A
        /// single wide patch over an eccentricity field with two wells forces a
        /// two-way split. The method returns a variable-count patch dict, so we
        /// compare patch COUNT and the order-free UNION of masks. This validates the
        /// orchestration against Allen's actual `split2`, not a transcription of it.
        /// Gated behind `oracle_live`.
        #[cfg(feature = "oracle_live")]
        #[test]
        fn split2_matches_genuine_nat_live() {
            use crate::test_support::oracle;
            const N: usize = 48;
            let mask = Array2::from_shape_fn((N, N), |(r, c)| {
                (12..36).contains(&r) && (6..42).contains(&c)
            });
            // Two wells (minima) separated by a central ridge at col 24.
            let ecc = Array2::from_shape_fn((N, N), |(r, c)| {
                let d1 = ((r as f64 - 24.0).powi(2) + (c as f64 - 14.0).powi(2)).sqrt();
                let d2 = ((r as f64 - 24.0).powi(2) + (c as f64 - 34.0).powi(2)).sqrt();
                d1.min(d2)
            });
            let patch = Patch { mask: mask.clone(), sign: 1 };
            let ours = split_patch_from_ecc(&patch, &ecc, 1.0, 2);

            let mask_f = mask.mapv(|b| if b { 1.0 } else { 0.0 });
            let genuine = oracle::nat_raw(
                "split2",
                &[mask_f, ecc.clone()],
                &[("sign", 1.0), ("cutStep", 1.0), ("borderWidth", 2.0)],
            );
            let genuine_masks: Vec<_> = genuine.iter().map(|o| o.bool()).collect();

            let union = |masks: &dyn Fn(usize, usize, usize) -> bool, n: usize| {
                Array2::from_shape_fn((N, N), |(r, c)| (0..n).any(|k| masks(k, r, c)))
            };
            let our_union = union(&|k, r, c| ours[k].mask[[r, c]], ours.len());
            let gen_union = union(&|k, r, c| genuine_masks[k][[r, c]], genuine_masks.len());

            let mut udiff = 0usize;
            for r in 0..N {
                for c in 0..N {
                    if our_union[[r, c]] != gen_union[[r, c]] {
                        udiff += 1;
                    }
                }
            }
            eprintln!(
                "split2 vs GENUINE NAT method (live): ours={} patches, genuine={} patches, union diff={udiff}",
                ours.len(),
                genuine_masks.len()
            );
            assert_eq!(ours.len(), genuine_masks.len(), "split2 patch count diverges from genuine NAT");
            assert_eq!(udiff, 0, "split2 union diverges from genuine NAT split2");
        }

        // (Cutover, objective 1) The frozen `uniform_filter_matches_scipy_reflect`
        // golden + its uf_*.bin fixtures + gen_uniform_filter_ecc_golden.py were
        // DELETED: the live `uniform_filter_matches_genuine_scipy_live` below was
        // enriched to cover the same cases (size 15 + 10 on 48×48, size 5 on 11×11)
        // against the genuine `scipy.ndimage.uniform_filter(mode='reflect')` live.

        /// **Live library-primitive oracle**: our `uniform_filter_finite` vs the
        /// GENUINE `scipy.ndimage.uniform_filter(mode='reflect')`, executed live in
        /// the uv-locked env. scipy is the oracle; the bridge only calls it. Covers
        /// every case the retired frozen golden held: odd (15) and even (10) windows
        /// on a 48×48 field (even = the asymmetric-window stress), and a small
        /// window (5) on an 11×11 field (window > a fraction of the grid). Gated
        /// behind `oracle_live`.
        #[cfg(feature = "oracle_live")]
        #[test]
        fn uniform_filter_matches_genuine_scipy_live() {
            use crate::test_support::oracle;
            let smooth = |n: usize| {
                Array2::from_shape_fn((n, n), |(r, c)| {
                    (r as f64 * 0.3).sin() + (c as f64 * 0.2).cos() + 0.05 * (r + c) as f64
                })
            };
            let big = smooth(48);
            let small = smooth(11);
            let cases: [(&Array2<f64>, usize); 3] = [(&big, 15), (&big, 10), (&small, 5)];
            let mut maxd = 0.0f64;
            for (a, size) in cases {
                let n = a.dim().0;
                let genuine = oracle::nat("scipy_uniform_filter", &[a.clone()], &[("size", size as f64)])
                    .remove(0);
                let ours = uniform_filter_finite(a, size as i32);
                let mut d = 0.0f64;
                for r in 0..n {
                    for c in 0..n {
                        d = d.max((ours[[r, c]] - genuine[[r, c]]).abs());
                    }
                }
                eprintln!("  uniform_filter {n}x{n} size={size} vs GENUINE scipy (live): max diff = {d:.2e}");
                maxd = maxd.max(d);
            }
            assert!(maxd < 1e-9, "uniform_filter_finite diverges from genuine scipy uniform_filter");
        }

        /// **Live library-primitive oracle**: our `watershed_from_markers` vs the
        /// GENUINE `skimage.segmentation.watershed` (`connectivity=ones((3,3))`,
        /// `watershed_line=False`) — the exact call Allen `Patch.split2` makes —
        /// executed live in the uv-locked env. The explicit marker labels carry
        /// through both implementations, so the labelings are directly comparable.
        /// Colliding basins on a flat plateau force the watershed line. Gated
        /// behind `oracle_live`.
        #[cfg(feature = "oracle_live")]
        #[test]
        fn watershed_from_markers_matches_genuine_skimage_live() {
            use crate::test_support::oracle;
            const N: usize = 24;
            // Two wells on a flat-ish plateau; markers seed each well; mask = the
            // whole interior with a couple of holes. (A clean colliding-basins
            // scene with no watershed-line tie ambiguity — the border-touching-
            // marker / thin-isthmus edge cases sit on plateaus where ours and
            // skimage legitimately differ on boundary placement, so they stay in
            // the frozen `watershed_from_markers_matches_skimage` golden, which is
            // a LIBRARY-PRIMITIVE pin against skimage, not a transcription.)
            let elevation = Array2::from_shape_fn((N, N), |(r, c)| {
                let d1 = (((r as f64 - 6.0).powi(2) + (c as f64 - 6.0).powi(2)) as f64).sqrt();
                let d2 = (((r as f64 - 17.0).powi(2) + (c as f64 - 17.0).powi(2)) as f64).sqrt();
                d1.min(d2)
            });
            let mut markers = Array2::<i32>::zeros((N, N));
            markers[[6, 6]] = 1;
            markers[[17, 17]] = 2;
            let mask = Array2::from_shape_fn((N, N), |(r, c)| {
                let edge = r == 0 || c == 0 || r == N - 1 || c == N - 1;
                let hole = (r, c) == (3, 20) || (r, c) == (20, 3);
                !edge && !hole
            });

            let elev_f = elevation.clone();
            let mark_f = markers.mapv(|v| v as f64);
            let mask_f = mask.mapv(|b| if b { 1.0 } else { 0.0 });
            let genuine = oracle::nat_raw("skimage_watershed", &[elev_f, mark_f, mask_f], &[])
                .remove(0)
                .i32();
            let ours = watershed_from_markers(&elevation, &markers, &mask);

            let mut diff = 0usize;
            for r in 0..N {
                for c in 0..N {
                    if ours[[r, c]] != genuine[[r, c]] {
                        diff += 1;
                    }
                }
            }
            eprintln!("watershed vs GENUINE skimage (live): differing px = {diff}");
            assert_eq!(diff, 0, "watershed_from_markers diverges from genuine skimage watershed");
        }

        // (Cutover, objective 1) The frozen `sigma_area_matches_allen_get_sigma_area`
        // golden + its sigarea_*.bin fixtures + gen_sigmaarea_golden.py were DELETED.
        // gen_sigmaarea_golden.py was a VERBATIM transcription of getSigmaArea
        // (np.sum(mask*detMap)). The live `sigma_area_matches_genuine_nat_live` above
        // was enriched to cover the same five cases (finite, NaN-in, NaN-out,
        // multi-component, negative/zero determinant) against the genuine NAT
        // `Patch.getSigmaArea` in the shim-free uv env.

        /// **Live genuine-oracle version**: our `sigma_area` vs the GENUINE
        /// `Patch.getSigmaArea` (`sum(array * detMap)`), covering every case the
        /// retired frozen golden held: finite, a NaN INSIDE the mask, a NaN OUTSIDE
        /// the mask (genuine numpy `0.0 * NaN = NaN` → whole sum NaN — the real
        /// NaN-handling surface), a MULTI-component mask, and a negative/zero
        /// determinant. Gated `oracle_live`.
        #[cfg(feature = "oracle_live")]
        #[test]
        fn sigma_area_matches_genuine_nat_live() {
            use crate::test_support::oracle;
            const H: usize = 24;
            const W: usize = 32;
            let one_sq = Array2::from_shape_fn((H, W), |(r, c)| (4..12).contains(&r) && (4..12).contains(&c));
            // Two disjoint components.
            let two_sq = Array2::from_shape_fn((H, W), |(r, c)| {
                ((4..10).contains(&r) && (4..10).contains(&c)) || ((14..20).contains(&r) && (20..28).contains(&c))
            });
            let base = Array2::from_shape_fn((H, W), |(r, c)| (r as f64 - c as f64) * 0.05 + 1.0);
            let mut nanin = base.clone();
            nanin[[6, 6]] = f64::NAN; // NaN inside the mask
            let mut nanout = base.clone();
            nanout[[20, 28]] = f64::NAN; // NaN outside the mask
            // Negative determinant + exact zeros.
            let negzero = Array2::from_shape_fn((H, W), |(r, c)| {
                if (r + c) % 3 == 0 { 0.0 } else { (c as f64 - r as f64) * 0.1 - 0.5 }
            });
            let cases: [(&str, &Array2<bool>, &Array2<f64>); 5] = [
                ("finite", &one_sq, &base),
                ("nanin", &one_sq, &nanin),
                ("nanout", &one_sq, &nanout),
                ("multicomp", &two_sq, &base),
                ("negzero", &one_sq, &negzero),
            ];
            let eq = |a: f64, b: f64| (a.is_nan() && b.is_nan()) || (a - b).abs() <= 1e-9 * (1.0 + b.abs());

            let mut diffs = Vec::new();
            for (name, mask, det) in cases {
                let mask_f = mask.mapv(|b| if b { 1.0 } else { 0.0 });
                let g = oracle::nat_raw("getSigmaArea", &[mask_f, (*det).clone()], &[])
                    .remove(0)
                    .f64()[[0, 0]];
                let o = sigma_area(mask, det);
                eprintln!("  sigma_area {name}: ours={o} genuine={g}");
                if !eq(o, g) {
                    diffs.push(format!("{name}: ours={o} genuine={g}"));
                }
            }
            assert!(diffs.is_empty(), "sigma_area diverges from genuine NAT getSigmaArea: {diffs:?}");
        }

        // (Cutover, objective 1) The frozen `eccentricity_full_image_and_center_match_
        // allen` golden + its eccfull_*.bin fixtures + gen_eccfull_golden.py (a
        // transcription of eccentricityMap + getPixelVisualCenter) were DELETED: the
        // live `eccentricity_full_image_and_center_match_genuine_nat_live` above drives
        // the GENUINE Patch.getPixelVisualCenter + eccentricityMap live (incl. the
        // alt==0 / azi==0 per-coordinate != 0 exclusion edge).

        /// **Live genuine-oracle, CLASS METHOD**: our `patch_visual_center` vs the
        /// GENUINE `Patch.getPixelVisualCenter` (per-coordinate `mean` over patch
        /// pixels where `mask·map != 0`), and our `eccentricity_full_image` vs the
        /// GENUINE `eccentricityMap` evaluated at that genuine centre. Stress: a
        /// patch pixel with `alt==0` and one with `azi==0` (excluded from their
        /// respective coordinate mean by the `!= 0` rule). Gated `oracle_live`.
        #[cfg(feature = "oracle_live")]
        #[test]
        fn eccentricity_full_image_and_center_match_genuine_nat_live() {
            use crate::test_support::oracle;
            const N: usize = 24;
            let mask = Array2::from_shape_fn((N, N), |(r, c)| (6..16).contains(&r) && (8..18).contains(&c));
            // Finite alt/azi degree ramps, with one patch pixel forced to alt==0
            // and one to azi==0 (the != 0 exclusion edge).
            let mut alt = Array2::from_shape_fn((N, N), |(r, _)| (r as f64 - 12.0) * 3.0 + 1.0);
            let mut azi = Array2::from_shape_fn((N, N), |(_, c)| (c as f64 - 12.0) * 2.5 + 0.7);
            alt[[8, 10]] = 0.0; // excluded from meanAlt
            azi[[12, 14]] = 0.0; // excluded from meanAzi
            let mask_f = mask.mapv(|b| if b { 1.0 } else { 0.0 });

            // Genuine centre.
            let c = oracle::nat_raw("getPixelVisualCenter", &[mask_f, alt.clone(), azi.clone()], &[]).remove(0).f64();
            let (g_alt_c, g_azi_c) = (c[[0, 0]], c[[0, 1]]);
            let (alt_c, azi_c) = patch_visual_center(&mask, &azi, &alt);
            Tol::abs(128, Eps::F64).assert(
                "patch_visual_center vs GENUINE getPixelVisualCenter (live)",
                &[alt_c, azi_c],
                &[g_alt_c, g_azi_c],
            );

            // Genuine eccentricity at the genuine centre vs ours.
            let genuine_ecc = oracle::nat(
                "eccentricityMap",
                &[alt.clone(), azi.clone()],
                &[("altCenter", g_alt_c), ("aziCenter", g_azi_c)],
            )
            .remove(0);
            let ours_ecc = eccentricity_full_image(&azi, &alt, g_alt_c, g_azi_c);
            Tol::abs(128, Eps::F64).assert(
                "eccentricity_full_image vs GENUINE eccentricityMap (live)",
                ours_ecc.as_slice().expect("contiguous"),
                genuine_ecc.as_slice().expect("contiguous"),
            );
            eprintln!("eccfull + center vs GENUINE NAT (live): matched");
        }

        // (Cutover, objective 1) The frozen `local_min_matches_allen_localmin`
        // golden + its lmin_*.bin fixtures + gen_local_min_golden.py (which
        // imported the `_allen_oracle` SHIM) were DELETED: the live
        // `local_min_matches_genuine_nat_live` below was enriched to exercise the
        // same branches (two-basin ≥2-CC stop + label numbering, single-min
        // exhaustion, border minimum, NaN background) against the genuine NAT
        // `localMin` in the shim-free uv env.

        /// **Live genuine-oracle version**: our `local_min_markers` vs the GENUINE
        /// NeuroAnalysisTools `localMin`, on several ecc maps built in Rust that
        /// exercise every branch the retired frozen golden held: two basins (the
        /// ≥2-CC stop + label numbering), a single basin (the exhaustion branch),
        /// a border minimum, and a NaN background (NaN handling). Integer marker
        /// maps compared exactly. Gated behind `oracle_live`.
        #[cfg(feature = "oracle_live")]
        #[test]
        fn local_min_matches_genuine_nat_live() {
            use crate::test_support::oracle;
            const N: usize = 32;
            let well = |r: usize, c: usize, cr: f64, cc: f64| {
                let (dr, dc) = (r as f64 - cr, c as f64 - cc);
                (dr * dr + dc * dc).sqrt()
            };
            let two_basins =
                Array2::from_shape_fn((N, N), |(r, c)| 0.1 * well(r, c, 8.0, 8.0).min(well(r, c, 24.0, 24.0)));
            let single_min = Array2::from_shape_fn((N, N), |(r, c)| 0.1 * well(r, c, 16.0, 16.0));
            // A minimum hard against the top-left border.
            let border_min =
                Array2::from_shape_fn((N, N), |(r, c)| 0.1 * well(r, c, 0.0, 0.0).min(well(r, c, 24.0, 24.0)));
            // NaN background outside a central disk (localMin must treat NaN as
            // non-minimum) + two interior wells.
            let nan_bg = Array2::from_shape_fn((N, N), |(r, c)| {
                if well(r, c, 16.0, 16.0) > 13.0 {
                    f64::NAN
                } else {
                    0.1 * well(r, c, 11.0, 11.0).min(well(r, c, 21.0, 21.0))
                }
            });
            let scenes = [
                ("two_basins", two_basins),
                ("single_min", single_min),
                ("border_min", border_min),
                ("nan_bg", nan_bg),
            ];
            let mut total = 0usize;
            for (name, ecc) in &scenes {
                let genuine = oracle::nat_raw("localMin", &[ecc.clone()], &[("binSize", 0.5)])
                    .remove(0)
                    .i32();
                let ours = local_min_markers(ecc, 0.5);
                let d = ndarray::Zip::from(&ours)
                    .and(&genuine)
                    .fold(0usize, |a, &o, &g| a + (o != g) as usize);
                eprintln!("  local_min {name:11} vs GENUINE NAT (live): differing = {d}");
                total += d;
            }
            assert_eq!(total, 0, "local_min_markers diverges from genuine NAT localMin");
        }

        // (Cutover, objective 1) The frozen `merge_two_matches_allen_mergepatches`
        // golden + its mt_*.bin fixtures + gen_merge_two_golden.py (which imported
        // the `_allen_oracle` SHIM) were DELETED: the live
        // `merge_two_matches_genuine_nat_live` below was enriched to cover the same
        // cases (edge-touch, 1px gap, gap==borderWidth boundary, diagonal-only
        // corner, too-far→raise) against genuine NAT `mergePatches` in the
        // shim-free uv env — verifying both the mergeable decision and the merged
        // map.

        /// **Live genuine-oracle version**: our `merge_two` vs the GENUINE
        /// NeuroAnalysisTools `mergePatches` (which RAISES when the two patches
        /// are too far apart → our `None`). Built-in-Rust mergeable + far cases.
        /// Gated behind `oracle_live`.
        #[cfg(feature = "oracle_live")]
        #[test]
        fn merge_two_matches_genuine_nat_live() {
            use crate::test_support::oracle;
            const N: usize = 32;
            let sq = |r0: usize, r1: usize, c0: usize, c1: usize| {
                let mut a = Array2::<f64>::zeros((N, N));
                for r in r0..r1 {
                    for c in c0..c1 {
                        a[[r, c]] = 1.0;
                    }
                }
                a
            };
            // Covers the cases the retired frozen golden held: edge-touch (merge),
            // a 1px gap (closing bridges), a gap equal to the border width
            // (boundary), diagonal-only corner contact (4-conn cross, not 8-conn),
            // and too-far (genuine RAISES → our None).
            let cases = [
                ("touch", sq(8, 16, 6, 12), sq(8, 16, 12, 18)),     // shared edge
                ("gap1", sq(8, 16, 6, 12), sq(8, 16, 13, 19)),      // 1px gap → closing(2) bridges
                ("gap_eq_bw", sq(8, 16, 6, 12), sq(8, 16, 14, 20)), // 2px gap == borderWidth
                ("diag", sq(6, 10, 6, 10), sq(10, 14, 10, 14)),     // corner-only contact
                ("far", sq(8, 16, 4, 9), sq(8, 16, 22, 27)),        // far → 2 CC → raises
            ];
            let mut diffs = Vec::new();
            for (name, a_f, b_f) in &cases {
                let (a, b) = (a_f.mapv(|v| v != 0.0), b_f.mapv(|v| v != 0.0));
                let outs = oracle::nat_raw("mergePatches", &[a_f.clone(), b_f.clone()], &[("borderWidth", 2.0)]);
                let g_mergeable = outs[1].bool()[[0, 0]];
                let g_spc = outs[0].bool();
                let ours = merge_two(&a, &b, 2);
                if ours.is_some() != g_mergeable {
                    diffs.push(format!("{name}: mergeable ours={} genuine={g_mergeable}", ours.is_some()));
                    continue;
                }
                if let Some(m) = ours {
                    let d = ndarray::Zip::from(&m).and(&g_spc).fold(0usize, |acc, &o, &gg| acc + (o != gg) as usize);
                    if d != 0 {
                        diffs.push(format!("{name}: merged map differs by {d} px"));
                    }
                }
                eprintln!("  merge_two {name}: mergeable ours={} genuine={g_mergeable}", g_mergeable);
            }
            assert!(diffs.is_empty(), "merge_two diverges from genuine NAT mergePatches: {diffs:?}");
        }

        /// **Live genuine-oracle, CLASS METHOD**: our `patch_visual_space` vs the
        /// GENUINE `Patch.getVisualSpace`, covering every case the retired frozen
        /// golden held: the scatter-into-grid (floor division) + `binary_closing`
        /// (cross SE) across closeIter 0/1/2/3, **uniqueArea** (`count·pixelSize²`),
        /// out-of-range gating (pixels outside the hardcoded alt [-40,60)/azi
        /// [-20,120) ranges are dropped), and NaN-skip. The `VisualGrid` is built to
        /// NAT's hardcoded ranges (our `derive_visual_grid` bounding box is an
        /// OpenISI choice, not the oracle). Gated behind `oracle_live`.
        #[cfg(feature = "oracle_live")]
        #[test]
        fn patch_visual_space_matches_genuine_nat_live() {
            use crate::test_support::oracle;
            const MH: usize = 40;
            const MW: usize = 40;
            let lin = |i: usize, n: usize, lo: f64, hi: f64| lo + (hi - lo) * i as f64 / (n - 1) as f64;
            let mask = Array2::from_shape_fn((MH, MW), |(r, c)| (8..28).contains(&r) && (6..30).contains(&c));
            let mask_f = mask.mapv(|b| if b { 1.0 } else { 0.0 });
            // In-range smooth maps.
            let alt = Array2::from_shape_fn((MH, MW), |(r, _)| lin(r, MH, -30.0, 50.0));
            let azi = Array2::from_shape_fn((MH, MW), |(_, c)| lin(c, MW, -10.0, 110.0));
            // Push some patch pixels OUT of the hardcoded ranges (gating) and inject
            // a NaN inside the patch (skip).
            let mut alt_edge = alt.clone();
            let mut azi_edge = azi.clone();
            alt_edge[[8, 6]] = 200.0; // > 60 → gated out
            azi_edge[[27, 29]] = -100.0; // < -20 → gated out
            alt_edge[[15, 15]] = f64::NAN; // NaN → skipped
            azi_edge[[20, 20]] = f64::NAN;

            let grid = VisualGrid { alt_min: -40.0, azi_min: -20.0, pixel_size: 1.0, h: 100, w: 140 };
            let scenes: [(&str, &Array2<f64>, &Array2<f64>); 2] =
                [("inrange", &alt, &azi), ("edge_nan", &alt_edge, &azi_edge)];
            let mut fails = Vec::new();
            for (name, a, z) in scenes {
                for close in [0.0_f64, 1.0, 2.0, 3.0] {
                    let g = oracle::nat_raw(
                        "getVisualSpace",
                        &[mask_f.clone(), a.clone(), z.clone()],
                        &[("pixelSize", 1.0), ("closeIter", close)],
                    );
                    let g_vs = g[0].bool();
                    let g_area = g[1].f64()[[0, 0]];
                    let (ours, our_area) = patch_visual_space(&mask, z, a, &grid, close as i32);
                    let d = ndarray::Zip::from(&ours).and(&g_vs).fold(0usize, |acc, &o, &gg| acc + (o != gg) as usize);
                    eprintln!("  patchvs {name} close={close}: vs_diff={d} area ours={our_area} genuine={g_area}");
                    if ours.dim() != g_vs.dim() || d != 0 {
                        fails.push(format!("{name} close={close}: vs_diff={d}"));
                    }
                    if (our_area - g_area).abs() > 1e-9 {
                        fails.push(format!("{name} close={close}: area {our_area} != {g_area}"));
                    }
                }
            }
            assert!(fails.is_empty(), "patch_visual_space diverges from genuine NAT getVisualSpace: {fails:?}");
        }

        // (Cutover, objective 1) The frozen `patch_visual_space_matches_allen_
        // get_visual_space` golden + its patchvs_*.bin fixtures + gen_patchvs_golden.py
        // (a transcription) were DELETED: the live
        // `patch_visual_space_matches_genuine_nat_live` above was enriched to cover the
        // same cases — scatter-into-grid + binary_closing across closeIter 0/1/2/3,
        // uniqueArea, out-of-range gating, and NaN-skip — against the genuine NAT
        // `Patch.getVisualSpace` live (the bridge now also returns uniqueArea).

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

// =============================================================================
// SNLC / Garrett 2014 split/fuse implementation (2nd refinement lineage)
// =============================================================================
//
// Parallel to `mod allen`. Built bedrock-up from the shared visual-space
// coverage primitive `overRep` (the SNLC analog of Allen's `patch_visual_space`),
// each step validated atomistically against the verbatim oracle under Octave.

// Shared, golden-validated MATLAB-op host helpers (used by the SNLC split/fuse
// refinement here AND by the pre-combine `direction_smoothing` adaptiveSmoother):
// `fspecial('gaussian', …)` and zero-padded `filter2(…, 'same')`.
pub(crate) use garrett::{filter2_same, fspecial_gaussian};

mod garrett {
    use std::sync::atomic::{AtomicBool, Ordering};

    use ndarray::Array2;
    use rustfft::num_complex::Complex64;
    use rustfft::FftPlanner;

    use crate::math::eccentricity_pixel_deg;
    use crate::segmentation::connectivity::{label_4conn, patches_from_labels_majority_sign};
    use crate::segmentation::morphology::{
        binary_closing_cross, binary_closing_disk, binary_dilation_cross, binary_dilation_disk,
        binary_erosion_cross, binary_fill_holes, binary_opening_cross, binary_opening_disk,
        disk_offsets,
    };
    use crate::segmentation::Patch;
    use crate::AnalysisError;

    /// MATLAB `sign`: `sign(0) == 0` — unlike Rust's `f64::signum`, which is ±1.
    /// The `overRep` field-sign restriction depends on this exact behavior.
    fn msign(x: f64) -> f64 {
        if x > 0.0 {
            1.0
        } else if x < 0.0 {
            -1.0
        } else {
            0.0
        }
    }

    /// Output of [`over_rep`] — the SNLC `overRep` (`splitPatchesX.m:188-215`).
    pub(super) struct Coverage {
        /// Sphere-domain (visual-field) binary coverage grid.
        pub sp_cov: Array2<bool>,
        /// `sum(|Jac|)/ (pixpermm·U)²` over the dominant-sign pixels (deg²).
        pub jac_coverage: f64,
        /// `count(sp_cov)` — unique covered visual area (deg²).
        pub actual_coverage: f64,
        /// `actual_coverage / kept_pixels` — overRep's `MagFac`; computed for a
        /// complete port, not consumed by the split/fuse criteria.
        #[allow(dead_code)]
        pub mag_fac: f64,
    }

    /// SNLC `overRep`: project a cortical patch into the sphere-domain visual
    /// grid and measure its *unique* covered area. Restricts to the patch's
    /// dominant field sign (a single visual area has one sign), bins each kept
    /// pixel's `(azimuth, altitude)` position into the grid, then
    /// `imfill → imclose(disk-1≡cross) → imfill`.
    ///
    /// Verbatim port of the `overRep` subfunction; golden-validated against
    /// `gen_overrep_golden.m`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn over_rep(
        kmap_hor: &Array2<f64>,  // azimuth (deg)
        kmap_vert: &Array2<f64>, // altitude (deg)
        u: f64,
        jac: &Array2<f64>,
        patch: &Array2<bool>,
        sph_min: f64, // sphdom(1), e.g. -90
        nsph: usize,  // length(sphdom), e.g. 181
        pixpermm: f64,
    ) -> Coverage {
        let ppm = pixpermm * u;
        let (h, w) = patch.dim();

        // posneg = sign(mean(Jac over patch))
        let (mut s, mut n) = (0.0f64, 0usize);
        for r in 0..h {
            for c in 0..w {
                if patch[[r, c]] {
                    s += jac[[r, c]];
                    n += 1;
                }
            }
        }
        let posneg = if n > 0 { msign(s / n as f64) } else { 0.0 };

        // Keep pixels with sign(Jac)==posneg AND Jac!=0; project each into the grid.
        // MATLAB linear index N·(sphlocX−1)+sphlocY is column-major → element
        // (row = altitude bin, col = azimuth bin).
        let mut sp_cov = Array2::<bool>::from_elem((nsph, nsph), false);
        let mut jac_sum = 0.0f64;
        let mut kept = 0usize;
        for r in 0..h {
            for c in 0..w {
                if !patch[[r, c]] {
                    continue;
                }
                let jv = jac[[r, c]];
                if msign(jv) != posneg || jv == 0.0 {
                    continue;
                }
                jac_sum += jv.abs();
                kept += 1;
                let col = (kmap_hor[[r, c]].round() - sph_min) as isize; // azimuth
                let row = (kmap_vert[[r, c]].round() - sph_min) as isize; // altitude
                if row >= 0 && col >= 0 && (row as usize) < nsph && (col as usize) < nsph {
                    sp_cov[[row as usize, col as usize]] = true;
                }
            }
        }
        let jac_coverage = jac_sum / (ppm * ppm);

        sp_cov = binary_fill_holes(&sp_cov);
        sp_cov = binary_closing_cross(&sp_cov, 1);
        sp_cov = binary_fill_holes(&sp_cov);

        let actual = sp_cov.iter().filter(|&&b| b).count();
        let mag_fac = if kept > 0 {
            actual as f64 / kept as f64
        } else {
            f64::NAN
        };

        Coverage {
            sp_cov,
            jac_coverage,
            actual_coverage: actual as f64,
            mag_fac,
        }
    }

    /// SNLC `getPatchCoM` — per-patch center of mass in **1-based `(x=col,
    /// y=row)`** pixel coordinates. (Only `CoMxy` is ported; the oracle's
    /// `Axisxy` principal axis is unused by the split/fuse path.)
    ///
    /// Verbatim port of `getPatchCoM.m`, including the curved-patch correction:
    /// if the centroid pixel is not on the patch, snap to the patch pixel
    /// nearest the centroid (MATLAB `find` → first in column-major order).
    /// Golden-validated against the real oracle via `gen_patchcom_golden.m`.
    pub(super) fn patch_com(im: &Array2<bool>) -> Vec<(f64, f64)> {
        let (labels, n_lab) = label_4conn(im);
        let (h, w) = im.dim();
        let mut out = Vec::with_capacity(n_lab);
        for lab in 1..=n_lab as i32 {
            // Centroid (1-based): mean column (x) and row (y) over patch pixels.
            let (mut sx, mut sy, mut n) = (0.0f64, 0.0f64, 0.0f64);
            for r in 0..h {
                for c in 0..w {
                    if labels[[r, c]] == lab {
                        sx += c as f64 + 1.0;
                        sy += r as f64 + 1.0;
                        n += 1.0;
                    }
                }
            }
            let (cx, cy) = (sx / n, sy / n);

            // Correction: is the rounded centroid pixel on this patch?
            let (rx, ry) = (cx.round() as isize, cy.round() as isize);
            let on_patch = rx >= 1
                && ry >= 1
                && rx <= w as isize
                && ry <= h as isize
                && labels[[(ry - 1) as usize, (rx - 1) as usize]] == lab;
            if on_patch {
                out.push((cx, cy));
                continue;
            }
            // Nearest patch pixel to the centroid; MATLAB `find(rdom==min)`
            // matches the first equal-distance pixel in column-major order.
            let dist = |r: usize, c: usize| {
                let dx = (c as f64 + 1.0) - cx;
                let dy = (r as f64 + 1.0) - cy;
                (dx * dx + dy * dy).sqrt()
            };
            let mut minval = f64::INFINITY;
            for r in 0..h {
                for c in 0..w {
                    if labels[[r, c]] == lab {
                        minval = minval.min(dist(r, c));
                    }
                }
            }
            let mut snapped = (cx, cy);
            'col: for c in 0..w {
                for r in 0..h {
                    if dist(r, c) == minval {
                        snapped = ((c + 1) as f64, (r + 1) as f64);
                        break 'col;
                    }
                }
            }
            out.push(snapped);
        }
        out
    }

    /// 3×3 binary median (zero-padded borders) — `medfilt2(BW,[3 3])` on a
    /// binary image: the window median of 9 values is `1` iff ≥5 are set.
    fn median_3x3(mask: &Array2<bool>) -> Array2<bool> {
        let (h, w) = mask.dim();
        Array2::from_shape_fn((h, w), |(r, c)| {
            let mut cnt = 0;
            for dr in -1..=1i32 {
                for dc in -1..=1i32 {
                    let rr = r as i32 + dr;
                    let cc = c as i32 + dc;
                    if rr >= 0
                        && rr < h as i32
                        && cc >= 0
                        && cc < w as i32
                        && mask[[rr as usize, cc as usize]]
                    {
                        cnt += 1;
                    }
                }
            }
            cnt >= 5
        })
    }

    /// SNLC `getCenterPatch` (`splitPatchesX.m` subfunction): the patch region
    /// within `r_lim` degrees of the visual-field center, cleaned by a disk-2
    /// opening + 3×3 median. Verbatim port; golden-validated as a unit (the
    /// `imopen`/`medfilt2` run as the real Octave ops in the fixture).
    pub(super) fn get_center_patch(
        kmap_rad: &Array2<f64>,
        im: &Array2<bool>,
        r_lim: f64,
    ) -> Array2<bool> {
        let (h, w) = im.dim();
        // centerPatch = (kmap_rad < R) .* im
        let cp = Array2::from_shape_fn((h, w), |(r, c)| kmap_rad[[r, c]] < r_lim && im[[r, c]]);
        let cp = binary_opening_disk(&cp, 2);
        median_3x3(&cp)
    }

    /// `bwdist`: exact Euclidean distance from each pixel to the nearest `true`
    /// (seed) pixel; all-`INFINITY` when there are no seeds (matching MATLAB
    /// `bwdist` of an empty mask). `resetPatch` builds its watershed elevation
    /// from this. Brute-force exact — patches are bounded and the refinement
    /// caps at 100 patches; swap for a separable EDT only if it profiles hot.
    pub(super) fn bwdist(seeds: &Array2<bool>) -> Array2<f64> {
        let (h, w) = seeds.dim();
        let pts: Vec<(i64, i64)> = seeds
            .indexed_iter()
            .filter_map(|((r, c), &b)| b.then_some((r as i64, c as i64)))
            .collect();
        Array2::from_shape_fn((h, w), |(r, c)| {
            let mut best = f64::INFINITY;
            for &(sr, sc) in &pts {
                let dr = (r as i64 - sr) as f64;
                let dc = (c as i64 - sc) as f64;
                let d2 = dr * dr + dc * dc;
                if d2 < best {
                    best = d2;
                }
            }
            best.sqrt()
        })
    }

    // -------------------------------------------------------------------------
    // Octave `watershed(·, 4)` — Meyer flooding, 4-connected, watershed-line=0.
    // A faithful port of Octave image's `watershed.cc` (a DIFFERENT oracle from
    // the Allen lineage's skimage 8-connected `watershed_from_markers`). Used by
    // `resetPatch`. Golden-validated against Octave's own `watershed.cc-tst`
    // vectors by `watershed_octave4_matches_octave`.
    // -------------------------------------------------------------------------

    /// 4-connected neighbour offsets (N/S/W/E). (The SNLC `watershed(·,4)`.)
    const N4: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

    /// 8-connected neighbour offsets — the Octave `watershed`/`imregionalmin`
    /// DEFAULT (used by `getNlocalmin`: `imregionalmin(rad,8)` + `watershed(rad2)`).
    #[rustfmt::skip]
    const N8: [(i32, i32); 8] = [
        (-1, -1), (-1, 0), (-1, 1),
        (0, -1),           (0, 1),
        (1, -1),  (1, 0),  (1, 1),
    ];

    fn imregionalmin8(im: &Array2<f64>) -> Array2<bool> {
        imregionalmin(im, &N8)
    }

    /// `imregionalmin(im, conn)`: regional minima — maximal flat zones (equal
    /// value, `conn`-connected) none of whose pixels has a strictly-lower neighbour.
    fn imregionalmin(im: &Array2<f64>, offs: &[(i32, i32)]) -> Array2<bool> {
        let (h, w) = im.dim();
        let in_bounds = |r: i32, c: i32| r >= 0 && r < h as i32 && c >= 0 && c < w as i32;
        // lower[p] = some neighbour is strictly less than im[p].
        let lower = Array2::from_shape_fn((h, w), |(r, c)| {
            let v = im[[r, c]];
            offs.iter().any(|&(dr, dc)| {
                let (nr, nc) = (r as i32 + dr, c as i32 + dc);
                in_bounds(nr, nc) && im[[nr as usize, nc as usize]] < v
            })
        });
        // Flat-zone flood; a zone is a regional min iff no member has a lower nbr.
        let mut is_min = Array2::<bool>::from_elem((h, w), false);
        let mut seen = Array2::<bool>::from_elem((h, w), false);
        for r0 in 0..h {
            for c0 in 0..w {
                if seen[[r0, c0]] {
                    continue;
                }
                let v = im[[r0, c0]];
                let mut stack = vec![(r0, c0)];
                let mut zone = Vec::new();
                let mut zone_has_lower = false;
                seen[[r0, c0]] = true;
                while let Some((r, c)) = stack.pop() {
                    zone.push((r, c));
                    zone_has_lower |= lower[[r, c]];
                    for &(dr, dc) in offs {
                        let (nr, nc) = (r as i32 + dr, c as i32 + dc);
                        if in_bounds(nr, nc) {
                            let (nu, cu) = (nr as usize, nc as usize);
                            if !seen[[nu, cu]] && im[[nu, cu]] == v {
                                seen[[nu, cu]] = true;
                                stack.push((nu, cu));
                            }
                        }
                    }
                }
                if !zone_has_lower {
                    for (r, c) in zone {
                        is_min[[r, c]] = true;
                    }
                }
            }
        }
        is_min
    }

    fn label_colmajor4(mask: &Array2<bool>) -> Array2<i32> {
        label_colmajor(mask, &N4)
    }

    /// `conn`-connected labelling in **column-major** first-encounter order
    /// (matching Octave `bwlabeln`, so the watershed's label numbers match it).
    fn label_colmajor(mask: &Array2<bool>, offs: &[(i32, i32)]) -> Array2<i32> {
        let (h, w) = mask.dim();
        let in_bounds = |r: i32, c: i32| r >= 0 && r < h as i32 && c >= 0 && c < w as i32;
        let mut lab = Array2::<i32>::zeros((h, w));
        let mut next = 0i32;
        for c in 0..w {
            for r in 0..h {
                if mask[[r, c]] && lab[[r, c]] == 0 {
                    next += 1;
                    lab[[r, c]] = next;
                    let mut stack = vec![(r, c)];
                    while let Some((rr, cc)) = stack.pop() {
                        for &(dr, dc) in offs {
                            let (nr, nc) = (rr as i32 + dr, cc as i32 + dc);
                            if in_bounds(nr, nc) {
                                let (nu, cu) = (nr as usize, nc as usize);
                                if mask[[nu, cu]] && lab[[nu, cu]] == 0 {
                                    lab[[nu, cu]] = next;
                                    stack.push((nu, cu));
                                }
                            }
                        }
                    }
                }
            }
        }
        lab
    }

    /// A queued voxel. The priority queue pops the lowest `val`, ties broken by
    /// lowest `pos` (insertion order = FIFO), exactly as Octave's `Voxel`.
    struct Vox {
        val: f64,
        pos: u64,
        r: usize,
        c: usize,
    }
    impl PartialEq for Vox {
        fn eq(&self, o: &Self) -> bool {
            self.cmp(o) == std::cmp::Ordering::Equal
        }
    }
    impl Eq for Vox {}
    impl PartialOrd for Vox {
        fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(o))
        }
    }
    impl Ord for Vox {
        fn cmp(&self, o: &Self) -> std::cmp::Ordering {
            // BinaryHeap is a max-heap; invert so the *smallest* (val, pos) is on
            // top: self is "greater" when it has the smaller val (then pos).
            o.val
                .total_cmp(&self.val)
                .then_with(|| o.pos.cmp(&self.pos))
        }
    }

    pub(super) fn watershed_octave4(im: &Array2<f64>) -> Array2<i32> {
        watershed_octave(im, &N4)
    }
    pub(super) fn watershed_octave8(im: &Array2<f64>) -> Array2<i32> {
        watershed_octave(im, &N8)
    }

    /// `watershed(im, conn)` — Octave's Meyer flooding. Returns labels with
    /// watershed-line pixels at 0. Verbatim port of `watershed.cc`.
    fn watershed_octave(im: &Array2<f64>, offs: &[(i32, i32)]) -> Array2<i32> {
        use std::collections::BinaryHeap;
        let (h, w) = im.dim();
        let in_bounds = |r: i32, c: i32| r >= 0 && r < h as i32 && c >= 0 && c < w as i32;

        let markers = imregionalmin(im, offs);
        let mut label = label_colmajor(&markers, offs);
        let mut label_flag = markers.clone(); // labelled (minima start labelled)
        let mut queue_flag = markers.clone(); // queued/processed (minima start true)
        let mut pos: u64 = 0;
        let mut q: BinaryHeap<Vox> = BinaryHeap::new();

        // Seed: push the unqueued neighbours of every labelled (minima) pixel.
        for r in 0..h {
            for c in 0..w {
                if label_flag[[r, c]] {
                    for &(dr, dc) in offs {
                        let (nr, nc) = (r as i32 + dr, c as i32 + dc);
                        if in_bounds(nr, nc) {
                            let (nu, cu) = (nr as usize, nc as usize);
                            if !queue_flag[[nu, cu]] {
                                queue_flag[[nu, cu]] = true;
                                q.push(Vox { val: im[[nu, cu]], pos, r: nu, c: cu });
                                pos += 1;
                            }
                        }
                    }
                }
            }
        }

        while let Some(v) = q.pop() {
            let mut common: Option<i32> = None;
            let mut all_equal = true;
            let mut ic: Vec<(usize, usize)> = Vec::new();
            for &(dr, dc) in offs {
                let (nr, nc) = (v.r as i32 + dr, v.c as i32 + dc);
                if !in_bounds(nr, nc) {
                    continue;
                }
                let (nu, cu) = (nr as usize, nc as usize);
                if label_flag[[nu, cu]] {
                    let l = label[[nu, cu]];
                    match common {
                        None => common = Some(l),
                        Some(cl) if cl != l => all_equal = false,
                        _ => {}
                    }
                } else if !queue_flag[[nu, cu]] {
                    ic.push((nu, cu));
                }
            }
            if let (Some(l), true) = (common, all_equal) {
                label[[v.r, v.c]] = l;
                label_flag[[v.r, v.c]] = true;
                for (nu, cu) in ic {
                    queue_flag[[nu, cu]] = true;
                    q.push(Vox { val: im[[nu, cu]], pos, r: nu, c: cu });
                    pos += 1;
                }
            }
            // else: watershed line — label stays 0, neighbours not pushed.
        }
        label
    }

    /// SNLC `resetPatch` (`splitPatchesX.m` subfunction): if limiting patch `q`
    /// to the central visual field fragments it into ≥2 components, split it via
    /// a distance-transform watershed seeded at those components. Returns the
    /// updated patch image (`q` replaced by its split, or unchanged if no split).
    ///
    /// Verbatim port — every step is a golden'd primitive: `bwdist`, the
    /// `watershed_octave4`, `phi` (= `max(·,0)`), and the disk-1 cross
    /// morphology. The `-inf` barrier outside the dilated patch and the `0`
    /// forced minima at the center components reproduce the oracle exactly.
    pub(super) fn reset_patch(
        im: &Array2<bool>,
        imlab: &Array2<i32>,
        center_patch: &Array2<bool>,
        q: i32,
    ) -> Array2<bool> {
        let (h, w) = im.dim();
        let imorig = Array2::from_shape_fn((h, w), |(r, c)| imlab[[r, c]] == q);
        let imdil = binary_dilation_cross(&imorig, 1);
        // impatch = open( patch q ∩ centerPatch ).
        let center_q = Array2::from_shape_fn((h, w), |(r, c)| imlab[[r, c]] == q && center_patch[[r, c]]);
        let impatch = binary_opening_cross(&center_q, 1);
        let (_lab, n_cc) = label_4conn(&impatch);
        if n_cc < 2 {
            return im.clone(); // limiting to the center did NOT fragment it → no split
        }
        // Elevation: bwdist(impatch), `-inf` outside the dilated patch (barrier),
        // `0` at the center-component seeds (forced minima).
        let mut imdist = bwdist(&impatch);
        for r in 0..h {
            for c in 0..w {
                if !imdil[[r, c]] {
                    imdist[[r, c]] = f64::NEG_INFINITY;
                }
                if impatch[[r, c]] {
                    imdist[[r, c]] = 0.0;
                }
            }
        }
        let wshed = watershed_octave4(&imdist);
        // sign(phi(wshed-1)): label 1 = the `-inf` surround, 0 = watershed lines,
        // ≥2 = the seed basins → keep only the seed-basin interiors.
        let basins = Array2::from_shape_fn((h, w), |(r, c)| wshed[[r, c]] >= 2);
        let basins = binary_erosion_cross(&basins, 1); // widen the fracture
        // im(idorig)=0; im = im + wshed.
        Array2::from_shape_fn((h, w), |(r, c)| {
            let base = if imlab[[r, c]] == q { false } else { im[[r, c]] };
            base || basins[[r, c]]
        })
    }

    /// MATLAB `fspecial('gaussian', [h w], sigma)`: a centred Gaussian kernel of
    /// the full `[h, w]` size, with the `< eps·max` truncation, normalised to
    /// sum 1. (For even sizes the centre is the half-pixel, as in MATLAB.)
    pub(crate) fn fspecial_gaussian(h: usize, w: usize, sigma: f64) -> Array2<f64> {
        let sr = (h as f64 - 1.0) / 2.0;
        let sc = (w as f64 - 1.0) / 2.0;
        let two_s2 = 2.0 * sigma * sigma;
        let mut g = Array2::<f64>::zeros((h, w));
        let mut maxv = 0.0f64;
        for r in 0..h {
            let y = r as f64 - sr;
            for c in 0..w {
                let x = c as f64 - sc;
                let v = (-(x * x + y * y) / two_s2).exp();
                g[[r, c]] = v;
                maxv = maxv.max(v);
            }
        }
        let thr = f64::EPSILON * maxv;
        let mut sum = 0.0;
        for v in g.iter_mut() {
            if *v < thr {
                *v = 0.0;
            }
            sum += *v;
        }
        if sum != 0.0 {
            g.mapv_inplace(|v| v / sum);
        }
        g
    }

    /// 2-D FFT (or inverse) via `rustfft` — row transforms then column
    /// transforms. `rustfft` is unnormalised; the inverse is scaled by `1/(h·w)`
    /// by the caller to match MATLAB `ifft2`.
    fn fft2(data: &Array2<Complex64>, inverse: bool) -> Array2<Complex64> {
        let (h, w) = data.dim();
        let mut planner = FftPlanner::<f64>::new();
        let frow = if inverse {
            planner.plan_fft_inverse(w)
        } else {
            planner.plan_fft_forward(w)
        };
        let fcol = if inverse {
            planner.plan_fft_inverse(h)
        } else {
            planner.plan_fft_forward(h)
        };
        let mut buf: Vec<Complex64> = data.iter().copied().collect(); // row-major
        for r in 0..h {
            frow.process(&mut buf[r * w..(r + 1) * w]);
        }
        let mut col = vec![Complex64::new(0.0, 0.0); h];
        for c in 0..w {
            for r in 0..h {
                col[r] = buf[r * w + c];
            }
            fcol.process(&mut col);
            for r in 0..h {
                buf[r * w + c] = col[r];
            }
        }
        Array2::from_shape_vec((h, w), buf).expect("fft2 reshape")
    }

    /// The fft-based circular Gaussian smooth `splitPatchesX` applies to the
    /// position maps: `real(ifft2( fft2(map) .* abs(fft2(fspecial_gaussian)) ))`
    /// — a zero-phase circular blur. FFT via `rustfft`; matched to Octave at an
    /// ε-grounded tolerance (cross-library FFT roundoff ⇒ not bit-exact).
    pub(super) fn fft_gaussian_smooth(map: &Array2<f64>, sigma: f64) -> Array2<f64> {
        let (h, w) = map.dim();
        let hh = fspecial_gaussian(h, w, sigma);
        let cmap = map.mapv(|v| Complex64::new(v, 0.0));
        let chh = hh.mapv(|v| Complex64::new(v, 0.0));
        let fmap = fft2(&cmap, false);
        let fhh = fft2(&chh, false);
        let prod = Array2::from_shape_fn((h, w), |(r, c)| fmap[[r, c]] * fhh[[r, c]].norm());
        let inv = fft2(&prod, true);
        let n = (h * w) as f64;
        inv.mapv(|c| c.re / n)
    }

    /// Solve a tridiagonal system (sub `l`, diag `dg`, super `u`) for `rhs` via
    /// the Thomas algorithm. `l`/`u` have length m−1, `dg`/`rhs` length m.
    fn thomas(l: &[f64], dg: &[f64], u: &[f64], rhs: &[f64]) -> Vec<f64> {
        let m = dg.len();
        let mut cp = vec![0.0f64; m];
        let mut dp = vec![0.0f64; m];
        cp[0] = if m > 1 { u[0] / dg[0] } else { 0.0 };
        dp[0] = rhs[0] / dg[0];
        for k in 1..m {
            let den = dg[k] - l[k - 1] * cp[k - 1];
            if k < m - 1 {
                cp[k] = u[k] / den;
            }
            dp[k] = (rhs[k] - l[k - 1] * dp[k - 1]) / den;
        }
        let mut out = vec![0.0f64; m];
        out[m - 1] = dp[m - 1];
        for k in (0..m - 1).rev() {
            out[k] = dp[k] - cp[k] * out[k + 1];
        }
        out
    }

    /// 1-D **not-a-knot** cubic spline (Octave `spline.m`, the default end
    /// condition). Returns per-interval pp coefficients `(a, b, c, d)` (length
    /// n−1); on interval `i`, value at `t = q − x[i]` is `a+b·t+c·t²+d·t³`.
    fn spline_not_a_knot(x: &[f64], y: &[f64]) -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
        let n = x.len();
        assert!(n >= 4, "not-a-knot spline needs n>=4 (got {n})");
        let h: Vec<f64> = (0..n - 1).map(|i| x[i + 1] - x[i]).collect();
        let m = n - 2;
        // RHS g (length m).
        let mut g = vec![0.0f64; m];
        g[0] = 3.0 / (h[0] + h[1]) * (y[2] - y[1] - h[1] / h[0] * (y[1] - y[0]));
        g[m - 1] = 3.0 / (h[n - 2] + h[n - 3])
            * (h[n - 3] / h[n - 2] * (y[n - 1] - y[n - 2]) - (y[n - 2] - y[n - 3]));
        for gi in 1..m - 1 {
            g[gi] = 3.0 * (y[gi + 2] - y[gi + 1]) / h[gi + 1] - 3.0 * (y[gi + 1] - y[gi]) / h[gi];
        }
        // Tridiagonal (the not-a-knot system; this general form also covers n==4).
        let mut dg = vec![0.0f64; m];
        for k in 0..m {
            dg[k] = 2.0 * (h[k] + h[k + 1]);
        }
        dg[0] -= h[0];
        dg[m - 1] -= h[n - 2];
        let mut udg = h[1..m].to_vec(); // length m-1
        let mut ldg = udg.clone();
        udg[0] -= h[0];
        ldg[m - 2] -= h[n - 2];
        let c_inner = thomas(&ldg, &dg, &udg, &g); // length m = c(2:n-1)
        // Full c (length n) with the not-a-knot boundary extrapolation.
        let mut c = vec![0.0f64; n];
        c[1..=m].copy_from_slice(&c_inner);
        c[0] = c[1] + h[0] / h[1] * (c[1] - c[2]);
        c[n - 1] = c[n - 2] + h[n - 2] / h[n - 3] * (c[n - 2] - c[n - 3]);
        // b, d (length n-1).
        let mut b = vec![0.0f64; n - 1];
        let mut d = vec![0.0f64; n - 1];
        for i in 0..n - 1 {
            b[i] = (y[i + 1] - y[i]) / h[i] - h[i] / 3.0 * (c[i + 1] + 2.0 * c[i]);
            d[i] = (c[i + 1] - c[i]) / (3.0 * h[i]);
        }
        (y[0..n - 1].to_vec(), b, c[0..n - 1].to_vec(), d)
    }

    /// Evaluate the spline pp at `q` (extrapolating via the boundary interval's
    /// polynomial outside `[x0, xn]`, as Octave's spline does).
    fn ppval(x: &[f64], a: &[f64], b: &[f64], c: &[f64], d: &[f64], q: f64) -> f64 {
        let n = x.len();
        // Interval i with x[i] <= q <= x[i+1]; clamp at the ends.
        let i = match x.binary_search_by(|v| v.partial_cmp(&q).unwrap()) {
            Ok(idx) => idx.min(n - 2),
            Err(0) => 0,
            Err(idx) => (idx - 1).min(n - 2),
        };
        let t = q - x[i];
        ((d[i] * t + c[i]) * t + b[i]) * t + a[i]
    }

    /// `interp2(x, y, z, xi, yi, 'spline')` — tensor-product not-a-knot cubic
    /// spline (Octave's `__splinen__`): spline along x (rows), then along y
    /// (columns). `z` is indexed `[row=y, col=x]`. Matched to Octave at an
    /// ε-grounded tolerance.
    pub(super) fn interp2_spline(
        x: &[f64],
        y: &[f64],
        z: &Array2<f64>,
        xi: &[f64],
        yi: &[f64],
    ) -> Array2<f64> {
        let (h, w) = z.dim();
        let (nxi, nyi) = (xi.len(), yi.len());
        let mut temp = Array2::<f64>::zeros((h, nxi));
        for r in 0..h {
            let row: Vec<f64> = (0..w).map(|c| z[[r, c]]).collect();
            let (a, b, c, d) = spline_not_a_knot(x, &row);
            for (j, &q) in xi.iter().enumerate() {
                temp[[r, j]] = ppval(x, &a, &b, &c, &d, q);
            }
        }
        let mut zi = Array2::<f64>::zeros((nyi, nxi));
        for j in 0..nxi {
            let col: Vec<f64> = (0..h).map(|r| temp[[r, j]]).collect();
            let (a, b, c, d) = spline_not_a_knot(y, &col);
            for (i, &q) in yi.iter().enumerate() {
                zi[[i, j]] = ppval(y, &a, &b, &c, &d, q);
            }
        }
        zi
    }

    /// `filter2(kernel, a)` = `conv2(a, kernel, 'same')` (symmetric kernel),
    /// zero-padded. The conv2 'same' offset is `floor(Hk/2)`; with `fspecial`'s
    /// centre at `(Hk-1)/2` this reproduces the even-size half-pixel convention.
    /// Only non-zero kernel entries are summed (the Gaussian is truncated), so a
    /// full-size kernel is still cheap.
    pub(crate) fn filter2_same(kernel: &Array2<f64>, a: &Array2<f64>) -> Array2<f64> {
        let (h, w) = a.dim();
        let (hk, wk) = kernel.dim();
        let (r0, c0) = (hk as isize / 2, wk as isize / 2);
        let nz: Vec<(isize, isize, f64)> = kernel
            .indexed_iter()
            .filter(|(_, &v)| v != 0.0)
            .map(|((p, q), &v)| (p as isize, q as isize, v))
            .collect();
        Array2::from_shape_fn((h, w), |(r, c)| {
            let mut s = 0.0;
            for &(p, q, v) in &nz {
                let ai = r0 + r as isize - p;
                let aj = c0 + c as isize - q;
                if ai >= 0 && ai < h as isize && aj >= 0 && aj < w as isize {
                    s += v * a[[ai as usize, aj as usize]];
                }
            }
            s
        })
    }

    /// SNLC `smoothPatchesX(map, im)`: background (outside any patch) → 45, then
    /// each patch — in bwlabel/column-major order — is Gaussian-smoothed with
    /// σ = area/2000 via `roifilt2` (`filter2` kept within the patch),
    /// accumulating into the output.
    pub(super) fn smooth_patches_x(map: &Array2<f64>, im: &Array2<bool>) -> Array2<f64> {
        let (h, w) = map.dim();
        let mut mapout = map.clone();
        for r in 0..h {
            for c in 0..w {
                if !im[[r, c]] {
                    mapout[[r, c]] = 45.0;
                }
            }
        }
        let labels = label_colmajor4(im);
        let n = labels.iter().copied().max().unwrap_or(0);
        for q in 1..=n {
            let count = labels.iter().filter(|&&l| l == q).count();
            let sig = count as f64 / 2000.0;
            let hh = fspecial_gaussian(h, w, sig);
            let filtered = filter2_same(&hh, &mapout);
            for r in 0..h {
                for c in 0..w {
                    if labels[[r, c]] == q {
                        mapout[[r, c]] = filtered[[r, c]];
                    }
                }
            }
        }
        mapout
    }

    // ── getNlocalmin toolbox: grayscale morphology, prctile, imimposemin ─────

    /// Grayscale erosion with a disk SE — local min over the SE (out-of-bounds
    /// ignored, = Octave's `imerode` of a flat SE, which pads with +Inf).
    fn gray_erode_disk(im: &Array2<f64>, radius: i32) -> Array2<f64> {
        let (h, w) = im.dim();
        let offs = disk_offsets(radius);
        Array2::from_shape_fn((h, w), |(r, c)| {
            let mut m = f64::INFINITY;
            for &(dr, dc) in &offs {
                let (nr, nc) = (r as i32 + dr, c as i32 + dc);
                if nr >= 0 && nr < h as i32 && nc >= 0 && nc < w as i32 {
                    m = m.min(im[[nr as usize, nc as usize]]);
                }
            }
            m
        })
    }

    /// Grayscale dilation with a disk SE — local max over the SE.
    fn gray_dilate_disk(im: &Array2<f64>, radius: i32) -> Array2<f64> {
        let (h, w) = im.dim();
        let offs = disk_offsets(radius);
        Array2::from_shape_fn((h, w), |(r, c)| {
            let mut m = f64::NEG_INFINITY;
            for &(dr, dc) in &offs {
                let (nr, nc) = (r as i32 + dr, c as i32 + dc);
                if nr >= 0 && nr < h as i32 && nc >= 0 && nc < w as i32 {
                    m = m.max(im[[nr as usize, nc as usize]]);
                }
            }
            m
        })
    }

    /// Grayscale `imopen(im, strel('disk',r,0))` = dilate(erode(·)).
    fn gray_imopen_disk(im: &Array2<f64>, radius: i32) -> Array2<f64> {
        gray_dilate_disk(&gray_erode_disk(im, radius), radius)
    }

    /// `medfilt2(im, [3 3])` on a grayscale image: the 3×3 window median
    /// (zero-padded borders), the 5th of the 9 sorted values.
    fn gray_medfilt2_3x3(im: &Array2<f64>) -> Array2<f64> {
        let (h, w) = im.dim();
        Array2::from_shape_fn((h, w), |(r, c)| {
            let mut v = [0.0f64; 9];
            let mut k = 0;
            for dr in -1..=1i32 {
                for dc in -1..=1i32 {
                    let (nr, nc) = (r as i32 + dr, c as i32 + dc);
                    v[k] = if nr >= 0 && nr < h as i32 && nc >= 0 && nc < w as i32 {
                        im[[nr as usize, nc as usize]]
                    } else {
                        0.0
                    };
                    k += 1;
                }
            }
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v[4]
        })
    }

    /// `prctile(data, p)` — Octave's default (quantile method 5 / Hazen):
    /// piecewise-linear through `((k-0.5)/n, x_(k))`.
    fn prctile(data: &[f64], p: f64) -> f64 {
        let n = data.len();
        let mut x = data.to_vec();
        x.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let pos = p / 100.0 * n as f64 + 0.5; // 1-based fractional rank
        if pos <= 1.0 {
            return x[0];
        }
        if pos >= n as f64 {
            return x[n - 1];
        }
        let k = pos.floor();
        let g = pos - k;
        let ki = k as usize; // 1-based k → x[k-1], x[k]
        (1.0 - g) * x[ki - 1] + g * x[ki]
    }

    /// Morphological reconstruction by erosion of `mask` from `marker`
    /// (`marker ≥ mask`): iterate `J = max(erode(J), mask)` to stability.
    fn reconstruct_by_erosion(
        marker: &Array2<f64>,
        mask: &Array2<f64>,
        offs: &[(i32, i32)],
    ) -> Array2<f64> {
        let (h, w) = marker.dim();
        let mut j = Array2::from_shape_fn((h, w), |(r, c)| marker[[r, c]].max(mask[[r, c]]));
        loop {
            let newj = Array2::from_shape_fn((h, w), |(r, c)| {
                let mut m = j[[r, c]]; // SE includes the centre
                for &(dr, dc) in offs {
                    let (nr, nc) = (r as i32 + dr, c as i32 + dc);
                    if nr >= 0 && nr < h as i32 && nc >= 0 && nc < w as i32 {
                        m = m.min(j[[nr as usize, nc as usize]]);
                    }
                }
                m.max(mask[[r, c]])
            });
            if newj == j {
                break;
            }
            j = newj;
        }
        j
    }

    /// `imimposemin(im, bw)` (conn=8): impose regional minima of `im` at `bw`.
    /// = reconstruction-by-erosion of `min(im+δ, fm)` from the ±Inf marker `fm`
    /// (the `imcomplement` constants cancel). `δ = (max-min)/1000`.
    fn imimposemin(im: &Array2<f64>, bw: &Array2<bool>) -> Array2<f64> {
        let (h, w) = im.dim();
        let fm = Array2::from_shape_fn((h, w), |(r, c)| {
            if bw[[r, c]] {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            }
        });
        let (mut mn, mut mx) = (f64::INFINITY, f64::NEG_INFINITY);
        for &v in im.iter() {
            if v.is_finite() {
                mn = mn.min(v);
                mx = mx.max(v);
            }
        }
        let delta = (mx - mn) / 1000.0;
        let min_im = Array2::from_shape_fn((h, w), |(r, c)| (im[[r, c]] + delta).min(fm[[r, c]]));
        reconstruct_by_erosion(&fm, &min_im, &N8)
    }

    /// SNLC `getNlocalmin(idpatch, Rmax, kmap_rad)` — the compute-path outputs
    /// `(Nmin, newpatches)`. Discretises the patch eccentricity into percentile
    /// bins, opens + medians it, finds the regional minima, then watersheds an
    /// `imimposemin`'d map to cut the patch into sub-regions (the `centerPatch2`
    /// passed to `resetPatch`).
    pub(super) fn get_nlocalmin(
        patch_mask: &Array2<bool>, // dumpatch (idpatch as a mask)
        rmax: f64,
        kmap_rad: &Array2<f64>,
    ) -> (usize, Array2<bool>) {
        let (h, w) = kmap_rad.dim();
        let patch_vals = |m: &Array2<f64>| -> Vec<f64> {
            (0..h)
                .flat_map(|r| {
                    (0..w)
                        .filter(move |&c| patch_mask[[r, c]])
                        .map(move |c| m[[r, c]])
                })
                .collect()
        };
        let kr = patch_vals(kmap_rad);
        let npatch = kr.len();
        let kr_min = kr.iter().copied().fold(f64::INFINITY, f64::min);
        let kr_max = kr.iter().copied().fold(f64::NEG_INFINITY, f64::max);

        // threshdom = [min(kr)-1, prctile(kr, 2:10:90), max(kr)+1].
        let mut thresh = vec![kr_min - 1.0];
        let mut p = 2;
        while p <= 90 {
            thresh.push(prctile(&kr, p as f64));
            p += 10;
        }
        thresh.push(kr_max + 1.0);

        // Discretise the whole map: each open bin (lo, hi) → its mean.
        let mut disc = kmap_rad.clone();
        for win in thresh.windows(2) {
            let (lo, hi) = (win[0], win[1]);
            let (mut sum, mut cnt) = (0.0, 0usize);
            for &v in kmap_rad.iter() {
                if v > lo && v < hi {
                    sum += v;
                    cnt += 1;
                }
            }
            if cnt > 0 {
                let mean = sum / cnt as f64;
                for (d, &v) in disc.iter_mut().zip(kmap_rad.iter()) {
                    if v > lo && v < hi {
                        *d = mean;
                    }
                }
            }
        }
        // Background → max over the (discretised) patch.
        let patch_max = patch_vals(&disc).iter().copied().fold(f64::NEG_INFINITY, f64::max);
        for r in 0..h {
            for c in 0..w {
                if !patch_mask[[r, c]] {
                    disc[[r, c]] = patch_max;
                }
            }
        }
        let disc = gray_imopen_disk(&disc, 3);

        // rad = Rmax outside the patch, the opened map inside; then medfilt2.
        let mut rad = Array2::from_shape_fn((h, w), |(r, c)| {
            if patch_mask[[r, c]] {
                disc[[r, c]]
            } else {
                rmax
            }
        });
        rad = gray_medfilt2_3x3(&rad);

        // minpatch = regional minima ∩ patch, dilated by disk(round(√npatch/20)).
        let rmin = imregionalmin8(&rad);
        let minpatch0 = Array2::from_shape_fn((h, w), |(r, c)| rmin[[r, c]] && patch_mask[[r, c]]);
        let d = ((npatch as f64).sqrt() / 20.0).round() as i32;
        let dil = binary_dilation_disk(&minpatch0, d.max(0));
        let minpatch = Array2::from_shape_fn((h, w), |(r, c)| dil[[r, c]] && patch_mask[[r, c]]);
        let (_, nmin) = label_4conn(&minpatch);

        // newpatches = watershed8 of imimposemin(rad, minpatch) with the patch
        // boundary reset to Rmax and the dilated-patch exterior set to -inf.
        let dumpatch2 = binary_dilation_disk(patch_mask, 3);
        let mut rad2 = imimposemin(&rad, &minpatch);
        for r in 0..h {
            for c in 0..w {
                if !patch_mask[[r, c]] {
                    rad2[[r, c]] = rmax;
                }
                if !dumpatch2[[r, c]] {
                    rad2[[r, c]] = f64::NEG_INFINITY;
                }
            }
        }
        let ws = watershed_octave8(&rad2);
        let newpatches = Array2::from_shape_fn((h, w), |(r, c)| ws[[r, c]] > 1);
        (nmin, newpatches)
    }

    // ── splitPatchesX orchestration ──────────────────────────────────────────

    /// MATLAB `gradient(F)` (spacing 1) → `(gx = ∂/∂x along columns, gy = ∂/∂y
    /// along rows)`: central differences interior, one-sided at the borders.
    fn gradient2(f: &Array2<f64>) -> (Array2<f64>, Array2<f64>) {
        let (h, w) = f.dim();
        let gx = Array2::from_shape_fn((h, w), |(r, c)| {
            if w == 1 {
                0.0
            } else if c == 0 {
                f[[r, 1]] - f[[r, 0]]
            } else if c == w - 1 {
                f[[r, w - 1]] - f[[r, w - 2]]
            } else {
                (f[[r, c + 1]] - f[[r, c - 1]]) / 2.0
            }
        });
        let gy = Array2::from_shape_fn((h, w), |(r, c)| {
            if h == 1 {
                0.0
            } else if r == 0 {
                f[[1, c]] - f[[0, c]]
            } else if r == h - 1 {
                f[[h - 1, c]] - f[[h - 2, c]]
            } else {
                (f[[r + 1, c]] - f[[r - 1, c]]) / 2.0
            }
        });
        (gx, gy)
    }

    /// `round(interp2(·, 'nearest'))` upsample of a binary mask by factor `u`
    /// (uniform grid → nearest = round of the fractional index).
    fn nearest_upsample(im: &Array2<bool>, u: usize) -> Array2<bool> {
        let (h, w) = im.dim();
        let (hi, wi) = (u * h, u * w);
        Array2::from_shape_fn((hi, wi), |(i, j)| {
            let c = ((j as f64) * (w as f64 - 1.0) / (wi as f64 - 1.0)).round() as usize;
            let r = ((i as f64) * (h as f64 - 1.0) / (hi as f64 - 1.0)).round() as usize;
            im[[r.min(h - 1), c.min(w - 1)]]
        })
    }

    /// `round(interp2(·, 'nearest'))` upsample of an f64 map by factor `u`.
    fn nearest_upsample_f64(z: &Array2<f64>, u: usize) -> Array2<f64> {
        let (h, w) = z.dim();
        let (hi, wi) = (u * h, u * w);
        Array2::from_shape_fn((hi, wi), |(i, j)| {
            let c = ((j as f64) * (w as f64 - 1.0) / (wi as f64 - 1.0)).round() as usize;
            let r = ((i as f64) * (h as f64 - 1.0) / (hi as f64 - 1.0)).round() as usize;
            z[[r.min(h - 1), c.min(w - 1)]].round()
        })
    }

    fn median(vals: &[f64]) -> f64 {
        let mut v = vals.to_vec();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = v.len();
        if n % 2 == 1 {
            v[n / 2]
        } else {
            (v[n / 2 - 1] + v[n / 2]) / 2.0
        }
    }

    /// SNLC `splitPatchesX(im, kmap_hor, kmap_vert, kmap_rad, pixpermm)` — the
    /// over-representation split. Smooths + spline-upsamples (U=3) the position
    /// maps, then runs three passes: (1) limit patches to R=30° eccentricity
    /// (`reset_patch`), (2) split over-representing patches via local minima
    /// (`get_nlocalmin` → `reset_patch`), (3) remove patches whose visual-space
    /// coverage is redundant or negligible (`over_rep`). Returns the refined mask.
    pub(super) fn split_patches_x(
        im: &Array2<bool>,
        kmap_hor: &Array2<f64>,
        kmap_vert: &Array2<f64>,
        kmap_rad: &Array2<f64>,
        pixpermm: f64,
    ) -> Array2<bool> {
        const U: usize = 3;
        let (h, w) = im.dim();
        let sph_min = -90.0;
        let nsph = 181;

        // ── setup ──
        let kmap_rad = smooth_patches_x(kmap_rad, im);
        let kmap_hor_s = fft_gaussian_smooth(kmap_hor, 2.0);
        let kmap_vert_s = fft_gaussian_smooth(kmap_vert, 2.0);
        // Spline upsample (invariant to affine x-rescale → use index coords).
        let x: Vec<f64> = (0..w).map(|v| v as f64).collect();
        let y: Vec<f64> = (0..h).map(|v| v as f64).collect();
        let xi: Vec<f64> = (0..U * w)
            .map(|k| (w as f64 - 1.0) * k as f64 / (U * w - 1) as f64)
            .collect();
        let yi: Vec<f64> = (0..U * h)
            .map(|k| (h as f64 - 1.0) * k as f64 / (U * h - 1) as f64)
            .collect();
        let kmap_hor_i = interp2_spline(&x, &y, &kmap_hor_s, &xi, &yi).mapv(f64::round);
        let kmap_vert_i = interp2_spline(&x, &y, &kmap_vert_s, &xi, &yi).mapv(f64::round);
        let kmap_rad_i = interp2_spline(&x, &y, &kmap_rad, &xi, &yi).mapv(f64::round);
        let (dhx, dhy) = gradient2(&kmap_hor_i);
        let (dvx, dvy) = gradient2(&kmap_vert_i);
        let ppm_u2 = (pixpermm * U as f64).powi(2);
        let jac_i = Array2::from_shape_fn((U * h, U * w), |(r, c)| {
            (dhx[[r, c]] * dvy[[r, c]] - dvx[[r, c]] * dhy[[r, c]]) * ppm_u2
        });
        // im = imerode(imopen(im, disk1), disk1)  (disk1 ≡ cross)
        let mut im = binary_erosion_cross(&binary_opening_cross(im, 1), 1);

        let overlap = |dumpatch_i: &Array2<bool>| -> Coverage {
            over_rep(&kmap_hor_i, &kmap_vert_i, U as f64, &jac_i, dumpatch_i, sph_min, nsph, pixpermm)
        };

        // ── pass 1: limit each patch to R=30° eccentricity ──
        let r_lim = 30.0;
        let imlab = label_colmajor4(&im);
        let center = get_center_patch(&kmap_rad, &im, r_lim);
        let n = imlab.iter().copied().max().unwrap_or(0);
        for q in 1..=n {
            im = reset_patch(&im, &imlab, &center, q);
        }

        // ── pass 2: split over-representing patches via local minima ──
        let im_i = nearest_upsample(&im, U);
        let imlab = label_colmajor4(&im);
        let imlab_i = label_colmajor4(&im_i);
        let center = get_center_patch(&kmap_rad, &im, r_lim);
        let center_i = get_center_patch(&kmap_rad_i, &im_i, r_lim);
        let n = imlab.iter().copied().max().unwrap_or(0);
        for q in 1..=n {
            let dumpatch = Array2::from_shape_fn((h, w), |(r, c)| imlab[[r, c]] == q && center[[r, c]]);
            if !dumpatch.iter().any(|&b| b) {
                continue;
            }
            let dumpatch_i =
                Array2::from_shape_fn((U * h, U * w), |(r, c)| imlab_i[[r, c]] == q && center_i[[r, c]]);
            let cov = overlap(&dumpatch_i);
            if cov.jac_coverage / cov.actual_coverage > 0.999 {
                let hv: Vec<f64> = dumpatch
                    .indexed_iter()
                    .filter(|(_, &b)| b)
                    .map(|((r, c), _)| kmap_hor[[r, c]])
                    .collect();
                let vv: Vec<f64> = dumpatch
                    .indexed_iter()
                    .filter(|(_, &b)| b)
                    .map(|((r, c), _)| kmap_vert[[r, c]])
                    .collect();
                let (hc, vc) = (median(&hv), median(&vv));
                let rad_dum = Array2::from_shape_fn((h, w), |(r, c)| {
                    if dumpatch[[r, c]] {
                        ((kmap_hor[[r, c]] - hc).powi(2) + (kmap_vert[[r, c]] - vc).powi(2)).sqrt()
                    } else {
                        0.0
                    }
                });
                let (_nmin, center2) = get_nlocalmin(&dumpatch, r_lim, &rad_dum);
                im = reset_patch(&im, &imlab, &center2, q);
            }
        }

        // ── pass 3: remove redundant / negligible-coverage patches (R=35) ──
        let r_lim = 35.0;
        let imlab = label_colmajor4(&im);
        let im_i = nearest_upsample(&im, U);
        let imlab_i = label_colmajor4(&im_i);
        let center_i = get_center_patch(&kmap_rad_i, &im_i, r_lim);
        let n = imlab.iter().copied().max().unwrap_or(0);
        let mut out = im.clone();
        for q in 1..=n {
            let dumpatch_i =
                Array2::from_shape_fn((U * h, U * w), |(r, c)| imlab_i[[r, c]] == q && center_i[[r, c]]);
            let cov = overlap(&dumpatch_i);
            let neg = cov.jac_coverage / (std::f64::consts::PI * r_lim * r_lim) < 0.01;
            if cov.jac_coverage / cov.actual_coverage > 1.05 || neg {
                for r in 0..h {
                    for c in 0..w {
                        if imlab[[r, c]] == q {
                            out[[r, c]] = false;
                        }
                    }
                }
            }
        }
        out
    }

    /// SNLC `fusePatchesX(im, kmap_hor, kmap_vert, pixpermm)` — fuse pairs of
    /// patches that are the same field sign, border each other, and represent
    /// *unique* (non-overlapping) regions of visual space (i.e. one area split
    /// in two by noise). For each adjacent same-sign pair whose visual-space
    /// overlap is < 10%, close the union and replace the pair. Returns the mask.
    pub(super) fn fuse_patches_x(
        im: &Array2<bool>,
        kmap_hor: &Array2<f64>,
        kmap_vert: &Array2<f64>,
        pixpermm: f64,
    ) -> Array2<bool> {
        const U: usize = 3;
        let (h, w) = im.dim();
        let sph_min = -90.0;
        let nsph = 181;

        // Sereno (VFS) from the gradient directions of the (uninterpolated) maps.
        let sereno = sereno_vfs(kmap_hor, kmap_vert);
        let imlab = label_colmajor4(im);

        // fft-Gaussian smooth → nearest-upsample (U=3) → JacI, imI.
        let hor_i = nearest_upsample_f64(&fft_gaussian_smooth(kmap_hor, 2.0), U);
        let vert_i = nearest_upsample_f64(&fft_gaussian_smooth(kmap_vert, 2.0), U);
        let (dhxi, dhyi) = gradient2(&hor_i);
        let (dvxi, dvyi) = gradient2(&vert_i);
        let ppm_u2 = (pixpermm * U as f64).powi(2);
        let jac_i = Array2::from_shape_fn((U * h, U * w), |(r, c)| {
            (dhxi[[r, c]] * dvyi[[r, c]] - dvxi[[r, c]] * dhyi[[r, c]]) * ppm_u2
        });
        let im_i = nearest_upsample(im, U);
        let imlab_i = label_colmajor4(&im_i);
        let n = imlab_i.iter().copied().max().unwrap_or(0);

        // Per-patch visual coverage (dilated, upsampled) + field sign.
        let mut sp_cov: Vec<Array2<bool>> = Vec::with_capacity(n as usize);
        let mut area_sign: Vec<f64> = Vec::with_capacity(n as usize);
        for i in 1..=n {
            let pi = Array2::from_shape_fn((U * h, U * w), |(r, c)| imlab_i[[r, c]] == i);
            let pid = binary_dilation_disk(&pi, 1);
            sp_cov
                .push(over_rep(&hor_i, &vert_i, U as f64, &jac_i, &pid, sph_min, nsph, pixpermm).sp_cov);
            let (mut s, mut cnt) = (0.0, 0usize);
            for r in 0..h {
                for c in 0..w {
                    if imlab[[r, c]] == i {
                        s += sereno[[r, c]];
                        cnt += 1;
                    }
                }
            }
            area_sign.push(if cnt > 0 { msign(s / cnt as f64) } else { 0.0 });
        }

        let mut im = im.clone();
        let mut imlab2 = imlab.clone(); // updated labels (tracks prior fuses)
        for ii in 0..n as usize {
            for jj in (ii + 1)..n as usize {
                if area_sign[ii] * area_sign[jj] != 1.0 {
                    continue;
                }
                let (i, j) = ((ii + 1) as i32, (jj + 1) as i32);
                // Resolve each patch's CURRENT label via the median trick (so a
                // patch already fused earlier maps to its fused label).
                let mut li: Vec<f64> = Vec::new();
                let mut lj: Vec<f64> = Vec::new();
                for r in 0..h {
                    for c in 0..w {
                        if imlab[[r, c]] == i {
                            li.push(imlab2[[r, c]] as f64);
                        }
                        if imlab[[r, c]] == j && imlab2[[r, c]] > 0 {
                            lj.push(imlab2[[r, c]] as f64);
                        }
                    }
                }
                if li.is_empty() || lj.is_empty() {
                    continue;
                }
                let mi = median(&li).round() as i32;
                let mj = median(&lj).round() as i32;
                let p1 = Array2::from_shape_fn((h, w), |(r, c)| imlab2[[r, c]] == mi);
                let p2 = Array2::from_shape_fn((h, w), |(r, c)| imlab2[[r, c]] == mj);

                // Touch? (disk-3 dilations overlap.)
                let p1d = binary_dilation_disk(&p1, 3);
                let p2d = binary_dilation_disk(&p2, 3);
                if !(0..h).any(|r| (0..w).any(|c| p1d[[r, c]] && p2d[[r, c]])) {
                    continue;
                }
                // OLap = fraction of the smaller coverage that the two share.
                let sum_i = sp_cov[ii].iter().filter(|&&b| b).count();
                let sum_j = sp_cov[jj].iter().filter(|&&b| b).count();
                let inter = sp_cov[ii]
                    .iter()
                    .zip(sp_cov[jj].iter())
                    .filter(|(&a, &b)| a && b)
                    .count();
                let norm = sum_i.min(sum_j);
                let olap = if norm > 0 { inter as f64 / norm as f64 } else { 0.0 };
                if olap >= 0.1 {
                    continue;
                }

                // Fuse: close the union, keep it clear of other patches, replace.
                let p12 = Array2::from_shape_fn((h, w), |(r, c)| p1[[r, c]] || p2[[r, c]]);
                let mut pf = binary_closing_disk(&p12, 5);
                let im_minus = Array2::from_shape_fn((h, w), |(r, c)| im[[r, c]] && !p12[[r, c]]);
                let imdum = binary_dilation_disk(&im_minus, 1);
                for r in 0..h {
                    for c in 0..w {
                        if imdum[[r, c]] {
                            pf[[r, c]] = false;
                        }
                    }
                }
                im = Array2::from_shape_fn((h, w), |(r, c)| {
                    (im[[r, c]] && !p1[[r, c]] && !p2[[r, c]]) || pf[[r, c]]
                });
                im = binary_opening_cross(&im, 1);
                imlab2 = label_colmajor4(&im);
                let un = Array2::from_shape_fn(sp_cov[ii].dim(), |(r, c)| {
                    sp_cov[ii][[r, c]] || sp_cov[jj][[r, c]]
                });
                sp_cov[ii] = un.clone();
                sp_cov[jj] = un;
            }
        }
        im
    }

    /// SNLC visual field sign (`Sereno`): `sin(∠hor − ∠vert)` of the gradient
    /// directions of the position maps.
    fn sereno_vfs(kmap_hor: &Array2<f64>, kmap_vert: &Array2<f64>) -> Array2<f64> {
        let (h, w) = kmap_hor.dim();
        let (dhx, dhy) = gradient2(kmap_hor);
        let (dvx, dvy) = gradient2(kmap_vert);
        Array2::from_shape_fn((h, w), |(r, c)| {
            let gh = dhy[[r, c]].atan2(dhx[[r, c]]);
            let gv = dvy[[r, c]].atan2(dvx[[r, c]]);
            (gh - gv).sin()
        })
    }

    /// V1-centred eccentricity (`getMouseAreasX`): V1 = the largest patch (after a
    /// disk-10 open); its center-of-mass gives the visual reference point, and
    /// `kmap_rad` is the great-circle eccentricity of every pixel from it.
    fn v1_eccentricity(im: &Array2<bool>, azi: &Array2<f64>, alt: &Array2<f64>) -> Array2<f64> {
        let (h, w) = im.dim();
        let imdum = binary_opening_disk(im, 10);
        let (labels, n) = label_4conn(&imdum);
        if n == 0 {
            return Array2::zeros((h, w));
        }
        let mut areas = vec![0usize; n + 1];
        for &l in labels.iter() {
            if l > 0 {
                areas[l as usize] += 1;
            }
        }
        let v1 = (1..=n).max_by_key(|&i| areas[i]).unwrap_or(1);
        let coms = patch_com(&imdum);
        let (cx, cy) = coms.get(v1 - 1).copied().unwrap_or((1.0, 1.0));
        let px = ((cx.round() as usize).max(1) - 1).min(w - 1);
        let py = ((cy.round() as usize).max(1) - 1).min(h - 1);
        let (vcent_azi, vcent_alt) = (azi[[py, px]], alt[[py, px]]);
        Array2::from_shape_fn((h, w), |(r, c)| {
            eccentricity_pixel_deg(alt[[r, c]], azi[[r, c]], vcent_alt, vcent_azi)
        })
    }

    /// `PatchRefinement::Garrett2014SplitFuse` driver: split over-representing
    /// patches then fuse under-segmented ones (SNLC `splitPatchesX` →
    /// `fusePatchesX`). Operates on the binary union of the input patches and
    /// rebuilds `Patch`es (with majority-sign from the VFS) from the result.
    pub(super) fn run_split_fuse(
        patches: Vec<Patch>,
        azi_deg: &Array2<f64>,
        alt_deg: &Array2<f64>,
        um_per_pixel: f64,
        cancel: &AtomicBool,
    ) -> Result<Vec<Patch>, AnalysisError> {
        let (h, w) = azi_deg.dim();
        let pixpermm = 1000.0 / um_per_pixel;

        let mut im = Array2::<bool>::from_elem((h, w), false);
        for p in &patches {
            for ((r, c), &b) in p.mask.indexed_iter() {
                if b {
                    im[[r, c]] = true;
                }
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(AnalysisError::Cancelled);
        }

        let kmap_rad = v1_eccentricity(&im, azi_deg, alt_deg);
        let im = split_patches_x(&im, azi_deg, alt_deg, &kmap_rad, pixpermm);
        if cancel.load(Ordering::Relaxed) {
            return Err(AnalysisError::Cancelled);
        }
        let im = fuse_patches_x(&im, azi_deg, alt_deg, pixpermm);

        let sereno = sereno_vfs(azi_deg, alt_deg);
        let (labels, n) = label_4conn(&im);
        Ok(patches_from_labels_majority_sign(&labels, n, &sereno))
    }

    #[cfg(test)]
    mod golden {
        use super::*;
        use agreement::{Eps, Tol};
        use crate::test_support::load_f64;


        // (Cutover, objective 1) The frozen `patch_com_matches_snlc_get_patch_com`
        // golden + its patchcom_*.bin fixtures + gen_patchcom_golden.m were DELETED:
        // the live `patch_com_matches_genuine_snlc_live` above was enriched with an
        // L-shaped patch whose centroid lands off-patch (unique nearest pixel, no
        // tie) to exercise the same snap-to-nearest-pixel correction against the
        // genuine getPatchCoM.m live.

        /// **Live genuine-oracle, SNLC/Octave**: our `patch_com` vs the GENUINE
        /// `getPatchCoM.m` (`reference/ISI`), executed live via Octave. Three
        /// rectangles → centroids are each exactly on-patch (no snap-correction
        /// tie ambiguity — that path stays covered by the frozen fixture). MATLAB
        /// `bwlabel` is column-major vs our row-major `label_4conn`, so the centroid
        /// SET is compared order-independently. Gated behind `oracle_live`.
        #[cfg(feature = "oracle_live")]
        #[test]
        fn patch_com_matches_genuine_snlc_live() {
            use crate::test_support::oracle;
            const H: usize = 40;
            const W: usize = 48;
            let mut im_f = Array2::<f64>::zeros((H, W));
            // Two clean rectangles (on-patch centroids, no snap).
            for (r0, r1, c0, c1) in [(4, 12, 5, 15), (24, 32, 28, 42)] {
                for r in r0..r1 {
                    for c in c0..c1 {
                        im_f[[r, c]] = 1.0;
                    }
                }
            }
            // An L-shape whose centroid lands OFF the patch → exercises the
            // snap-to-nearest-pixel correction. Geometry chosen so the nearest
            // patch pixel to the centroid is UNIQUE (the vertical arm is closer
            // than the horizontal arm), avoiding the MATLAB-find tie-break.
            for r in 4..20 {
                for c in 18..21 {
                    im_f[[r, c]] = 1.0; // vertical arm (3 wide, 16 tall)
                }
            }
            for r in 17..20 {
                for c in 21..33 {
                    im_f[[r, c]] = 1.0; // horizontal arm (12 wide, 3 tall)
                }
            }
            let im = im_f.mapv(|v| v != 0.0);

            let genuine = oracle::snlc("getPatchCoM", &[im_f], &[]);
            let comxy = &genuine[0]; // np x 2 (x, y)
            let np = comxy.dim().0;
            let got = patch_com(&im);
            assert_eq!(got.len(), np, "patch count vs genuine getPatchCoM");

            let sort = |v: &mut Vec<(f64, f64)>| v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mut got_s = got.clone();
            let mut exp_s: Vec<(f64, f64)> = (0..np).map(|i| (comxy[[i, 0]], comxy[[i, 1]])).collect();
            sort(&mut got_s);
            sort(&mut exp_s);
            let g: Vec<f64> = got_s.iter().flat_map(|&(x, y)| [x, y]).collect();
            let e: Vec<f64> = exp_s.iter().flat_map(|&(x, y)| [x, y]).collect();
            eprintln!("getPatchCoM vs GENUINE SNLC (live): {} patches", np);
            Tol::abs(128, Eps::F64).assert("getPatchCoM CoMxy vs genuine SNLC", &g, &e);
        }


        // (Cutover, objective 1) The frozen `watershed_octave_matches_octave`
        // golden + its ws4_*/ws8_*.bin fixtures + gen_watershed4_golden.m were
        // DELETED: the live `watershed_octave_matches_genuine_octave_live` above was
        // enriched to cover the same topology classes (monotonic ramp = single
        // basin, two-basin, four-well multi-basin stress) × conn {4,8} against the
        // genuine Octave `watershed` live, exact i32 labels.

        /// **Live library-primitive oracle, Octave**: our `watershed_octave{4,8}`
        /// vs the GENUINE Octave IPT `watershed(A, conn)`, executed live. Octave's
        /// watershed IS the oracle; the bridge only calls it. Exact i32 catchment
        /// labels (incl. watershed-line 0s and Octave's own label numbering) across
        /// the topologies the retired frozen golden held: a monotonic ramp (single
        /// basin), two basins (one ridge), and a four-well stress (multi-basin).
        /// Gated behind `oracle_live`.
        #[cfg(feature = "oracle_live")]
        #[test]
        fn watershed_octave_matches_genuine_octave_live() {
            use crate::test_support::oracle;
            const N: usize = 24;
            let well = |r: usize, c: usize, cr: f64, cc: f64| (r as f64 - cr).powi(2) + (c as f64 - cc).powi(2);
            let ramp = Array2::from_shape_fn((N, N), |(r, c)| (r + c) as f64);
            let two_basins = Array2::from_shape_fn((N, N), |(r, c)| well(r, c, 6.0, 6.0).min(well(r, c, 17.0, 17.0)));
            let four_wells = Array2::from_shape_fn((N, N), |(r, c)| {
                well(r, c, 5.0, 5.0)
                    .min(well(r, c, 5.0, 18.0))
                    .min(well(r, c, 18.0, 5.0))
                    .min(well(r, c, 18.0, 18.0))
            });
            let scenes = [("ramp", ramp), ("two_basins", two_basins), ("four_wells", four_wells)];
            for (name, elev) in &scenes {
                for (conn, ours) in [(4.0_f64, watershed_octave4(elev)), (8.0, watershed_octave8(elev))] {
                    let genuine = oracle::snlc("watershed", &[elev.clone()], &[("conn", conn)]).remove(0);
                    let mut diff = 0usize;
                    for r in 0..N {
                        for c in 0..N {
                            if ours[[r, c]] != genuine[[r, c]].round() as i32 {
                                diff += 1;
                            }
                        }
                    }
                    eprintln!("watershed{} {name} vs GENUINE Octave (live): diff={diff}", conn as i32);
                    assert_eq!(diff, 0, "watershed_octave {name} diverges from genuine Octave watershed");
                }
            }
        }

        // (Cutover, objective 1) The frozen `bwdist_matches_octave` golden + its
        // bwdist_*.bin fixtures + gen_bwdist_golden.m were DELETED: the live
        // `bwdist_matches_genuine_octave_live` computes the genuine Octave `bwdist`
        // on scattered seeds (same semantic; f32 single-precision tolerance) live.

        /// **Live library-primitive oracle, Octave**: our `bwdist` vs the GENUINE
        /// Octave IPT `bwdist`, executed live. Octave's bwdist is the oracle; the
        /// bridge only calls it. Octave returns SINGLE, so the comparison is to f32
        /// precision (the documented oracle dtype). Scattered seeds. Gated
        /// behind `oracle_live`.
        #[cfg(feature = "oracle_live")]
        #[test]
        fn bwdist_matches_genuine_octave_live() {
            use crate::test_support::oracle;
            const H: usize = 20;
            const W: usize = 28;
            let mut seeds_f = Array2::<f64>::zeros((H, W));
            for (r, c) in [(2, 3), (5, 20), (15, 8), (18, 25), (9, 13)] {
                seeds_f[[r, c]] = 1.0;
            }
            let seeds = seeds_f.mapv(|v| v != 0.0);

            let genuine = oracle::snlc("bwdist", &[seeds_f], &[]).remove(0);
            let ours = bwdist(&seeds);
            Tol::rel(2, Eps::F32, 2).assert(
                "bwdist vs GENUINE Octave (single-precision oracle)",
                ours.as_slice().expect("contiguous"),
                genuine.as_slice().expect("contiguous"),
            );
            eprintln!("bwdist vs GENUINE Octave (live): matched to f32");
        }


        /// `fft_gaussian_smooth` vs the Octave fft-based circular Gaussian blur.
        /// Cross-library FFT roundoff (FFTW vs rustfft) precludes bit-equality;
        /// a relative f64 bound a few ε_f64 wide is the right grounding.
        /// Fixtures from `gen_fftgauss_golden.m`.
        #[test]
        fn fft_gaussian_smooth_matches_octave() {
            let inp = load_f64(include_bytes!("../../tests/golden/fixtures/fftgauss_in.bin"));
            let exp = load_f64(include_bytes!("../../tests/golden/fixtures/fftgauss_out.bin"));
            let meta = load_f64(include_bytes!("../../tests/golden/fixtures/fftgauss_meta.bin"));
            let (h, w, sigma) = (meta[0] as usize, meta[1] as usize, meta[2]);
            let map = Array2::from_shape_fn((h, w), |(r, c)| inp[r * w + c]);

            let out = fft_gaussian_smooth(&map, sigma);
            // Cross-library FFT roundoff (rustfft vs FFTW); observed max_rel ≈
            // 30·ε_f64 → K=64 (smallest power-of-two bounding it, with margin).
            Tol::rel(64, Eps::F64, 64).assert(
                "fft_gaussian_smooth vs Octave",
                out.as_slice().expect("contiguous"),
                &exp,
            );
        }

        // (Cutover, objective 1) The frozen `interp2_spline_matches_octave` golden
        // + its i2s_*.bin fixtures + gen_interp2spline_golden.m were DELETED: the
        // live `interp2_spline_matches_genuine_octave_live` below builds the
        // identical scene (same smooth non-separable Z, U=3 upsample) and computes
        // the genuine Octave `interp2(...,'spline')` live.

        /// **Live library-primitive oracle, Octave**: our `interp2_spline`
        /// (ported not-a-knot tensor-product cubic spline) vs the GENUINE Octave
        /// `interp2(...,'spline')`, executed live. Octave's spline is the oracle;
        /// the bridge only calls it. A smooth non-separable Z, U=3 upsample (the
        /// splitPatchesX case). Gated behind `oracle_live`.
        #[cfg(feature = "oracle_live")]
        #[test]
        fn interp2_spline_matches_genuine_octave_live() {
            use crate::test_support::oracle;
            const H: usize = 12;
            const W: usize = 15;
            const U: usize = 3;
            let z = Array2::from_shape_fn((H, W), |(r, c)| {
                10.0 * (c as f64 / 3.0).sin() * (r as f64 / 4.0).cos() + 0.5 * c as f64 + 0.3 * r as f64
            });
            let x: Vec<f64> = (1..=W).map(|v| v as f64).collect();
            let y: Vec<f64> = (1..=H).map(|v| v as f64).collect();
            let xi: Vec<f64> = (0..U * W)
                .map(|k| 1.0 + (W as f64 - 1.0) * k as f64 / (U * W - 1) as f64)
                .collect();
            let yi: Vec<f64> = (0..U * H)
                .map(|k| 1.0 + (H as f64 - 1.0) * k as f64 / (U * H - 1) as f64)
                .collect();
            let xi_row = Array2::from_shape_fn((1, U * W), |(_, k)| xi[k]);
            let yi_row = Array2::from_shape_fn((1, U * H), |(_, k)| yi[k]);

            let genuine = oracle::snlc("interp2_spline", &[z.clone(), xi_row, yi_row], &[]).remove(0);
            let ours = interp2_spline(&x, &y, &z, &xi, &yi);
            Tol::rel(64, Eps::F64, 64).assert(
                "interp2_spline vs GENUINE Octave (live)",
                ours.as_slice().expect("contiguous"),
                genuine.as_slice().expect("contiguous"),
            );
            eprintln!("interp2_spline vs GENUINE Octave (live): matched");
        }



        /// **Live library-primitive oracle, Octave**: our `imimposemin` vs the
        /// GENUINE Octave IPT `imimposemin`, executed live. Octave's morphological
        /// reconstruction is the oracle; the bridge only calls it. A bumpy field
        /// with two marker minima. Gated behind `oracle_live`.
        #[cfg(feature = "oracle_live")]
        #[test]
        fn imimposemin_matches_genuine_octave_live() {
            use crate::test_support::oracle;
            const N: usize = 20;
            let im = Array2::from_shape_fn((N, N), |(r, c)| {
                (r as f64 * 0.4).sin() + (c as f64 * 0.3).cos() + 0.02 * (r * c) as f64
            });
            let bw = Array2::from_shape_fn((N, N), |(r, c)| (r, c) == (5, 5) || (r, c) == (14, 12));
            let bw_f = bw.mapv(|b| if b { 1.0 } else { 0.0 });

            let genuine = oracle::snlc("imimposemin", &[im.clone(), bw_f], &[]).remove(0);
            let ours = imimposemin(&im, &bw);
            Tol::rel(64, Eps::F64, 64).assert(
                "imimposemin vs GENUINE Octave (live)",
                ours.as_slice().expect("contiguous"),
                genuine.as_slice().expect("contiguous"),
            );
            eprintln!("imimposemin vs GENUINE Octave (live): matched");
        }

        // (Cutover, objective 1) over_rep / get_center_patch / reset_patch /
        // get_nlocalmin frozen goldens DELETED: gen_{overrep,centerpatch,
        // resetpatch,getnlocalmin} VERBATIM-transcribe splitPatchesX.m's
        // FILE-LOCAL subfunctions (not separately callable; the parent
        // splitPatchesX needs roifilt2 → can't run shim-free), so there is no
        // genuine separable reference to call — they were self-authored oracles.
        // The frozen imimposemin golden was also dropped (the live
        // imimposemin_matches_genuine_octave_live supersedes it).


        // (Cutover, objective 1+3) smooth_patches_x / split_patches_x /
        // fuse_patches_x frozen goldens DELETED: their smpatch_/spx_/fpx_
        // fixtures were generated by the genuine smoothPatchesX/splitPatchesX/
        // fusePatchesX .m run against a roifilt2 SHIM (Octave lacks roifilt2) —
        // shim-contaminated, not clean genuine-reference values. Octave can't run
        // them shim-free and we will not author a roifilt2 (forbidden circularity),
        // so rather than keep a falsely-'validated' golden they are removed; the
        // constituent primitives (watershed/bwdist/imimposemin/interp2/morphology)
        // are validated live. See oracle/README irreducible-gaps.
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
        let out = m.apply(vec![], &v, &v, &v, 20.0, &cancel).unwrap();
        assert_eq!(out.len(), 0);
    }
}
