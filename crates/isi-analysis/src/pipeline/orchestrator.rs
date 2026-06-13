//! The pipeline orchestrator. Builds the DAG, walks it in topological order,
//! runs each stage (skipping those restored from the incremental cache), and
//! assembles the final [`AnalysisResult`] from the blackboard.
//!
//! A stage is skipped when it was **seeded** by the boundary — either an
//! import/cache seed surfaced by `is_satisfied` (stage 0's complex maps), or a
//! member of the `restored` set the incremental cut computed (every cacheable
//! tail stage whose Merkle fingerprint matched what produced the cached output).
//! The fingerprint comparison and disk reads live at the I/O boundary
//! (`analyze` + `io.rs` + [`super::fingerprint`]); this walk only honors what was
//! seeded. The cross-implementation equivalence test (empty `restored`, full
//! recompute) is the gate that the walk reproduces the procedural pipeline.

use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::time::Instant;

use ndarray::Array2;

use crate::{AnalysisError, AnalysisResult, RawAcquisition};

use super::graph::StageGraph;
use super::stage::{RunEnv, StageCtx, StageId};
use super::stages::all_stages;
use super::state::PipelineState;

/// What `run` produces: the assembled result plus the two non-result
/// intermediates the incremental cache persists to `/cache` (so a later run can
/// restore the patch-threshold stage without recomputing it). Each is `Some`
/// only when its producing stage actually executed this run; `None` when the
/// stage was restored (the values are already on disk and unchanged).
pub struct RunOutput {
    pub result: AnalysisResult,
    /// Binary candidate-patch mask — `PatchThreshold` output, consumed by
    /// `PatchExtraction`. Not a `/results` field.
    pub imseg: Option<Array2<bool>>,
    /// Scalar `|VFS|` threshold actually applied — `PatchThreshold` byproduct,
    /// consumed by `DerivedMaps`. Not a `/results` field.
    pub threshold_applied: Option<f64>,
}

/// Run the pipeline — stage 0 (complex maps) through derived maps — over a
/// boundary-prepared blackboard.
///
/// `state` arrives pre-seeded by the I/O boundary: stage 0's complex maps (from
/// a cache/import) and every tail-stage output the incremental cut restored from
/// disk. `restored` names the cacheable stages to skip (their output is already
/// in `state`, or — for a stage whose only consumers are themselves restored —
/// is not needed at all). A stage runs iff it is neither in `restored` nor
/// already `is_satisfied` (the stage-0 seed seam). `cancel` is checked at each
/// stage boundary (and per-cycle inside stage 0); `progress` reports the stage.
pub fn run(
    raw: Option<&RawAcquisition>,
    mut state: PipelineState,
    restored: &HashSet<StageId>,
    user_polygon: Option<Array2<bool>>,
    env: RunEnv,
) -> Result<RunOutput, AnalysisError> {
    let RunEnv {
        acquisition,
        params,
        cancel,
        progress,
    } = env;

    let stages = all_stages();
    let order = StageGraph::build(&stages).topo_order()?;

    let ctx = StageCtx {
        raw,
        user_polygon: user_polygon.as_ref(),
        acquisition,
        params,
        cancel,
        progress,
    };

    for id in order {
        // Find the stage with this id (small fixed set; linear scan is fine).
        let stage = stages
            .iter()
            .find(|s| s.id() == id)
            .expect("topo_order yields only ids present in `stages`");
        if restored.contains(&id) || stage.is_satisfied(&state) {
            tracing::debug!(stage = id.label(), "restored from cache");
            continue;
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(AnalysisError::Cancelled);
        }
        progress.set_stage(id.label());
        let started = Instant::now();
        stage.execute(&mut state, &ctx)?;
        tracing::debug!(
            stage = id.label(),
            elapsed_ms = started.elapsed().as_secs_f64() * 1e3,
            "stage complete"
        );
    }

    // Lift the two non-result intermediates out before `assemble` consumes the
    // blackboard, so the boundary can persist them to `/cache`. `imseg` is taken
    // (assemble doesn't read it); `threshold_applied` is `Copy`.
    let imseg = state.imseg.take();
    let threshold_applied = state.threshold_applied;
    let result = assemble(state)?;
    Ok(RunOutput {
        result,
        imseg,
        threshold_applied,
    })
}

/// Move the produced intermediates out of the blackboard into the result.
/// Every field must be `Some` after a full walk; a `None` is an internal bug
/// surfaced as a typed error rather than an `unwrap`.
fn assemble(st: PipelineState) -> Result<AnalysisResult, AnalysisError> {
    let retino = st.retino.ok_or_else(|| missing("retinotopy"))?;
    let complex_maps = st.complex_maps.ok_or_else(|| missing("complex maps"))?;
    Ok(AnalysisResult {
        complex_maps,
        azi_phase: retino.azi_phase,
        alt_phase: retino.alt_phase,
        azi_phase_degrees: retino.azi_phase_degrees,
        alt_phase_degrees: retino.alt_phase_degrees,
        azi_amplitude: retino.azi_amplitude,
        alt_amplitude: retino.alt_amplitude,
        vfs: retino.vfs,
        // Unmasked Jacobian magnitude — persisted so a cached retinotopy can be
        // restored (stage 8 / derived maps need it) and a legitimate output.
        magnification_raw: retino.magnification_raw,
        vfs_smoothed: st.vfs_smooth.ok_or_else(|| missing("vfs_smoothed"))?,
        vfs_smoothed_thresholded: st
            .vfs_smoothed_thresholded
            .ok_or_else(|| missing("vfs_smoothed_thresholded"))?,
        cortex_mask: st.cortex_mask.ok_or_else(|| missing("cortex_mask"))?,
        area_labels: st.area_labels.ok_or_else(|| missing("area_labels"))?,
        area_signs: st.area_signs.ok_or_else(|| missing("area_signs"))?,
        area_borders: st.area_borders.ok_or_else(|| missing("area_borders"))?,
        eccentricity: st.eccentricity.ok_or_else(|| missing("eccentricity"))?,
        magnification: st.magnification.ok_or_else(|| missing("magnification"))?,
        contours_azi: st.contours_azi.ok_or_else(|| missing("contours_azi"))?,
        contours_alt: st.contours_alt.ok_or_else(|| missing("contours_alt"))?,
        // Responsiveness / reliability are projection byproducts, produced by
        // `Projection` or seeded by the boundary — carried straight through.
        responsiveness: st.responsiveness,
        reliability: st.reliability,
    })
}

fn missing(what: &str) -> AnalysisError {
    AnalysisError::Compute(format!("pipeline produced no {what}"))
}
