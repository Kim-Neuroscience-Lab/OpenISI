//! The incremental cache — the per-stage restore *cut* over the pipeline DAG.
//!
//! Given the freshly-computed Merkle [`StageFingerprints`] and what's stored on
//! disk, this decides, for every cacheable tail stage (`Retinotopy`..
//! `DerivedMaps`), whether it must **execute** or can be **restored**, then seeds
//! the blackboard with the restored stages' outputs read from the `.oisi` file.
//! The pipeline itself stays HDF5-free; this boundary owns the fingerprint
//! comparison + disk reads (the seam the orchestrator's `restored` set plugs
//! into).
//!
//! **The decision (demand-driven, never-stale).** A stage *executes* iff either:
//!   - its fingerprint no longer matches the stored one (its inputs changed —
//!     recompute, never serve stale); or
//!   - *(persisted stages)* its cached artifacts are absent (nothing to restore);
//!     or
//!   - *(non-persisted stages — `PatchExtraction`/`PatchRefinement`, whose
//!     `Vec<Patch>` output is never written to disk)* some downstream stage that
//!     consumes that output will itself execute, so this stage must run to
//!     supply it.
//!
//! That last clause is the key to skipping the ~1 s patch-refinement hotspot
//! when only a downstream tail param changes: if `Labels` (refinement's only
//! consumer) restores from `/results`, refinement's output is never needed, so
//! refinement — and the whole patch chain above it — is skipped. Conversely a
//! patch-refinement param edit makes refinement (and, demand-driven, extraction)
//! execute, restoring `imseg`/`threshold_applied` from `/cache` as their input.
//!
//! By the Merkle property the executing set is descendant-closed and the
//! restored set ancestor-closed, so the cut is always consistent: an executing
//! stage's inputs are either restored from disk or produced by an upstream
//! stage that also executes.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::io;
use crate::pipeline::fingerprint::StageFingerprints;
use crate::pipeline::{self, CacheClass, PipelineState, StageId};
use crate::AnalysisError;

/// The cacheable tail stages, in pipeline order — **derived** from
/// [`StageId::cache_class`] (the single source of truth), never hand-listed, so a
/// new stage is classified in exactly one place and cannot silently bypass the
/// cut. `Baseline`/`Projection` (`CacheClass::NotCached`) are excluded: their
/// cache is the separate `/complex_maps` projection fingerprint, folded
/// transitively into every tail key.
fn tail_stages() -> Vec<StageId> {
    StageId::ALL
        .into_iter()
        .filter(|s| s.cache_class() != CacheClass::NotCached)
        .collect()
}

/// Whether a stage's output is persisted to disk (so it can be *restored*) vs.
/// recomputed on demand. The non-persisted tail stages produce a `Vec<Patch>`
/// consumed only within the patch chain; they are never written, so they restore
/// only by virtue of all their consumers restoring.
fn persisted(id: StageId) -> bool {
    id.cache_class() == CacheClass::Persisted
}

fn is_tail(id: StageId) -> bool {
    id.cache_class() != CacheClass::NotCached
}

