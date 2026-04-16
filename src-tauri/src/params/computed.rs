//! Computed/derived values — not parameters, not stored.
//!
//! Methods on Registry that compute from current parameter values + hardware context.
//! These are queried by the frontend and by internal logic but are never persisted.

use super::registry::Registry;
use super::Envelope;

impl Registry {
    /// Visual field width in degrees, or None if monitor not connected.
    pub fn visual_field_width_deg(&self) -> Option<f64> {
        self.build_display_geometry()
            .map(|g| g.visual_field_width_deg())
    }

    /// Visual field height in degrees, or None if monitor not connected.
    pub fn visual_field_height_deg(&self) -> Option<f64> {
        self.build_display_geometry()
            .map(|g| g.visual_field_height_deg())
    }

    /// Maximum eccentricity in degrees (for ring stimulus), or None if monitor not connected.
    pub fn max_eccentricity_deg(&self) -> Option<f64> {
        self.build_display_geometry()
            .map(|g| g.get_max_eccentricity_deg())
    }

    /// Sweep duration in seconds for the current envelope, or None if not applicable.
    ///
    /// - Bar: visual_field_width / sweep_speed
    /// - Wedge: 360 / rotation_speed
    /// - Ring: max_eccentricity / expansion_speed
    /// - Fullfield: None (continuous)
    pub fn sweep_duration_sec(&self) -> Option<f64> {
        match self.stimulus_envelope() {
            Envelope::Bar => {
                let vf_width = self.visual_field_width_deg()?;
                let speed = self.sweep_speed_deg_per_sec();
                if speed > 0.0 {
                    Some((vf_width + self.stimulus_width_deg()) / speed)
                } else {
                    None
                }
            }
            Envelope::Wedge => {
                let speed = self.rotation_speed_deg_per_sec();
                if speed > 0.0 {
                    Some(360.0 / speed)
                } else {
                    None
                }
            }
            Envelope::Ring => {
                let max_ecc = self.max_eccentricity_deg()?;
                let speed = self.expansion_speed_deg_per_sec();
                if speed > 0.0 {
                    Some(max_ecc / speed)
                } else {
                    None
                }
            }
            Envelope::Fullfield => None,
        }
    }

    /// Luminance high = mean_luminance * (1 + contrast).
    /// Clamped to [0, 1] for display output.
    pub fn luminance_high(&self) -> f64 {
        let mean = self.mean_luminance();
        let contrast = self.contrast();
        (mean + contrast * mean).clamp(0.0, 1.0)
    }

    /// Luminance low = mean_luminance * (1 - contrast).
    /// Clamped to [0, 1] for display output.
    pub fn luminance_low(&self) -> f64 {
        let mean = self.mean_luminance();
        let contrast = self.contrast();
        (mean - contrast * mean).clamp(0.0, 1.0)
    }

    /// Build a DisplayGeometry from current params + hardware context.
    /// Returns None if monitor dimensions are not available.
    fn build_display_geometry(&self) -> Option<openisi_stimulus::geometry::DisplayGeometry> {
        let hw = &self.hardware;
        let (width_cm, height_cm) = match (hw.monitor_width_cm, hw.monitor_height_cm) {
            (Some(w), Some(h)) if w > 0.0 && h > 0.0 => (w, h),
            _ => return None,
        };

        let width_px = hw.monitor_width_px.unwrap_or(1920);
        let height_px = hw.monitor_height_px.unwrap_or(1080);

        Some(openisi_stimulus::geometry::DisplayGeometry::new(
            self.experiment_projection(),
            self.viewing_distance_cm(),
            self.horizontal_offset_deg(),
            self.vertical_offset_deg(),
            width_cm,
            height_cm,
            width_px,
            height_px,
        ))
    }
}
