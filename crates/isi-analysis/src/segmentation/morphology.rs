//! Morphological primitives used by the Garrett 2014 / SNLC and Allen
//! Zhuang 2017 segmentation pipelines.
//!
//! Two SE families are needed:
//!
//! - **Disk SE** (`*_disk` functions). Garrett 2014 (J Neurosci PMC4160785)
//!   and the SNLC ISI MATLAB toolbox (`getMouseAreasX.m`) use MATLAB
//!   `strel('disk', R, 0)` — a flat Euclidean disk of radius R pixels
//!   (29 pixels at R=3). Used by `CortexSourceMethod::SnlcGarrett2014ImBound`.
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
//!   - `binary_skeletonize_skimage` — bit-faithful port of
//!     `skimage.morphology.skeletonize` (2D), the function Allen
//!     `dilationPatches2` calls.
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
    if sigma <= 0.0 {
        return data.clone();
    }
    // scipy `gaussian_filter` kernel radius: `int(truncate*sigma + 0.5)` with
    // the default `truncate = 4.0`. Faithful to Allen's `ni.gaussian_filter`
    // (golden-tested in `golden_cortex_morph.rs`).
    let radius = (sigma * 4.0 + 0.5).floor() as usize;
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
pub(crate) fn disk_offsets(radius: i32) -> Vec<(i32, i32)> {
    if radius <= 0 {
        return vec![(0, 0)];
    }
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
    if radius <= 0 {
        return mask.clone();
    }
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
                if rr < 0 || rr >= h as i32 || cc < 0 || cc >= w as i32 {
                    continue;
                }
                if mask[[rr as usize, cc as usize]] {
                    any = true;
                    break;
                }
            }
            out[[r, c]] = any;
        }
    }
    out
}

