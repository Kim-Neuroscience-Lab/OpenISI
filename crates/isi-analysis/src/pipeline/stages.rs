//! The concrete pipeline stages. Each wraps a canonical `methods/*.rs`
//! `apply()` (or a `math::` helper) and mirrors `compute_analysis` verbatim —
//! same inputs, same calls, same order. The equivalence test against the
//! committed baseline is the gate that this refactor changed nothing.

use ndarray::Array2;

use crate::methods::cortex_source::CortexResolveContext;
use crate::{math, segmentation, AnalysisError};

use super::stage::{Stage, StageCtx, StageId};
use super::state::PipelineState;

/// Iso-contour interval (degrees) for the derived azimuth/altitude contour
/// maps. A fixed diagnostic constant in the procedural code; named here.
const CONTOUR_INTERVAL_DEG: f64 = 4.0;

/// The pipeline's stages, in declaration order. The orchestrator topologically
/// sorts them via their `deps()`, which reproduces this order.
pub fn all_stages() -> Vec<Box<dyn Stage>> {
    vec![
        Box::new(Baseline),
        Box::new(Projection),
        Box::new(Retinotopy),
        Box::new(SignSmoothing),
        Box::new(CortexSource),
        Box::new(PatchThreshold),
        Box::new(PatchExtraction),
        Box::new(PatchRefinement),
        Box::new(Labels),
        Box::new(Eccentricity),
        Box::new(DerivedMaps),
    ]
}

// ── Baseline — ΔF/F F0 (method-dispatched) ──────────────────────────────────

struct Baseline;
impl Stage for Baseline {
    fn id(&self) -> StageId {
        StageId::Baseline
    }
    fn deps(&self) -> &'static [StageId] {
        &[]
    }
    fn execute(&self, st: &mut PipelineState, ctx: &StageCtx) -> Result<(), AnalysisError> {
        // Reached only when not seeded (see `is_satisfied`); a missing `raw`
        // here is a genuine "neither raw frames nor seeded maps" error.
        let raw = ctx.raw.ok_or_else(|| {
            AnalysisError::MissingData(
                "baseline stage has no raw acquisition and no complex maps were seeded".into(),
            )
        })?;
        let baseline = ctx.params.baseline.apply(raw);
        st.baseline_floor = Some(baseline.floor);
        st.baseline_f0 = Some(baseline.f0);
        Ok(())
    }
    /// Skipped when the boundary seeded `/complex_maps` (so the projection that
    /// would consume `F0` is also skipped), or when `F0` is already present.
    fn is_satisfied(&self, st: &PipelineState) -> bool {
        st.complex_maps.is_some() || st.baseline_f0.is_some()
    }
}

// ── Projection — per-cycle DFT → complex maps (+ reliability, SNR) ───────────

struct Projection;
impl Stage for Projection {
    fn id(&self) -> StageId {
        StageId::Projection
    }
    fn deps(&self) -> &'static [StageId] {
        &[StageId::Baseline]
    }
    fn execute(&self, st: &mut PipelineState, ctx: &StageCtx) -> Result<(), AnalysisError> {
        let raw = ctx.raw.ok_or_else(|| {
            AnalysisError::MissingData(
                "projection stage has no raw acquisition and no complex maps were seeded".into(),
            )
        })?;
        let f0 = st.baseline_f0.as_ref().ok_or_else(|| {
            AnalysisError::Compute("pipeline ordering bug: F0 read before the baseline stage".into())
        })?;
        let floor = st.baseline_floor.ok_or_else(|| {
            AnalysisError::Compute(
                "pipeline ordering bug: ΔF/F floor read before the baseline stage".into(),
            )
        })?;
        let out = crate::compute::projection::run(
            raw,
            f0,
            floor,
            &ctx.params.cycle_average,
            ctx.cancel,
            ctx.progress,
        )?;
        st.complex_maps = Some(out.complex_maps);
        st.responsiveness = out.responsiveness;
        st.reliability = out.reliability;
        Ok(())
    }
    /// The seed seam: when the boundary restored `/complex_maps` (or seeded an
    /// import), `st.complex_maps` is already `Some` and projection is skipped —
    /// mirrors `Retinotopy::is_satisfied`.
    fn is_satisfied(&self, st: &PipelineState) -> bool {
        st.complex_maps.is_some()
    }
}

// ── Retinotopy (fused former stages 1–3 + assembly) ─────────────────────────

