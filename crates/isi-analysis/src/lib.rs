//! ISI retinotopy analysis library.
//!
//! Pure Rust implementation of intrinsic signal imaging analysis.
//! Single file format: `.oisi` (HDF5 internally). The system introspects
//! what data is present and determines what operations are available.
//!
//! No GUI dependency. Used by both the Tauri app and the headless CLI.

pub mod math;
pub mod segmentation;
pub mod io;
pub mod mat5;
pub mod params;

use ndarray::Array2;
pub use num_complex::Complex64;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Create a Complex64 from polar coordinates (re-exported for external use).
pub fn complex_from_polar(r: f64, theta: f64) -> Complex64 {
    Complex64::from_polar(r, theta)
}

pub use params::AnalysisParams;
pub use io::FileCapabilities;

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
pub struct RetinotopyMaps {
    pub azi_phase: Array2<f64>,
    pub alt_phase: Array2<f64>,
    pub azi_phase_degrees: Array2<f64>,
    pub alt_phase_degrees: Array2<f64>,
    pub azi_amplitude: Array2<f64>,
    pub alt_amplitude: Array2<f64>,
    pub vfs: Array2<f64>,
}

/// SNR maps — computed per-condition from raw acquisition frames.
pub struct SnrMaps {
    pub snr_azi: Array2<f64>,
    pub snr_alt: Array2<f64>,
}

/// Complete unified analysis result — every output of the pipeline.
/// All stored flat in `/results/`. The UI discovers what's available and displays it.
pub struct AnalysisResult {
    // Core retinotopy (always present)
    pub azi_phase: Array2<f64>,
    pub alt_phase: Array2<f64>,
    pub azi_phase_degrees: Array2<f64>,
    pub alt_phase_degrees: Array2<f64>,
    pub azi_amplitude: Array2<f64>,
    pub alt_amplitude: Array2<f64>,
    pub vfs: Array2<f64>,

    // Segmentation outputs
    pub vfs_thresholded: Array2<f64>,
    pub area_labels: Array2<i32>,
    pub area_signs: Vec<i8>,
    pub area_borders: Array2<bool>,

    // Derived maps
    pub eccentricity: Array2<f64>,
    pub magnification: Array2<f64>,
    pub contours_azi: Array2<bool>,
    pub contours_alt: Array2<bool>,

    // SNR (only from raw acquisition)
    pub snr_azi: Option<Array2<f64>>,
    pub snr_alt: Option<Array2<f64>>,
}

