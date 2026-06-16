//! Stage fingerprints for the incremental cache — a **Merkle DAG** over the
//! pipeline stages.
//!
//! Each stage's fingerprint is a blake3 hash over that stage's **direct inputs**
//! — its own parameters, the acquisition geometry it reads — **plus the
//! fingerprints of its dependency stages** (`Stage::deps()`). Chaining the
//! dependency fingerprints (rather than re-flattening every transitive input at
//! each node) makes upstream changes propagate automatically: a baseline edit
//! changes `baseline`'s fingerprint, which changes `projection`'s, which changes
//! `retinotopy`'s, and so on down the DAG. This is the Bazel/Nix
//! "derivation/action key" shape — each action's key folds in its inputs' keys.
//!
//! A cached stage output may be restored only if its **stored** fingerprint
//! exactly matches the **recomputed** one; any mismatch recomputes, loudly. It
//! never serves stale: the risk asymmetry is stale = wrong science vs
//! over-invalidate = a few seconds of recompute, so we bias to over-invalidate.
//!
//! **Cache-vs-code-change safety.** Every fingerprint folds in a *code identity*
//! ([`CARGO_PKG_VERSION`] automatically + [`PIPELINE_ALGO_VERSION`] manually) so
//! a change to the *algorithm* (not its inputs) invalidates the cache. Bump
//! `PIPELINE_ALGO_VERSION` when changing pipeline math within a release; the
//! cross-implementation equivalence test fails precisely when output drifts,
//! which is the signal a bump is due.
//!
//! **Coverage invariant (load-bearing — do not weaken).** Every `match` over a
//! method enum here is exhaustive with NO `_` wildcard, and every variant that
//! carries tunables is destructured fully (e.g. `{ sigma_px }`) with NO `..`
//! rest-pattern. Same-crate exhaustiveness then makes BOTH a new variant AND a
//! new tunable field a hard compile error until its fingerprint contribution is
//! defined — which is what guarantees a param change can't silently leave a
//! cache key stale. The `tests/incremental.rs` sensitivity tests pin this per
//! input. Never introduce a wildcard or rest-pattern in this module.

use blake3::Hasher;

use super::StageId;
use crate::methods::{
    BaselineMethod, CortexSourceMethod, CycleAverageMethod, CycleCombineMethod, EccentricityMethod,
    PatchExtractionMethod, PatchRefinementMethod, PatchThresholdMethod, PhaseSmoothingMethod,
    SignMapSmoothingMethod, VfsComputationMethod,
};
use crate::{AcquisitionProperties, AnalysisParams};

/// Manual algorithm-version tag for the whole pipeline. **Bump this whenever you
/// change pipeline math** in a way that alters output for unchanged inputs. The
/// cross-implementation equivalence test fails precisely when output drifts,
/// which is your signal that a bump is due.
pub const PIPELINE_ALGO_VERSION: u32 = 2;

/// A new hasher seeded with the code identity and the stage's name. Every
/// stage fingerprint starts here, so a crate/algo-version bump invalidates all.
/// The stage tag is the stage's [`StageId::fingerprint_key`] — the same SSoT the
/// on-disk attribute name uses, so the two can never disagree about a stage.
fn base_hasher(stage: StageId) -> Hasher {
    let mut h = Hasher::new();
    h.update(b"oisi.pipeline.v");
    h.update(&PIPELINE_ALGO_VERSION.to_le_bytes());
    h.update(b"|crate:");
    h.update(env!("CARGO_PKG_VERSION").as_bytes());
    h.update(b"|stage:");
    h.update(stage.fingerprint_key().as_bytes());
    h
}

/// Fold a dependency stage's fingerprint into this stage's hash.
fn dep(h: &mut Hasher, fp: &str) {
    h.update(b"|dep:");
    h.update(fp.as_bytes());
}

fn hex(h: Hasher) -> String {
    h.finalize().to_hex().to_string()
}

