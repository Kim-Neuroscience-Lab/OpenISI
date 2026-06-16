//! `ConfigStore` — the single live, typed parameter store.
//!
//! Holds the three typed configs (`RigConfig`/`ExperimentConfig`/`AnalysisConfig`)
//! live, plus the runtime `HardwareContext`. serde/schemars/garde own
//! (de)serialization, schema, and the *static* per-field bounds; the only
//! hand-rolled logic here is the **dynamic hardware constraints** (in
//! `constraints.rs`), which cross rig/experiment struct boundaries and need live
//! hardware — the one justified domain predicate the tools can't supply (see
//! `docs/TOOL_LEDGER.md`). [`ConfigSnapshot`] is the frozen payload carried on
//! thread channels and written into `.oisi` provenance.

use std::path::{Path, PathBuf};

use garde::Validate;

use crate::config::{AnalysisConfig, ExperimentConfig, RigConfig};
use crate::error::{ParamsError, ParamsResult};
use crate::hardware::{effective_hardware_value, HardwareContext};
use crate::observer::ParamChangeObserver;

/// The live, typed configuration store. One per app; held behind a `Mutex` in
/// `AppState` (replacing `Registry`).
pub struct ConfigStore {
    rig: RigConfig,
    experiment: ExperimentConfig,
    analysis: AnalysisConfig,
    /// View-only UI display state (runtime only — not loaded/saved as a config
    /// file, not persisted into the `.oisi`).
    ui_state: crate::config::UiStateConfig,
    hardware: HardwareContext,

    shipped_dir: PathBuf,
    user_dir: PathBuf,

    /// Whether the user has explicitly calibrated the monitor panel size.
    /// Only these two fields use the user-override > EDID precedence (see
    /// [`crate::hardware::effective_hardware_value`]); tracking two booleans is
    /// exactly the right size — a general per-field override set would be
    /// ceremony for a two-field need.
    monitor_width_user_set: bool,
    monitor_height_user_set: bool,

    observer: Option<Box<dyn ParamChangeObserver>>,
}

impl ConfigStore {
    /// Create a store at PARAM defaults. `shipped_dir` holds the read-only
    /// baseline `*.json`; `user_dir` is where the user layer reads/writes.
    pub fn new(shipped_dir: &Path, user_dir: &Path) -> Self {
        Self {
            rig: RigConfig::default(),
            experiment: ExperimentConfig::default(),
            analysis: AnalysisConfig::default(),
            ui_state: crate::config::UiStateConfig::default(),
            hardware: HardwareContext::default(),
            shipped_dir: shipped_dir.to_path_buf(),
            user_dir: user_dir.to_path_buf(),
            monitor_width_user_set: false,
            monitor_height_user_set: false,
            observer: None,
        }
    }

    /// Install the change-event observer (the Tauri shell forwards to the UI).
    pub fn set_observer(&mut self, observer: Box<dyn ParamChangeObserver>) {
        self.observer = Some(observer);
    }

    // ── Accessors (typed field access; consumers read these directly) ────────

    pub fn rig(&self) -> &RigConfig {
        &self.rig
    }
    pub fn experiment(&self) -> &ExperimentConfig {
        &self.experiment
    }
    pub fn analysis(&self) -> &AnalysisConfig {
        &self.analysis
    }
    pub fn ui_state(&self) -> &crate::config::UiStateConfig {
        &self.ui_state
    }
    pub fn hardware(&self) -> &HardwareContext {
        &self.hardware
    }

    /// Directory where named experiment templates live. Uses
    /// `rig.paths.experiments_directory` if set, else `user_dir/experiments`
    /// (the user layer must be writable). Mirrors the old `Registry::experiments_dir`.
    pub fn experiments_dir(&self) -> PathBuf {
        let dir = &self.rig.paths.experiments_directory;
        if dir.is_empty() {
            self.user_dir.join("experiments")
        } else {
            PathBuf::from(dir)
        }
    }
    pub fn shipped_dir(&self) -> &Path {
        &self.shipped_dir
    }
    pub fn user_dir(&self) -> &Path {
        &self.user_dir
    }

