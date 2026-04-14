//! Runtime session state.
//!
//! Holds volatile runtime state that doesn't persist across launches:
//! selected display, camera connection, validation results, save path.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Information about a detected monitor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorInfo {
    /// Monitor index (0-based)
    pub index: usize,
    /// Display name from EDID
    pub name: String,
    /// Resolution width in pixels
    pub width_px: u32,
    /// Resolution height in pixels
    pub height_px: u32,
    /// Physical width in cm (from EDID, 0 if unknown)
    pub width_cm: f64,
    /// Physical height in cm (from EDID, 0 if unknown)
    pub height_cm: f64,
    /// Reported refresh rate in Hz
    pub refresh_hz: u32,
    /// Monitor position (x, y) in virtual desktop coordinates
    pub position: (i32, i32),
    /// Whether physical dimensions came from EDID or user override
    pub physical_source: String,
}

/// Display validation results (from WaitForVBlank measurement).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayValidation {
    /// Measured refresh rate in Hz
    pub measured_refresh_hz: f64,
    /// Number of vsync samples used (after warmup skip)
    pub sample_count: u32,
    /// Timing jitter (std dev) in microseconds
    pub jitter_us: f64,
    /// 95% confidence interval half-width in Hz
    pub ci95_hz: f64,
    /// Whether measured rate matches reported rate (within tolerance)
    pub matches_reported: bool,
    /// OS-reported refresh rate
    pub reported_refresh_hz: f64,
    /// Warnings (empty if validation is clean)
    pub warnings: Vec<String>,
}

/// Camera connection state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraInfo {
    /// Camera model name
    pub model: String,
    /// Sensor width in pixels
    pub width_px: u32,
    /// Sensor height in pixels
    pub height_px: u32,
    /// Bits per pixel
    pub bits_per_pixel: u32,
    /// Current exposure in microseconds
    pub exposure_us: u32,
}

/// Runtime session state — volatile, not persisted.
#[derive(Debug, Clone, Default)]
pub struct Session {
    // Display
    pub selected_display: Option<MonitorInfo>,
    pub display_validation: Option<DisplayValidation>,

    // Camera
    pub camera: Option<CameraInfo>,
    pub camera_connected: bool,

    // Acquisition state
    pub is_acquiring: bool,

    // Save path — set before acquisition starts (save-path-first workflow).
    pub save_path: Option<PathBuf>,

    // Session metadata — set by user before acquisition.
    pub animal_id: String,
    pub notes: String,

    // Timing characterization — populated by timing validation before acquisition.
    pub timing_characterization: Option<crate::timing::TimingCharacterization>,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a display has been selected with valid physical dimensions.
    pub fn has_valid_display(&self) -> bool {
        self.selected_display
            .as_ref()
            .is_some_and(|d| d.width_cm > 0.0 && d.height_cm > 0.0)
    }

    /// Check if display refresh rate has been validated.
    pub fn display_refresh_validated(&self) -> bool {
        self.display_validation.is_some()
    }

    /// Get the measured refresh rate. Panics if display hasn't been validated.
    pub fn display_measured_refresh_hz(&self) -> f64 {
        self.display_validation
            .as_ref()
            .expect("Display has not been validated — run validation before accessing measured refresh rate")
            .measured_refresh_hz
    }

    /// Set the selected display.
    pub fn set_selected_display(&mut self, monitor: MonitorInfo) {
        self.selected_display = Some(monitor);
        // Clear validation when display changes
        self.display_validation = None;
    }

    /// Set display validation results.
    pub fn set_display_validation(&mut self, validation: DisplayValidation) {
        self.display_validation = Some(validation);
    }

    /// Clear display validation.
    pub fn clear_display_validation(&mut self) {
        self.display_validation = None;
    }

    /// Set the save path for the next acquisition.
    pub fn set_save_path(&mut self, path: PathBuf) {
        self.save_path = Some(path);
    }

    /// Clear the save path.
    pub fn clear_save_path(&mut self) {
        self.save_path = None;
    }

    /// Check all prerequisites for starting an acquisition.
    pub fn acquisition_prerequisites(&self) -> Result<(), String> {
        if self.selected_display.is_none() {
            return Err("No display selected".into());
        }
        if !self.has_valid_display() {
            return Err("Display has no valid physical dimensions".into());
        }
        if self.display_validation.is_none() {
            return Err("Display refresh rate not validated".into());
        }
        if !self.camera_connected {
            return Err("Camera not connected".into());
        }
        if self.save_path.is_none() {
            return Err("No save path set — choose where to save before acquiring".into());
        }
        Ok(())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_monitor() -> MonitorInfo {
        MonitorInfo {
            index: 1,
            name: "Test Monitor".into(),
            width_px: 1920,
            height_px: 1080,
            width_cm: 53.0,
            height_cm: 30.0,
            refresh_hz: 60,
            position: (1920, 0),
            physical_source: "edid".into(),
        }
    }

    #[test]
    fn new_session_has_no_prerequisites() {
        let session = Session::new();
        assert!(session.acquisition_prerequisites().is_err());
    }

    #[test]
    fn prerequisites_check_all_conditions() {
        let mut session = Session::new();
        assert!(session.acquisition_prerequisites().unwrap_err().contains("display"));

        session.set_selected_display(test_monitor());
        // Still need validation
        assert!(session.acquisition_prerequisites().unwrap_err().contains("validated"));

        session.set_display_validation(DisplayValidation {
            measured_refresh_hz: 59.94,
            sample_count: 150,
            jitter_us: 50.0,
            ci95_hz: 0.1,
            matches_reported: true,
            reported_refresh_hz: 60.0,
            warnings: Vec::new(),
        });
        // Still need camera
        assert!(session.acquisition_prerequisites().unwrap_err().contains("Camera"));

        session.camera_connected = true;
        session.camera = Some(CameraInfo {
            model: "Test".into(),
            width_px: 1024,
            height_px: 1024,
            bits_per_pixel: 16,
            exposure_us: 33000,
        });
        // Still need save path
        assert!(session.acquisition_prerequisites().unwrap_err().contains("save path"));

        session.set_save_path(PathBuf::from("/tmp/test.oisi"));
        // Now all prerequisites met
        assert!(session.acquisition_prerequisites().is_ok());
    }

    #[test]
    fn display_change_clears_validation() {
        let mut session = Session::new();
        session.set_selected_display(test_monitor());
        session.set_display_validation(DisplayValidation {
            measured_refresh_hz: 60.0,
            sample_count: 100,
            jitter_us: 10.0,
            ci95_hz: 0.05,
            matches_reported: true,
            reported_refresh_hz: 60.0,
            warnings: Vec::new(),
        });
        assert!(session.display_refresh_validated());

        // Changing display should clear validation
        session.set_selected_display(test_monitor());
        assert!(!session.display_refresh_validated());
    }
}
