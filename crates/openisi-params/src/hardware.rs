//! Hardware context — runtime hardware capabilities injected into the registry.
//!
//! Not persisted. Camera capabilities arrive on camera connect, monitor info
//! on display select, measured refresh on display validate.

/// Runtime hardware context. Updated when hardware state changes.
/// Constraint functions read these to compute effective bounds.
#[derive(Debug, Clone, Default)]
pub struct HardwareContext {
    // Camera capabilities (set on camera connect)
    pub camera_min_exposure_us: Option<u32>,
    pub camera_max_exposure_us: Option<u32>,
    pub camera_max_binning: Option<u16>,

    // Monitor info (set on display select)
    pub monitor_width_px: Option<u32>,
    pub monitor_height_px: Option<u32>,
    pub monitor_width_cm: Option<f64>,
    pub monitor_height_cm: Option<f64>,
    pub monitor_refresh_hz: Option<u32>,

    // Measured (set on display validate)
    pub measured_refresh_hz: Option<f64>,
}
