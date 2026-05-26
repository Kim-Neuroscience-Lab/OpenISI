//! Visual area segmentation — Garrett 2014 / SNLC port.
//!
//! Verbatim port of the algorithm in:
//!
//!   - Garrett, Nauhaus, Marshel & Callaway 2014 J Neurosci, PMC4160785.
//!     "Topography and areal organization of mouse visual cortex."
//!     Methods section quote: *"we created discrete patches on the
//!     cortex by thresholding S at ±1.5 times its SD"* → *"morphological
//!     opening on S_Thresh eliminated [noise pixels]"* → *"morphological
//!     'closing' on |S_Thresh|, followed by 'opening,' followed by
//!     'dilation'"* (cortex boundary) → *"we recomputed areal borders
//!     using morphological 'thinning,' iterating to infinity."*
//!
//!   - SNLC ISI MATLAB `getMouseAreasX.m` — the published implementation
//!     of the algorithm (same lab lineage; this is the code that was
//!     used to produce the validation figures and is the source of the
//!     L54/R43/R44 sample data shipped in `SNLC/ISI/Sample Data.zip`).
//!
//! **Pipeline:**
//!
//!   1. Smooth the signed VFS with a Gaussian of σ = `vfs_smoothing_sigma`
//!      (Garrett 2014 used σ = 3 pixels).
//!   2. Compute the data-driven threshold `thr = threshold_k × std(VFS)`
//!      (Garrett 2014 used K = 1.5). The patch mask is
//!      `|VFS_smoothed| ≥ thr/2` (SNLC's `imseg`).
//!   3. Open the threshold mask with a small disk SE
//!      (`open_radius`, SNLC = 2) to remove salt-and-pepper noise.
//!   4. **Build the cortex mask** (Garrett 2014: *"closing… opening…
//!      dilation"*; SNLC `imbound`):
//!         - `imclose` with disk of radius `cortex_close_radius` (SNLC = 10)
//!         - fill holes (`imfill`)
//!         - `imdilate` with disk of radius `cortex_dilate_radius` (SNLC = 3)
//!         - fill holes again
//!         - keep the single largest 4-connected component
//!   5. Construct the inter-patch border within cortex:
//!      `border = cortex_mask − threshold_mask`, then thin to a 1-px
//!      skeleton (`bwmorph thin Inf`), then prune endpoint chains shorter
//!      than `border_spur_iter` pixels (`bwmorph spur N`, SNLC = 4).
//!   6. Patches = `bwlabel(1 − border, 4)`, dropping the surround label.
//!   7. Filter patches with area < `small_patch_thr`.
//!   8. Assign each patch the sign of `mean(VFS_smoothed)` over its mask
//!      (SNLC `getPatchSign.m`).
//!
//! **Deliberately NOT here**: the Juavinett 2017 / Allen
//! `dilation_patches2` skeleton-on-patches step. That step is what
//! produced our V1-slice artifact on concave V1 shapes — its dilation
//! merges patches into a single mass and the resulting skeleton can
//! traverse a patch's concave neck. Garrett 2014 instead constructs the
//! cortex boundary first and computes the border as the *gap inside
//! cortex between patches*, which is robust to concave patch shapes.
//!
//! **Not yet ported**: `splitPatchesX.m` (Jacobian-based redundant-
//! coverage splitting) and `fusePatchesX.m` (same-sign adjacency
//! fusing). These are downstream refinement passes; the base pipeline
//! above is what defines patches.

pub(crate) mod connectivity;
pub(crate) mod morphology;

use ndarray::Array2;

use connectivity::keep_largest_component;
pub use morphology::gaussian_smooth_f64;

use morphology::binary_fill_holes;

// =============================================================================
// Public types
// =============================================================================

/// One labelled patch on the cortex.
#[derive(Clone)]
pub struct Patch {
    pub mask: Array2<bool>,
    pub sign: i8,
}

impl Patch {
    pub fn area(&self) -> usize {
        self.mask.iter().filter(|&&x| x).count()
    }
}

// =============================================================================
// Cortex mask derivation from cross-cycle reliability — Allen / Engel
// =============================================================================