/// Result of raw frame processing: complex maps + SNR.
pub struct RawProcessingResult {
    pub complex_maps: ComplexMaps,
    pub snr: SnrMaps,
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

/// Compute retinotopy from complex maps.
pub fn compute_retinotopy(complex_maps: &ComplexMaps, params: &AnalysisParams) -> RetinotopyMaps {
    math::compute_retinotopy(complex_maps, params)
}

/// Compute full unified analysis: all outputs from one pipeline.
pub fn compute_analysis(complex_maps: &ComplexMaps, params: &AnalysisParams) -> AnalysisResult {
    let retino = math::compute_retinotopy(complex_maps, params);

    // Segmentation (produces area_labels, area_signs, area_borders).
    let seg = params.segmentation.as_ref()
        .map(|sp| segmentation::segment_visual_areas(&retino, sp));

    let (h, w) = retino.vfs.dim();
    let (area_labels, area_signs, area_borders) = match &seg {
        Some(s) => (s.area_labels.clone(), s.area_signs.clone(), s.borders.clone()),
        None => (Array2::zeros((h, w)), Vec::new(), Array2::from_elem((h, w), false)),
    };

    // Derived maps.
    let vfs_thresholded = math::compute_vfs_thresholded(&retino.vfs, &area_labels);
    let eccentricity = math::compute_eccentricity(
        &retino.azi_phase_degrees, &retino.alt_phase_degrees, &area_labels,
    );
    let magnification = math::compute_magnification(
        &retino.azi_phase_degrees, &retino.alt_phase_degrees,
    );
    let contours_azi = math::compute_contours(&retino.azi_phase_degrees, &area_labels, 4.0);
    let contours_alt = math::compute_contours(&retino.alt_phase_degrees, &area_labels, 4.0);

    AnalysisResult {
        azi_phase: retino.azi_phase,
        alt_phase: retino.alt_phase,
        azi_phase_degrees: retino.azi_phase_degrees,
        alt_phase_degrees: retino.alt_phase_degrees,
        azi_amplitude: retino.azi_amplitude,
        alt_amplitude: retino.alt_amplitude,
        vfs: retino.vfs,
        vfs_thresholded,
        area_labels,
        area_signs,
        area_borders,
        eccentricity,
        magnification,
        contours_azi,
        contours_alt,
        snr_azi: None,
        snr_alt: None,
    }
}

// =============================================================================
// Top-level orchestrator (I/O boundary)
// =============================================================================

/// Analyze an .oisi file. Introspects what's present and does the right thing:
/// - Has raw acquisition frames → dF/F + DFT → complex maps + SNR → retinotopy + borders
/// - Has complex maps (import or prior run) → retinotopy + borders (no SNR)
pub fn analyze(
    path: &Path,
    params: &AnalysisParams,
    progress: &dyn ProgressSink,
    cancel: &AtomicBool,
) -> Result<(), AnalysisError> {
    let caps = io::inspect(path)?;

    let (complex_maps, snr) = if caps.has_acquisition {
        // Always reprocess from raw when raw data exists — ensures per-sweep DFT
        // and picks up any parameter changes.
        progress.set_stage("Processing raw frames");
        progress.set_progress(0.0);

        let raw = io::compute_complex_maps_from_raw(path, params, progress, cancel)?;

        if cancel.load(Ordering::Relaxed) {
            return Err(AnalysisError::Cancelled);
        }

        io::write_complex_maps(path, &raw.complex_maps)?;
        (raw.complex_maps, Some(raw.snr))
    } else if caps.has_complex_maps {
        // No raw data — use pre-computed complex maps (imports).
        progress.set_stage("Loading complex maps");
        progress.set_progress(0.0);

        let maps = io::read_complex_maps(path)?;

        if cancel.load(Ordering::Relaxed) {
            return Err(AnalysisError::Cancelled);
        }

        (maps, None)
    } else {
        return Err(AnalysisError::MissingData(
            "file has neither raw acquisition data nor complex maps".into()
        ));
    };

    progress.set_stage("Computing retinotopy");
    progress.set_progress(0.7);

    let mut result = compute_analysis(&complex_maps, params);

    // Merge SNR into the result if available (raw acquisition path).
    if let Some(snr_maps) = snr {
        result.snr_azi = Some(snr_maps.snr_azi);
        result.snr_alt = Some(snr_maps.snr_alt);
    }

    if cancel.load(Ordering::Relaxed) {
        return Err(AnalysisError::Cancelled);
    }

    progress.set_stage("Writing results");
    progress.set_progress(0.9);

    io::write_results(path, &result, params)?;

    progress.set_stage("Complete");
    progress.set_progress(1.0);
    Ok(())
}

// =============================================================================
// Error type
// =============================================================================

#[derive(Debug)]
pub enum AnalysisError {
    Io(std::io::Error),
    Hdf5(String),
    InvalidPackage(String),
    MissingData(String),
    Cancelled,
}

impl std::fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Hdf5(e) => write!(f, "HDF5 error: {e}"),
            Self::InvalidPackage(e) => write!(f, "Invalid .oisi file: {e}"),
            Self::MissingData(e) => write!(f, "Missing data: {e}"),
            Self::Cancelled => write!(f, "Analysis cancelled"),
        }
    }
}

impl std::error::Error for AnalysisError {}

impl From<std::io::Error> for AnalysisError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<hdf5::Error> for AnalysisError {
    fn from(e: hdf5::Error) -> Self {
        Self::Hdf5(e.to_string())
    }
}
