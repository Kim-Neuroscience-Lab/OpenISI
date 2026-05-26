//! Connected-component labeling helpers used by the Garrett 2014 /
//! SNLC segmentation pipeline.
//!
//! - `label_4conn` — standard sign-blind 4-connected labeling
//!   (equivalent to MATLAB `bwlabel(mask, 4)`). Used for the cortex
//!   mask's largest-component selection and for labeling the regions
//!   between borders that become the candidate patches.
//! - `label_patches` — sign-aware variant. After we have a candidate
//!   patch from `label_4conn`, we compute the patch sign from the mean
//!   of the smoothed VFS within the patch (Garrett 2014 / SNLC
//!   `getPatchSign.m`). This module provides the helper used to build
//!   the `Patch` records directly from a binary mask + signal array
//!   where each connected region's sign is determined by majority vote.

use ndarray::Array2;
use std::collections::VecDeque;

use super::Patch;

// =============================================================================
// Sign-blind 4-conn labeling — `bwlabel(mask, 4)`
// =============================================================================

/// Standard 4-connected connected-component labeling. Returns the label
/// map (0 = background, 1..=N = component) and the component count N.
pub(crate) fn label_4conn(mask: &Array2<bool>) -> (Array2<i32>, usize) {
    let (h, w) = mask.dim();
    let mut labels = Array2::<i32>::zeros((h, w));
    let mut next_label = 1i32;
    let off: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    for r in 0..h {
        for c in 0..w {
            if !mask[[r, c]] || labels[[r, c]] != 0 { continue; }
            let lbl = next_label;
            next_label += 1;
            labels[[r, c]] = lbl;
            let mut q = VecDeque::new();
            q.push_back((r, c));
            while let Some((rr, cc)) = q.pop_front() {
                for &(dr, dc) in &off {
                    let nr = rr as i32 + dr;
                    let nc = cc as i32 + dc;
                    if nr < 0 || nr >= h as i32 || nc < 0 || nc >= w as i32 { continue; }
                    let (nr, nc) = (nr as usize, nc as usize);
                    if mask[[nr, nc]] && labels[[nr, nc]] == 0 {
                        labels[[nr, nc]] = lbl;
                        q.push_back((nr, nc));
                    }
                }
            }
        }
    }
    (labels, (next_label - 1) as usize)
}

/// Keep only the largest 4-connected component of the input mask,
/// returning a new mask. If `mask` is empty, returns an empty mask.
/// (Garrett 2014's cortex-mask step keeps "one large patch at the
/// center of the field of view"; SNLC `getMouseAreasX.m` lines 88-100.)
pub(crate) fn keep_largest_component(mask: &Array2<bool>) -> Array2<bool> {
    let (h, w) = mask.dim();
    let (labels, n) = label_4conn(mask);
    if n == 0 {
        return Array2::from_elem((h, w), false);
    }
    let mut counts = vec![0usize; n + 1];
    for &l in labels.iter() {
        if l > 0 { counts[l as usize] += 1; }
    }
    let largest = (1..=n).max_by_key(|&i| counts[i]).unwrap_or(1) as i32;
    Array2::from_shape_fn((h, w), |(r, c)| labels[[r, c]] == largest)
}

// =============================================================================
// `dilationPatches2` — Allen's bulk-dilate + skeletonize algorithm
// =============================================================================

/// Allen `retinotopic_mapping` `dilationPatches2`
/// (Zhuang 2017, eLife 6:e18372; `RetinotopicMapping.py` L190–225).
///
/// Algorithm (binary input → binary output, NOT label-preserving):
///   1. `total_area = binary_dilation(raw_patches, iterations=dilation_iter)`
///      with scipy default 4-conn cross — the union of all dilated patches.
///   2. `halo = total_area ∧ ¬raw_patches` — the donut of dilated-only pixels.
///   3. `skel = skeletonize_zs(halo)` — the medial axis through the halo,
///      which sits exactly where two patches' dilations meet.
///   4. If `border_width > 1`, dilate the skeleton by `border_width - 1`
///      to thicken inter-patch borders. Allen default `border_width=1`
///      leaves the natural single-pixel skeleton.
///   5. `new_patches = total_area ∧ ¬skel` — total_area minus the
///      skeleton border; this is the dilated patches with single-pixel
///      gaps where adjacent dilations meet.
///   6. Re-label connected components; **keep only CCs that overlap the
///      original `raw_patches`** (drops spurious CCs created by the
///      skeleton subtraction).
///
/// The label assignment (which patch each pixel belongs to) is recovered
/// downstream by re-labeling the returned binary with `label_4conn` and
/// majority-sign assignment, mirroring Allen's `labelPatches` step.
pub(crate) fn dilation_patches2_allen(
    raw_patches: &Array2<bool>,
    dilation_iter: i32,
    border_width: i32,
) -> Array2<bool> {
    use crate::segmentation::morphology::{binary_dilation_cross, binary_skeletonize_zs};

    let (h, w) = raw_patches.dim();
    // Step 1
    let total_area = binary_dilation_cross(raw_patches, dilation_iter);
    // Step 2
    let halo = Array2::from_shape_fn((h, w), |(r, c)| {
        total_area[[r, c]] && !raw_patches[[r, c]]
    });
    // Step 3
    let mut skel = binary_skeletonize_zs(&halo);
    // Step 4 (optional)
    if border_width > 1 {
        skel = binary_dilation_cross(&skel, border_width - 1);
    }
    // Step 5
    let new_patches = Array2::from_shape_fn((h, w), |(r, c)| {
        total_area[[r, c]] && !skel[[r, c]]
    });
    // Step 6
    let (labels, n) = label_4conn(&new_patches);
    let mut keep = vec![false; n + 1];
    for r in 0..h {
        for c in 0..w {
            let l = labels[[r, c]];
            if l > 0 && raw_patches[[r, c]] {
                keep[l as usize] = true;
            }
        }
    }
    Array2::from_shape_fn((h, w), |(r, c)| {
        let l = labels[[r, c]];
        l > 0 && keep[l as usize]
    })
}