struct Retinotopy;
impl Stage for Retinotopy {
    fn id(&self) -> StageId {
        StageId::Retinotopy
    }
    fn deps(&self) -> &'static [StageId] {
        &[StageId::Projection]
    }
    fn execute(&self, st: &mut PipelineState, ctx: &StageCtx) -> Result<(), AnalysisError> {
        st.retino = Some(math::compute_retinotopy(
            st.complex_maps_ref()?,
            ctx.acquisition,
            ctx.params,
            ctx.cancel,
        )?);
        Ok(())
    }
    /// Retinotopy is the cacheable restore point: when `analyze` has seeded
    /// `st.retino` from disk (fingerprint matched), the walk skips the
    /// expensive device recompute. See `super::fingerprint::retinotopy`.
    fn is_satisfied(&self, st: &PipelineState) -> bool {
        st.retino.is_some()
    }
}

// ── Stage 4 — sign map smoothing ────────────────────────────────────────────

struct SignSmoothing;
impl Stage for SignSmoothing {
    fn id(&self) -> StageId {
        StageId::SignSmoothing
    }
    fn deps(&self) -> &'static [StageId] {
        &[StageId::Retinotopy]
    }
    fn execute(&self, st: &mut PipelineState, ctx: &StageCtx) -> Result<(), AnalysisError> {
        let vfs = &st.retino_ref()?.vfs;
        st.vfs_smooth = Some(
            ctx.params
                .sign_map_smoothing
                .apply(vfs, ctx.acquisition.um_per_pixel),
        );
        Ok(())
    }
}

// ── Stage 5 — cortex source resolve ─────────────────────────────────────────

struct CortexSource;
impl Stage for CortexSource {
    fn id(&self) -> StageId {
        StageId::CortexSource
    }
    fn deps(&self) -> &'static [StageId] {
        &[StageId::SignSmoothing]
    }
    fn execute(&self, st: &mut PipelineState, ctx: &StageCtx) -> Result<(), AnalysisError> {
        let vfs_smooth = st.vfs_smooth_ref()?;
        let cortex_ctx = CortexResolveContext {
            shape: vfs_smooth.dim(),
            reliability: st.reliability.as_ref(),
            // The method clones internally for the UserPolygon variant; we hand
            // it an owned copy to match the procedural call's data flow exactly.
            user_polygon: ctx.user_polygon.cloned(),
            vfs_smoothed: Some(vfs_smooth),
        };
        st.cortex_mask = Some(ctx.params.cortex_source.apply(&cortex_ctx)?);
        Ok(())
    }
}

// ── Stage 6 — patch threshold ───────────────────────────────────────────────

struct PatchThreshold;
impl Stage for PatchThreshold {
    fn id(&self) -> StageId {
        StageId::PatchThreshold
    }
    fn deps(&self) -> &'static [StageId] {
        &[StageId::SignSmoothing, StageId::CortexSource]
    }
    fn execute(&self, st: &mut PipelineState, ctx: &StageCtx) -> Result<(), AnalysisError> {
        let out = ctx
            .params
            .patch_threshold
            .apply(st.vfs_smooth_ref()?, st.cortex_mask_ref()?);
        st.imseg = Some(out.imseg);
        st.threshold_applied = Some(out.threshold_applied);
        Ok(())
    }
}

// ── Stage 7 — patch extraction ──────────────────────────────────────────────

struct PatchExtraction;
impl Stage for PatchExtraction {
    fn id(&self) -> StageId {
        StageId::PatchExtraction
    }
    fn deps(&self) -> &'static [StageId] {
        &[StageId::PatchThreshold, StageId::SignSmoothing]
    }
    fn execute(&self, st: &mut PipelineState, ctx: &StageCtx) -> Result<(), AnalysisError> {
        let extraction = ctx
            .params
            .patch_extraction
            .apply(st.imseg_ref()?, st.vfs_smooth_ref()?);
        st.patches = Some(extraction.patches);
        Ok(())
    }
}

// ── Stage 8 — patch refinement (split + merge) ──────────────────────────────

struct PatchRefinement;
impl Stage for PatchRefinement {
    fn id(&self) -> StageId {
        StageId::PatchRefinement
    }
    fn deps(&self) -> &'static [StageId] {
        &[StageId::PatchExtraction, StageId::Retinotopy]
    }
    fn execute(&self, st: &mut PipelineState, ctx: &StageCtx) -> Result<(), AnalysisError> {
        let patches_in = st.take_patches()?;
        let retino = st.retino_ref()?;
        // Allen's split criterion compares visual-space area to the
        // determinant-of-Jacobian integral (`magnification_raw`).
        let patches = ctx.params.patch_refinement.apply(
            patches_in,
            &retino.azi_phase_degrees,
            &retino.alt_phase_degrees,
            &retino.magnification_raw,
            ctx.cancel,
        )?;
        st.patches = Some(patches);
        Ok(())
    }
}

