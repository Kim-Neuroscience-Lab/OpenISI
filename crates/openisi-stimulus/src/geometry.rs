//! DisplayGeometry — Display geometry and projection transformation.
//!
//! Port of `display_geometry.gd`. Handles mapping between visual angles (degrees)
//! and screen coordinates for different display configurations: cartesian (flat),
//! spherical (Marshel et al. 2012), and cylindrical.

use serde::{Deserialize, Serialize};

/// Projection type for the display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProjectionType {
    /// Cartesian coordinates on flat screen (physical cm units)
    Cartesian = 0,
    /// Spherical correction for flat screen (Marshel et al. 2012)
    /// Sphere radius = viewing distance. Ensures constant spatial/temporal
    /// frequency across the visual field.
    Spherical = 1,
    /// Cylindrical curved display
    Cylindrical = 2,
}

impl ProjectionType {
    pub fn from_int(v: i32) -> Option<Self> {
        match v {
            0 => Some(ProjectionType::Cartesian),
            1 => Some(ProjectionType::Spherical),
            2 => Some(ProjectionType::Cylindrical),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ProjectionType::Cartesian => "cartesian",
            ProjectionType::Spherical => "spherical",
            ProjectionType::Cylindrical => "cylindrical",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "cartesian" => Some(ProjectionType::Cartesian),
            "spherical" => Some(ProjectionType::Spherical),
            "cylindrical" => Some(ProjectionType::Cylindrical),
            _ => None,
        }
    }
}

/// Display geometry configuration and coordinate transforms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayGeometry {
    /// Current projection type
    pub projection_type: ProjectionType,
    /// Distance from subject's eye to display surface (cm)
    pub viewing_distance_cm: f64,
    /// Horizontal angular offset of display center from straight ahead (degrees, + = right)
    pub center_azimuth_deg: f64,
    /// Vertical angular offset of display center from straight ahead (degrees, + = up)
    pub center_elevation_deg: f64,
    /// Physical display width (cm)
    pub display_width_cm: f64,
    /// Physical display height (cm)
    pub display_height_cm: f64,
    /// Display width in pixels
    pub display_width_px: u32,
    /// Display height in pixels
    pub display_height_px: u32,
}

impl DisplayGeometry {
    /// Create with explicit parameters.
    pub fn new(
        projection_type: ProjectionType,
        viewing_distance_cm: f64,
        center_azimuth_deg: f64,
        center_elevation_deg: f64,
        display_width_cm: f64,
        display_height_cm: f64,
        display_width_px: u32,
        display_height_px: u32,
    ) -> Self {
        Self {
            projection_type,
            viewing_distance_cm,
            center_azimuth_deg,
            center_elevation_deg,
            display_width_cm,
            display_height_cm,
            display_width_px,
            display_height_px,
        }
    }

    // =========================================================================
    // Visual Field Calculations
    // =========================================================================

    /// Compute the visual field width in degrees.
    pub fn visual_field_width_deg(&self) -> f64 {
        match self.projection_type {
            ProjectionType::Cartesian | ProjectionType::Spherical => {
                // Standard flat screen: 2 * atan(half_width / distance)
                // Spherical: same visual field extent, but coordinate transform differs
                let half_width = self.display_width_cm / 2.0;
                2.0 * (half_width / self.viewing_distance_cm).atan().to_degrees()
            }
            ProjectionType::Cylindrical => {
                // Cylindrical: horizontal arc, radius = viewing distance
                let arc_length = self.display_width_cm;
                (arc_length / self.viewing_distance_cm).to_degrees()
            }
        }
    }

    /// Compute the visual field height in degrees.
    pub fn visual_field_height_deg(&self) -> f64 {
        match self.projection_type {
            ProjectionType::Cartesian
            | ProjectionType::Spherical
            | ProjectionType::Cylindrical => {
                // All types: flat in vertical
                let half_height = self.display_height_cm / 2.0;
                2.0 * (half_height / self.viewing_distance_cm).atan().to_degrees()
            }
        }
    }

    // =========================================================================
    // Coordinate Transformations
    // =========================================================================

