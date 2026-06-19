//! ISI retinotopy analysis library.
//!
//! Pure Rust implementation of intrinsic signal imaging analysis.
//! Single file format: `.oisi` (HDF5 internally). The system introspects
//! what data is present and determines what operations are available.
//!
//! No GUI dependency. Used by both the Tauri app and the headless CLI.

pub mod bridge;
pub mod compute;
mod incremental;
pub mod io;
pub mod mat5;
pub mod math;
pub mod methods;
pub mod migrate;
/// The `.oisi` schema (single source of truth for the on-disk layout) now lives
/// in the dedicated `oisi` crate; re-exported here so existing
/// `isi_analysis::oisi_schema::*` paths keep resolving.
pub use oisi::schema as oisi_schema;
pub mod params;
pub mod pipeline;
pub mod segmentation;

/// Shared helpers for the in-crate golden tests (fixture decoders + the
/// disagreement-count comparator). Test-only — not compiled into releases.
#[cfg(test)]
mod test_support;

use ndarray::{Array2, Array3};
pub use num_complex::Complex64;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Create a Complex64 from polar coordinates (re-exported for external use).
pub fn complex_from_polar(r: f64, theta: f64) -> Complex64 {
    Complex64::from_polar(r, theta)
}

pub use io::{FileCapabilities, MapMeta};
pub use params::{AcquisitionProperties, AnalysisParams, ProvenanceLevel};

// =============================================================================
// Core data types
// =============================================================================

/// Four complex maps — one per stimulus direction.
/// Universal input to retinotopy computation regardless of data source.
#[derive(Clone)]
pub struct ComplexMaps {
    pub azi_fwd: Array2<Complex64>,
    pub azi_rev: Array2<Complex64>,
    pub alt_fwd: Array2<Complex64>,
    pub alt_rev: Array2<Complex64>,
}

/// Raw acquisition data loaded from an `.oisi` file — the input to pipeline
/// stage 0 (ΔF/F baseline → per-cycle DFT → complex maps + reliability + SNR).
///
/// This is the host-side hand-off that keeps the pipeline HDF5-free: the I/O
/// boundary (`io::read_raw_acquisition`) reads these arrays from the file, and
/// the pipeline's `Baseline`/`Projection` stages borrow them — never touching
/// HDF5 themselves. The
/// `frames` array is large (the full camera movie); it is moved/borrowed, never
/// cloned.
pub struct RawAcquisition {
    /// All camera frames `[T, H, W]` (raw u16 counts).
    pub frames: Array3<u16>,
    /// Per-frame camera timestamps (seconds), length `T`.
    pub cam_ts_sec: Vec<f64>,
    /// Per-sweep stimulus onset times (seconds), from `/acquisition/schedule`.
    pub sweep_start_sec: Vec<f64>,
    /// Per-sweep stimulus offset times (seconds).
    pub sweep_end_sec: Vec<f64>,
    /// Per-sweep direction labels (the `sweep_sequence` SSoT) — the ground
    /// truth for grouping cycles by direction.
    pub sweep_sequence: Vec<String>,
}

/// Retinotopy maps — computed from combined complex maps.
///
/// `magnification_raw` is the unmasked absolute Jacobian determinant of the
/// retinotopic mapping (in degree units), computed on device from the same
/// gradient tensors that produce VFS. ROI gating to segmented areas is
/// applied downstream in `compute_analysis`.
pub struct RetinotopyMaps {
    pub azi_phase: Array2<f64>,
    pub alt_phase: Array2<f64>,
    pub azi_phase_degrees: Array2<f64>,
    pub alt_phase_degrees: Array2<f64>,
    pub azi_amplitude: Array2<f64>,
    pub alt_amplitude: Array2<f64>,
    pub vfs: Array2<f64>,
    pub magnification_raw: Array2<f64>,
    /// Magnification **anisotropy** — SNLC `getMagFactors.m` invariants of the
    /// same Jacobian as `magnification_raw`. `magnification_axis` = `prefAxisMF`
    /// (preferred axis, degrees, wraps at 180°); `magnification_distortion` =
    /// `Distrtion` (anisotropy coherence, `[0,1]`). Full-frame, calibration-free.
    pub magnification_axis: Array2<f64>,
    pub magnification_distortion: Array2<f64>,
    /// Hemodynamic delay maps (degrees, `(0, 180]`) — SNLC `Gprocesskret.m`
    /// `delay_hor`/`delay_vert`. `Some` only under delay-subtraction
    /// cycle-combine; `None` for the unweighted-average method. Carried with
    /// the retinotopy bundle so they cache/restore as a unit.
    pub azi_delay: Option<Array2<f64>>,
    pub alt_delay: Option<Array2<f64>>,
}