// ── Per-method input emitters (full-destructure; the coverage invariant) ──────

fn emit_baseline(h: &mut Hasher, b: &BaselineMethod) {
    h.update(b"|baseline:");
    h.update(match b {
        BaselineMethod::AllenAllFrameMean => b"af_mean".as_slice(),
        BaselineMethod::AllenAllFrameMedian => b"af_median",
        BaselineMethod::OpenIsiInterSweepMean => b"is_mean",
        BaselineMethod::OpenIsiInterSweepMedian => b"is_median",
    });
}

fn emit_cycle_average(h: &mut Hasher, m: &CycleAverageMethod) {
    h.update(b"|cycle_average:");
    h.update(match m {
        CycleAverageMethod::SimpleComplexAverage => b"simple".as_slice(),
        CycleAverageMethod::PhaseLockedAverage => b"phase_locked",
    });
}

fn emit_cycle_combine(h: &mut Hasher, m: &CycleCombineMethod) {
    h.update(b"|cycle_combine:");
    h.update(match m {
        CycleCombineMethod::KalatskyStryker2003DelaySubtraction => b"ks2003".as_slice(),
        CycleCombineMethod::UnweightedCycleAverage => b"uca",
    });
}

fn emit_phase_smoothing(h: &mut Hasher, m: &PhaseSmoothingMethod) {
    h.update(b"|phase_smoothing:");
    match m {
        // Same `awp:` tag as before the SNLC rename — the math is unchanged, so
        // the fingerprint must stay valid across the rename.
        PhaseSmoothingMethod::SnlcAmpWeightedPhasor { sigma_px } => {
            h.update(b"awp:");
            h.update(&sigma_px.to_le_bytes());
        }
        PhaseSmoothingMethod::AllenZhuang2017PositionGaussian { sigma_px } => {
            h.update(b"allen_pos_gauss:");
            h.update(&sigma_px.to_le_bytes());
        }
    }
}

fn emit_vfs(h: &mut Hasher, m: &VfsComputationMethod) {
    h.update(b"|vfs_computation:");
    match m {
        VfsComputationMethod::OpenIsiChainRulePhasorGradient => h.update(b"crpg"),
    };
}

fn emit_sign_map_smoothing(h: &mut Hasher, m: &SignMapSmoothingMethod) {
    h.update(b"|sign_map_smoothing:");
    match m {
        SignMapSmoothingMethod::Gaussian { sigma_um } => {
            h.update(b"gaussian:");
            h.update(&sigma_um.to_le_bytes());
        }
    }
}

fn emit_cortex_source(h: &mut Hasher, m: &CortexSourceMethod) {
    h.update(b"|cortex_source:");
    match m {
        CortexSourceMethod::Reliability { threshold } => {
            h.update(b"reliability:");
            h.update(&threshold.to_le_bytes());
        }
        CortexSourceMethod::UserPolygon => {
            h.update(b"user_polygon");
        }
        CortexSourceMethod::SnlcGarrett2014ImBound { k, close, dilate } => {
            h.update(b"snlc_imbound:");
            h.update(&k.to_le_bytes());
            h.update(&close.to_le_bytes());
            h.update(&dilate.to_le_bytes());
        }
        CortexSourceMethod::NoRestriction => {
            h.update(b"no_restriction");
        }
    }
}

fn emit_patch_threshold(h: &mut Hasher, m: &PatchThresholdMethod) {
    h.update(b"|patch_threshold:");
    match m {
        PatchThresholdMethod::AllenZhuang2017FixedSignMapThr { value } => {
            h.update(b"allen_fixed:");
            h.update(&value.to_le_bytes());
        }
        PatchThresholdMethod::Garrett2014SigmaScaled { k } => {
            h.update(b"garrett_sigma:");
            h.update(&k.to_le_bytes());
        }
    }
}