    // ── Persistence (shipped baseline + optional user overlay) ───────────────

    /// Load the rig config from `shipped_dir` (+ `user_dir` overlay). Records
    /// which monitor-cm fields the user layer set (for the user > EDID precedence).
    pub fn load_rig(&mut self) -> ParamsResult<()> {
        self.rig = crate::config::loader::load_target_from_dir(
            &self.shipped_dir,
            Some(&self.user_dir),
            "rig.json",
        )?;
        let user_rig = self.user_dir.join("rig.json");
        if user_rig.exists() {
            if let Ok(text) = std::fs::read_to_string(&user_rig) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    self.monitor_width_user_set = json_has_path(&v, &["geometry", "monitor_width_cm"]);
                    self.monitor_height_user_set =
                        json_has_path(&v, &["geometry", "monitor_height_cm"]);
                }
            }
        }
        self.clamp_to_hardware();
        Ok(())
    }

    /// Load the experiment config from `shipped_dir` (+ `user_dir` overlay).
    pub fn load_experiment(&mut self) -> ParamsResult<()> {
        self.experiment = crate::config::loader::load_target_from_dir(
            &self.shipped_dir,
            Some(&self.user_dir),
            "experiment.json",
        )?;
        Ok(())
    }

    /// Load the analysis config from `shipped_dir` (+ `user_dir` overlay).
    pub fn load_analysis(&mut self) -> ParamsResult<()> {
        self.analysis = crate::config::loader::load_target_from_dir(
            &self.shipped_dir,
            Some(&self.user_dir),
            "analysis.json",
        )?;
        Ok(())
    }

    /// Load a named experiment template (a full `ExperimentConfig` JSON document)
    /// from `path`, replacing the live experiment config. Validates statically
    /// (garde) and against live hardware bounds before committing.
    pub fn load_experiment_template(&mut self, path: &Path) -> ParamsResult<()> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| ParamsError::Config(format!("reading experiment template: {e}")))?;
        let next: ExperimentConfig = crate::config::loader::load_merged(&text, None)?;
        self.hw_bounds_with(&self.rig, &next).validate(&self.rig, &next)?;
        self.experiment = next;
        self.emit(&serde_json::Value::Null);
        Ok(())
    }

    /// Save the live experiment config as a named template (a full JSON document)
    /// at `path`, creating the parent directory if needed.
    pub fn save_experiment_template(&self, path: &Path) -> ParamsResult<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ParamsError::Config(format!("creating template dir: {e}")))?;
        }
        std::fs::write(path, crate::config::to_json(&self.experiment)?)
            .map_err(|e| ParamsError::Config(format!("writing experiment template: {e}")))?;
        Ok(())
    }

    /// Load all three persisted configs (rig + experiment + analysis).
    pub fn load_all(&mut self) -> ParamsResult<()> {
        self.load_rig()?;
        self.load_experiment()?;
        self.load_analysis()?;
        Ok(())
    }

    /// Persist the user layer (written as a full document; sparse shipped/dev
    /// layers are hand-maintained and merged on load).
    pub fn save_all(&self) -> ParamsResult<()> {
        std::fs::create_dir_all(&self.user_dir)
            .map_err(|e| ParamsError::Config(format!("creating user dir: {e}")))?;
        std::fs::write(self.user_dir.join("rig.json"), crate::config::to_json(&self.rig)?)
            .map_err(|e| ParamsError::Config(format!("writing rig.json: {e}")))?;
        std::fs::write(
            self.user_dir.join("experiment.json"),
            crate::config::to_json(&self.experiment)?,
        )
        .map_err(|e| ParamsError::Config(format!("writing experiment.json: {e}")))?;
        std::fs::write(
            self.user_dir.join("analysis.json"),
            crate::config::to_json(&self.analysis)?,
        )
        .map_err(|e| ParamsError::Config(format!("writing analysis.json: {e}")))?;
        Ok(())
    }

    // ── Mutation (UI overlay) ────────────────────────────────────────────────

    /// Apply a sparse JSON overlay to the rig config (RFC 7386 merge), validate
    /// statically (garde) and against live hardware bounds, then store + emit.
    /// Rejects out-of-bound values — never silently clamps a user's explicit set
    /// (clamping is reserved for hardware *narrowing*, see [`inject_hardware`]).
    pub fn merge_rig(&mut self, overlay: &serde_json::Value) -> ParamsResult<()> {
        let next: RigConfig = merge_into(&self.rig, overlay)?;
        next.validate().map_err(validation_err)?;
        self.hw_bounds_with(&next, &self.experiment).validate(&next, &self.experiment)?;
        if json_has_path(overlay, &["geometry", "monitor_width_cm"]) {
            self.monitor_width_user_set = true;
        }
        if json_has_path(overlay, &["geometry", "monitor_height_cm"]) {
            self.monitor_height_user_set = true;
        }
        self.rig = next;
        self.emit(overlay);
        Ok(())
    }

    /// Calibrate `camera.um_per_pixel` from the head-ring overlay: a ring of
    /// known physical diameter spanning its pixel radius defines the pixel↔µm
    /// scale (see [`crate::config::rig::RingOverlay::um_per_pixel`]). Sets the
    /// value and returns it. This is computed-WITH-override: it writes the
    /// measured value into the same field the user can still set manually
    /// (mirrors the monitor-cm "user value wins" precedence). Errors — never
    /// silently — when the ring can't define a scale, so a half-set ring can't
    /// quietly calibrate to a garbage number.
    pub fn calibrate_um_per_pixel_from_ring(&mut self) -> ParamsResult<f64> {
        let um_per_pixel = self.rig.ring_overlay.um_per_pixel().ok_or_else(|| {
            ParamsError::Validation(
                "head ring is not set up for calibration: enable it and set a \
                 non-zero radius and physical diameter first"
                    .into(),
            )
        })?;
        self.merge_rig(&serde_json::json!({ "camera": { "um_per_pixel": um_per_pixel } }))?;
        Ok(um_per_pixel)
    }

    /// Apply a sparse JSON overlay to the experiment config.
    pub fn merge_experiment(&mut self, overlay: &serde_json::Value) -> ParamsResult<()> {
        let next: ExperimentConfig = merge_into(&self.experiment, overlay)?;
        next.validate().map_err(validation_err)?;
        self.hw_bounds_with(&self.rig, &next).validate(&self.rig, &next)?;
        self.experiment = next;
        self.emit(overlay);
        Ok(())
    }

    /// Apply a sparse JSON overlay to the analysis config.
    pub fn merge_analysis(&mut self, overlay: &serde_json::Value) -> ParamsResult<()> {
        let next: AnalysisConfig = merge_into(&self.analysis, overlay)?;
        next.validate().map_err(validation_err)?;
        self.analysis = next;
        self.emit(overlay);
        Ok(())
    }

    /// Apply a sparse JSON overlay to the (runtime-only) UI-state config.
    pub fn merge_ui_state(&mut self, overlay: &serde_json::Value) -> ParamsResult<()> {
        let next: crate::config::UiStateConfig = merge_into(&self.ui_state, overlay)?;
        next.validate().map_err(validation_err)?;
        self.ui_state = next;
        self.emit(overlay);
        Ok(())
    }

    // ── Hardware context ─────────────────────────────────────────────────────

    /// Inject new hardware capabilities and clamp existing values to the new
    /// dynamic bounds (the old `inject_hardware` semantics).
    pub fn inject_hardware(&mut self, ctx: HardwareContext) {
        self.hardware = ctx;
        self.clamp_to_hardware();
        // A hardware change can move effective monitor dims / clamped values.
        self.emit(&serde_json::Value::Null);
    }

    /// Clamp the 5 hardware-constrained fields to the current hardware bounds.
    fn clamp_to_hardware(&mut self) {
        let bounds = self.hw_bounds_with(&self.rig, &self.experiment);
        if let Some((min, max)) = bounds.exposure_us {
            self.rig.camera.exposure_us = self.rig.camera.exposure_us.clamp(min, max);
        }
        if let Some((min, max)) = bounds.binning {
            self.rig.camera.binning = self.rig.camera.binning.clamp(min, max);
        }
        if let Some((min, max)) = bounds.fps {
            self.rig.display.target_stimulus_fps =
                self.rig.display.target_stimulus_fps.clamp(min, max);
        }
        if let Some((min, max)) = bounds.strobe_hz {
            self.experiment.stimulus.params.strobe_frequency_hz =
                self.experiment.stimulus.params.strobe_frequency_hz.clamp(min, max);
        }
        if let Some((min, max)) = bounds.stimulus_width_deg {
            self.experiment.stimulus.params.stimulus_width_deg =
                self.experiment.stimulus.params.stimulus_width_deg.clamp(min, max);
        }
    }

    /// Compute the live dynamic-constraint bounds (the 5 edges from the old
    /// `constraints.rs`) for a candidate rig+experiment against current hardware.
    fn hw_bounds_with(&self, rig: &RigConfig, exp: &ExperimentConfig) -> HwBounds {
        HwBounds::compute(rig, exp, &self.hardware)
    }

    // ── Snapshot ─────────────────────────────────────────────────────────────

    /// Freeze the current config for a thread message or `.oisi` provenance.
    pub fn snapshot(&self) -> ConfigSnapshot {
        ConfigSnapshot {
            rig: self.rig.clone(),
            experiment: self.experiment.clone(),
            analysis: self.analysis.clone(),
            hardware: self.hardware.clone(),
            monitor_width_user_set: self.monitor_width_user_set,
            monitor_height_user_set: self.monitor_height_user_set,
        }
    }

    fn emit(&mut self, _overlay: &serde_json::Value) {
        if let Some(ref observer) = self.observer {
            // The typed schema-driven UI re-reads the full config on change;
            // the payload just signals "config changed". (Slice 3e refines this
            // to carry the changed paths if the UI needs finer granularity.)
            observer.notify(serde_json::json!({ "changed": true }));
        }
    }
}