// ── Label assembly — sort by area; build labels / signs / borders ───────────

struct Labels;
impl Stage for Labels {
    fn id(&self) -> StageId {
        StageId::Labels
    }
    fn deps(&self) -> &'static [StageId] {
        &[StageId::PatchRefinement]
    }
    fn execute(&self, st: &mut PipelineState, _ctx: &StageCtx) -> Result<(), AnalysisError> {
        let mut patches = st.take_patches()?;
        patches.sort_by_key(|b| std::cmp::Reverse(b.area()));
        let (h, w) = st.vfs_smooth_ref()?.dim();
        let mut area_labels = Array2::<i32>::zeros((h, w));
        let mut area_signs: Vec<i8> = Vec::with_capacity(patches.len());
        for (i, patch) in patches.iter().enumerate() {
            let label = (i + 1) as i32;
            for r in 0..h {
                for c in 0..w {
                    if patch.mask[[r, c]] {
                        area_labels[[r, c]] = label;
                    }
                }
            }
            area_signs.push(patch.sign);
        }
        st.area_borders = Some(segmentation::extract_label_borders(&area_labels));
        st.area_labels = Some(area_labels);
        st.area_signs = Some(area_signs);
        Ok(())
    }
}

// ── Stage 10 — eccentricity ─────────────────────────────────────────────────

struct Eccentricity;
impl Stage for Eccentricity {
    fn id(&self) -> StageId {
        StageId::Eccentricity
    }
    fn deps(&self) -> &'static [StageId] {
        &[StageId::Labels, StageId::Retinotopy]
    }
    fn execute(&self, st: &mut PipelineState, ctx: &StageCtx) -> Result<(), AnalysisError> {
        let retino = st.retino_ref()?;
        let ecc = ctx.params.eccentricity.apply(
            &retino.azi_phase_degrees,
            &retino.alt_phase_degrees,
            st.area_labels_ref()?,
        );
        st.eccentricity = Some(ecc);
        Ok(())
    }
}

// ── Universal derived maps (not method-dispatched) ──────────────────────────

struct DerivedMaps;
impl Stage for DerivedMaps {
    fn id(&self) -> StageId {
        StageId::DerivedMaps
    }
    fn deps(&self) -> &'static [StageId] {
        &[
            StageId::Labels,
            StageId::Retinotopy,
            StageId::SignSmoothing,
            StageId::CortexSource,
            StageId::PatchThreshold,
        ]
    }
    fn execute(&self, st: &mut PipelineState, _ctx: &StageCtx) -> Result<(), AnalysisError> {
        let retino = st.retino_ref()?;
        let area_labels = st.area_labels_ref()?;
        // The `magnification` leaf is the Allen cortical magnification factor
        // (px²/deg²) — the reciprocal of the raw Jacobian determinant
        // (`magnification_raw`, deg²/px², which the split criterion still reads
        // un-inverted).
        let magnification = math::cortical_magnification_factor(&retino.magnification_raw, area_labels);
        let contours_azi =
            math::compute_contours(&retino.azi_phase_degrees, area_labels, CONTOUR_INTERVAL_DEG);
        let contours_alt =
            math::compute_contours(&retino.alt_phase_degrees, area_labels, CONTOUR_INTERVAL_DEG);

        // Threshold-mask the smoothed VFS for diagnostic display: NaN outside
        // cortex (renderer shows "no measurement"), the value where it clears
        // the same cutoff the patch-threshold strategy applied, else zero.
        let cortex_mask = st.cortex_mask_ref()?;
        let threshold_applied = st.threshold_applied.ok_or_else(|| {
            AnalysisError::Compute(
                "pipeline ordering bug: threshold_applied read before produced".into(),
            )
        })?;
        let vfs_smooth = st.vfs_smooth_ref()?;
        let vfs_smoothed_thresholded = Array2::from_shape_fn(vfs_smooth.dim(), |(r, c)| {
            if !cortex_mask[[r, c]] {
                f64::NAN
            } else {
                let v = vfs_smooth[[r, c]];
                if v.is_finite() && v.abs() >= threshold_applied {
                    v
                } else {
                    0.0
                }
            }
        });

        st.magnification = Some(magnification);
        st.contours_azi = Some(contours_azi);
        st.contours_alt = Some(contours_alt);
        st.vfs_smoothed_thresholded = Some(vfs_smoothed_thresholded);
        Ok(())
    }
}