fn emit_patch_extraction(h: &mut Hasher, m: &PatchExtractionMethod) {
    h.update(b"|patch_extraction:");
    match m {
        PatchExtractionMethod::AllenZhuang2017LabelOpenCloseDilate {
            open_iter,
            close_iter,
            dilation_iter,
            border_width,
            small_patch_thr,
        } => {
            h.update(b"allen_locd:");
            h.update(&open_iter.to_le_bytes());
            h.update(&close_iter.to_le_bytes());
            h.update(&dilation_iter.to_le_bytes());
            h.update(&border_width.to_le_bytes());
            h.update(&small_patch_thr.to_le_bytes());
        }
    }
}

fn emit_patch_refinement(h: &mut Hasher, m: &PatchRefinementMethod) {
    h.update(b"|patch_refinement:");
    match m {
        PatchRefinementMethod::None => {
            h.update(b"none");
        }
        PatchRefinementMethod::AllenZhuang2017SplitMerge {
            split_overlap_thr,
            split_local_min_cut_step,
            merge_overlap_thr,
            visual_space_pixel_size,
            visual_space_close_iter,
            ecc_map_filter_sigma,
            border_width,
            small_patch_thr,
        } => {
            h.update(b"allen_split_merge:");
            h.update(&split_overlap_thr.to_le_bytes());
            h.update(&split_local_min_cut_step.to_le_bytes());
            h.update(&merge_overlap_thr.to_le_bytes());
            h.update(&visual_space_pixel_size.to_le_bytes());
            h.update(&visual_space_close_iter.to_le_bytes());
            h.update(&ecc_map_filter_sigma.to_le_bytes());
            h.update(&border_width.to_le_bytes());
            h.update(&small_patch_thr.to_le_bytes());
        }
    }
}

fn emit_eccentricity(h: &mut Hasher, m: &EccentricityMethod) {
    h.update(b"|eccentricity:");
    h.update(match m {
        EccentricityMethod::OpenIsiWholeCortexV1 => b"openisi_v1".as_slice(),
        EccentricityMethod::SnlcGetAreaBordersV1Center => b"snlc_getareaborders",
    });
}

/// Retinotopy geometry (rotation, angular ranges, offsets) — consumed by the
/// Retinotopy stage for degree scaling and magnification.
fn emit_retino_geometry(h: &mut Hasher, acq: &AcquisitionProperties) {
    h.update(b"|rotation_k:");
    h.update(&acq.rotation_k.to_le_bytes());
    h.update(b"|azi_range:");
    h.update(&acq.azi_angular_range.to_le_bytes());
    h.update(b"|alt_range:");
    h.update(&acq.alt_angular_range.to_le_bytes());
    h.update(b"|offset_azi:");
    h.update(&acq.offset_azi.to_le_bytes());
    h.update(b"|offset_alt:");
    h.update(&acq.offset_alt.to_le_bytes());
}

// ── The Merkle DAG over the 11 stages ─────────────────────────────────────────

/// Per-stage fingerprints for the incremental cache. Each field is the
/// Merkle key of that stage — its direct inputs plus its dependency stages'
/// keys. A stage may be restored from cache only if its stored key matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageFingerprints {
    pub baseline: String,
    pub projection: String,
    pub retinotopy: String,
    pub sign_smoothing: String,
    pub cortex_source: String,
    pub patch_threshold: String,
    pub patch_extraction: String,
    pub patch_refinement: String,
    pub labels: String,
    pub eccentricity: String,
    pub derived_maps: String,
}

impl StageFingerprints {
    /// The Merkle key of a given stage. Used by the incremental cut to compare a
    /// freshly-computed key against the one stored on disk.
    pub fn get(&self, id: StageId) -> &str {
        match id {
            StageId::Baseline => &self.baseline,
            StageId::Projection => &self.projection,
            StageId::Retinotopy => &self.retinotopy,
            StageId::SignSmoothing => &self.sign_smoothing,
            StageId::CortexSource => &self.cortex_source,
            StageId::PatchThreshold => &self.patch_threshold,
            StageId::PatchExtraction => &self.patch_extraction,
            StageId::PatchRefinement => &self.patch_refinement,
            StageId::Labels => &self.labels,
            StageId::Eccentricity => &self.eccentricity,
            StageId::DerivedMaps => &self.derived_maps,
        }
    }

