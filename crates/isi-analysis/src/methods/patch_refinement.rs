//! Stage 8 — Patch refinement (split + merge).
//!
//! Given raw patches from the extraction stage, optionally apply split
//! and merge passes to refine multi-area blobs into canonical visual
//! areas (~7 for typical mouse retinotopy).

use ndarray::Array2;
use openisi_params::{
    PatchRefinementAllenBorderWidth, PatchRefinementAllenEccMapFilterSigma,
    PatchRefinementAllenMergeOverlapThr, PatchRefinementAllenSmallPatchThr,
    PatchRefinementAllenSplitLocalMinCutStep, PatchRefinementAllenSplitOverlapThr,
    PatchRefinementAllenVisualSpaceCloseIter, PatchRefinementAllenVisualSpacePixelSize, Tagged,
};

use crate::segmentation::Patch;

/// Method choice for patch refinement.
///
/// `#[non_exhaustive]` + per-variant constructors enforce registry-
/// sourced tunables.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum PatchRefinementMethod {
    /// No refinement. Raw patches pass through unchanged.
    None,

    /// Allen `_splitPatches` + `_mergePatches` (Zhuang 2017, eLife
    /// 6:e18372; `RetinotopicMapping.py` L1247–1370 and L1371–1527).
    AllenZhuang2017SplitMerge {
        split_overlap_thr: f64,
        split_local_min_cut_step: f64,
        merge_overlap_thr: f64,
        visual_space_pixel_size: f64,
        visual_space_close_iter: i32,
        ecc_map_filter_sigma: i32,
        border_width: i32,
        small_patch_thr: usize,
    },
}

impl PatchRefinementMethod {
    pub fn none() -> Self {
        Self::None
    }

    pub fn allen_zhuang2017_split_merge(
        split_overlap_thr: Tagged<PatchRefinementAllenSplitOverlapThr>,
        split_local_min_cut_step: Tagged<PatchRefinementAllenSplitLocalMinCutStep>,
        merge_overlap_thr: Tagged<PatchRefinementAllenMergeOverlapThr>,
        visual_space_pixel_size: Tagged<PatchRefinementAllenVisualSpacePixelSize>,
        visual_space_close_iter: Tagged<PatchRefinementAllenVisualSpaceCloseIter>,
        ecc_map_filter_sigma: Tagged<PatchRefinementAllenEccMapFilterSigma>,
        border_width: Tagged<PatchRefinementAllenBorderWidth>,
        small_patch_thr: Tagged<PatchRefinementAllenSmallPatchThr>,
    ) -> Self {
        Self::AllenZhuang2017SplitMerge {
            split_overlap_thr: split_overlap_thr.into_inner(),
            split_local_min_cut_step: split_local_min_cut_step.into_inner(),
            merge_overlap_thr: merge_overlap_thr.into_inner(),
            visual_space_pixel_size: visual_space_pixel_size.into_inner(),
            visual_space_close_iter: visual_space_close_iter.into_inner(),
            ecc_map_filter_sigma: ecc_map_filter_sigma.into_inner(),
            border_width: border_width.into_inner(),
            small_patch_thr: small_patch_thr.into_inner(),
        }
    }
}

impl PatchRefinementMethod {
    /// Apply the refinement. `determinant_map` is `|det(grad)|` of the
    /// position-in-visual-angle maps (= our `magnification_raw`).
    /// `azi_position_deg` / `alt_position_deg` are positions in visual-
    /// angle degrees (= our `azi_phase_degrees` / `alt_phase_degrees`,
    /// which `phase_to_degrees` produces in visual-angle units).
    pub fn apply(
        &self,
        patches: Vec<Patch>,
        azi_position_deg: &Array2<f64>,
        alt_position_deg: &Array2<f64>,
        determinant_map: &Array2<f64>,
    ) -> Vec<Patch> {
        match self {
            Self::None => patches,
            Self::AllenZhuang2017SplitMerge {
                split_overlap_thr,
                split_local_min_cut_step,
                merge_overlap_thr,
                visual_space_pixel_size,
                visual_space_close_iter,
                ecc_map_filter_sigma,
                border_width,
                small_patch_thr,
            } => allen::run_split_merge(
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
            ),
        }
    }
}

// =============================================================================
// Allen split/merge implementation
// =============================================================================

mod allen {
    use ndarray::Array2;