/// Spectral responsiveness maps — per-orientation signal-quality metrics
/// computed from raw acquisition frames (the cycle-averaged movie's temporal
/// spectrum). Two validated metrics, each honestly named:
/// - `spectral_snr_*` — OpenISI multi-bin ratio SNR (no external oracle).
/// - `allen_power_snr_*` — Allen `corticalmapping` power z-score (bit-exact
///   oracle), continuous form of the power-SNR mask.
///
/// Cross-cycle **reliability** is the third responsiveness metric but lives in
/// its own [`ReliabilityMaps`] (it is a phasor-coherence quantity, not spectral).
#[derive(Clone)]
pub struct ResponsivenessMaps {
    pub spectral_snr_azi: Array2<f64>,
    pub spectral_snr_alt: Array2<f64>,
    pub allen_power_snr_azi: Array2<f64>,
    pub allen_power_snr_alt: Array2<f64>,
}

/// Cross-cycle reliability maps — amp-weighted vector coherence of
/// per-cycle complex projections, per stimulus direction. Allen Brain
/// Observatory (Zhuang 2017) / Engel 1994 coherence in the cycle
/// domain.
///
/// Per pixel, per direction:
/// ```text
/// reliability = | Σ_k Z_k |  /  Σ_k |Z_k|         ∈ [0, 1]
/// ```
/// where `Z_k` is the cycle-`k` complex projection at the stimulus
/// frequency. `1.0` = every cycle's phasor points the same direction
/// (perfectly repeatable response); `0.0` = phasors cancel (noise).
/// Amplitude-weighted: low-amp cycles with noisy phase don't dominate.
///
/// Requires ≥ 2 cycles per direction; with `K = 1` reliability is
/// trivially `1.0` and meaningless, so the pipeline errors out.
#[derive(Clone)]
pub struct ReliabilityMaps {
    pub rel_azi_fwd: Array2<f64>,
    pub rel_azi_rev: Array2<f64>,
    pub rel_alt_fwd: Array2<f64>,
    pub rel_alt_rev: Array2<f64>,
}

/// Complete unified analysis result — every output of the pipeline.
///
/// **Data is data.** Each f64 map is the raw computed value at its
/// algorithm stage, full frame, no pre-baked masking. Cortex masking
/// is *not* applied at the data layer — `cortex_mask` is its own
/// output, and consumers (renderers, downstream code) apply masks as
/// views. Sentinel zeros that *do* appear (eccentricity / magnification
/// outside patches, contour booleans outside patches) come from the
/// compute functions' native patch-scoped output, not from cortex
/// post-masking.
///
/// **The three VFS stages** capture the algorithm verbatim:
///
/// - `vfs` — raw mathematical VFS direct from gradient operations.
/// - `vfs_smoothed` — Gaussian-smoothed VFS (Allen's `signMapf`);
///   what segmentation thresholded.
/// - `vfs_smoothed_thresholded` — threshold-masked: `vfs_smoothed`
///   where `|vfs_smoothed| ≥ threshold_used`, zero elsewhere.
///
/// All three are full frame, no cortex masking; they're different
/// mathematical objects, not redundant copies.
pub struct AnalysisResult {
    /// Stage-0 output: the four per-direction complex maps the rest of the
    /// pipeline derives from. Carried on the result so the I/O boundary can
    /// persist them to `/complex_maps` (the DFT cache) after a from-raw run,
    /// and so importer/cached paths round-trip the maps they seeded.
    pub complex_maps: ComplexMaps,

