//! The `Stage` trait, the `StageId` enum, and the per-run `StageCtx`.
//!
//! A `Stage` is a thin wrapper around an existing `methods/*.rs` `apply()` —
//! it reads its inputs from the [`PipelineState`] blackboard (and the
//! immutable [`StageCtx`]), calls the canonical method, and writes its output
//! back. **No stage reimplements algorithm logic** — the method enums in
//! `openisi-params` / `methods/*.rs` remain the single source of truth.
//!
//! The trait carries `id`/`deps`/`execute` today; Part 3's incremental engine
//! adds the cache seam (`cacheable`/`fingerprint`/`restore`/`persist`). For now
//! the orchestrator runs every stage in topological order — reproducing the
//! procedural `compute_analysis` exactly (the equivalence test is the gate).

use std::sync::atomic::AtomicBool;

use ndarray::Array2;

use crate::{AcquisitionProperties, AnalysisError, AnalysisParams, ProgressSink, RawAcquisition};

use super::state::PipelineState;

/// Identity of each pipeline stage. Topological order over the pipeline DAG
/// reproduces the procedural sequence in `compute_analysis`. Former stages
/// 1–3 are fused into `Retinotopy` (they exchange device tensors with no host
/// boundary); the dead no-op quality-gate stage is removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StageId {
    /// ΔF/F baseline `F0` (method-dispatched). Tiny `[H, W]` output the
    /// `Projection` stage consumes. Skipped when complex maps are already seeded.
    Baseline,
    /// Per-cycle ΔF/F (using `F0`) → bin-1 DFT → per-direction complex maps
    /// (+ reliability, SNR). The Fourier-projection half of the former stage 0,
    /// now homed in the pipeline. Skipped (via `is_satisfied`) when the boundary
    /// seeds the maps from a cached `/complex_maps` or an import.
    Projection,
    /// Fused cycle-combine + phasor-smoothing + VFS + assembly (former 1–3).
    Retinotopy,
    /// Sign-map (VFS) smoothing (former stage 4).
    SignSmoothing,
    /// Imaged-cortex resolution (former stage 5).
    CortexSource,
    /// Patch threshold → binary candidate mask (former stage 6).
    PatchThreshold,
    /// Patch extraction → labeled patches (former stage 7).
    PatchExtraction,
    /// Patch refinement → split/merge (former stage 8).
    PatchRefinement,
    /// Sort patches; assemble area labels / signs / borders.
    Labels,
    /// Eccentricity map (former stage 10).
    Eccentricity,
    /// Universal derived maps: magnification, contours, thresholded VFS.
    DerivedMaps,
}

/// How a stage participates in the incremental cache. Declared per stage by
/// [`StageId::cache_class`] so the *one* exhaustive match is the single source of
/// truth for the restore cut — adding a `StageId` variant is a compile error
/// until its cache behavior is stated here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheClass {
    /// Not part of the tail cut. `Baseline`/`Projection` are restored via the
    /// separate `/complex_maps` projection fingerprint, folded transitively into
    /// every tail key.
    NotCached,
    /// Output is written to disk (`/results` or `/cache`) and restored from it
    /// when this stage's fingerprint matches.
    Persisted,
    /// Output (a `Vec<Patch>`) is never written to disk; the stage is restored
    /// (skipped) only when all its consumers are also restored, and otherwise
    /// co-recomputes on demand. `PatchExtraction`/`PatchRefinement`.
    NonPersisted,
}

impl StageId {
    /// Every stage, in pipeline order. Pairs with the exhaustive matches in
    /// [`Self::cache_class`] / [`Self::fingerprint_key`]: the incremental cut
    /// derives its stage set from this rather than a hand-kept list, so a new
    /// variant can't silently bypass the cache.
    pub const ALL: [StageId; 11] = [
        StageId::Baseline,
        StageId::Projection,
        StageId::Retinotopy,
        StageId::SignSmoothing,
        StageId::CortexSource,
        StageId::PatchThreshold,
        StageId::PatchExtraction,
        StageId::PatchRefinement,
        StageId::Labels,
        StageId::Eccentricity,
        StageId::DerivedMaps,
    ];

    /// This stage's role in the incremental cache. The single source of truth
    /// for the restore cut (`crate::incremental`); exhaustive, no wildcard, so
    /// adding a stage forces a deliberate decision here.
    pub fn cache_class(self) -> CacheClass {
        match self {
            // Stage 0: cached via the projection fingerprint, not the tail cut.
            StageId::Baseline | StageId::Projection => CacheClass::NotCached,
            // Outputs land in `/results` (or `/cache` for PatchThreshold).
            StageId::Retinotopy
            | StageId::SignSmoothing
            | StageId::CortexSource
            | StageId::PatchThreshold
            | StageId::Labels
            | StageId::Eccentricity
            | StageId::DerivedMaps => CacheClass::Persisted,
            // `Vec<Patch>` — never serialized; restored only when consumers are.
            StageId::PatchExtraction | StageId::PatchRefinement => CacheClass::NonPersisted,
        }
    }

