//! `PipelineState` — the blackboard the DAG stages read from and write to.
//!
//! The analysis stages are genuinely heterogeneous (a stage consumes a
//! `Vec<Patch>` plus three `f64` maps; another emits a mask plus a scalar).
//! Rather than force a uniform `Stage<Input, Output>` over tuple-of-everything,
//! the stages share this named-field blackboard — which is exactly what the
//! procedural `compute_analysis` already was (named locals threaded stage to
//! stage). Heterogeneity lives in the fields; the `Stage` trait stays uniform.
//!
//! Each field is the output of one stage (or, for derived maps, a small group).
//! `None` means "not yet produced (or restored)". Topological execution order
//! guarantees a field is `Some` before any downstream stage reads it — the
//! `*_ref` accessors return a typed `MissingData` error rather than panicking,
//! so an ordering bug surfaces as a clean error, never an `unwrap`.

use ndarray::Array2;

use crate::segmentation::Patch;
use crate::{AnalysisError, ComplexMaps, ReliabilityMaps, ResponsivenessMaps, RetinotopyMaps};

/// Intermediate values produced by the pipeline (Retinotopy → DerivedMaps).
/// Inputs to the pipeline (`complex_maps`, `reliability`, `user_polygon`,
/// `acquisition`, `params`) live in [`super::stage::StageCtx`], not here —
/// this holds only what stages *produce*.
#[derive(Default)]
pub struct PipelineState {
    /// ΔF/F baseline `F0 [H, W]` — `Baseline` stage output, consumed by
    /// `Projection`. Paired with `baseline_floor`.
    pub baseline_f0: Option<Array2<f64>>,
    /// ΔF/F denominator floor for `baseline_f0` (half its median).
    pub baseline_floor: Option<f64>,
    /// Per-direction complex maps — `Projection` output (or seeded by the
    /// boundary from a cached `/complex_maps` / import).
    pub complex_maps: Option<ComplexMaps>,
    /// Cross-cycle reliability — `Projection` byproduct (consumed by
    /// `CortexSource`). `None` for imports / `K=1` recordings.
    pub reliability: Option<ReliabilityMaps>,
    /// Spectral responsiveness maps — `Projection` byproduct, passed straight
    /// through to the result.
    pub responsiveness: Option<ResponsivenessMaps>,
    /// Retinotopy maps — fused former stages 1–3 (`compute_retinotopy`).
    pub retino: Option<RetinotopyMaps>,
    /// Gaussian-smoothed VFS (stage 4).
    pub vfs_smooth: Option<Array2<f64>>,
    /// Imaged-cortex mask (stage 5).
    pub cortex_mask: Option<Array2<bool>>,
    /// Binary candidate-patch mask (stage 6).
    pub imseg: Option<Array2<bool>>,
    /// Actual scalar threshold applied to `|VFS|` (stage 6 byproduct).
    pub threshold_applied: Option<f64>,
    /// Patches — produced by extraction (stage 7), refined in place (stage 8).
    pub patches: Option<Vec<Patch>>,
    /// Area label map, sorted by area, 1-based (post-refinement assembly).
    pub area_labels: Option<Array2<i32>>,
    /// Per-area sign, index-aligned with `area_labels` values (1-based).
    pub area_signs: Option<Vec<i8>>,
    /// Area borders (post-assembly).
    pub area_borders: Option<Array2<bool>>,
    /// Eccentricity map (stage 10).
    pub eccentricity: Option<Array2<f64>>,
    /// Polar-angle map (stage 10, produced alongside eccentricity).
    pub polar_angle: Option<Array2<f64>>,
    /// ROI-masked magnification (derived).
    pub magnification: Option<Array2<f64>>,
    /// Azimuth iso-contours (derived).
    pub contours_azi: Option<Array2<bool>>,
    /// Altitude iso-contours (derived).
    pub contours_alt: Option<Array2<bool>>,
    /// Threshold-masked smoothed VFS, diagnostic (derived).
    pub vfs_smoothed_thresholded: Option<Array2<f64>>,
}

impl PipelineState {
    /// Borrow `complex_maps`, or a typed error if a stage ran out of order.
    pub fn complex_maps_ref(&self) -> Result<&ComplexMaps, AnalysisError> {
        self.complex_maps
            .as_ref()
            .ok_or_else(|| missing("complex maps"))
    }

    /// Borrow `retino`, or a typed error if a stage ran out of order.
    pub fn retino_ref(&self) -> Result<&RetinotopyMaps, AnalysisError> {
        self.retino
            .as_ref()
            .ok_or_else(|| missing("retinotopy maps"))
    }

    /// Borrow `vfs_smooth`, or a typed error if a stage ran out of order.
    pub fn vfs_smooth_ref(&self) -> Result<&Array2<f64>, AnalysisError> {
        self.vfs_smooth
            .as_ref()
            .ok_or_else(|| missing("smoothed VFS"))
    }

    /// Borrow `cortex_mask`, or a typed error if a stage ran out of order.
    pub fn cortex_mask_ref(&self) -> Result<&Array2<bool>, AnalysisError> {
        self.cortex_mask
            .as_ref()
            .ok_or_else(|| missing("cortex mask"))
    }

    /// Borrow `imseg`, or a typed error if a stage ran out of order.
    pub fn imseg_ref(&self) -> Result<&Array2<bool>, AnalysisError> {
        self.imseg.as_ref().ok_or_else(|| missing("imseg"))
    }

    /// Borrow `area_labels`, or a typed error if a stage ran out of order.
    pub fn area_labels_ref(&self) -> Result<&Array2<i32>, AnalysisError> {
        self.area_labels
            .as_ref()
            .ok_or_else(|| missing("area labels"))
    }

    /// Take `patches`, or a typed error if a stage ran out of order. Used by
    /// refinement (consumes extraction's patches) and label assembly.
    pub fn take_patches(&mut self) -> Result<Vec<Patch>, AnalysisError> {
        self.patches.take().ok_or_else(|| missing("patches"))
    }
}

fn missing(what: &str) -> AnalysisError {
    AnalysisError::Compute(format!(
        "pipeline ordering bug: {what} read before it was produced"
    ))
}