    // Phases — raw computed values, full frame.
    pub azi_phase: Array2<f64>,
    pub alt_phase: Array2<f64>,
    pub azi_phase_degrees: Array2<f64>,
    pub alt_phase_degrees: Array2<f64>,

    // Amplitudes — full frame; the noise floor outside cortex is
    // itself meaningful (it defines the cortex extent).
    pub azi_amplitude: Array2<f64>,
    pub alt_amplitude: Array2<f64>,

    /// Raw mathematical VFS direct from gradients in `compute_retinotopy`.
    pub vfs: Array2<f64>,
    /// Gaussian-smoothed VFS (Allen's `signMapf`). The array
    /// segmentation thresholded.
    pub vfs_smoothed: Array2<f64>,
    /// Threshold-masked smoothed VFS: `vfs_smoothed` where
    /// `|vfs_smoothed| ≥ threshold_used`, zero elsewhere. The
    /// threshold is data-driven (`threshold_k × σ(vfs_smoothed)`),
    /// computed during segmentation and frozen here.
    pub vfs_smoothed_thresholded: Array2<f64>,

    // Segmentation outputs
    /// Binary mask of the imaged cortical region (Garrett 2014
    /// `imbound`). True inside cortex, false outside. Auxiliary
    /// output — consumers use this to apply cortex views; never
    /// pre-baked into other maps.
    pub cortex_mask: Array2<bool>,
    pub area_labels: Array2<i32>,
    pub area_signs: Vec<i8>,
    pub area_borders: Array2<bool>,

    // Patch-scoped derived maps — zero outside `area_labels > 0` as
    // a native output of their compute functions (not cortex masking).
    pub eccentricity: Array2<f64>,
    /// Polar-angle map (degrees, wrap ±180) about the V1 center — the SNLC
    /// `getRadialEccMapX.m` `kmap_ang` companion to `eccentricity`'s `kmap_rad`.
    /// Zero outside `area_labels > 0`, same patch scoping as eccentricity.
    pub polar_angle: Array2<f64>,
    pub magnification: Array2<f64>,
    /// Unmasked absolute Jacobian determinant (deg²/px²) — full frame, the raw
    /// magnification before ROI gating. Persisted to `/results` so a parameter
    /// tweak downstream of retinotopy can restore retinotopy from disk (stage 8
    /// and the derived maps consume it). `magnification` is this gated to areas.
    pub magnification_raw: Array2<f64>,
    /// Magnification anisotropy (SNLC `getMagFactors.m`), full-frame: the
    /// preferred axis (degrees, wraps at 180°) and the distortion coherence
    /// (`[0,1]`) — the other invariants of the `magnification_raw` Jacobian.
    pub magnification_axis: Array2<f64>,
    pub magnification_distortion: Array2<f64>,
    pub contours_azi: Array2<bool>,
    pub contours_alt: Array2<bool>,

    // Spectral responsiveness (spectral SNR + Allen power-SNR) — present only
    // when the file had raw acquisition frames; absent for imported complex maps.
    pub responsiveness: Option<ResponsivenessMaps>,

    /// Cross-cycle reliability maps. Present only for raw-acquisition
    /// data (requires per-cycle complex projections); `None` for
    /// imported data path where only cycle-averaged complex maps exist.
    /// Source of truth for `cortex_mask` when present.
    pub reliability: Option<ReliabilityMaps>,

    /// Hemodynamic delay maps (degrees, in `(0, 180]`) — the SNLC
    /// `Gprocesskret.m` `delay_hor`/`delay_vert`: the forward+reverse-symmetric
    /// phase, separated from the antisymmetric retinotopic position. Present
    /// only under delay-subtraction cycle-combine (the `UnweightedCycleAverage`
    /// method does no delay correction, so it has no delay to report); `None`
    /// otherwise. Full-frame (unmasked), like the phase maps.
    pub azi_delay: Option<Array2<f64>>,
    pub alt_delay: Option<Array2<f64>>,
}

