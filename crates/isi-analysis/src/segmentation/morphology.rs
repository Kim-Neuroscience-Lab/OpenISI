//! Morphological primitives used by the Garrett 2014 / SNLC and Allen
//! Zhuang 2017 segmentation pipelines.
//!
//! Two SE families are needed:
//!
//! - **Disk SE** (`*_disk` functions). Garrett 2014 (J Neurosci PMC4160785)
//!   and the SNLC ISI MATLAB toolbox (`getMouseAreasX.m`) use MATLAB
//!   `strel('disk', R, 0)` — a flat Euclidean disk of radius R pixels
//!   (29 pixels at R=3). Used by `CortexSource::SnlcGarrett2014ImBound`.
//!
//! - **Cross-iter SE** (`*_cross` functions). Allen
//!   `retinotopic_mapping` (Zhuang 2017, eLife) uses scipy.ndimage
//!   `binary_opening(..., iterations=N)` whose default structure is the
//!   4-connected cross. N iterations of cross dilation/erosion is
//!   equivalent to a Manhattan-disk (diamond) SE of radius N (13 pixels
//!   at N=3). Used by `PatchExtractionMethod::AllenZhuang2017*`.
//!
//! Disk-3 and cross-3-iter are NOT interchangeable: disk-3 erodes/dilates
//! roughly twice as aggressively as cross-3-iter on the same mask.
//!
//! Operations:
//!
//!   - `gaussian_smooth_f64` — plain Gaussian (Garrett 2014 smooths the
//!     VFS with a fixed-σ Gaussian before thresholding).
//!   - `binary_dilation_disk` / `binary_erosion_disk` — Minkowski sum /
//!     difference with a flat circular SE of radius `r` pixels.
//!   - `binary_opening_disk` / `binary_closing_disk` — open = erode then
//!     dilate; close = dilate then erode.
//!   - `binary_fill_holes` — `imfill`-equivalent: any background component
//!     not touching the image edge becomes foreground.
//!   - `skeletonize_zs` — Zhang-Suen iterative thinning (`bwmorph(...,
//!     'thin', Inf)` equivalent).
//!   - `spur_prune` — `bwmorph(..., 'spur', n)`: iteratively delete
//!     endpoint pixels (8-conn pixels with ≤1 neighbor) up to `n` times.
//!
//! The amp-weighted normalized-convolution variant used during phase
//! smoothing now lives on-device as `compute::amp_weighted_complex_smooth`
//! and operates on the complex phasor representation directly; it is no
//! longer needed here.

use ndarray::Array2;

// =============================================================================
// Gaussian smoothing
// =============================================================================

/// Plain Gaussian smoothing of a real-valued map at σ pixels. Border
/// handling is the same as `math::separable_filter`'s (reflective).
pub fn gaussian_smooth_f64(data: &Array2<f64>, sigma: f64) -> Array2<f64> {
    if sigma <= 0.0 { return data.clone(); }
    let radius = (sigma * 3.0).ceil() as usize;
    let kernel = crate::math::gaussian_kernel_1d(sigma, radius);
    crate::math::separable_filter(data, &kernel)
}

// =============================================================================
// Disk structuring element
// =============================================================================

/// Generate the offsets `(dr, dc)` for a flat disk SE of radius `r`.
/// Matches MATLAB `strel('disk', r, 0)` for r ≥ 1 (the trailing `0`
/// argument disables MATLAB's decomposition into line SEs, so the disk
/// is exact, not the 4-segment approximation). For r = 0 returns just
/// the origin.
fn disk_offsets(radius: i32) -> Vec<(i32, i32)> {
    if radius <= 0 { return vec![(0, 0)]; }
    let mut offs = Vec::with_capacity(((2 * radius + 1).pow(2)) as usize);
    let r2 = (radius * radius) as f64;
    for dr in -radius..=radius {
        for dc in -radius..=radius {
            // Use the "inside or on the circle" test: dr² + dc² ≤ r²
            // (matches `strel('disk', r, 0)` for the simple flat case).
            if (dr * dr + dc * dc) as f64 <= r2 + 1e-9 {
                offs.push((dr, dc));
            }
        }
    }
    offs
}

// =============================================================================
// Binary morphology with disk SE
// =============================================================================

pub(crate) fn binary_dilation_disk(mask: &Array2<bool>, radius: i32) -> Array2<bool> {
    if radius <= 0 { return mask.clone(); }
    let (h, w) = mask.dim();
    let offs = disk_offsets(radius);
    let mut out = Array2::<bool>::from_elem((h, w), false);
    for r in 0..h {
        for c in 0..w {
            // Set out[r,c] = true iff any pixel in mask within the disk
            // (centered at r,c) is true. Equivalent to Minkowski sum
            // with the SE.
            let mut any = false;
            for &(dr, dc) in &offs {
                let rr = r as i32 + dr;
                let cc = c as i32 + dc;
                if rr < 0 || rr >= h as i32 || cc < 0 || cc >= w as i32 { continue; }
                if mask[[rr as usize, cc as usize]] { any = true; break; }
            }
            out[[r, c]] = any;
        }
    }
    out
}

