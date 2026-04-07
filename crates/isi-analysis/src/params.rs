//! Analysis parameters.

use serde::{Deserialize, Serialize};

/// All parameters needed to compute retinotopy from complex maps.
/// Must be constructed from config — no Default impl, no hardcoded values.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnalysisParams {
    /// Gaussian smoothing sigma in pixels (for complex map smoothing before gradients)
    pub smoothing_sigma: f64,
    /// Number of 90° CCW rotations to apply to maps (0–3)
    pub rotation_k: i32,
    /// Total azimuth stimulus sweep in degrees
    pub azi_angular_range: f64,
    /// Total altitude stimulus sweep in degrees
    pub alt_angular_range: f64,
    /// Azimuth visual field offset in degrees
    pub offset_azi: f64,
    /// Altitude visual field offset in degrees
    pub offset_alt: f64,
    /// Epsilon for dF/F divide-by-zero protection
    pub epsilon: f64,
    /// Garrett et al. 2014 / Juavinett et al. 2017 segmentation parameters.
    /// None = skip segmentation.
    #[serde(default)]
    pub segmentation: Option<SegmentationParams>,
}

/// Parameters for visual area segmentation (Garrett et al. 2014 / Juavinett et al. 2017).
/// Values from Table 1 of Garrett et al. and the Nature Protocols reference implementation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SegmentationParams {
    /// Gaussian sigma for smoothing VFS before thresholding (pixels).
    /// Reference: 9.0 (Python impl), ~3.0 (MATLAB impl at different resolution).
    pub sign_map_filter_sigma: f64,
    /// |VFS| threshold for initial patch extraction.
    /// 0.0 = use auto-threshold (1.5 × std(VFS)). Reference: 0.35.
    pub sign_map_threshold: f64,
    /// Morphological opening disk radius (pixels) — noise removal. Reference: 2.
    pub open_radius: usize,
    /// Morphological closing disk radius (pixels) — gap filling. Reference: 10.
    pub close_radius: usize,
    /// Dilation disk radius (pixels) — connect nearby patches. Reference: 3.
    pub dilate_radius: usize,
    /// Image padding (pixels) before closing. Reference: 30.
    pub pad_border: usize,
    /// Spur removal iterations on thinned borders. Reference: 4.
    pub spur_iterations: usize,
    /// A_sigma / A_union ratio threshold for eccentricity-based patch splitting. Reference: 1.1.
    pub split_overlap_threshold: f64,
    /// Max visual space overlap fraction for merging adjacent same-sign patches. Reference: 0.1.
    pub merge_overlap_threshold: f64,
    /// Dilation radius for adjacency detection during merge (pixels). Reference: 3.
    pub merge_dilate_radius: usize,
    /// Closing radius for smoothing fused patches (pixels). Reference: 5.
    pub merge_close_radius: usize,
    /// Visual field radius for eccentricity analysis (degrees). Reference: 30.0.
    pub eccentricity_radius: f64,
}