/// Result of raw frame processing: complex maps plus optional spectral
/// responsiveness and reliability maps. Both require per-cycle data:
/// responsiveness uses the frame-domain cycle-averaged movie; reliability uses
/// the per-cycle complex projections.
pub struct RawProcessingResult {
    pub complex_maps: ComplexMaps,
    pub responsiveness: Option<ResponsivenessMaps>,
    pub reliability: Option<ReliabilityMaps>,
}

// =============================================================================
// Progress reporting
// =============================================================================

/// Progress reporting trait. The library calls these; consumers decide
/// how to present them (terminal, GUI polling, etc).
pub trait ProgressSink: Send + Sync {
    fn set_stage(&self, name: &str);
    fn set_progress(&self, fraction: f64);
}

/// No-op progress sink.
pub struct SilentProgress;
impl ProgressSink for SilentProgress {
    fn set_stage(&self, _name: &str) {}
    fn set_progress(&self, _fraction: f64) {}
}

// =============================================================================
// Computation (pure, no I/O)
// =============================================================================

pub use math::compute_retinotopy;

/// Compute full unified analysis: all outputs from one pipeline.
///
/// **Cortex source is an input, not derived here.** `cortex_mask`
/// comes from cross-cycle reliability (Allen / Engel — see
/// `cortex_from_reliability`) or a user-drawn polygon. The orchestrator
/// (`analyze`) resolves the cortex source and passes the resulting
/// mask in; this function only does the segmentation within it.
///
/// **Data layer is unmasked.** Every map is the raw computed value at
/// its algorithm stage. `cortex_mask` is its own auxiliary output;
/// no map is NaN-masked by it. Patch-scoped maps (eccentricity,
/// magnification, contours) zero outside `area_labels > 0` as a
/// native output of their compute functions.
///
/// SNR and reliability are left `None` here — the I/O orchestrator
/// attaches them when raw frames were the input.
/// Compute full unified analysis: all outputs from one pipeline. Each
/// pipeline stage dispatches via its method enum (see `crate::methods`).
/// Selected methods plus their parameters are recorded in
/// `.oisi /analysis_params` so re-analysis is bit-reproducible.
///
/// `reliability` and `user_polygon` are inputs to the cortex-source
/// stage; if the selected `CortexSourceMethod` variant requires data not
/// supplied here (e.g. `Reliability` needs reliability maps), the
/// function returns an `AnalysisError::MissingData` rather than
/// silently falling back.
///
/// `acquisition` carries the capture-time facts (stimulus geometry +
/// camera `um_per_pixel`). Read by the orchestrator from the file's
/// `/rig_params` + `/experiment_params` JSON attributes; `compute_analysis`
/// does not need to know how it was sourced.
/// Compute the retinotopy→derived-maps pipeline from combined complex maps.
///
/// Expressed as a DAG of `pipeline` stages walked by the orchestrator; this is a
/// thin delegation that seeds the given `complex_maps` (+ optional `reliability`)
/// so the `Baseline`/`Projection` stages are skipped, and runs the rest. The
/// full I/O path with caching lives in [`analyze`].
pub fn compute_analysis(
    complex_maps: &ComplexMaps,
    reliability: Option<&ReliabilityMaps>,
    user_polygon: Option<ndarray::Array2<bool>>,
    acquisition: &AcquisitionProperties,
    params: &AnalysisParams,
) -> Result<AnalysisResult> {
    // Pure path: the complex maps are the *input*, so they are seeded into the
    // pipeline (stage 0 is skipped). No cache restore (empty `restored` set → the
    // full tail recomputes), no cancellation, silent progress — the incremental
    // cache + cancellation are wired in `analyze`.
    let never_cancel = AtomicBool::new(false);
    let seed = pipeline::PipelineState {
        complex_maps: Some(complex_maps.clone()),
        reliability: reliability.cloned(),
        ..Default::default()
    };
    let out = pipeline::run(
        None,
        seed,
        &std::collections::HashSet::new(),
        user_polygon,
        pipeline::RunEnv {
            acquisition,
            params,
            cancel: &never_cancel,
            progress: &SilentProgress,
        },
    )?;
    Ok(out.result)
}

// =============================================================================
// Top-level orchestrator (I/O boundary)
// =============================================================================