    /// Convert visual angle (degrees from center) to normalized screen UV (0–1).
    /// Returns (u, v) where (0,0) is top-left, (1,1) is bottom-right.
    pub fn angle_to_uv(&self, azimuth_deg: f64, elevation_deg: f64) -> (f64, f64) {
        let local_az = azimuth_deg - self.center_azimuth_deg;
        let local_el = elevation_deg - self.center_elevation_deg;

        match self.projection_type {
            ProjectionType::Cartesian => self.flat_angle_to_uv(local_az, local_el),
            ProjectionType::Spherical => self.spherical_angle_to_uv(local_az, local_el),
            ProjectionType::Cylindrical => self.cylindrical_angle_to_uv(local_az, local_el),
        }
    }

    /// Convert normalized screen UV to visual angle (degrees from center).
    /// Returns (azimuth, elevation) in degrees.
    pub fn uv_to_angle(&self, u: f64, v: f64) -> (f64, f64) {
        let (local_az, local_el) = match self.projection_type {
            ProjectionType::Cartesian => self.flat_uv_to_angle(u, v),
            ProjectionType::Spherical => self.spherical_uv_to_angle(u, v),
            ProjectionType::Cylindrical => self.cylindrical_uv_to_angle(u, v),
        };

        (
            local_az + self.center_azimuth_deg,
            local_el + self.center_elevation_deg,
        )
    }

    /// Convert visual angle to pixel coordinates.
    pub fn angle_to_px(&self, azimuth_deg: f64, elevation_deg: f64) -> (f64, f64) {
        let (u, v) = self.angle_to_uv(azimuth_deg, elevation_deg);
        (u * self.display_width_px as f64, v * self.display_height_px as f64)
    }

    /// Convert pixel coordinates to visual angle.
    pub fn px_to_angle(&self, px_x: f64, px_y: f64) -> (f64, f64) {
        let u = px_x / self.display_width_px as f64;
        let v = px_y / self.display_height_px as f64;
        self.uv_to_angle(u, v)
    }

    /// Convert degrees to pixels (linear approximation).
    pub fn deg_to_px(&self, deg: f64, horizontal: bool) -> f64 {
        if horizontal {
            (deg / self.visual_field_width_deg()) * self.display_width_px as f64
        } else {
            (deg / self.visual_field_height_deg()) * self.display_height_px as f64
        }
    }

    /// Convert pixels to degrees (linear approximation).
    pub fn px_to_deg(&self, px: f64, horizontal: bool) -> f64 {
        if horizontal {
            (px / self.display_width_px as f64) * self.visual_field_width_deg()
        } else {
            (px / self.display_height_px as f64) * self.visual_field_height_deg()
        }
    }

    // =========================================================================
    // Flat Projection (Cartesian)
    // =========================================================================

    fn flat_angle_to_uv(&self, az_deg: f64, el_deg: f64) -> (f64, f64) {
        let x_cm = self.viewing_distance_cm * az_deg.to_radians().tan();
        let y_cm = self.viewing_distance_cm * el_deg.to_radians().tan();
        let u = 0.5 + (x_cm / self.display_width_cm);
        let v = 0.5 - (y_cm / self.display_height_cm); // Y inverted
        (u, v)
    }

    fn flat_uv_to_angle(&self, u: f64, v: f64) -> (f64, f64) {
        let x_cm = (u - 0.5) * self.display_width_cm;
        let y_cm = (0.5 - v) * self.display_height_cm;
        let az = (x_cm / self.viewing_distance_cm).atan().to_degrees();
        let el = (y_cm / self.viewing_distance_cm).atan().to_degrees();
        (az, el)
    }

    // =========================================================================
    // Spherical Projection (Marshel et al. 2012)
    // =========================================================================

    fn spherical_angle_to_uv(&self, az_deg: f64, el_deg: f64) -> (f64, f64) {
        let xo = self.viewing_distance_cm;
        let az_rad = az_deg.to_radians();
        let el_rad = el_deg.to_radians();

        // Horizontal position from azimuth
        let y_cm = xo * az_rad.tan();

        // Vertical position from elevation (depends on horizontal eccentricity)
        let r_horizontal = (xo * xo + y_cm * y_cm).sqrt();
        let z_cm = r_horizontal * el_rad.tan();

        let u = 0.5 + (y_cm / self.display_width_cm);
        let v = 0.5 - (z_cm / self.display_height_cm);
        (u, v)
    }

