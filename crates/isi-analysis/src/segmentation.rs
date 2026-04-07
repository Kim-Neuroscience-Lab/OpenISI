//! Visual area segmentation — Garrett et al. 2014 / Juavinett et al. 2017.
//!
//! Implements the automated visual area identification algorithm from:
//! - Garrett, Nauhaus, Marshel & Callaway (2014) J Neurosci 34:12587-12600
//! - Juavinett, Nauhaus, Garrett, Zhuang & Callaway (2017) Nat Protoc 12:32-43
//!
//! Current implementation: steps 1-2 (smooth + threshold) and step 6 (sign-based
//! labeling + border extraction). Morphological cleanup (steps 3-5), eccentricity
//! splitting (step 7), patch fusion (step 8), and final cleanup (step 9) are
//! designed but disabled pending a performant morphology backend.

use ndarray::Array2;
use std::collections::VecDeque;

use crate::params::SegmentationParams;
use crate::RetinotopyMaps;

// =============================================================================
// Output type
// =============================================================================

/// Result of visual area segmentation.
#[derive(Clone)]
pub struct SegmentationResult {
    /// Area label per pixel: 0 = unassigned, 1..=area_count = area ID.
    pub area_labels: Array2<i32>,
    /// VFS sign for each area (+1 or -1). Index 0 = area 1's sign.
    pub area_signs: Vec<i8>,
    /// Number of distinct visual areas found.
    pub area_count: usize,
    /// Border mask between areas (true = border pixel).
    pub borders: Array2<bool>,
}

// =============================================================================
// Main entry point
// =============================================================================

/// Segment visual areas from retinotopy maps using the Garrett/Juavinett method.
///
/// Current implementation: smooth VFS → threshold → sign-based labeling → borders.
/// Full morphological pipeline (open/close/dilate, largest component, thinning,
/// eccentricity splitting, patch fusion) is designed but pending morphology backend.
pub fn segment_visual_areas(
    retinotopy: &RetinotopyMaps,
    params: &SegmentationParams,
) -> SegmentationResult {
    let (h, w) = retinotopy.vfs.dim();

    // ── Step 1: Smooth VFS ──────────────────────────────────────────
    let vfs_smooth = gaussian_smooth_f64(&retinotopy.vfs, params.sign_map_filter_sigma);

    // ── Step 2: Threshold |VFS| ─────────────────────────────────────
    let threshold = if params.sign_map_threshold > 0.0 {
        params.sign_map_threshold
    } else {
        1.5 * std_dev(&vfs_smooth)
    };
    let mask = vfs_smooth.mapv(|v| v.abs() > threshold);

    // ── Steps 3-5: Morphological cleanup ─────────────────────────────
    // TODO: open(disk(2)), pad+close(disk(10))+fill+dilate(disk(3)),
    //       keep largest connected component, clear edge margin.
    // Disabled pending performant morphology backend.

    // ── Step 6: Sign-based labeling ──────────────────────────────────
    let sign_map = Array2::from_shape_fn((h, w), |(r, c)| {
        if !mask[[r, c]] { 0i8 }
        else if vfs_smooth[[r, c]] > 0.0 { 1i8 }
        else { -1i8 }
    });

    let patch_labels = label_sign_regions(&sign_map, &mask);
    let borders = extract_borders(&patch_labels);

    // ── Steps 7-9: Splitting, fusion, cleanup ────────────────────────
    // TODO: eccentricity-based splitting, patch fusion, final open.
    // Disabled pending morphology backend.

    let area_labels = relabel_contiguous(&patch_labels);

    // Build area_signs by majority vote.
    let max_label = *area_labels.iter().max().unwrap_or(&0);
    let area_count = max_label as usize;
    let mut area_signs = Vec::with_capacity(area_count);
    for label in 1..=max_label {
        let mut pos = 0usize;
        let mut neg = 0usize;
        for r in 0..h {
            for c in 0..w {
                if area_labels[[r, c]] == label {
                    if retinotopy.vfs[[r, c]] > 0.0 { pos += 1; } else { neg += 1; }
                }
            }
        }
        area_signs.push(if pos >= neg { 1i8 } else { -1i8 });
    }

    SegmentationResult { area_labels, area_signs, area_count, borders }
}

// =============================================================================
// Connected component labeling for sign regions (BFS flood-fill)
// =============================================================================

