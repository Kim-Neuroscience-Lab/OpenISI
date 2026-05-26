//! Stage 7 — Patch extraction (label binary mask → patches with signs).
//!
//! Given a binary patch mask and the smoothed VFS, produce a labelled
//! patch list and per-patch signs.

use ndarray::Array2;
use openisi_params::{
    PatchExtractionAllenBorderWidth, PatchExtractionAllenCloseIter,
    PatchExtractionAllenDilationIter, PatchExtractionAllenOpenIter,
    PatchExtractionAllenSmallPatchThr, Tagged,
};

use crate::segmentation::Patch;

/// Method choice for extracting patches from the threshold mask.
///
/// `#[non_exhaustive]` + constructor below enforce registry-sourced
/// tunables.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum PatchExtractionMethod {
    /// Allen `retinotopic_mapping` `_getRawPatchMap` + `_getRawPatches`
    /// (Zhuang 2017, eLife 6:e18372; `RetinotopicMapping.py` L1089–1210).
    ///
    /// Pipeline (faithful to scipy.ndimage call sites in
    /// `RetinotopicMapping.py`):
    /// 1. `scipy.ndimage.binary_opening(imseg, iterations=open_iter)` —
    ///    remove salt-and-pepper. The default 2D structure is a 4-conn
    ///    cross, so `open_iter` iterations = a Manhattan-disk SE of
    ///    radius `open_iter` (a diamond), NOT a Euclidean disk.
    /// 2. `scipy.ndimage.label(imseg)` — 4-conn CC labelling.
    /// 3. Per-patch `scipy.ndimage.binary_closing(currPatch,
    ///    iterations=close_iter)` — smooth each patch's boundary using
    ///    the same 4-conn cross / Manhattan-disk SE.
    /// 4. `_dilationPatches2(labels, dilation_iter, border_width)` —
    ///    iterative label-aware dilation that preserves single-pixel
    ///    inter-patch borders (a pixel only takes a label if exactly
    ///    one label is in its 8-neighborhood).
    /// 5. Trim labels within `border_width` of the image edge.
    /// 6. Majority-sign assignment per final patch from `vfs_smoothed`.
    AllenZhuang2017LabelOpenCloseDilate {
        open_iter: i32,
        close_iter: i32,
        dilation_iter: i32,
        border_width: i32,
        small_patch_thr: usize,
    },
}

impl PatchExtractionMethod {
    pub fn allen_zhuang2017_label_open_close_dilate(
        open_iter: Tagged<PatchExtractionAllenOpenIter>,
        close_iter: Tagged<PatchExtractionAllenCloseIter>,
        dilation_iter: Tagged<PatchExtractionAllenDilationIter>,
        border_width: Tagged<PatchExtractionAllenBorderWidth>,
        small_patch_thr: Tagged<PatchExtractionAllenSmallPatchThr>,
    ) -> Self {
        Self::AllenZhuang2017LabelOpenCloseDilate {
            open_iter: open_iter.into_inner(),
            close_iter: close_iter.into_inner(),
            dilation_iter: dilation_iter.into_inner(),
            border_width: border_width.into_inner(),
            small_patch_thr: small_patch_thr.into_inner(),
        }
    }
}

/// Output of the patch-extraction stage: a list of `Patch` (mask + sign)
/// and a copy of the post-morphology binary mask for downstream tooling.
pub struct PatchExtractionOutput {
    pub patches: Vec<Patch>,
}

impl PatchExtractionMethod {
    /// Extract patches from `imseg` (the post-threshold binary mask) and
    /// the smoothed VFS (used to assign patch signs). Mirrors Allen's
    /// `_getRawPatchMap` (`RetinotopicMapping.py` L1089–1124) followed
    /// by `_getRawPatches` (L1126–1182) end-to-end.
    pub fn apply(
        &self,
        imseg: &Array2<bool>,
        vfs_smoothed: &Array2<f64>,
    ) -> PatchExtractionOutput {
        use crate::segmentation::morphology::{binary_closing_cross, binary_opening_cross};
        use crate::segmentation::connectivity::{
            dilation_patches2_allen, is_adjacent, label_4conn,
            patches_from_labels_majority_sign,
        };
        match self {
            Self::AllenZhuang2017LabelOpenCloseDilate {
                open_iter,
                close_iter,
                dilation_iter,
                border_width,
                small_patch_thr,
            } => {
                let (open_iter, close_iter, dilation_iter, border_width, small_patch_thr) =
                    (*open_iter, *close_iter, *dilation_iter, *border_width, *small_patch_thr);
                let (h, w) = imseg.dim();

                let opened = binary_opening_cross(imseg, open_iter);
                let (labels, n_initial) = label_4conn(&opened);

                let mut patchmap2 = Array2::<bool>::from_elem((h, w), false);
                for k in 1..=n_initial as i32 {
                    let mask_k = Array2::from_shape_fn((h, w), |(r, c)| labels[[r, c]] == k);
                    let closed = if close_iter > 0 {
                        binary_closing_cross(&mask_k, close_iter)
                    } else {
                        mask_k
                    };
                    for r in 0..h {
                        for c in 0..w {
                            if closed[[r, c]] { patchmap2[[r, c]] = true; }
                        }
                    }
                }

                let dilated = dilation_patches2_allen(&patchmap2, dilation_iter, border_width);
                let (final_labels, n_final) = label_4conn(&dilated);
                let mut patches = patches_from_labels_majority_sign(
                    &final_labels, n_final, vfs_smoothed,
                );

                patches.retain(|p| p.area() >= small_patch_thr);

                // Drop isolated patches: no other patch adjacent within `2·border_width`.
                let adjacency_width = (2 * border_width).max(1);
                let masks: Vec<Array2<bool>> = patches.iter().map(|p| p.mask.clone()).collect();
                let mut keep = vec![true; patches.len()];
                for i in 0..patches.len() {
                    let mut touching = false;
                    for j in 0..patches.len() {
                        if i == j { continue; }
                        if is_adjacent(&masks[i], &masks[j], adjacency_width) {
                            touching = true;
                            break;
                        }
                    }
                    if !touching { keep[i] = false; }
                }
                let mut idx = 0;
                patches.retain(|_| {
                    let k = keep[idx];
                    idx += 1;
                    k
                });

                PatchExtractionOutput { patches }
            }
        }
    }
}