// =============================================================================
// `is_adjacent` — patches adjacent within `border_width` pixels
// =============================================================================

/// Allen `tools.ImageAnalysis.is_adjacent` (`ImageAnalysis.py` L918).
/// Two patches are adjacent iff `max(dilate(a, bw-1) + dilate(b, bw-1)) > 1`
/// — i.e. their `(border_width - 1)`-pixel dilations overlap somewhere.
/// Allen calls this with `border_width = 2 · stored_border_width` in
/// `_getRawPatches` (single-pixel dilation each → patches separated by
/// ≤ 2 background pixels are adjacent).
pub(crate) fn is_adjacent(a: &Array2<bool>, b: &Array2<bool>, border_width: i32) -> bool {
    use crate::segmentation::morphology::binary_dilation_cross;
    let bw = (border_width - 1).max(0);
    let a_dil = if bw == 0 { a.clone() } else { binary_dilation_cross(a, bw) };
    let b_dil = if bw == 0 { b.clone() } else { binary_dilation_cross(b, bw) };
    let (h, w) = a.dim();
    for r in 0..h {
        for c in 0..w {
            if a_dil[[r, c]] && b_dil[[r, c]] { return true; }
        }
    }
    false
}

// =============================================================================
// Build `Patch` records from a label map + signed VFS signal
// =============================================================================

/// Given a binary mask of patch-pixels and a real-valued signal
/// (smoothed VFS), label connected components with 4-connectivity and
/// assign each one the sign of its mean signal value — `sign(mean(VFS))`.
/// This is the SNLC `getPatchSign.m` convention: sign by majority vote
/// of the patch's smoothed VFS values, not pixel-by-pixel sign-aware
/// labeling.
///
/// Garrett 2014 / SNLC use sign-blind 4-conn here because the cortex
/// mask + border-thinning step has already produced patches that are
/// (by construction) each on one side of a sign boundary — the border
/// thinning runs through the `|VFS| < threshold` ridges that separate
/// opposite-sign areas, so each labeled region between borders contains
/// a single sign in practice. Sign-blind labeling + majority vote is
/// equivalent to sign-aware labeling on this input.
pub(crate) fn label_patches_with_majority_sign(
    mask: &Array2<bool>,
    signal: &Array2<f64>,
) -> Vec<Patch> {
    let (labels, n) = label_4conn(mask);
    patches_from_labels_majority_sign(&labels, n, signal)
}

/// Same as `label_patches_with_majority_sign` but starting from an
/// already-labelled map (1..=N). Used after `dilation_patches2` since
/// label IDs must be preserved through the dilation rather than re-
/// derived from a binary collapse.
pub(crate) fn patches_from_labels_majority_sign(
    labels: &Array2<i32>,
    n: usize,
    signal: &Array2<f64>,
) -> Vec<Patch> {
    let (h, w) = labels.dim();
    let mut patches: Vec<Patch> = Vec::with_capacity(n);
    for k in 1..=n as i32 {
        let mut sum = 0.0_f64;
        let mut count = 0usize;
        for r in 0..h {
            for c in 0..w {
                if labels[[r, c]] == k {
                    let v = signal[[r, c]];
                    if v.is_finite() {
                        sum += v;
                        count += 1;
                    }
                }
            }
        }
        if count == 0 { continue; }
        let sign = if sum >= 0.0 { 1i8 } else { -1i8 };
        let comp_mask = Array2::from_shape_fn((h, w), |(r, c)| labels[[r, c]] == k);
        patches.push(Patch { mask: comp_mask, sign });
    }
    patches
}