fn label_sign_regions(sign_map: &Array2<i8>, mask: &Array2<bool>) -> Array2<i32> {
    let (h, w) = sign_map.dim();
    let mut labels = Array2::from_elem((h, w), 0i32);
    let mut next_label = 1i32;
    let offsets: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

    for row in 0..h {
        for col in 0..w {
            if !mask[[row, col]] || sign_map[[row, col]] == 0 || labels[[row, col]] != 0 {
                continue;
            }
            let sign = sign_map[[row, col]];
            let label = next_label;
            next_label += 1;
            labels[[row, col]] = label;

            let mut queue = VecDeque::new();
            queue.push_back((row, col));
            while let Some((r, c)) = queue.pop_front() {
                for &(dr, dc) in &offsets {
                    let nr = r as isize + dr;
                    let nc = c as isize + dc;
                    if nr >= 0 && nr < h as isize && nc >= 0 && nc < w as isize {
                        let (nr, nc) = (nr as usize, nc as usize);
                        if mask[[nr, nc]] && sign_map[[nr, nc]] == sign && labels[[nr, nc]] == 0 {
                            labels[[nr, nc]] = label;
                            queue.push_back((nr, nc));
                        }
                    }
                }
            }
        }
    }
    labels
}

// =============================================================================
// Border extraction
// =============================================================================

fn extract_borders(labels: &Array2<i32>) -> Array2<bool> {
    let (h, w) = labels.dim();
    let offsets: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    Array2::from_shape_fn((h, w), |(r, c)| {
        let l = labels[[r, c]];
        if l == 0 { return false; }
        // Border = adjacent to any pixel with a different label (including 0/unassigned).
        offsets.iter().any(|&(dr, dc)| {
            let nr = r as isize + dr;
            let nc = c as isize + dc;
            if nr < 0 || nr >= h as isize || nc < 0 || nc >= w as isize { return true; }
            let nl = labels[[nr as usize, nc as usize]];
            nl != l
        })
    })
}

// =============================================================================
// Utilities
// =============================================================================

fn gaussian_smooth_f64(input: &Array2<f64>, sigma: f64) -> Array2<f64> {
    if sigma <= 0.0 { return input.clone(); }
    let radius = (sigma * 3.0).ceil() as usize;
    let kernel = crate::math::gaussian_kernel_1d(sigma, radius);
    crate::math::separable_filter(input, &kernel)
}

fn std_dev(arr: &Array2<f64>) -> f64 {
    let n = arr.len() as f64;
    if n < 2.0 { return 0.0; }
    let mean = arr.iter().sum::<f64>() / n;
    (arr.iter().map(|&v| (v - mean).powi(2)).sum::<f64>() / n).sqrt()
}

fn relabel_contiguous(labels: &Array2<i32>) -> Array2<i32> {
    let max = *labels.iter().max().unwrap_or(&0);
    if max <= 0 { return labels.clone(); }
    let mut mapping = vec![0i32; max as usize + 1];
    let mut next = 1i32;
    for &l in labels.iter() {
        if l > 0 && mapping[l as usize] == 0 {
            mapping[l as usize] = next;
            next += 1;
        }
    }
    labels.mapv(|l| if l > 0 { mapping[l as usize] } else { 0 })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_sign_regions_two_blobs() {
        let sign = Array2::from_shape_fn((5, 10), |(_, c)| {
            if c < 4 { 1i8 } else if c > 5 { -1i8 } else { 0i8 }
        });
        let mask = sign.mapv(|s| s != 0);
        let labels = label_sign_regions(&sign, &mask);
        assert!(labels[[0, 0]] > 0);
        assert!(labels[[0, 8]] > 0);
        assert_ne!(labels[[0, 0]], labels[[0, 8]]);
    }

    #[test]
    fn relabel_contiguous_compacts() {
        let labels = Array2::from_shape_vec((2, 3), vec![0, 5, 5, 0, 10, 10]).unwrap();
        let result = relabel_contiguous(&labels);
        assert_eq!(result[[0, 1]], 1);
        assert_eq!(result[[1, 1]], 2);
    }

    #[test]
    fn extract_borders_finds_boundary() {
        let labels = Array2::from_shape_fn((5, 10), |(_, c)| if c < 5 { 1i32 } else { 2i32 });
        let borders = extract_borders(&labels);
        assert!(borders[[2, 4]]);
        assert!(borders[[2, 5]]);
        assert!(!borders[[2, 0]]);
    }
}