    /// `(fingerprint_key, value)` pairs for the cacheable tail stages
    /// (`Retinotopy`..`DerivedMaps`) — the ones the incremental cut restores and
    /// `analyze` persists after every run. `Baseline`/`Projection` are not here:
    /// their cache is keyed separately (the `/complex_maps` projection
    /// fingerprint), folded transitively into every tail key.
    pub fn tail_pairs(&self) -> [(&'static str, &str); 9] {
        [
            (StageId::Retinotopy.fingerprint_key(), &self.retinotopy),
            (StageId::SignSmoothing.fingerprint_key(), &self.sign_smoothing),
            (StageId::CortexSource.fingerprint_key(), &self.cortex_source),
            (StageId::PatchThreshold.fingerprint_key(), &self.patch_threshold),
            (StageId::PatchExtraction.fingerprint_key(), &self.patch_extraction),
            (StageId::PatchRefinement.fingerprint_key(), &self.patch_refinement),
            (StageId::Labels.fingerprint_key(), &self.labels),
            (StageId::Eccentricity.fingerprint_key(), &self.eccentricity),
            (StageId::DerivedMaps.fingerprint_key(), &self.derived_maps),
        ]
    }
}

/// Compute the Merkle fingerprint of every stage, in dependency order. The DAG
/// edges below MUST mirror each stage's `Stage::deps()` (verified: `deps()`
/// transitively covers every value a stage reads from `PipelineState`, so the
/// chain captures all input identities and cannot under-invalidate).
///
/// `raw_identity` is the recording's content identity (the pipeline-root input).
/// `user_polygon_id` identifies the user-drawn cortex ROI read by the
/// `UserPolygon` cortex source (an external file input, not a stage output);
/// `None` when the file carries no polygon.
pub fn compute(
    params: &AnalysisParams,
    acq: &AcquisitionProperties,
    raw_identity: &str,
    user_polygon_id: Option<&str>,
) -> StageFingerprints {
    // Baseline — direct: baseline method + the raw recording identity.
    let baseline = {
        let mut h = base_hasher(StageId::Baseline);
        emit_baseline(&mut h, &params.baseline);
        h.update(b"|raw:");
        h.update(raw_identity.as_bytes());
        hex(h)
    };

    // Projection (per-cycle DFT → complex maps) — direct: cycle_average. The DFT
    // itself is parameterless; deps = [Baseline].
    let projection = {
        let mut h = base_hasher(StageId::Projection);
        emit_cycle_average(&mut h, &params.cycle_average);
        dep(&mut h, &baseline);
        hex(h)
    };

    // Retinotopy (cycle-combine + smoothing + VFS + assembly) — direct:
    // cycle_combine, phase_smoothing, vfs, geometry; deps = [Projection].
    let retinotopy = {
        let mut h = base_hasher(StageId::Retinotopy);
        emit_cycle_combine(&mut h, &params.cycle_combine);
        emit_phase_smoothing(&mut h, &params.phase_smoothing);
        emit_vfs(&mut h, &params.vfs_computation);
        emit_retino_geometry(&mut h, acq);
        dep(&mut h, &projection);
        hex(h)
    };

    // SignSmoothing — direct: sign_map_smoothing + um_per_pixel; deps = [Retinotopy].
    let sign_smoothing = {
        let mut h = base_hasher(StageId::SignSmoothing);
        emit_sign_map_smoothing(&mut h, &params.sign_map_smoothing);
        h.update(b"|um_per_pixel:");
        h.update(&acq.um_per_pixel.to_le_bytes());
        dep(&mut h, &retinotopy);
        hex(h)
    };

    // CortexSource — direct: cortex_source params + the user-polygon identity
    // (external file input); deps = [SignSmoothing] (reliability identity is
    // captured transitively via Retinotopy → Projection).
    let cortex_source = {
        let mut h = base_hasher(StageId::CortexSource);
        emit_cortex_source(&mut h, &params.cortex_source);
        h.update(b"|user_polygon:");
        h.update(user_polygon_id.unwrap_or("none").as_bytes());
        dep(&mut h, &sign_smoothing);
        hex(h)
    };

    // PatchThreshold — direct: patch_threshold; deps = [SignSmoothing, CortexSource].
    let patch_threshold = {
        let mut h = base_hasher(StageId::PatchThreshold);
        emit_patch_threshold(&mut h, &params.patch_threshold);
        dep(&mut h, &sign_smoothing);
        dep(&mut h, &cortex_source);
        hex(h)
    };

    // PatchExtraction — direct: patch_extraction; deps = [PatchThreshold, SignSmoothing].
    let patch_extraction = {
        let mut h = base_hasher(StageId::PatchExtraction);
        emit_patch_extraction(&mut h, &params.patch_extraction);
        dep(&mut h, &patch_threshold);
        dep(&mut h, &sign_smoothing);
        hex(h)
    };

    // PatchRefinement — direct: patch_refinement; deps = [PatchExtraction, Retinotopy].
    let patch_refinement = {
        let mut h = base_hasher(StageId::PatchRefinement);
        emit_patch_refinement(&mut h, &params.patch_refinement);
        dep(&mut h, &patch_extraction);
        dep(&mut h, &retinotopy);
        hex(h)
    };

    // Labels — no direct params (derives from patches + vfs_smooth); deps =
    // [PatchRefinement] (SignSmoothing captured transitively via PatchExtraction).
    let labels = {
        let mut h = base_hasher(StageId::Labels);
        dep(&mut h, &patch_refinement);
        hex(h)
    };

    // Eccentricity — direct: eccentricity; deps = [Labels, Retinotopy].
    let eccentricity = {
        let mut h = base_hasher(StageId::Eccentricity);
        emit_eccentricity(&mut h, &params.eccentricity);
        dep(&mut h, &labels);
        dep(&mut h, &retinotopy);
        hex(h)
    };

    // DerivedMaps — no direct params (contours/magnification derive from prior
    // outputs); deps = [Labels, Retinotopy, SignSmoothing, CortexSource, PatchThreshold].
    let derived_maps = {
        let mut h = base_hasher(StageId::DerivedMaps);
        dep(&mut h, &labels);
        dep(&mut h, &retinotopy);
        dep(&mut h, &sign_smoothing);
        dep(&mut h, &cortex_source);
        dep(&mut h, &patch_threshold);
        hex(h)
    };

    StageFingerprints {
        baseline,
        projection,
        retinotopy,
        sign_smoothing,
        cortex_source,
        patch_threshold,
        patch_extraction,
        patch_refinement,
        labels,
        eccentricity,
        derived_maps,
    }
}

/// Fingerprint of the `Projection` stage (the complex maps). Compatibility
/// wrapper over the Merkle [`compute`] — gates reuse of a cached `/complex_maps`
/// when a recording has raw frames. (Acquisition geometry / polygon don't affect
/// projection, so any value yields the same key for this stage.)
pub fn projection(params: &AnalysisParams, raw_identity: &str) -> String {
    compute(params, &AcquisitionProperties::default(), raw_identity, None).projection
}

/// Fingerprint of the `Retinotopy` stage (the expensive device compute).
/// Compatibility wrapper over the Merkle [`compute`].
pub fn retinotopy(params: &AnalysisParams, acq: &AcquisitionProperties, raw_identity: &str) -> String {
    compute(params, acq, raw_identity, None).retinotopy
}

// Fingerprint stability/sensitivity is exercised in `tests/incremental.rs`,
// where a typed config snapshot (the real source of an `AnalysisParams`) is
// available — `AnalysisParams` has no standalone `Default`.