    use crate::segmentation::Patch;
    use crate::segmentation::connectivity::{is_adjacent, label_4conn};
    use crate::segmentation::morphology::{binary_closing_cross, binary_skeletonize_zs};

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
    ) -> Vec<Patch> {
        // Derive visual-space grid extents from the data.
        let grid = derive_visual_grid(alt, azi, p.visual_space_pixel_size);

        // -------- SPLIT --------
        let mut after_split: Vec<Patch> = Vec::with_capacity(patches.len());
        for patch in patches {
            let (_vs, au) = patch_visual_space(
                &patch.mask, azi, alt, &grid, p.visual_space_close_iter,
            );
            let as_area = sigma_area(&patch.mask, det_map);
            if au > 1e-9 && as_area / au >= p.split_overlap_thr {
                let split_into = split_patch(
                    &patch, azi, alt, p.split_local_min_cut_step,
                    p.ecc_map_filter_sigma, p.border_width,
                );
                if split_into.len() >= 2 {
                    after_split.extend(split_into);
                } else {
                    after_split.push(patch);
                }
            } else {
                after_split.push(patch);
            }
        }

        // -------- MERGE --------
        let mut current = after_split;
        loop {
            let n = current.len();
            // Find adjacent same-sign pairs (Allen calls with
            // borderWidth+1 — i.e. 1-pixel dilation of each).
            let adj_width = p.border_width + 1;
            let mut candidates: Vec<MergeCandidate> = Vec::new();
            for i in 0..n {
                for j in (i + 1)..n {
                    if current[i].sign != current[j].sign { continue; }
                    if !is_adjacent(&current[i].mask, &current[j].mask, adj_width) {
                        continue;
                    }
                    let merged_mask = merge_two(&current[i].mask, &current[j].mask, p.border_width);
                    let merged_mask = match merged_mask {
                        Some(m) => m,
                        None => continue, // too far apart even with closing
                    };
                    let (vs1, au1) = patch_visual_space(
                        &current[i].mask, azi, alt, &grid, p.visual_space_close_iter,
                    );
                    let (vs2, au2) = patch_visual_space(
                        &current[j].mask, azi, alt, &grid, p.visual_space_close_iter,
                    );
                    let (_vsm, au_m) = patch_visual_space(
                        &merged_mask, azi, alt, &grid, p.visual_space_close_iter,
                    );
                    if au1 < 1e-9 || au2 < 1e-9 { continue; }
                    let a_overlap = visual_space_overlap(&vs1, &vs2, p.visual_space_pixel_size);
                    let r1 = a_overlap / au1;
                    let r2 = a_overlap / au2;
                    if r1 <= p.merge_overlap_thr && r2 <= p.merge_overlap_thr {
                        candidates.push(MergeCandidate {
                            i, j,
                            merged_mask,
                            sign: current[i].sign,
                            max_ratio: r1.max(r2),
                            neg_au: -au_m,
                        });
                    }
                }
            }
            if candidates.is_empty() { break; }

            // Sort: max_ratio ascending, then neg_au ascending
            // (= au descending → bigger merges first when ratios tie).
            candidates.sort_by(|a, b| {
                a.max_ratio.partial_cmp(&b.max_ratio).unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.neg_au.partial_cmp(&b.neg_au).unwrap_or(std::cmp::Ordering::Equal))
            });

            // Greedy apply — skip any candidate whose indices have
            // already been consumed this iteration.
            let mut consumed = vec![false; n];
            let mut next_patches: Vec<Patch> = Vec::with_capacity(n);
            for cand in &candidates {
                if consumed[cand.i] || consumed[cand.j] { continue; }
                consumed[cand.i] = true;
                consumed[cand.j] = true;
                next_patches.push(Patch { mask: cand.merged_mask.clone(), sign: cand.sign });
            }
            // Carry through patches that weren't merged this round.
            for (idx, patch) in current.into_iter().enumerate() {
                if !consumed[idx] { next_patches.push(patch); }
            }
            current = next_patches;
        }

        // Final small-patch cull.
        current.retain(|p2| p2.area() >= p.small_patch_thr);
        current
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
                    if a < alt_min { alt_min = a; }
                    if a > alt_max { alt_max = a; }
                    if z < azi_min { azi_min = z; }
                    if z > azi_max { azi_max = z; }
                }
            }
        }
        if !alt_min.is_finite() {
            return VisualGrid { alt_min: 0.0, azi_min: 0.0, pixel_size, h: 1, w: 1 };
        }
        let pad = pixel_size;
        let alt_lo = alt_min - pad;
        let alt_hi = alt_max + pad;
        let azi_lo = azi_min - pad;
        let azi_hi = azi_max + pad;
        let h_v = ((alt_hi - alt_lo) / pixel_size).ceil().max(1.0) as usize;
        let w_v = ((azi_hi - azi_lo) / pixel_size).ceil().max(1.0) as usize;
        VisualGrid { alt_min: alt_lo, azi_min: azi_lo, pixel_size, h: h_v, w: w_v }
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
                if !patch_mask[[r, c]] { continue; }
                let a = alt[[r, c]];
                let z = azi[[r, c]];
                if !a.is_finite() || !z.is_finite() { continue; }
                let i_a = ((a - grid.alt_min) / grid.pixel_size).floor();
                let i_z = ((z - grid.azi_min) / grid.pixel_size).floor();
                if i_a < 0.0 || i_z < 0.0 { continue; }
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

    pub(super) fn visual_space_overlap(
        a: &Array2<bool>,
        b: &Array2<bool>,
        pixel_size: f64,
    ) -> f64 {
        let (h, w) = a.dim();
        let mut n = 0usize;
        for r in 0..h {
            for c in 0..w {
                if a[[r, c]] && b[[r, c]] { n += 1; }
            }
        }
        n as f64 * pixel_size * pixel_size
    }

    // -------------------------------------------------------------------------
    // Sigma area (`getSigmaArea`)
    // -------------------------------------------------------------------------

    pub(super) fn sigma_area(patch_mask: &Array2<bool>, det_map: &Array2<f64>) -> f64 {
        let (h, w) = patch_mask.dim();
        let mut s = 0.0_f64;
        for r in 0..h {
            for c in 0..w {
                if patch_mask[[r, c]] {
                    let v = det_map[[r, c]];
                    if v.is_finite() { s += v; }
                }
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
                if !a.is_finite() || !z.is_finite() { continue; }
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
            if patch_mask[[r, c]] { arr[[r, c]] } else { f64::NAN }
        })
    }

    /// Mean (alt, azi) within `patch_mask`.
    fn patch_visual_center(
        patch_mask: &Array2<bool>,
        azi: &Array2<f64>,
        alt: &Array2<f64>,
    ) -> (f64, f64) {
        let (h, w) = patch_mask.dim();
        let mut sum_alt = 0.0;
        let mut sum_azi = 0.0;
        let mut n = 0usize;
        for r in 0..h {
            for c in 0..w {
                if !patch_mask[[r, c]] { continue; }
                let a = alt[[r, c]];
                let z = azi[[r, c]];
                if a.is_finite() && z.is_finite() {
                    sum_alt += a;
                    sum_azi += z;
                    n += 1;
                }
            }
        }
        if n == 0 { return (0.0, 0.0); }
        (sum_alt / n as f64, sum_azi / n as f64)
    }

    /// `scipy.ndimage.uniform_filter(arr, size)` over finite values
    /// only — NaN-aware. Used on per-patch ecc maps before
    /// `localMin` (Allen `eccMapFilterSigma`).
    fn uniform_filter_finite(arr: &Array2<f64>, size: i32) -> Array2<f64> {
        if size <= 1 { return arr.clone(); }
        let radius = (size / 2).max(1);
        let (h, w) = arr.dim();
        let mut out = Array2::<f64>::from_elem((h, w), f64::NAN);
        for r in 0..h {
            for c in 0..w {
                let mut s = 0.0;
                let mut n = 0usize;
                for dr in -radius..=radius {
                    for dc in -radius..=radius {
                        let rr = r as i32 + dr;
                        let cc = c as i32 + dc;
                        if rr < 0 || rr >= h as i32 || cc < 0 || cc >= w as i32 { continue; }
                        let v = arr[[rr as usize, cc as usize]];
                        if v.is_finite() { s += v; n += 1; }
                    }
                }
                if n > 0 { out[[r, c]] = s / n as f64; }
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
                    if v < vmin { vmin = v; }
                    if v > vmax { vmax = v; }
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
            if n >= 2 { break; }
            cut += bin_size;
        }
        last_marker
    }

    // -------------------------------------------------------------------------
    // Watershed (marker-based, 8-conn)
    // -------------------------------------------------------------------------

    /// Marker-based watershed by immersion. Mirrors
    /// `skimage.morphology.watershed(elevation, markers, connectivity=8,
    /// mask=...)`. Processes pixels in ascending elevation order; each
    /// unlabelled pixel takes the unique label of its already-labelled
    /// 8-neighbors. Pixels touching multiple labels become the watershed
    /// boundary (kept as 0). Iterates until no changes.
    fn watershed_from_markers(
        elevation: &Array2<f64>,
        markers: &Array2<i32>,
        mask: &Array2<bool>,
    ) -> Array2<i32> {
        let (h, w) = elevation.dim();
        let mut labels = markers.clone();
        // Sort non-marker mask pixels by elevation.
        let mut items: Vec<(f64, usize, usize)> = Vec::new();
        for r in 0..h {
            for c in 0..w {
                if !mask[[r, c]] { continue; }
                if markers[[r, c]] > 0 { continue; }
                let v = elevation[[r, c]];
                if v.is_finite() { items.push((v, r, c)); }
            }
        }
        items.sort_by(|a, b| {
            a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)
        });
        loop {
            let mut changed = false;
            for &(_, r, c) in &items {
                if labels[[r, c]] > 0 { continue; }
                let mut candidate: i32 = 0;
                let mut watershed_line = false;
                for dr in -1i32..=1 {
                    if watershed_line { break; }
                    for dc in -1i32..=1 {
                        if dr == 0 && dc == 0 { continue; }
                        let rr = r as i32 + dr;
                        let cc = c as i32 + dc;
                        if rr < 0 || rr >= h as i32 || cc < 0 || cc >= w as i32 { continue; }
                        let nbr = labels[[rr as usize, cc as usize]];
                        if nbr <= 0 { continue; }
                        if candidate == 0 {
                            candidate = nbr;
                        } else if candidate != nbr {
                            watershed_line = true;
                            break;
                        }
                    }
                }
                if !watershed_line && candidate > 0 {
                    labels[[r, c]] = candidate;
                    changed = true;
                }
            }
            if !changed { break; }
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
        let markers = local_min_markers(&ecc_f, cut_step);
        let n_min = markers.iter().copied().max().unwrap_or(0);
        if n_min < 2 {
            return vec![patch.clone()];
        }
        // Watershed within the patch mask.
        let watershed = watershed_from_markers(&ecc_f, &markers, &patch.mask);
        // Build per-region masks from the watershed labels and
        // include the watershed-boundary subtraction: Allen's split2
        // builds borders by skeletonizing each labelled region's
        // boundary; if border_width > 1, dilate. Then subtract those
        // borders from the patch mask.
        let (h, w) = patch.mask.dim();
        let mut all_borders = Array2::<bool>::from_elem((h, w), false);
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
        let mut border = binary_skeletonize_zs(&all_borders);
        if border_width > 1 {
            border = crate::segmentation::morphology::binary_dilation_cross(
                &border, border_width - 1,
            );
        }
        // new_patches = dilate(patch.mask, 1) AND NOT border
        // — Allen does `binary_dilation(self.array)`; we mirror that.
        let dil_patch = crate::segmentation::morphology::binary_dilation_cross(&patch.mask, 1);
        let new_patches_bin = Array2::from_shape_fn((h, w), |(r, c)| {
            dil_patch[[r, c]] && !border[[r, c]]
        });
        let (labeled, n) = label_4conn(&new_patches_bin);
        let mut out: Vec<Patch> = Vec::with_capacity(n);
        for k in 1..=n as i32 {
            let curr = Array2::from_shape_fn((h, w), |(r, c)| {
                labeled[[r, c]] == k && patch.mask[[r, c]]
            });
            if curr.iter().any(|&b| b) {
                out.push(Patch { mask: curr, sign: patch.sign });
            }
        }
        if out.is_empty() { vec![patch.clone()] } else { out }
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
        if n == 1 { Some(merged) } else { None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_passes_through_unchanged() {
        let v = Array2::<f64>::zeros((4, 4));
        let m = PatchRefinementMethod::None;
        let out = m.apply(vec![], &v, &v, &v);
        assert_eq!(out.len(), 0);
    }
}