pub(super) fn binary_erosion_disk(mask: &Array2<bool>, radius: i32) -> Array2<bool> {
    if radius <= 0 { return mask.clone(); }
    let (h, w) = mask.dim();
    let offs = disk_offsets(radius);
    let mut out = Array2::<bool>::from_elem((h, w), false);
    for r in 0..h {
        for c in 0..w {
            // out[r,c] = true iff all pixels in mask within the disk
            // are true (and the disk is fully inside the image). Border
            // pixels treated as outside (= false), so they erode.
            let mut all = true;
            for &(dr, dc) in &offs {
                let rr = r as i32 + dr;
                let cc = c as i32 + dc;
                if rr < 0 || rr >= h as i32 || cc < 0 || cc >= w as i32 {
                    all = false; break;
                }
                if !mask[[rr as usize, cc as usize]] { all = false; break; }
            }
            out[[r, c]] = all;
        }
    }
    out
}

pub(crate) fn binary_opening_disk(mask: &Array2<bool>, radius: i32) -> Array2<bool> {
    let eroded = binary_erosion_disk(mask, radius);
    binary_dilation_disk(&eroded, radius)
}

pub(crate) fn binary_closing_disk(mask: &Array2<bool>, radius: i32) -> Array2<bool> {
    if radius <= 0 { return mask.clone(); }
    let dilated = binary_dilation_disk(mask, radius);
    binary_erosion_disk(&dilated, radius)
}

// =============================================================================
// Iterative cross (4-conn) morphology — Allen scipy.ndimage equivalent
// =============================================================================

/// `iterations` iterations of binary dilation with a 4-connected cross
/// SE. Mirrors `scipy.ndimage.binary_dilation(mask, iterations=N)` with
/// scipy's default 2D structure (which is the 4-conn cross). Used by
/// `PatchExtractionMethod::AllenZhuang2017*` for Allen-faithful
/// `openIter` / `closeIter` semantics.
pub(crate) fn binary_dilation_cross(mask: &Array2<bool>, iterations: i32) -> Array2<bool> {
    if iterations <= 0 { return mask.clone(); }
    let (h, w) = mask.dim();
    let mut cur = mask.clone();
    for _ in 0..iterations {
        let mut next = Array2::<bool>::from_elem((h, w), false);
        for r in 0..h {
            for c in 0..w {
                if cur[[r, c]] { next[[r, c]] = true; continue; }
                let n = (r > 0 && cur[[r - 1, c]])
                    || (r + 1 < h && cur[[r + 1, c]])
                    || (c > 0 && cur[[r, c - 1]])
                    || (c + 1 < w && cur[[r, c + 1]]);
                if n { next[[r, c]] = true; }
            }
        }
        cur = next;
    }
    cur
}

/// `iterations` iterations of binary erosion with a 4-connected cross
/// SE. Mirrors scipy's `binary_erosion(mask, iterations=N)` with the
/// default cross structure. Border pixels missing a 4-conn neighbor
/// erode (border_value defaults to 0 in scipy).
pub(crate) fn binary_erosion_cross(mask: &Array2<bool>, iterations: i32) -> Array2<bool> {
    if iterations <= 0 { return mask.clone(); }
    let (h, w) = mask.dim();
    let mut cur = mask.clone();
    for _ in 0..iterations {
        let mut next = Array2::<bool>::from_elem((h, w), false);
        for r in 0..h {
            for c in 0..w {
                if !cur[[r, c]] { continue; }
                let all_in = r > 0 && cur[[r - 1, c]]
                    && r + 1 < h && cur[[r + 1, c]]
                    && c > 0 && cur[[r, c - 1]]
                    && c + 1 < w && cur[[r, c + 1]];
                if all_in { next[[r, c]] = true; }
            }
        }
        cur = next;
    }
    cur
}

pub(crate) fn binary_opening_cross(mask: &Array2<bool>, iterations: i32) -> Array2<bool> {
    let eroded = binary_erosion_cross(mask, iterations);
    binary_dilation_cross(&eroded, iterations)
}

pub(crate) fn binary_closing_cross(mask: &Array2<bool>, iterations: i32) -> Array2<bool> {
    let dilated = binary_dilation_cross(mask, iterations);
    binary_erosion_cross(&dilated, iterations)
}

// =============================================================================
// Zhang-Suen iterative thinning — skimage.morphology.skeletonize equivalent
// =============================================================================

