//! ISI retinotopy analysis library.
//!
//! Pure Rust implementation of intrinsic signal imaging analysis.
//! Single file format: `.oisi` (HDF5 internally). The system introspects
//! what data is present and determines what operations are available.
//!
//! No GUI dependency. Used by both the Tauri app and the headless CLI.

pub mod bridge;
pub mod compute;
pub mod math;
pub mod methods;
pub mod segmentation;
pub mod io;
pub mod mat5;
pub mod migrate;
pub mod params;

use ndarray::Array2;
pub use num_complex::Complex64;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Create a Complex64 from polar coordinates (re-exported for external use).
pub fn complex_from_polar(r: f64, theta: f64) -> Complex64 {
    Complex64::from_polar(r, theta)
}

pub use params::{AcquisitionProperties, AnalysisParams, ProvenanceLevel};
pub use io::{FileCapabilities, MapMeta};

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
}

/// SNR maps — computed per-condition from raw acquisition frames.
pub struct SnrMaps {
    pub snr_azi: Array2<f64>,
    pub snr_alt: Array2<f64>,
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
    pub magnification: Array2<f64>,
    pub contours_azi: Array2<bool>,
    pub contours_alt: Array2<bool>,

    // SNR (genuine capability — present only when the file had raw acquisition
    // frames; absent when analysis ran from imported complex maps).
    pub snr: Option<SnrMaps>,

    /// Cross-cycle reliability maps. Present only for raw-acquisition
    /// data (requires per-cycle complex projections); `None` for
    /// imported data path where only cycle-averaged complex maps exist.
    /// Source of truth for `cortex_mask` when present.
    pub reliability: Option<ReliabilityMaps>,
}

