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
    /// 4. `dilation_patches2_allen(patchmap, dilation_iter, border_width)` —
    ///    Allen's bulk-dilate-then-skeletonize separation: dilate the seed
    ///    patches, skeletonize the halo where dilations collide, subtract
    ///    that skeleton from the dilated union, and keep only components
    ///    that still overlap a seed (`RetinotopicMapping.py` L190–225).
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

/// Allen `_getRawPatchMap` (`RetinotopicMapping.py` L1404-1439): the binary
/// patch-candidate map — `binary_opening(open_iter)` (4-conn cross) →
/// `label` (4-conn) → close each labeled component independently
/// (`binary_closing(close_iter)`) → recombine. Allen *sums* the closed patches;
/// we OR-union, which has identical binary support and is associative, so the
/// parallel reduce stays bit-exact. Extracted from [`PatchExtractionMethod::apply`]
/// so it is golden-testable directly against scipy.
pub(crate) fn raw_patch_map_allen(
    imseg: &Array2<bool>,
    open_iter: i32,
    close_iter: i32,
) -> Array2<bool> {
    use rayon::prelude::*;

    use crate::segmentation::connectivity::label_4conn;
    use crate::segmentation::morphology::{binary_closing_cross, binary_opening_cross};

    let (h, w) = imseg.dim();
    let opened = binary_opening_cross(imseg, open_iter);
    let (labels, n_initial) = label_4conn(&opened);
    (1..=n_initial as i32)
        .into_par_iter()
        .map(|k| {
            let mask_k = Array2::from_shape_fn((h, w), |(r, c)| labels[[r, c]] == k);
            if close_iter > 0 {
                binary_closing_cross(&mask_k, close_iter)
            } else {
                mask_k
            }
        })
        .reduce(
            || Array2::<bool>::from_elem((h, w), false),
            |mut acc, m| {
                ndarray::Zip::from(&mut acc).and(&m).for_each(|a, &b| {
                    if b {
                        *a = true;
                    }
                });
                acc
            },
        )
}

impl PatchExtractionMethod {
    /// Extract patches from `imseg` (the post-threshold binary mask) and
    /// the smoothed VFS (used to assign patch signs). Mirrors Allen's
    /// `_getRawPatchMap` (`RetinotopicMapping.py` L1089–1124) followed
    /// by `_getRawPatches` (L1126–1182) end-to-end.
    pub fn apply(&self, imseg: &Array2<bool>, vfs_smoothed: &Array2<f64>) -> PatchExtractionOutput {
        use rayon::prelude::*;

        use crate::segmentation::connectivity::{
            dilation_patches2_allen, is_adjacent, label_4conn, patches_from_labels_majority_sign,
        };
        match self {
            Self::AllenZhuang2017LabelOpenCloseDilate {
                open_iter,
                close_iter,
                dilation_iter,
                border_width,
                small_patch_thr,
            } => {
                let (open_iter, close_iter, dilation_iter, border_width, small_patch_thr) = (
                    *open_iter,
                    *close_iter,
                    *dilation_iter,
                    *border_width,
                    *small_patch_thr,
                );
                let (h, w) = imseg.dim();
                tracing::debug!(
                    width = w,
                    height = h,
                    open_iter,
                    close_iter,
                    dilation_iter,
                    "patch extraction start"
                );

                // Allen `_getRawPatchMap` — extracted so it is golden-tested
                // directly against scipy (see `raw_patch_map_allen`).
                let patchmap2 = raw_patch_map_allen(imseg, open_iter, close_iter);

                tracing::debug!("starting Allen dilation+skeleton");
                let dilated = dilation_patches2_allen(&patchmap2, dilation_iter, border_width);
                tracing::debug!("Allen dilation done; relabeling");
                let (final_labels, n_final) = label_4conn(&dilated);
                tracing::debug!(components = n_final, "final components; assigning signs");
                let mut patches =
                    patches_from_labels_majority_sign(&final_labels, n_final, vfs_smoothed);

                let before = patches.len();
                patches.retain(|p| p.area() >= small_patch_thr);
                tracing::debug!(
                    before,
                    after = patches.len(),
                    threshold = small_patch_thr,
                    "patches after size filter"
                );

                // Drop isolated patches: no other patch adjacent within
                // `2·border_width`. Skipped when the patch count is in
                // noise-dominated territory (real mouse retinotopy ≈ 10–15
                // areas; > 100 patches means the input VFS has no SNR and
                // the adjacency filter would just be expensive theatre on
                // garbage data — the user needs more cycles, not more
                // post-processing on a noise field).
                const ADJACENCY_FILTER_MAX_PATCHES: usize = 100;
                let adjacency_width = (2 * border_width).max(1);
                let keep: Vec<bool> = if patches.len() > ADJACENCY_FILTER_MAX_PATCHES {
                    tracing::warn!(
                        patches = patches.len(),
                        threshold = ADJACENCY_FILTER_MAX_PATCHES,
                        "skipping adjacency filter — patch count over threshold \
                         (input VFS is noise-dominated; acquire more cycles for better SNR)",
                    );
                    vec![true; patches.len()]
                } else {
                    let masks: Vec<Array2<bool>> = patches.iter().map(|p| p.mask.clone()).collect();
                    // Each patch's "is any other patch adjacent?" test is
                    // independent; ordered `collect` keeps `keep[i]` aligned.
                    (0..patches.len())
                        .into_par_iter()
                        .map(|i| {
                            (0..patches.len()).any(|j| {
                                i != j && is_adjacent(&masks[i], &masks[j], adjacency_width)
                            })
                        })
                        .collect()
                };
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
