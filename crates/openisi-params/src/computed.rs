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
    /// Returns None if **any** required field is unavailable:
    ///   - monitor panel cm (user-override > EDID; see
    ///     [`crate::hardware::effective_hardware_value`])
    ///   - monitor pixel resolution (must come from the connected display;
    ///     there is no sane "guess 1920×1080" fallback for a missing
    ///     monitor — geometry without a real panel is meaningless and the
    ///     UI must surface "no display selected" rather than render to a
    ///     fictional canvas).
    pub fn build_display_geometry(&self) -> Option<openisi_stimulus::geometry::DisplayGeometry> {
        let width_cm = self.effective_monitor_width_cm()?;
        let height_cm = self.effective_monitor_height_cm()?;
        let hw = &self.hardware;
        let width_px = hw.monitor_width_px?;
        let height_px = hw.monitor_height_px?;

        Some(openisi_stimulus::geometry::DisplayGeometry::new(
            self.experiment_projection(),
            self.viewing_distance_cm(),
            self.horizontal_offset_deg(),
            self.vertical_offset_deg(),
            self.bisector_x_cm(),
            self.bisector_y_cm(),
            width_cm,
            height_cm,
            width_px,
            height_px,
        ))
    }

    /// Effective monitor panel width in cm — see
    /// [`crate::hardware::effective_hardware_value`] for precedence rules.
    pub fn effective_monitor_width_cm(&self) -> Option<f64> {
        crate::hardware::effective_hardware_value(
            self.is_user_override(crate::ParamId::MonitorWidthCm),
            self.monitor_width_cm(),
            self.hardware.monitor_width_cm,
            |w| *w > 0.0,
        )
    }

    /// Effective monitor panel height in cm — same precedence as width.
    pub fn effective_monitor_height_cm(&self) -> Option<f64> {
        crate::hardware::effective_hardware_value(
            self.is_user_override(crate::ParamId::MonitorHeightCm),
            self.monitor_height_cm(),
            self.hardware.monitor_height_cm,
            |h| *h > 0.0,
        )
    }
}