/// Frozen configuration — the thread-channel + `.oisi`-provenance payload.
/// All fields are `Clone + Send + Sync + Serialize`.
#[derive(Debug, Clone)]
pub struct ConfigSnapshot {
    pub rig: RigConfig,
    pub experiment: ExperimentConfig,
    pub analysis: AnalysisConfig,
    pub hardware: HardwareContext,
    monitor_width_user_set: bool,
    monitor_height_user_set: bool,
}

impl ConfigSnapshot {
    /// Effective monitor panel width in cm — user calibration > EDID > None.
    pub fn effective_monitor_width_cm(&self) -> Option<f64> {
        effective_hardware_value(
            self.monitor_width_user_set,
            self.rig.geometry.monitor_width_cm,
            self.hardware.monitor_width_cm,
            |w| *w > 0.0,
        )
    }

    /// Effective monitor panel height in cm — same precedence as width.
    pub fn effective_monitor_height_cm(&self) -> Option<f64> {
        effective_hardware_value(
            self.monitor_height_user_set,
            self.rig.geometry.monitor_height_cm,
            self.hardware.monitor_height_cm,
            |h| *h > 0.0,
        )
    }

    /// Build a `DisplayGeometry` from current params + hardware, or `None` if any
    /// required field (monitor cm or pixel resolution) is unavailable. Mirrors
    /// the old `Registry::build_display_geometry` exactly.
    pub fn build_display_geometry(&self) -> Option<openisi_stimulus::geometry::DisplayGeometry> {
        let width_cm = self.effective_monitor_width_cm()?;
        let height_cm = self.effective_monitor_height_cm()?;
        let width_px = self.hardware.monitor_width_px?;
        let height_px = self.hardware.monitor_height_px?;
        Some(openisi_stimulus::geometry::DisplayGeometry::new(
            self.experiment.geometry.projection,
            self.rig.geometry.viewing_distance_cm,
            self.experiment.geometry.horizontal_offset_deg,
            self.experiment.geometry.vertical_offset_deg,
            self.rig.geometry.bisector_x_cm,
            self.rig.geometry.bisector_y_cm,
            width_cm,
            height_cm,
            width_px,
            height_px,
        ))
    }