pub(super) fn binary_erosion_disk(mask: &Array2<bool>, radius: i32) -> Array2<bool> {
    if radius <= 0 {
        return mask.clone();
    }
    let (h, w) = mask.dim();
    let offs = disk_offsets(radius);
    let mut out = Array2::<bool>::from_elem((h, w), false);
    for r in 0..h {
        for c in 0..w {
            // out[r,c] = true iff all *in-image* pixels within the disk are
            // true. Out-of-image neighbours are treated as foreground
            // (MATLAB `imerode` pads the border with 1s), so the image
            // edge alone never erodes a pixel — required for faithful
            // `strel('disk',R,0)` erosion (golden-tested in
            // `golden_cortex_morph.rs`).
            let mut all = true;
            for &(dr, dc) in &offs {
                let rr = r as i32 + dr;
                let cc = c as i32 + dc;
                if rr < 0 || rr >= h as i32 || cc < 0 || cc >= w as i32 {
                    continue;
                }
                if !mask[[rr as usize, cc as usize]] {
                    all = false;
                    break;
                }
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
    if radius <= 0 {
        return mask.clone();
    }
    // Genuine MATLAB `imclose` is NOT a naive `imerode(imdilate(.))`: it pads the
    // image with 0 by the SE radius, composes dilate→erode, then crops. Verified
    // bit-exact against MATLAB R2025b (`imclose == imerode(imdilate(pad0(bw))).crop`);
    // the naive compose (what we used to do) differs within
    // R px of the image edge. Reproduce the genuine MATLAB rule so the cortex
    // segmentation is faithful to the SNLC reference even for border-touching
    // objects. (Interior results are unchanged — the divergence is border-only.)
    let r = radius as usize;
    let (h, w) = mask.dim();
    let mut padded = Array2::<bool>::from_elem((h + 2 * r, w + 2 * r), false);
    for i in 0..h {
        for j in 0..w {
            padded[[i + r, j + r]] = mask[[i, j]];
        }
    }
    let eroded = binary_erosion_disk(&binary_dilation_disk(&padded, radius), radius);
    Array2::from_shape_fn((h, w), |(i, j)| eroded[[i + r, j + r]])
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
    if iterations <= 0 {
        return mask.clone();
    }
    let (h, w) = mask.dim();
    let mut cur = mask.clone();
    for _ in 0..iterations {
        let mut next = Array2::<bool>::from_elem((h, w), false);
        for r in 0..h {
            for c in 0..w {
                if cur[[r, c]] {
                    next[[r, c]] = true;
                    continue;
                }
                let n = (r > 0 && cur[[r - 1, c]])
                    || (r + 1 < h && cur[[r + 1, c]])
                    || (c > 0 && cur[[r, c - 1]])
                    || (c + 1 < w && cur[[r, c + 1]]);
                if n {
                    next[[r, c]] = true;
                }
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
    if iterations <= 0 {
        return mask.clone();
    }
    let (h, w) = mask.dim();
    let mut cur = mask.clone();
    for _ in 0..iterations {
        let mut next = Array2::<bool>::from_elem((h, w), false);
        for r in 0..h {
            for c in 0..w {
                if !cur[[r, c]] {
                    continue;
                }
                let all_in = r > 0
                    && cur[[r - 1, c]]
                    && r + 1 < h
                    && cur[[r + 1, c]]
                    && c > 0
                    && cur[[r, c - 1]]
                    && c + 1 < w
                    && cur[[r, c + 1]];
                if all_in {
                    next[[r, c]] = true;
                }
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
// skeletonize — bit-faithful port of skimage.morphology.skeletonize (2D)
// =============================================================================

/// skimage `_skeletonize_various_cy.pyx::_fast_skeletonize` lookup table.
/// One entry per 8-neighbour pattern (index 0..255). The value's bits select
/// removability: bit 0 set (value 1 or 3) → removable in sub-pass 1; bit 1 set
/// (value 2 or 3) → removable in sub-pass 2. Copied verbatim from skimage
/// 0.25.2 and validated bit-for-bit (see `binary_skeletonize_skimage`).
#[rustfmt::skip]
const SKELETONIZE_LUT: [u8; 256] = [
    0, 0, 0, 1, 0, 0, 1, 3, 0, 0, 3, 1, 1, 0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 0,
    2, 0, 2, 0, 3, 0, 3, 3, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 3, 0, 2, 2, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    2, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0,
    3, 0, 0, 0, 3, 0, 2, 0, 0, 0, 3, 1, 0, 0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 1,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 3, 1, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    2, 3, 1, 3, 0, 0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 2, 3, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0,
    3, 3, 0, 1, 0, 0, 0, 0, 2, 2, 0, 0, 2, 0, 0, 0,
];

/// Bit-faithful port of `skimage.morphology.skeletonize(mask)` (2D), the
/// exact function Allen `dilationPatches2` (`RetinotopicMapping.py` L201)
/// calls on the halo `total_area − rawPatches` to recover the medial axis
/// between adjacent patches.
///
/// skimage's docstring cites Zhang & Suen 1984, but its `_fast_skeletonize`
/// is a *specific* LUT thinning that differs from a literal textbook
/// Zhang-Suen by a few pixels on thin features. Faithfulness to Allen means
/// reproducing what Allen actually executes (skimage), not the textbook — so
/// we port skimage's exact 256-entry [`SKELETONIZE_LUT`] verbatim.
///
/// Algorithm: zero-pad by 1 px (so edge neighbours read as background, as
/// skimage's pad-then-thin does); repeat two sub-passes until a full
/// iteration removes nothing. For each foreground pixel, build an 8-neighbour
/// code with the fixed skimage weights
///   `NW=1 N=2 NE=4 E=8 SE=16 S=32 SW=64 W=128`
/// and remove it if `SKELETONIZE_LUT[code]` has the active sub-pass's bit set.
/// Removals are written to a copy so they don't affect the same pass's other
/// decisions (parallel sub-iteration); the copy is committed after each pass.
///
/// Validated bit-for-bit against `skimage.skeletonize` 0.25.2 by
/// `gen_skeletonize_golden.py` / `skeletonize_matches_skimage`.
pub(crate) fn binary_skeletonize_skimage(mask: &Array2<bool>) -> Array2<bool> {
    let (h, w) = mask.dim();
    // 1-px zero border so edge-pixel neighbour reads see background.
    let pw = w + 2;
    let mut sk = vec![0u8; (h + 2) * pw];
    for r in 0..h {
        for c in 0..w {
            if mask[[r, c]] {
                sk[(r + 1) * pw + (c + 1)] = 1;
            }
        }
    }
    let mut cleaned = sk.clone();
    loop {
        let mut removed = false;
        for first_pass in [true, false] {
            for r in 1..=h {
                for c in 1..=w {
                    let idx = r * pw + c;
                    if sk[idx] == 0 {
                        continue;
                    }
                    let code = sk[idx - pw - 1] as usize          // NW = 1
                        | (sk[idx - pw] as usize) << 1            // N  = 2
                        | (sk[idx - pw + 1] as usize) << 2        // NE = 4
                        | (sk[idx + 1] as usize) << 3             // E  = 8
                        | (sk[idx + pw + 1] as usize) << 4        // SE = 16
                        | (sk[idx + pw] as usize) << 5            // S  = 32
                        | (sk[idx + pw - 1] as usize) << 6        // SW = 64
                        | (sk[idx - 1] as usize) << 7;            // W  = 128
                    let bit = if first_pass { 1 } else { 2 };
                    if SKELETONIZE_LUT[code] & bit != 0 {
                        cleaned[idx] = 0;
                        removed = true;
                    }
                }
            }
            // Commit this sub-pass before the next reads neighbours.
            sk.copy_from_slice(&cleaned);
        }
        if !removed {
            break;
        }
    }
    Array2::from_shape_fn((h, w), |(r, c)| sk[(r + 1) * pw + (c + 1)] != 0)
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
        if !mask[[0, c]] {
            reached[[0, c]] = true;
            stack.push((0, c));
        }
        if !mask[[h - 1, c]] {
            reached[[h - 1, c]] = true;
            stack.push((h - 1, c));
        }
    }
    for r in 0..h {
        if !mask[[r, 0]] {
            reached[[r, 0]] = true;
            stack.push((r, 0));
        }
        if !mask[[r, w - 1]] {
            reached[[r, w - 1]] = true;
            stack.push((r, w - 1));
        }
    }

    let off: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    while let Some((r, c)) = stack.pop() {
        for &(dr, dc) in &off {
            let rr = r as i32 + dr;
            let cc = c as i32 + dc;
            if rr < 0 || rr >= h as i32 || cc < 0 || cc >= w as i32 {
                continue;
            }
            let (rr, cc) = (rr as usize, cc as usize);
            if mask[[rr, cc]] || reached[[rr, cc]] {
                continue;
            }
            reached[[rr, cc]] = true;
            stack.push((rr, cc));
        }
    }

    Array2::from_shape_fn((h, w), |(r, c)| mask[[r, c]] || !reached[[r, c]])
}

#[cfg(test)]
mod morphology_property_tests {
    use super::*;

    // Property: dilating a single-pixel mask by a disk SE of radius R produces
    // the discrete disk of radius R, matching disk_offsets exactly. Per MATLAB
    // strel('disk', R, 0) semantics: the "inside or on the circle" predicate
    // dr² + dc² ≤ R² + ε defines the set.
    #[test]
    fn property_disk_dilation_of_point_is_disk() {
        for radius in 1..=3i32 {
            let size = (2 * radius + 5) as usize;
            let mut mask = Array2::<bool>::from_elem((size, size), false);
            let center = size / 2;
            mask[[center, center]] = true;

            let dilated = binary_dilation_disk(&mask, radius);

            let r2 = (radius * radius) as f64 + 1e-9;
            for r in 0..size {
                for c in 0..size {
                    let dr = r as i32 - center as i32;
                    let dc = c as i32 - center as i32;
                    let in_disk = (dr * dr + dc * dc) as f64 <= r2;
                    assert_eq!(
                        dilated[[r, c]],
                        in_disk,
                        "dilation of point with disk radius {} disagrees at ({}, {})",
                        radius,
                        r,
                        c,
                    );
                }
            }
        }
    }
}

// Zhang-Suen skeletonize + bwmorph spur prune were used by the old
// SNLC-style border-skeletonization patch-extraction. Allen's
// `retinotopic_mapping` doesn't use either — patches come directly
// from labeling connected components of the threshold mask. Removed.