    fn spherical_uv_to_angle(&self, u: f64, v: f64) -> (f64, f64) {
        let xo = self.viewing_distance_cm;
        let y_cm = (u - 0.5) * self.display_width_cm;
        let z_cm = (0.5 - v) * self.display_height_cm;

        let az_rad = (y_cm / xo).atan();
        let r = (xo * xo + y_cm * y_cm + z_cm * z_cm).sqrt();
        let el_rad = if r > 0.0 { (z_cm / r).asin() } else { 0.0 };

        (az_rad.to_degrees(), el_rad.to_degrees())
    }

    // =========================================================================
    // Cylindrical Projection
    // =========================================================================

    fn cylindrical_angle_to_uv(&self, az_deg: f64, el_deg: f64) -> (f64, f64) {
        // Horizontal: curved (radius = viewing distance)
        let az_rad = az_deg.to_radians();
        let x_arc = self.viewing_distance_cm * az_rad;
        let u = 0.5 + (x_arc / self.display_width_cm);

        // Vertical: flat
        let y_cm = self.viewing_distance_cm * el_deg.to_radians().tan();
        let v = 0.5 - (y_cm / self.display_height_cm);
        (u, v)
    }

    fn cylindrical_uv_to_angle(&self, u: f64, v: f64) -> (f64, f64) {
        let x_arc = (u - 0.5) * self.display_width_cm;
        let az = (x_arc / self.viewing_distance_cm).to_degrees();

        let y_cm = (0.5 - v) * self.display_height_cm;
        let el = (y_cm / self.viewing_distance_cm).atan().to_degrees();
        (az, el)
    }

    // =========================================================================
    // Maximum Eccentricity (for ring stimulus)
    // =========================================================================

    /// Compute maximum eccentricity from center offset to the furthest corner.
    /// Used by ring stimulus to ensure it fully enters/exits the screen.
    pub fn get_max_eccentricity_deg(&self) -> f64 {
        let corners = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)];
        corners
            .iter()
            .map(|&(u, v)| self.compute_eccentricity_at_uv(u, v))
            .fold(0.0_f64, f64::max)
    }

    /// Compute eccentricity at a UV position.
    fn compute_eccentricity_at_uv(&self, u: f64, v: f64) -> f64 {
        let y_cm = (u - 0.5) * self.display_width_cm;
        let z_cm = (0.5 - v) * self.display_height_cm;

        let xo = self.viewing_distance_cm;
        let offset_y_cm = self.center_azimuth_deg.to_radians().tan() * xo;
        let offset_z_cm = self.center_elevation_deg.to_radians().tan() * xo;

        let y_cm = y_cm - offset_y_cm;
        let z_cm = z_cm - offset_z_cm;

        let radial_cm = (y_cm * y_cm + z_cm * z_cm).sqrt();
        radial_cm.atan2(xo).to_degrees()
    }

    // =========================================================================
    // Shader Parameters
    // =========================================================================

    /// Get parameters for passing to GPU shader uniforms.
    pub fn get_shader_params(&self) -> ShaderGeometryParams {
        ShaderGeometryParams {
            projection_type: self.projection_type as u32,
            viewing_distance_cm: self.viewing_distance_cm as f32,
            center_offset_deg: [self.center_azimuth_deg as f32, self.center_elevation_deg as f32],
            display_size_cm: [self.display_width_cm as f32, self.display_height_cm as f32],
            visual_field_deg: [
                self.visual_field_width_deg() as f32,
                self.visual_field_height_deg() as f32,
            ],
        }
    }
}