    pub fn visual_field_width_deg(&self) -> Option<f64> {
        self.build_display_geometry().map(|g| g.visual_field_width_deg())
    }

    pub fn visual_field_height_deg(&self) -> Option<f64> {
        self.build_display_geometry().map(|g| g.visual_field_height_deg())
    }

    pub fn max_eccentricity_deg(&self) -> Option<f64> {
        self.build_display_geometry().map(|g| g.get_max_eccentricity_deg())
    }

    /// Sweep duration in seconds for the current envelope, or `None` if not
    /// applicable. Mirrors the old `Registry::sweep_duration_sec`.
    pub fn sweep_duration_sec(&self) -> Option<f64> {
        use crate::Envelope;
        let p = &self.experiment.stimulus.params;
        match self.experiment.stimulus.envelope {
            Envelope::Bar => {
                let vf_width = self.visual_field_width_deg()?;
                (p.sweep_speed_deg_per_sec > 0.0)
                    .then(|| (vf_width + p.stimulus_width_deg) / p.sweep_speed_deg_per_sec)
            }
            Envelope::Wedge => (p.rotation_speed_deg_per_sec > 0.0)
                .then(|| 360.0 / p.rotation_speed_deg_per_sec),
            Envelope::Ring => {
                let max_ecc = self.max_eccentricity_deg()?;
                (p.expansion_speed_deg_per_sec > 0.0)
                    .then(|| max_ecc / p.expansion_speed_deg_per_sec)
            }
            Envelope::Fullfield => None,
        }
    }