/// Analyze an .oisi file. Introspects what's present and does the right thing:
/// - Has raw acquisition frames → DFT per cycle → complex maps + SNR +
///   per-direction cross-cycle reliability → cortex = `largest_cc(min_rel > T)`
///   → retinotopy + segmentation within cortex.
/// - Has complex maps (import or prior run) with no per-cycle data →
///   currently unsupported by this orchestrator: reliability can't be
///   computed, and we don't fall back to anatomy-derived cortex (per
///   the `feedback_no_anatomy_inference` principle). The caller must
///   supply a user-drawn cortex mask in a future revision.
///
/// `params_tree` is the tagged-`AnalysisConfig` JSON to stamp into
/// `/analysis_params` for provenance. When `Some`, it is written **inside the
/// same atomic transaction** as the results (so results + provenance publish
/// together — no separate in-place attribute write that a crash could leave
/// half-applied). Pass `None` when the caller stamps params itself, separately,
/// or not at all (e.g. test harnesses that pre-stamp before analyzing).
pub fn analyze(
    path: &Path,
    params: &AnalysisParams,
    params_tree: Option<&serde_json::Value>,
    progress: &dyn ProgressSink,
    cancel: &AtomicBool,
) -> Result<()> {
    // Forward-compatibility gate: refuse a file whose format version this build
    // does not recognize, rather than silently misreading a newer layout.
    io::verify_format_version(path)?;

    let caps = io::inspect(path)?;

    // Recording identity — keys stage 0's complex-maps cache and folds into
    // every stage fingerprint (so two recordings can never share a cache key).
    let identity = io::read_acquisition_identity(path)?;
    let raw_identity = format!("{}|{}", identity.animal_id, identity.created_at);

    // Acquisition properties (stimulus geometry, camera calibration) come from
    // `/rig_params` + `/experiment_params` JSON attrs written at capture time.
    // Each `.oisi` records its own — re-analysis on a different machine uses the
    // original rig's values. The `ProvenanceLevel` records which fields came
    // from the file vs were defaulted. Read up front: the retinotopy fingerprint
    // folds the geometry, so the incremental cut needs it before deciding what
    // to restore.
    let rig_attr = io::read_rig_params(path)?;
    let exp_attr = io::read_experiment_params(path)?;
    let acquisition = AcquisitionProperties::from_oisi_attrs(rig_attr.as_ref(), exp_attr.as_ref());
    if let Some(msg) = acquisition.provenance.warning_summary() {
        tracing::warn!("{msg}");
    }

    // The user-drawn cortex polygon (an external file input the `UserPolygon`
    // cortex source reads) and its content identity — folded into the
    // cortex_source fingerprint so editing the ROI invalidates that stage down.
    let user_polygon = io::read_cortex_roi(path)?;
    let user_polygon_id = user_polygon.as_ref().map(polygon_identity);

    // The full Merkle fingerprint set — one key per stage, each folding its own
    // inputs and its dependency stages' keys. Computed once; drives both the
    // stage-0 reuse decision and the tail restore cut.
    let fps = pipeline::fingerprint::compute(
        params,
        &acquisition,
        &raw_identity,
        user_polygon_id.as_deref(),
    );

    // ── Stage 0 (complex maps): seed from cache/import, or compute from raw ──
    // A cached `/complex_maps` is reused only when it was produced under the
    // current baseline (the projection fingerprint matches). Imports (no raw
    // frames) carry no baseline — their complex maps ARE the input — so they are
    // always seeded. A raw recording with a stale/absent cache recomputes from
    // raw. This is the load-bearing skip for the "tweak morphology → fast re-run"
    // workflow; the tail cut below extends it to every downstream stage.
    let projection_key = pipeline::StageId::Projection.fingerprint_key();
    let projection_fp = fps.projection.clone();
    let cached_maps_usable = caps.has_complex_maps
        && (!caps.has_acquisition
            || io::read_stage_fingerprint(path, projection_key)?.as_deref()
                == Some(projection_fp.as_str()));

    let mut seed = pipeline::PipelineState::default();
    let raw: Option<RawAcquisition> = if cached_maps_usable {
        tracing::info!("using cached complex_maps");
        progress.set_stage("Loading cached complex maps");
        seed = pipeline::PipelineState {
            complex_maps: Some(io::read_complex_maps(path)?),
            responsiveness: io::read_responsiveness_maps(path)?,
            reliability: io::read_reliability_maps(path)?,
            ..Default::default()
        };
        None
    } else if caps.has_acquisition {
        progress.set_stage("Loading camera frames");
        progress.set_progress(0.0);
        Some(io::read_raw_acquisition(path)?)
    } else {
        return Err(AnalysisError::MissingData(
            "file has neither raw acquisition data nor complex maps".into(),
        ));
    };

    // ── Incremental tail cut ────────────────────────────────────────────────
    // Decide restore-vs-recompute per cacheable stage from the fingerprints +
    // what's on disk, seeding the restored stages' outputs into the blackboard.
    // It never serves stale (a fingerprint mismatch recomputes); a change to a
    // downstream-only param (e.g. eccentricity) restores everything above it —
    // including the patch-refinement hotspot — and recomputes just the tail.
    let (seed, restored) = incremental::restore(path, &fps, seed)?;

    progress.set_stage("Computing retinotopy");
    progress.set_progress(0.7);
    use crate::methods::CortexSourceExt;
    tracing::info!(method = %params.cortex_source.short_label(), "cortex source method");

    let from_raw = raw.is_some();
    let t_run = Instant::now();
    let out = pipeline::run(
        raw.as_ref(),
        seed,
        &restored,
        user_polygon,
        pipeline::RunEnv {
            acquisition: &acquisition,
            params,
            cancel,
            progress,
        },
    )?;
    tracing::debug!(
        pipeline_ms = t_run.elapsed().as_secs_f64() * 1e3,
        from_raw,
        "pipeline complete"
    );

    if cancel.load(Ordering::Relaxed) {
        return Err(AnalysisError::Cancelled);
    }

    progress.set_stage("Writing results");
    progress.set_progress(0.9);

    // Persist all analysis outputs ATOMICALLY: a single copy-temp → mutate →
    // fsync → rename, so a crash / disk-full mid-write cannot corrupt the live
    // file (which also holds the irreplaceable raw `/acquisition`). All
    // `write_*` calls target the temp; the rename publishes them together. See
    // `docs/FOUNDATION_AUDIT.md` A1.
    let t_wr = Instant::now();
    io::atomic_update(path, |tmp| {
        // Freshly-computed complex maps (+ projection fingerprint) so the next
        // run can take the fast seeded stage-0 path.
        if from_raw {
            io::write_complex_maps(tmp, &out.result.complex_maps)?;
            io::write_stage_fingerprint(tmp, projection_key, &projection_fp)?;
        }

        io::write_results(tmp, &out.result, &acquisition, params)?;

        // Patch-threshold intermediates → `/cache` whenever that stage executed,
        // so its fresh `imseg`/`threshold_applied` can be restored next run. When
        // it was restored, `/cache` already holds the matching values.
        if !restored.contains(&pipeline::StageId::PatchThreshold) {
            if let (Some(imseg), Some(thr)) = (out.imseg.as_ref(), out.threshold_applied) {
                io::write_stage_cache(tmp, imseg, thr)?;
            }
        }

        // Each tail stage's fingerprint (written AFTER its data, so a crash
        // yields a missing fingerprint → safe recompute, never a premature one).
        for (key, value) in fps.tail_pairs() {
            io::write_stage_fingerprint(tmp, key, value)?;
        }

        // Provenance stamp, inside the SAME transaction as the results it
        // describes — results and their `/analysis_params` publish atomically.
        if let Some(tree) = params_tree {
            io::write_analysis_params_attr(tmp, tree)?;
        }
        Ok(())
    })?;
    tracing::debug!(
        write_results_ms = t_wr.elapsed().as_secs_f64() * 1e3,
        "io: write results (atomic)"
    );

    progress.set_stage("Complete");
    progress.set_progress(1.0);
    Ok(())
}