/// Geometry parameters formatted for GPU uniform buffer.
/// The renderer crate wraps this with `bytemuck::Pod` for GPU upload.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ShaderGeometryParams {
    pub projection_type: u32,
    pub viewing_distance_cm: f32,
    pub center_offset_deg: [f32; 2],
    pub display_size_cm: [f32; 2],
    pub visual_field_deg: [f32; 2],
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_geometry(proj: ProjectionType) -> DisplayGeometry {
        DisplayGeometry::new(proj, 25.0, 0.0, 0.0, 53.0, 30.0, 1920, 1080)
    }

    // --- Visual Field ---

    #[test]
    fn test_cartesian_visual_field_width() {
        let g = test_geometry(ProjectionType::Cartesian);
        let vf = g.visual_field_width_deg();
        // 2 * atan(26.5 / 25.0) ≈ 93.3°
        assert!((vf - 93.3).abs() < 1.0, "Cartesian VF width: {vf}");
    }

    #[test]
    fn test_spherical_visual_field_same_as_cartesian() {
        let gc = test_geometry(ProjectionType::Cartesian);
        let gs = test_geometry(ProjectionType::Spherical);
        assert!(
            (gc.visual_field_width_deg() - gs.visual_field_width_deg()).abs() < 0.001,
            "Spherical and Cartesian should have same VF width"
        );
    }

    #[test]
    fn test_cylindrical_visual_field_width() {
        let g = test_geometry(ProjectionType::Cylindrical);
        let vf = g.visual_field_width_deg();
        // arc_length / radius in degrees = (53.0 / 25.0) * (180/π) ≈ 121.5°
        let expected = (53.0 / 25.0_f64).to_degrees();
        assert!(
            (vf - expected).abs() < 0.1,
            "Cylindrical VF width: {vf} vs expected {expected}"
        );
    }

    // --- Coordinate Roundtrips ---

    #[test]
    fn test_flat_roundtrip() {
        let g = test_geometry(ProjectionType::Cartesian);
        let (u, v) = g.angle_to_uv(10.0, -5.0);
        let (az, el) = g.uv_to_angle(u, v);
        assert!((az - 10.0).abs() < 0.01, "Azimuth roundtrip: {az}");
        assert!((el - (-5.0)).abs() < 0.01, "Elevation roundtrip: {el}");
    }

    #[test]
    fn test_spherical_roundtrip() {
        let g = test_geometry(ProjectionType::Spherical);
        let (u, v) = g.angle_to_uv(15.0, 8.0);
        let (az, el) = g.uv_to_angle(u, v);
        assert!((az - 15.0).abs() < 0.01, "Spherical az roundtrip: {az}");
        assert!((el - 8.0).abs() < 0.01, "Spherical el roundtrip: {el}");
    }

    #[test]
    fn test_cylindrical_roundtrip() {
        let g = test_geometry(ProjectionType::Cylindrical);
        let (u, v) = g.angle_to_uv(20.0, -10.0);
        let (az, el) = g.uv_to_angle(u, v);
        assert!(
            (az - 20.0).abs() < 0.01,
            "Cylindrical az roundtrip: {az}"
        );
        assert!(
            (el - (-10.0)).abs() < 0.01,
            "Cylindrical el roundtrip: {el}"
        );
    }

    #[test]
    fn test_center_maps_to_half() {
        for proj in [
            ProjectionType::Cartesian,
            ProjectionType::Spherical,
            ProjectionType::Cylindrical,
        ] {
            let g = test_geometry(proj);
            let (u, v) = g.angle_to_uv(0.0, 0.0);
            assert!(
                (u - 0.5).abs() < 0.001 && (v - 0.5).abs() < 0.001,
                "{proj:?}: center should map to (0.5, 0.5), got ({u}, {v})"
            );
        }
    }

    // --- Center Offset ---

    #[test]
    fn test_center_offset() {
        let mut g = test_geometry(ProjectionType::Cartesian);
        g.center_azimuth_deg = 10.0;
        g.center_elevation_deg = 5.0;
        // Looking at center offset point should map to screen center
        let (u, v) = g.angle_to_uv(10.0, 5.0);
        assert!(
            (u - 0.5).abs() < 0.001 && (v - 0.5).abs() < 0.001,
            "Center offset should map to (0.5, 0.5), got ({u}, {v})"
        );
    }

    // --- Max Eccentricity ---

    #[test]
    fn test_max_eccentricity_positive() {
        let g = test_geometry(ProjectionType::Cartesian);
        let ecc = g.get_max_eccentricity_deg();
        assert!(ecc > 0.0, "Max eccentricity should be positive: {ecc}");
        // For a 53x30cm screen at 25cm, corners are quite far out
        assert!(ecc > 40.0, "Max eccentricity should be > 40°: {ecc}");
    }

    #[test]
    fn test_max_eccentricity_with_offset() {
        let mut g = test_geometry(ProjectionType::Cartesian);
        let ecc_centered = g.get_max_eccentricity_deg();
        g.center_azimuth_deg = 20.0;
        let ecc_offset = g.get_max_eccentricity_deg();
        // Offsetting should change max eccentricity (one corner closer, opposite farther)
        assert!(
            (ecc_centered - ecc_offset).abs() > 0.1,
            "Offset should change max eccentricity"
        );
    }
}