    /// Stable on-disk key for this stage's fingerprint (the `/analysis_state`
    /// attribute name) — distinct from `label` (which is human-facing and may be
    /// reworded). Changing one of these strings silently invalidates that
    /// stage's cache on existing files, so treat them as a persisted format.
    pub fn fingerprint_key(self) -> &'static str {
        match self {
            StageId::Baseline => "baseline",
            StageId::Projection => "projection",
            StageId::Retinotopy => "retinotopy",
            StageId::SignSmoothing => "sign_smoothing",
            StageId::CortexSource => "cortex_source",
            StageId::PatchThreshold => "patch_threshold",
            StageId::PatchExtraction => "patch_extraction",
            StageId::PatchRefinement => "patch_refinement",
            StageId::Labels => "labels",
            StageId::Eccentricity => "eccentricity",
            StageId::DerivedMaps => "derived_maps",
        }
    }

    /// Stable human-readable name (matches the `[analyze] stage:` log lines).
    pub fn label(self) -> &'static str {
        match self {
            StageId::Baseline => "ΔF/F baseline",
            StageId::Projection => "Fourier projection (complex maps)",
            StageId::Retinotopy => "retinotopy",
            StageId::SignSmoothing => "sign map smoothing",
            StageId::CortexSource => "cortex source resolve",
            StageId::PatchThreshold => "patch threshold",
            StageId::PatchExtraction => "patch extraction",
            StageId::PatchRefinement => "patch refinement",
            StageId::Labels => "label assembly",
            StageId::Eccentricity => "eccentricity",
            StageId::DerivedMaps => "derived maps",
        }
    }
}

/// Immutable per-run inputs shared by every stage. These are the pipeline's
/// *inputs* (the raw acquisition the `Baseline`/`Projection` stages consume, the
/// caller's `user_polygon`, geometry/calibration, the param set, plus the
/// cancel/progress channels projection needs) — distinct from the *produced*
/// intermediates in [`PipelineState`] (incl. `complex_maps`/`reliability`/`snr`).
pub struct StageCtx<'a> {
    /// Raw acquisition (frames + timing + schedule) — consumed by `Baseline`
    /// (F0) and `Projection` (DFT). `None` when the boundary seeded the complex
    /// maps from a cache/import (then both stages are skipped).
    pub raw: Option<&'a RawAcquisition>,
    /// Caller-supplied cortex polygon — consumed by `CortexSource`.
    pub user_polygon: Option<&'a Array2<bool>>,
    /// Acquisition geometry/calibration (rotation, angular range, offsets, µm/px).
    pub acquisition: &'a AcquisitionProperties,
    /// The full analysis parameter set (each stage reads its own slice).
    pub params: &'a AnalysisParams,
    /// Cancellation flag — the `Projection` per-cycle DFT checks it (the only
    /// stage long enough to need interruption mid-execute).
    pub cancel: &'a AtomicBool,
    /// Progress sink — `Projection` reports per-direction progress through it.
    pub progress: &'a dyn ProgressSink,
}

/// The ambient environment for a pipeline run — the geometry/params/channels
/// that are constant across the whole walk, as opposed to the per-call inputs
/// (`raw`, the seeded `PipelineState`, the restore set, the cortex polygon).
/// `orchestrator::run` takes one of these instead of four loose arguments; it
/// fans out into each stage's [`StageCtx`].
pub struct RunEnv<'a> {
    /// Acquisition geometry/calibration (rotation, angular range, offsets, µm/px).
    pub acquisition: &'a AcquisitionProperties,
    /// The full analysis parameter set (each stage reads its own slice).
    pub params: &'a AnalysisParams,
    /// Cancellation flag — checked at every stage boundary (and within the two
    /// long stages).
    pub cancel: &'a AtomicBool,
    /// Progress sink — reports the active stage.
    pub progress: &'a dyn ProgressSink,
}

/// A pipeline stage: a thin wrapper around a canonical `methods/*.rs` `apply()`.
pub trait Stage {
    /// This stage's identity.
    fn id(&self) -> StageId;

    /// Stages whose output this one consumes (the DAG edges).
    fn deps(&self) -> &'static [StageId];

    /// Run the stage: read inputs from `st`/`ctx`, call the canonical method,
    /// write outputs back into `st`.
    fn execute(&self, st: &mut PipelineState, ctx: &StageCtx) -> Result<(), AnalysisError>;

    /// Whether this stage's output is already present in `st` — i.e. it was
    /// restored from the incremental cache before the walk — so `execute` can
    /// be skipped. Default `false` (always run). A cacheable stage overrides
    /// this to report when its output has been seeded.
    ///
    /// The fingerprint check + disk read that decide *whether* to seed live at
    /// the I/O boundary (`analyze` + `io.rs` + [`super::fingerprint`]); the
    /// pipeline itself stays free of HDF5 — it only honors what was seeded.
    fn is_satisfied(&self, _st: &PipelineState) -> bool {
        false
    }
}