/// Content identity of a user-drawn cortex ROI — a blake3 hash over its shape +
/// bytes — folded into the cortex_source fingerprint so editing the polygon
/// invalidates that stage (and everything downstream) but nothing above it.
fn polygon_identity(polygon: &Array2<bool>) -> String {
    let mut h = blake3::Hasher::new();
    let (rows, cols) = polygon.dim();
    h.update(&(rows as u64).to_le_bytes());
    h.update(&(cols as u64).to_le_bytes());
    // Hash the mask as one contiguous byte buffer (0/1 per pixel) rather than a
    // per-pixel `update` — a single pass instead of millions of calls on a
    // full-frame ROI. Row-major order is fixed by `as_standard_layout`.
    let bytes: Vec<u8> = polygon
        .as_standard_layout()
        .iter()
        .map(|&b| b as u8)
        .collect();
    h.update(&bytes);
    h.finalize().to_hex().to_string()
}

// =============================================================================
// Error type
// =============================================================================

/// Analysis-pipeline errors.
///
/// Each variant carries its **stable machine-readable code** (`E_*`) as a
/// `strum` attribute on the variant itself — the single source of truth for the
/// code, co-located with the variant (no separate mapping table). The companion
/// fieldless [`AnalysisCode`] enum (derived by `EnumDiscriminants`) gives both the
/// runtime lookup ([`AnalysisError::code`]) and compile-time enumeration of every
/// code (for the cross-language error catalog), from that one declaration.
#[derive(Debug, thiserror::Error, strum::EnumDiscriminants)]
#[strum_discriminants(
    name(AnalysisCode),
    vis(pub),
    derive(strum::IntoStaticStr, strum::EnumIter),
)]
pub enum AnalysisError {
    #[error("I/O error: {0}")]
    #[strum_discriminants(strum(serialize = "E_IO"))]
    Io(#[from] std::io::Error),

    #[error("HDF5 error ({context}): {source}")]
    #[strum_discriminants(strum(serialize = "E_HDF5"))]
    Hdf5 {
        context: String,
        #[source]
        source: hdf5::Error,
    },

    #[error("Invalid .oisi file: {0}")]
    #[strum_discriminants(strum(serialize = "E_INVALID_PACKAGE"))]
    InvalidPackage(String),

    #[error("Missing data: {0}")]
    #[strum_discriminants(strum(serialize = "E_MISSING_DATA"))]
    MissingData(String),

    /// Compute-layer errors — tensor shape/kind/device, ndarray-tensor
    /// conversion, GPU device init/selection. Surfaces as a clean error
    /// to the UI instead of a panic from the compute backend.
    #[error("Compute: {0}")]
    #[strum_discriminants(strum(serialize = "E_COMPUTE"))]
    Compute(String),

    /// Validation failures inside the analysis crate that aren't I/O,
    /// HDF5, or compute — bad parameter values, bad method-tunable
    /// names, validation constraints. Distinct from `InvalidPackage`
    /// (which is for malformed file contents) so the UI can tell the
    /// user "the value you entered isn't valid" vs "the file is
    /// broken."
    #[error("Validation: {0}")]
    #[strum_discriminants(strum(serialize = "E_VALIDATION"))]
    Validation(String),

    #[error("Analysis cancelled")]
    #[strum_discriminants(strum(serialize = "E_CANCELLED"))]
    Cancelled,
}

impl AnalysisError {
    /// The stable machine-readable error code (e.g. `"E_HDF5"`), derived from the
    /// variant's `strum` attribute — the SSoT the IPC wire + frontend rely on.
    pub fn code(&self) -> &'static str {
        AnalysisCode::from(self).into()
    }

    /// Construct an HDF5 error that **preserves the underlying `hdf5::Error`** as
    /// its `source` (never stringified), with `context` naming the operation.
    pub fn hdf5(context: impl Into<String>, source: hdf5::Error) -> Self {
        Self::Hdf5 {
            context: context.into(),
            source,
        }
    }
}

impl From<hdf5::Error> for AnalysisError {
    fn from(source: hdf5::Error) -> Self {
        Self::Hdf5 {
            context: String::new(),
            source,
        }
    }
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, AnalysisError>;