    /// Luminance high = mean * (1 + contrast), clamped to [0, 1].
    pub fn luminance_high(&self) -> f64 {
        let p = &self.experiment.stimulus.params;
        (p.mean_luminance + p.contrast * p.mean_luminance).clamp(0.0, 1.0)
    }

    /// Luminance low = mean * (1 - contrast), clamped to [0, 1].
    pub fn luminance_low(&self) -> f64 {
        let p = &self.experiment.stimulus.params;
        (p.mean_luminance - p.contrast * p.mean_luminance).clamp(0.0, 1.0)
    }
}

/// The live dynamic-constraint bounds (the 5 edges from `constraints.rs`).
/// `None` = no hardware override; the static garde bound applies.
struct HwBounds {
    exposure_us: Option<(u32, u32)>,
    binning: Option<(u16, u16)>,
    fps: Option<(u32, u32)>,
    strobe_hz: Option<(f64, f64)>,
    stimulus_width_deg: Option<(f64, f64)>,
}

impl HwBounds {
    fn compute(rig: &RigConfig, exp: &ExperimentConfig, hw: &HardwareContext) -> Self {
        // Edge 5: stimulus width max from visual-field width (viewing distance +
        // monitor dims + projection). No fictional pixel fallback — unknown
        // resolution ⇒ no dynamic bound (the static garde bound stands).
        let stimulus_width_deg = (|| {
            let (w_cm, h_cm) = match (hw.monitor_width_cm, hw.monitor_height_cm) {
                (Some(w), Some(h)) if w > 0.0 && h > 0.0 => (w, h),
                _ => return None,
            };
            let (w_px, h_px) = match (hw.monitor_width_px, hw.monitor_height_px) {
                (Some(w), Some(h)) if w > 0 && h > 0 => (w, h),
                _ => return None,
            };
            let vd = rig.geometry.viewing_distance_cm;
            if vd <= 0.0 {
                return None;
            }
            let geom = openisi_stimulus::geometry::DisplayGeometry::new(
                exp.geometry.projection,
                vd,
                0.0,
                0.0,
                0.0,
                0.0,
                w_cm,
                h_cm,
                w_px,
                h_px,
            );
            Some((0.001, geom.visual_field_width_deg()))
        })();

        Self {
            exposure_us: match (hw.camera_min_exposure_us, hw.camera_max_exposure_us) {
                (Some(min), Some(max)) => Some((min, max)),
                _ => None,
            },
            binning: hw.camera_max_binning.map(|max| (1, max)),
            fps: hw.monitor_refresh_hz.map(|hz| (1, hz)),
            strobe_hz: hw.measured_refresh_hz.map(|hz| (0.0, hz / 2.0)),
            stimulus_width_deg,
        }
    }