/// Build the cortex mask from per-direction reliability maps. The
/// cortex is the largest connected component of the intersection
/// `min_k(reliability_k) > threshold` — i.e., pixels where *every*
/// direction has a phasor that's repeatable across cycles. This is
/// the Allen Brain Observatory criterion (Zhuang 2017) and is
/// conceptually identical to Engel 1994's coherence threshold from
/// fMRI retinotopy.
///
/// Cleanup is `largest_cc → fill_holes` only — no morphology that
/// expands the boundary past quality-passing pixels. The cortex
/// mask is therefore an exact subset of the quality-passing region,
/// minus orphan blobs and with small interior holes filled.
pub fn cortex_from_reliability(
    rel_azi_fwd: &Array2<f64>,
    rel_azi_rev: &Array2<f64>,
    rel_alt_fwd: &Array2<f64>,
    rel_alt_rev: &Array2<f64>,
    threshold: f64,
) -> Array2<bool> {
    let (h, w) = rel_azi_fwd.dim();
    let raw = Array2::from_shape_fn((h, w), |(r, c)| {
        let min_rel = rel_azi_fwd[[r, c]]
            .min(rel_azi_rev[[r, c]])
            .min(rel_alt_fwd[[r, c]])
            .min(rel_alt_rev[[r, c]]);
        min_rel.is_finite() && min_rel > threshold
    });
    // Reliability-derived cortex: minimal cleanup (largest_cc +
    // fill_holes). Reliability is already a coherent quality metric;
    // we don't apply Allen mskBound morphology (closing/dilation)
    // because reliability doesn't suffer from the threshold
    // self-cancellation that motivates them.
    let largest = keep_largest_component(&raw);
    binary_fill_holes(&largest)
}

// =============================================================================
// Helpers
// =============================================================================

pub(crate) fn extract_label_borders(labels: &Array2<i32>) -> Array2<bool> {
    let (h, w) = labels.dim();
    let off: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    Array2::from_shape_fn((h, w), |(r, c)| {
        let l = labels[[r, c]];
        if l == 0 { return false; }
        off.iter().any(|&(dr, dc)| {
            let nr = r as i32 + dr;
            let nc = c as i32 + dc;
            if nr < 0 || nr >= h as i32 || nc < 0 || nc >= w as i32 { return true; }
            labels[[nr as usize, nc as usize]] != l
        })
    })
}

// =============================================================================
// Public diagnostic — threshold-only segmentation (for sweep grid)
// =============================================================================

/// Same as `segment_visual_areas` but with the absolute threshold
/// value supplied directly (Allen `signMapThr`). Used by the
/// `--threshold-sweep` diagnostic to compare different absolute
/// threshold values against a fixed (reliability-derived or user-
/// drawn) cortex.
pub fn segment_threshold_only(
    vfs_smooth: &Array2<f64>,
    cortex_mask: &Array2<bool>,
    threshold: f64,
    small_patch_thr: usize,
) -> (Array2<i32>, Vec<i8>) {
    use morphology::binary_opening_disk;
    use connectivity::label_patches_with_majority_sign;

    let (h, w) = vfs_smooth.dim();
    let imseg = Array2::from_shape_fn((h, w), |(r, c)| {
        let v = vfs_smooth[[r, c]];
        cortex_mask[[r, c]] && v.is_finite() && v.abs() >= threshold
    });
    let imseg = binary_opening_disk(&imseg, 3);
    let mut patches = label_patches_with_majority_sign(&imseg, vfs_smooth);
    patches.retain(|p| p.area() >= small_patch_thr);
    patches.sort_by(|a, b| b.area().cmp(&a.area()));

    let mut area_labels = Array2::<i32>::zeros((h, w));
    let mut area_signs: Vec<i8> = Vec::with_capacity(patches.len());
    for (i, patch) in patches.iter().enumerate() {
        let label = (i + 1) as i32;
        for r in 0..h {
            for c in 0..w {
                if patch.mask[[r, c]] { area_labels[[r, c]] = label; }
            }
        }
        area_signs.push(patch.sign);
    }
    (area_labels, area_signs)
}