/// Zhang-Suen iterative thinning (Zhang & Suen 1984, CACM 27:236-239).
/// Equivalent to `skimage.morphology.skeletonize(mask)` (2D), which Allen
/// `_dilationPatches2` calls on the halo of `total_area - rawPatches` to
/// recover the medial axis between adjacent patches.
///
/// Iterates two sub-passes until no pixels change:
///   Sub-pass 1 deletes p iff:
///     - p has 2 ≤ B(p) ≤ 6 nonzero 8-neighbors
///     - A(p) = 1, where A counts 0→1 transitions in the clockwise
///       sequence (N, NE, E, SE, S, SW, W, NW)
///     - At least one of (N, E, S) is background
///     - At least one of (E, S, W) is background
///   Sub-pass 2 same conditions but with:
///     - At least one of (N, E, W) is background
///     - At least one of (N, S, W) is background
pub(crate) fn binary_skeletonize_zs(mask: &Array2<bool>) -> Array2<bool> {
    let (h, w) = mask.dim();
    let mut cur = mask.clone();
    loop {
        let mut changed = false;

        // Two-phase sweep so deletions in one phase don't influence the
        // neighborhood seen by the other phase mid-iteration.
        for phase in 0..2 {
            let mut to_delete: Vec<(usize, usize)> = Vec::new();
            for r in 1..h.saturating_sub(1) {
                for c in 1..w.saturating_sub(1) {
                    if !cur[[r, c]] { continue; }
                    // 8-neighbors clockwise starting north: p2..p9
                    let p = [
                        cur[[r - 1, c]],     // p2 N
                        cur[[r - 1, c + 1]], // p3 NE
                        cur[[r, c + 1]],     // p4 E
                        cur[[r + 1, c + 1]], // p5 SE
                        cur[[r + 1, c]],     // p6 S
                        cur[[r + 1, c - 1]], // p7 SW
                        cur[[r, c - 1]],     // p8 W
                        cur[[r - 1, c - 1]], // p9 NW
                    ];
                    let b = p.iter().filter(|&&x| x).count();
                    if !(2..=6).contains(&b) { continue; }
                    let mut a = 0usize;
                    for i in 0..8 {
                        if !p[i] && p[(i + 1) % 8] { a += 1; }
                    }
                    if a != 1 { continue; }
                    // p2=N(0), p4=E(2), p6=S(4), p8=W(6)
                    if phase == 0 {
                        // (N ∧ E ∧ S) ∧ (E ∧ S ∧ W) must both be FALSE
                        if p[0] && p[2] && p[4] { continue; }
                        if p[2] && p[4] && p[6] { continue; }
                    } else {
                        // (N ∧ E ∧ W) ∧ (N ∧ S ∧ W) must both be FALSE
                        if p[0] && p[2] && p[6] { continue; }
                        if p[0] && p[4] && p[6] { continue; }
                    }
                    to_delete.push((r, c));
                }
            }
            if !to_delete.is_empty() {
                changed = true;
                for (r, c) in to_delete { cur[[r, c]] = false; }
            }
        }

        if !changed { break; }
    }
    cur
}

// =============================================================================
// `imfill` — fill holes (background components not touching the edge)
// =============================================================================

/// MATLAB `imfill(mask)` equivalent: any background component (`false`
/// pixel) not 4-connected to the image edge becomes foreground (`true`).
/// Implemented as flood-fill from the border, then complement.
pub(crate) fn binary_fill_holes(mask: &Array2<bool>) -> Array2<bool> {
    let (h, w) = mask.dim();
    let mut reached = Array2::<bool>::from_elem((h, w), false);
    let mut stack: Vec<(usize, usize)> = Vec::new();

    for c in 0..w {
        if !mask[[0, c]]     { reached[[0, c]] = true; stack.push((0, c)); }
        if !mask[[h-1, c]]   { reached[[h-1, c]] = true; stack.push((h-1, c)); }
    }
    for r in 0..h {
        if !mask[[r, 0]]     { reached[[r, 0]] = true; stack.push((r, 0)); }
        if !mask[[r, w-1]]   { reached[[r, w-1]] = true; stack.push((r, w-1)); }
    }

    let off: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    while let Some((r, c)) = stack.pop() {
        for &(dr, dc) in &off {
            let rr = r as i32 + dr;
            let cc = c as i32 + dc;
            if rr < 0 || rr >= h as i32 || cc < 0 || cc >= w as i32 { continue; }
            let (rr, cc) = (rr as usize, cc as usize);
            if mask[[rr, cc]] || reached[[rr, cc]] { continue; }
            reached[[rr, cc]] = true;
            stack.push((rr, cc));
        }
    }

    Array2::from_shape_fn((h, w), |(r, c)| mask[[r, c]] || !reached[[r, c]])
}

// Zhang-Suen skeletonize + bwmorph spur prune were used by the old
// SNLC-style border-skeletonization patch-extraction. Allen's
// `retinotopic_mapping` doesn't use either — patches come directly
// from labeling connected components of the threshold mask. Removed.