    /// Reject a candidate config whose hardware-constrained fields fall outside
    /// the live bounds (the old `set()` dynamic-validation semantics).
    fn validate(&self, rig: &RigConfig, exp: &ExperimentConfig) -> ParamsResult<()> {
        let oor = |what: &str, v: String, min: String, max: String| {
            Err(ParamsError::Validation(format!(
                "{what} {v} out of hardware range [{min}, {max}]"
            )))
        };
        if let Some((min, max)) = self.exposure_us {
            let v = rig.camera.exposure_us;
            if v < min || v > max {
                return oor("exposure_us", v.to_string(), min.to_string(), max.to_string());
            }
        }
        if let Some((min, max)) = self.binning {
            let v = rig.camera.binning;
            if v < min || v > max {
                return oor("binning", v.to_string(), min.to_string(), max.to_string());
            }
        }
        if let Some((min, max)) = self.fps {
            let v = rig.display.target_stimulus_fps;
            if v < min || v > max {
                return oor("target_stimulus_fps", v.to_string(), min.to_string(), max.to_string());
            }
        }
        if let Some((min, max)) = self.strobe_hz {
            let v = exp.stimulus.params.strobe_frequency_hz;
            if v < min || v > max {
                return oor("strobe_frequency_hz", v.to_string(), min.to_string(), max.to_string());
            }
        }
        if let Some((min, max)) = self.stimulus_width_deg {
            let v = exp.stimulus.params.stimulus_width_deg;
            if v < min || v > max {
                return oor("stimulus_width_deg", v.to_string(), min.to_string(), max.to_string());
            }
        }
        Ok(())
    }
}

/// Apply a sparse JSON overlay onto a typed config (serialize → RFC 7386 merge →
/// deserialize). serde + `json-patch`, not hand-rolled tree walking.
fn merge_into<T>(current: &T, overlay: &serde_json::Value) -> ParamsResult<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let mut base = serde_json::to_value(current)
        .map_err(|e| ParamsError::Config(format!("serializing config for merge: {e}")))?;
    json_patch::merge(&mut base, overlay);
    serde_json::from_value(base)
        .map_err(|e| ParamsError::Config(format!("applying config overlay: {e}")))
}

/// Map a garde validation report to a `ParamsError`.
fn validation_err(report: garde::Report) -> ParamsError {
    ParamsError::Validation(report.to_string())
}