/// Compute the restore cut and seed `state` with every restored stage's output.
///
/// Returns the augmented blackboard plus the set of tail stages the orchestrator
/// should skip. `state` arrives with stage 0 (complex maps + reliability +
/// responsiveness) already seeded by `analyze`; this adds the restorable tail
/// outputs to it.
pub fn restore(
    path: &Path,
    fps: &StageFingerprints,
    mut state: PipelineState,
) -> Result<(PipelineState, HashSet<StageId>), AnalysisError> {
    let stored = io::read_all_stage_fingerprints(path)?;
    let artifacts = io::stage_artifacts_present(path)?;

    let fp_ok = |id: StageId| stored.get(id.fingerprint_key()).map(String::as_str) == Some(fps.get(id));
    // Exhaustive over `StageId` (no wildcard) so a new variant forces a decision
    // about its restore artifact. Non-persisted / not-cached stages have no
    // on-disk artifact; the value is never consulted for them (their execution is
    // decided by their dependents, not by artifact presence).
    let artifacts_present = |id: StageId| match id {
        StageId::Retinotopy => artifacts.retinotopy,
        StageId::SignSmoothing => artifacts.sign_smoothing,
        StageId::CortexSource => artifacts.cortex_source,
        StageId::PatchThreshold => artifacts.patch_threshold,
        StageId::Labels => artifacts.labels,
        StageId::Eccentricity => artifacts.eccentricity,
        StageId::DerivedMaps => artifacts.derived_maps,
        StageId::PatchExtraction
        | StageId::PatchRefinement
        | StageId::Baseline
        | StageId::Projection => true,
    };

    let executes = compute_executes(&fp_ok, &artifacts_present);
    let runs = |id: StageId| *executes.get(&id).unwrap_or(&true);

    // Seed each *restored* (non-executing) tail stage's output from disk. A
    // non-persisted restored stage (extraction/refinement) has no output to seed
    // — by the cut, all its consumers are also restored, so it's never read.
    for s in tail_stages() {
        if runs(s) {
            continue;
        }
        match s {
            StageId::Retinotopy => {
                state.retino = Some(io::read_retinotopy_maps(path)?.ok_or_else(|| {
                    AnalysisError::MissingData("retinotopy cache present then unreadable".into())
                })?);
            }
            StageId::SignSmoothing => {
                state.vfs_smooth = Some(io::read_result_map(path, "vfs_smoothed")?);
            }
            StageId::CortexSource => {
                state.cortex_mask = Some(io::read_result_mask(path, "cortex_mask")?);
            }
            StageId::PatchThreshold => {
                // Seed only the intermediates a *running* dependent will read.
                if runs(StageId::PatchExtraction) {
                    state.imseg = Some(io::read_cache_imseg(path)?);
                }
                if runs(StageId::DerivedMaps) {
                    state.threshold_applied = Some(io::read_cache_threshold(path)?);
                }
            }
            StageId::PatchExtraction | StageId::PatchRefinement => {
                // Non-persisted: nothing on disk to seed (and nothing needs it).
            }
            StageId::Labels => {
                state.area_labels = Some(io::read_result_labels(path)?);
                state.area_signs = Some(io::read_result_signs(path)?);
                state.area_borders = Some(io::read_result_mask(path, "area_borders")?);
            }
            StageId::Eccentricity => {
                state.eccentricity = Some(io::read_result_map(path, "eccentricity")?);
            }
            StageId::DerivedMaps => {
                state.magnification = Some(io::read_result_map(path, "magnification")?);
                state.contours_azi = Some(io::read_result_mask(path, "contours_azi")?);
                state.contours_alt = Some(io::read_result_mask(path, "contours_alt")?);
                state.vfs_smoothed_thresholded =
                    Some(io::read_result_map(path, "vfs_smoothed_thresholded")?);
            }
            StageId::Baseline | StageId::Projection => unreachable!("not a tail stage"),
        }
    }

    let restored: HashSet<StageId> = tail_stages().into_iter().filter(|&s| !runs(s)).collect();
    Ok((state, restored))
}

/// Solve the execution flags for the tail stages to a fixpoint. Persisted stages
/// are decided directly (fingerprint + artifacts); a non-persisted stage runs if
/// its own fingerprint changed or any tail dependent runs. Iterating to a
/// fixpoint (≤ one pass per stage) propagates "a consumer runs" up the
/// non-persisted patch chain without hard-coding the topological order.
fn compute_executes(
    fp_ok: &dyn Fn(StageId) -> bool,
    artifacts_present: &dyn Fn(StageId) -> bool,
) -> HashMap<StageId, bool> {
    // Reverse the DAG edges (SSoT: each stage's `deps()`) into a dependents map.
    let mut dependents: HashMap<StageId, Vec<StageId>> = HashMap::new();
    for (id, deps) in pipeline::stage_dependencies() {
        for dep in deps {
            dependents.entry(*dep).or_default().push(id);
        }
    }

    let tail = tail_stages();
    let mut executes: HashMap<StageId, bool> = HashMap::new();
    for _ in 0..tail.len() {
        let mut changed = false;
        for &s in &tail {
            let v = if persisted(s) {
                !fp_ok(s) || !artifacts_present(s)
            } else {
                !fp_ok(s)
                    || dependents
                        .get(&s)
                        .map(|ds| {
                            ds.iter()
                                .filter(|d| is_tail(**d))
                                // An as-yet-unresolved dependent is assumed to run
                                // (conservative over-approximation); the fixpoint
                                // tightens it monotonically downward.
                                .any(|d| *executes.get(d).unwrap_or(&true))
                        })
                        .unwrap_or(false)
            };
            if executes.insert(s, v) != Some(v) {
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    executes
}