/// Result of raw frame processing: complex maps plus optional SNR
/// and reliability maps. Both require per-cycle data: SNR uses the
/// frame-domain cycle-averaged movie; reliability uses the per-cycle
/// complex projections.
pub struct RawProcessingResult {
    pub complex_maps: ComplexMaps,
    pub snr: Option<SnrMaps>,
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
/// stage; if the selected `CortexSource` variant requires data not
/// supplied here (e.g. `Reliability` needs reliability maps), the
/// function returns an `AnalysisError::MissingData` rather than
/// silently falling back.
///
/// `acquisition` carries the capture-time facts (stimulus geometry +
/// camera `um_per_pixel`). Read by the orchestrator from the file's
/// `/rig_params` + `/experiment_params` JSON attributes; `compute_analysis`
/// does not need to know how it was sourced.
pub fn compute_analysis(
    complex_maps: &ComplexMaps,
    reliability: Option<&ReliabilityMaps>,
    user_polygon: Option<ndarray::Array2<bool>>,
    acquisition: &AcquisitionProperties,
    params: &AnalysisParams,
) -> Result<AnalysisResult> {
    use crate::methods::cortex_source::CortexResolveContext;

    let retino = math::compute_retinotopy(complex_maps, acquisition, params)?;

    // Stage 4 — sign map smoothing.
    let vfs_smooth = params.sign_map_smoothing.apply(&retino.vfs, acquisition.um_per_pixel);

    // Stage 5 — cortex source.
    let cortex_ctx = CortexResolveContext {
        shape: vfs_smooth.dim(),
        reliability,
        user_polygon,
        vfs_smoothed: Some(&vfs_smooth),
    };
    let cortex_mask = params.cortex_source.resolve(&cortex_ctx)?;

    // Stage 6 — patch threshold (binary mask of candidate patch pixels).
    // `threshold_applied` is the actual scalar cutoff used on |VFS| —
    // for σ-scaled variants this is the runtime k·σ·0.5 value, not the
    // multiplier k. Used below for the `vfs_smoothed_thresholded`
    // diagnostic so the figure mirrors what the strategy actually
    // applied.
    let threshold_out = params.patch_threshold.apply(&vfs_smooth, &cortex_mask);
    let imseg = threshold_out.imseg;
    let threshold_applied = threshold_out.threshold_applied;

    // Stage 7 — patch extraction (label + per-patch close → patches).
    let extraction = params.patch_extraction.apply(&imseg, &vfs_smooth);

    // Stage 8 — patch refinement (split + merge).
    // Allen's split criterion compares the patch's visual-space area
    // (AU) to the determinant-of-Jacobian integral (AS = ∑ |det(grad)|);
    // `magnification_raw` is precisely that determinant magnitude in
    // deg²/pixel², computed once during retinotopy on-device.
    let mut patches = params.patch_refinement.apply(
        extraction.patches,
        &retino.azi_phase_degrees,
        &retino.alt_phase_degrees,
        &retino.magnification_raw,
    );

    // Stage 9 — per-pixel quality gate. Currently only `None` is a
    // selectable variant (matches Allen `retinotopic_mapping`'s
    // published behaviour — no per-pixel gate, the sign-map threshold +
    // cortex envelope do all the gating). A non-`None` variant would
    // be applied here as a filter on `imseg` before patch extraction.

    // Sort patches by area; build area_labels and area_signs.
    patches.sort_by(|a, b| b.area().cmp(&a.area()));
    let (h, w) = vfs_smooth.dim();
    let mut area_labels = ndarray::Array2::<i32>::zeros((h, w));
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
    let area_borders = segmentation::extract_label_borders(&area_labels);

    // Stage 10 — eccentricity (uses the resolved area_labels).
    let eccentricity = params.eccentricity.apply(
        &retino.azi_phase_degrees,
        &retino.alt_phase_degrees,
        &area_labels,
    );

    // Universal derived maps (not method-dispatched).
    let magnification = math::apply_label_roi(&retino.magnification_raw, &area_labels);
    let contours_azi = math::compute_contours(&retino.azi_phase_degrees, &area_labels, 4.0);
    let contours_alt = math::compute_contours(&retino.alt_phase_degrees, &area_labels, 4.0);

    // Literal threshold-mask of the smoothed VFS for diagnostic display:
    // shows where `|VFS_smooth|` cleared the same cutoff the patch-
    // threshold strategy applied to `imseg`. NaN outside the cortex
    // mask so the renderer can show "no measurement here" rather than
    // a misleading zero.
    let vfs_smoothed_thresholded = ndarray::Array2::from_shape_fn(
        vfs_smooth.dim(),
        |(r, c)| {
            if !cortex_mask[[r, c]] {
                f64::NAN
            } else {
                let v = vfs_smooth[[r, c]];
                if v.is_finite() && v.abs() >= threshold_applied { v } else { 0.0 }
            }
        },
    );

    Ok(AnalysisResult {
        azi_phase: retino.azi_phase,
        alt_phase: retino.alt_phase,
        azi_phase_degrees: retino.azi_phase_degrees,
        alt_phase_degrees: retino.alt_phase_degrees,
        azi_amplitude: retino.azi_amplitude,
        alt_amplitude: retino.alt_amplitude,
        vfs: retino.vfs,
        vfs_smoothed: vfs_smooth,
        vfs_smoothed_thresholded,
        cortex_mask,
        area_labels,
        area_signs,
        area_borders,
        eccentricity,
        magnification,
        contours_azi,
        contours_alt,
        snr: None,
        reliability: None,
    })
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
pub fn analyze(
    path: &Path,
    params: &AnalysisParams,
    progress: &dyn ProgressSink,
    cancel: &AtomicBool,
) -> Result<()> {
    let caps = io::inspect(path)?;

    let (complex_maps, snr, reliability) = if caps.has_acquisition {
        // Always reprocess from raw when raw data exists — ensures per-sweep DFT
        // and picks up any parameter changes.
        progress.set_stage("Processing raw frames");
        progress.set_progress(0.0);

        let raw = io::compute_complex_maps_from_raw(path, params, progress, cancel)?;

        if cancel.load(Ordering::Relaxed) {
            return Err(AnalysisError::Cancelled);
        }

        io::write_complex_maps(path, &raw.complex_maps)?;
        (raw.complex_maps, raw.snr, raw.reliability)
    } else if caps.has_complex_maps {
        // No raw data — use pre-computed complex maps (imports).
        progress.set_stage("Loading complex maps");
        progress.set_progress(0.0);

        let maps = io::read_complex_maps(path)?;

        if cancel.load(Ordering::Relaxed) {
            return Err(AnalysisError::Cancelled);
        }

        (maps, None, None)
    } else {
        return Err(AnalysisError::MissingData(
            "file has neither raw acquisition data nor complex maps".into()
        ));
    };

    progress.set_stage("Computing retinotopy");
    progress.set_progress(0.7);

    // The `cortex_source` method in `params` decides the cortex source.
    // We supply the inputs available — reliability from raw acquisition,
    // user-drawn polygon from the file — and the method picks what to
    // do with them. If the selected method requires data we don't have
    // (e.g. `Reliability` on a cycle-averaged import), compute_analysis
    // returns MissingData and the orchestrator surfaces a clear error.
    let user_polygon = io::read_cortex_roi(path)?;
    eprintln!("[analyze] cortex source method: {}", params.cortex_source.short_label());

    // Acquisition properties (stimulus geometry, camera calibration)
    // come from `/rig_params` + `/experiment_params` JSON attrs written
    // at capture time. Each `.oisi` records its own — re-analysis on a
    // different machine uses the original rig's values. The
    // `ProvenanceLevel` on the result records exactly which fields
    // came from the file vs were defaulted; we MUST match on it (the
    // type forbids silent fallbacks).
    let rig_attr = io::read_rig_params(path)?;
    let exp_attr = io::read_experiment_params(path)?;
    let acquisition = AcquisitionProperties::from_oisi_attrs(rig_attr.as_ref(), exp_attr.as_ref());
    if let Some(msg) = acquisition.provenance.warning_summary() {
        eprintln!("[analyze] {msg}");
    }

    let mut result = compute_analysis(
        &complex_maps,
        reliability.as_ref(),
        user_polygon,
        &acquisition,
        params,
    )?;
    result.snr = snr;
    result.reliability = reliability;

    if cancel.load(Ordering::Relaxed) {
        return Err(AnalysisError::Cancelled);
    }

    progress.set_stage("Writing results");
    progress.set_progress(0.9);

    io::write_results(path, &result, &acquisition, params)?;

    progress.set_stage("Complete");
    progress.set_progress(1.0);
    Ok(())
}

// =============================================================================
// Error type
// =============================================================================

#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HDF5 error: {0}")]
    Hdf5(String),

    #[error("Invalid .oisi file: {0}")]
    InvalidPackage(String),

    #[error("Missing data: {0}")]
    MissingData(String),

    /// Compute-layer errors — tensor shape/kind/device, ndarray-tensor
    /// conversion, GPU device init/selection. Surfaces as a clean error
    /// to the UI instead of a panic from `tch` internals.
    #[error("Compute: {0}")]
    Compute(String),

    /// Validation failures inside the analysis crate that aren't I/O,
    /// HDF5, or compute — bad parameter values, bad method-tunable
    /// names, validation constraints. Distinct from `InvalidPackage`
    /// (which is for malformed file contents) so the UI can tell the
    /// user "the value you entered isn't valid" vs "the file is
    /// broken."
    #[error("Validation: {0}")]
    Validation(String),

    #[error("Analysis cancelled")]
    Cancelled,
}

impl From<hdf5::Error> for AnalysisError {
    fn from(e: hdf5::Error) -> Self {
        Self::Hdf5(e.to_string())
    }
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, AnalysisError>;