/// True iff the JSON object tree contains the given nested path with a non-null leaf.
fn json_has_path(v: &serde_json::Value, path: &[&str]) -> bool {
    let mut cur = v;
    for seg in path {
        match cur.get(seg) {
            Some(next) => cur = next,
            None => return false,
        }
    }
    !cur.is_null()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn config_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config")
    }

    #[test]
    fn defaults_construct() {
        let store = ConfigStore::new(Path::new("."), Path::new("."));
        assert_eq!(store.rig().camera.binning, 4);
        assert_eq!(store.experiment().presentation.repetitions, 1);
    }

    #[test]
    fn computed_luminance_matches_registry_math() {
        let store = ConfigStore::new(Path::new("."), Path::new("."));
        let snap = store.snapshot();
        // Default mean=0.5, contrast=1.0 ⇒ high=1.0, low=0.0
        assert!((snap.luminance_high() - 1.0).abs() < 1e-10);
        assert!((snap.luminance_low() - 0.0).abs() < 1e-10);
    }

    #[test]
    fn merge_rejects_out_of_static_bound() {
        let mut store = ConfigStore::new(Path::new("."), Path::new("."));
        let overlay = serde_json::json!({ "camera": { "binning": 99 } }); // max 16
        assert!(store.merge_rig(&overlay).is_err());
    }

    #[test]
    fn merge_applies_valid_overlay() {
        let mut store = ConfigStore::new(Path::new("."), Path::new("."));
        let overlay = serde_json::json!({ "camera": { "exposure_us": 5000 } });
        store.merge_rig(&overlay).unwrap();
        assert_eq!(store.rig().camera.exposure_us, 5000);
    }

    #[test]
    fn calibrate_um_per_pixel_from_ring_sets_camera() {
        let mut store = ConfigStore::new(Path::new("."), Path::new("."));
        // Enable + size the ring: 8 mm @ 200 px ⇒ 20.0 µm/px.
        store
            .merge_rig(&serde_json::json!({
                "ring_overlay": { "enabled": true, "radius_px": 200, "diameter_mm": 8.0 }
            }))
            .unwrap();
        let upp = store.calibrate_um_per_pixel_from_ring().unwrap();
        assert_eq!(upp, 20.0);
        assert_eq!(store.rig().camera.um_per_pixel, 20.0); // written into the live field
    }

    #[test]
    fn calibrate_um_per_pixel_from_ring_errors_when_ring_unset() {
        let mut store = ConfigStore::new(Path::new("."), Path::new("."));
        // Default ring is disabled ⇒ no scale ⇒ explicit error, never a silent 0.
        assert!(store.calibrate_um_per_pixel_from_ring().is_err());
    }

    #[test]
    fn ring_overlay_partial_merge_preserves_unsent_fields() {
        // The `set_ring_overlay` command relies on this RFC-7386 partial-merge
        // semantics: a UI resize sends only the geometry keys and must NOT wipe a
        // previously-calibrated diameter.
        let mut store = ConfigStore::new(Path::new("."), Path::new("."));
        store
            .merge_rig(&serde_json::json!({ "ring_overlay": { "diameter_mm": 8.0 } }))
            .unwrap();
        assert_eq!(store.rig().ring_overlay.diameter_mm, 8.0);

        store
            .merge_rig(&serde_json::json!({ "ring_overlay": { "enabled": true, "radius_px": 150 } }))
            .unwrap();
        assert_eq!(store.rig().ring_overlay.radius_px, 150);
        assert!(store.rig().ring_overlay.enabled);
        assert_eq!(store.rig().ring_overlay.diameter_mm, 8.0); // preserved, not reset
    }

    #[test]
    fn hardware_injection_clamps_exposure() {
        let mut store = ConfigStore::new(Path::new("."), Path::new("."));
        store.merge_rig(&serde_json::json!({ "camera": { "exposure_us": 1000 } })).unwrap();
        store.inject_hardware(HardwareContext {
            camera_min_exposure_us: Some(5000),
            camera_max_exposure_us: Some(100_000),
            ..Default::default()
        });
        assert_eq!(store.rig().camera.exposure_us, 5000); // clamped up
    }

    #[test]
    fn merge_rejects_beyond_hardware_range() {
        let mut store = ConfigStore::new(Path::new("."), Path::new("."));
        store.inject_hardware(HardwareContext {
            camera_min_exposure_us: Some(100),
            camera_max_exposure_us: Some(50_000),
            ..Default::default()
        });
        // Beyond the dynamic max ⇒ rejected (not clamped).
        assert!(store
            .merge_rig(&serde_json::json!({ "camera": { "exposure_us": 100_000 } }))
            .is_err());
        // Within range ⇒ accepted.
        assert!(store
            .merge_rig(&serde_json::json!({ "camera": { "exposure_us": 25_000 } }))
            .is_ok());
    }

    #[test]
    fn fps_clamped_to_monitor_refresh() {
        let mut store = ConfigStore::new(Path::new("."), Path::new("."));
        store.merge_rig(&serde_json::json!({ "display": { "target_stimulus_fps": 120 } })).unwrap();
        store.inject_hardware(HardwareContext {
            monitor_refresh_hz: Some(60),
            ..Default::default()
        });
        assert_eq!(store.rig().display.target_stimulus_fps, 60);
    }

    #[test]
    fn monitor_cm_precedence_user_over_hardware() {
        let mut store = ConfigStore::new(Path::new("."), Path::new("."));
        // User calibrates width ⇒ effective width is the user value, not EDID.
        store.merge_rig(&serde_json::json!({ "geometry": { "monitor_width_cm": 52.0 } })).unwrap();
        store.inject_hardware(HardwareContext {
            monitor_width_cm: Some(99.0),
            ..Default::default()
        });
        let snap = store.snapshot();
        assert_eq!(snap.effective_monitor_width_cm(), Some(52.0));
    }

    #[test]
    fn monitor_cm_falls_back_to_hardware_when_not_user_set() {
        let mut store = ConfigStore::new(Path::new("."), Path::new("."));
        store.inject_hardware(HardwareContext {
            monitor_height_cm: Some(30.0),
            ..Default::default()
        });
        let snap = store.snapshot();
        assert_eq!(snap.effective_monitor_height_cm(), Some(30.0));
    }

    #[test]
    fn loads_shipped_json_configs() {
        // analysis.json exists in config/; rig.json/experiment.json may not yet
        // (authored in slice 3d). This exercises the analysis load path.
        let dir = config_dir();
        let analysis: AnalysisConfig = crate::config::loader::load_target_from_dir(
            &dir,
            None,
            "analysis.json",
        )
        .expect("shipped analysis.json loads");
        // cortex_source tuned to reliability/0.85 in the shipped file.
        assert_eq!(
            analysis.cortex_source,
            crate::config::analysis::CortexSource::Reliability { threshold: 0.85 }
        );
    }

    /// The shipped `config/experiment.json` loads cleanly into `ExperimentConfig`
    /// (no unknown/missing keys) — the production load path the GUI/headless use.
    #[test]
    fn loads_shipped_experiment_config() {
        let dir = config_dir();
        let _exp: ExperimentConfig =
            crate::config::loader::load_target_from_dir(&dir, None, "experiment.json")
                .expect("shipped experiment.json loads");
    }

    /// The shipped `config/rig.json` loads cleanly into `RigConfig`.
    #[test]
    fn loads_shipped_rig_config() {
        let dir = config_dir();
        let _rig: RigConfig =
            crate::config::loader::load_target_from_dir(&dir, None, "rig.json")
                .expect("shipped rig.json loads");
    }

    /// The committed dev overlay (`config/dev/*.json`) merges cleanly onto the
    /// shipped baseline for every target — the dev-profile load path. A typo or a
    /// stale key in a dev overlay (which `deny_unknown_fields` rejects) fails here.
    #[test]
    fn dev_overlay_merges_onto_shipped() {
        let shipped = config_dir();
        let dev = shipped.join("dev");
        let _rig: RigConfig =
            crate::config::loader::load_target_from_dir(&shipped, Some(&dev), "rig.json")
                .expect("dev rig.json overlay merges");
        let _exp: ExperimentConfig =
            crate::config::loader::load_target_from_dir(&shipped, Some(&dev), "experiment.json")
                .expect("dev experiment.json overlay merges");
        let _ana: AnalysisConfig =
            crate::config::loader::load_target_from_dir(&shipped, Some(&dev), "analysis.json")
                .expect("dev analysis.json overlay merges");
    }
}
