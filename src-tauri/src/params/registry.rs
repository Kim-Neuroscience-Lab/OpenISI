//! Registry — owns all parameter values, validates on set, serializes to TOML.
//!
//! Phase 2 additions: hardware context, dynamic constraints, batch mode,
//! event emission, snapshots.

use std::path::{Path, PathBuf};

use tauri::Emitter;

use super::constraints::{
    build_dynamic_constraints, ConstraintDependency, DynamicConstraint, EffectiveConstraint,
};
use super::hardware::HardwareContext;
use super::snapshot::RegistrySnapshot;
use super::{ParamDef, ParamId, ParamValue, PARAM_DEFS};

/// The parameter registry. Owns all ~70 parameter values in a flat Vec
/// indexed by `ParamId as usize`.
pub struct Registry {
    pub(crate) values: Vec<ParamValue>,
    config_dir: PathBuf,

    // ── Phase 2: hardware context ────────────────────────────────────
    pub(crate) hardware: HardwareContext,

    // ── Phase 2: dynamic constraints ─────────────────────────────────
    dynamic_constraints: Vec<DynamicConstraint>,
    /// Cached effective constraints, one per param. None = use static.
    effective_constraints: Vec<Option<EffectiveConstraint>>,

    // ── Phase 2: batch mode ──────────────────────────────────────────
    batch_depth: u32,
    pending_changes: Vec<ParamId>,

    // ── Phase 2: event emission ──────────────────────────────────────
    app_handle: Option<tauri::AppHandle>,
}

impl Registry {
    /// Create a new registry with all parameters at their defaults.
    pub fn new(config_dir: &Path) -> Self {
        let values: Vec<ParamValue> = PARAM_DEFS.iter().map(|def| def.default.clone()).collect();
        let dynamic_constraints = build_dynamic_constraints();
        let effective_constraints = vec![None; ParamId::count()];
        Self {
            values,
            config_dir: config_dir.to_path_buf(),
            hardware: HardwareContext::default(),
            dynamic_constraints,
            effective_constraints,
            batch_depth: 0,
            pending_changes: Vec::new(),
            app_handle: None,
        }
    }

    /// Set the Tauri app handle for event emission.
    pub fn set_app_handle(&mut self, handle: tauri::AppHandle) {
        self.app_handle = Some(handle);
    }

    // ── Get / Set ────────────────────────────────────────────────────

    /// Get a parameter value by ID.
    pub fn get(&self, id: ParamId) -> &ParamValue {
        &self.values[id as usize]
    }

    /// Set a parameter value, validating against its effective constraint.
    /// After storing, recomputes dependent constraints and clamps violators.
    pub fn set(&mut self, id: ParamId, value: ParamValue) -> Result<(), String> {
        let def = &PARAM_DEFS[id as usize];

        // Validate against effective constraint (dynamic override or static).
        let constraint = self.effective_constraint(id);
        constraint.validate(&value, &def.constraint)?;

        self.values[id as usize] = value;
        self.pending_changes.push(id);

        // Recompute constraints that depend on this param.
        self.recompute_dependents(id);

        // If not in batch mode, emit immediately.
        if self.batch_depth == 0 {
            self.emit_changes();
        }

        Ok(())
    }

    /// Set a parameter value without validation (used during TOML loading where
    /// we trust the file contents).
    pub(crate) fn set_unchecked(&mut self, id: ParamId, value: ParamValue) {
        self.values[id as usize] = value;
    }

    // ── Hardware Context ─────────────────────────────────────────────

    /// Inject new hardware context and recompute all hardware-dependent constraints.
    pub fn inject_hardware(&mut self, ctx: HardwareContext) {
        self.hardware = ctx;
        self.recompute_all_hardware_constraints();
        if self.batch_depth == 0 {
            self.emit_changes();
        }
    }

    /// Get a reference to the current hardware context.
    pub fn hardware(&self) -> &HardwareContext {
        &self.hardware
    }

    // ── Effective Constraints ────────────────────────────────────────

    /// Get the effective constraint for a parameter.
    /// Returns the dynamic override if present, otherwise wraps the static constraint.
    pub fn effective_constraint(&self, id: ParamId) -> EffectiveConstraint {
        self.effective_constraints[id as usize]
            .clone()
            .unwrap_or(EffectiveConstraint::Static)
    }

    /// Whether a parameter is currently active (should be visible/editable).
    /// Parameters with no active_when condition are always active.
    pub fn is_active(&self, id: ParamId) -> bool {
        let def = &PARAM_DEFS[id as usize];
        match def.active_when {
            Some(f) => f(self),
            None => true,
        }
    }

    // ── Batch Mode ───────────────────────────────────────────────────

    /// Run a closure in batch mode. Constraint recomputation happens after each
    /// individual set(), but event emission is deferred until the closure returns.
    pub fn batch<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.batch_depth += 1;
        let result = f(self);
        self.batch_depth -= 1;
        if self.batch_depth == 0 {
            self.emit_changes();
        }
        result
    }

    // ── Snapshots ────────────────────────────────────────────────────

    /// Create a frozen snapshot of all current parameter values.
    pub fn snapshot(&self) -> RegistrySnapshot {
        RegistrySnapshot {
            values: self.values.clone(),
        }
    }

    // ── Internal: constraint recomputation ────────────────────────────

    /// Recompute dynamic constraints whose dependencies include the given param.
    fn recompute_dependents(&mut self, changed: ParamId) {
        for i in 0..self.dynamic_constraints.len() {
            let depends_on_changed = self.dynamic_constraints[i]
                .dependencies
                .iter()
                .any(|dep| matches!(dep, ConstraintDependency::Param(p) if *p == changed));

            if depends_on_changed {
                let target = self.dynamic_constraints[i].target;
                let new_constraint =
                    (self.dynamic_constraints[i].compute)(&self.values, &self.hardware);
                self.apply_effective_constraint(target, new_constraint);
            }
        }
    }

    /// Recompute all constraints that depend on hardware context.
    fn recompute_all_hardware_constraints(&mut self) {
        for i in 0..self.dynamic_constraints.len() {
            let depends_on_hardware = self.dynamic_constraints[i]
                .dependencies
                .iter()
                .any(|dep| matches!(dep, ConstraintDependency::Hardware));

            if depends_on_hardware {
                let target = self.dynamic_constraints[i].target;
                let new_constraint =
                    (self.dynamic_constraints[i].compute)(&self.values, &self.hardware);
                self.apply_effective_constraint(target, new_constraint);
            }
        }
    }

    /// Store an effective constraint and clamp the current value if it violates the new bounds.
    fn apply_effective_constraint(&mut self, target: ParamId, constraint: EffectiveConstraint) {
        let def = &PARAM_DEFS[target as usize];
        let current = &self.values[target as usize];

        // Check if clamping is needed.
        if let Some(clamped) = constraint.clamp(current, &def.constraint) {
            self.values[target as usize] = clamped;
            self.pending_changes.push(target);
        }

        // Store the effective constraint (or clear if it's just Static).
        match constraint {
            EffectiveConstraint::Static => {
                self.effective_constraints[target as usize] = None;
            }
            _ => {
                self.effective_constraints[target as usize] = Some(constraint);
            }
        }
    }

    // ── Internal: event emission ─────────────────────────────────────

    /// Emit pending changes as a `params:changed` Tauri event, then clear the queue.
    fn emit_changes(&mut self) {
        if self.pending_changes.is_empty() {
            return;
        }

        let changes = std::mem::take(&mut self.pending_changes);

        if let Some(ref handle) = self.app_handle {
            // Build the event payload: list of changed param IDs with their new values.
            let payload: Vec<serde_json::Value> = changes
                .iter()
                .map(|id| {
                    let def = &PARAM_DEFS[*id as usize];
                    serde_json::json!({
                        "id": def.toml_path,
                        "value": param_value_to_json(&self.values[*id as usize]),
                    })
                })
                .collect();

            if let Err(e) = handle.emit("params:changed", payload) {
                eprintln!("[params] failed to emit params:changed event: {e}");
            }
        }
    }

    // ── Persistence ──────────────────────────────────────────────────

    /// Save all Rig-target parameters to rig.toml.
    pub fn save_rig(&self) -> Result<(), String> {
        let path = self.config_dir.join("rig.toml");
        super::toml_io::save_rig(self, &path)
    }

    /// Save all Experiment-target parameters to experiment.toml.
    pub fn save_experiment(&self) -> Result<(), String> {
        let path = self.config_dir.join("experiment.toml");
        super::toml_io::save_experiment(self, &path)
    }

    /// Load Rig-target parameters from rig.toml.
    pub fn load_rig(&mut self) -> Result<(), String> {
        let path = self.config_dir.join("rig.toml");
        super::toml_io::load_rig(self, &path)
    }

    /// Load Experiment-target parameters from experiment.toml.
    pub fn load_experiment(&mut self) -> Result<(), String> {
        let path = self.config_dir.join("experiment.toml");
        super::toml_io::load_experiment(self, &path)
    }

    /// Path to the experiments directory (mirrors ConfigManager::experiments_dir).
    pub fn experiments_dir(&self) -> PathBuf {
        let dir = self.experiments_directory();
        if !dir.is_empty() {
            PathBuf::from(dir)
        } else {
            self.config_dir.join("experiments")
        }
    }

    /// Config directory accessor.
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// Rig TOML path.
    pub fn rig_path(&self) -> PathBuf {
        self.config_dir.join("rig.toml")
    }

    /// Experiment TOML path.
    pub fn experiment_path(&self) -> PathBuf {
        self.config_dir.join("experiment.toml")
    }

    /// Get the ParamDef for a given ID.
    pub fn def(id: ParamId) -> &'static ParamDef {
        &PARAM_DEFS[id as usize]
    }
}

/// Convert a ParamValue to a serde_json::Value for event payloads.
pub(crate) fn param_value_to_json(value: &ParamValue) -> serde_json::Value {
    match value {
        ParamValue::Bool(v) => serde_json::json!(*v),
        ParamValue::U16(v) => serde_json::json!(*v),
        ParamValue::U32(v) => serde_json::json!(*v),
        ParamValue::I32(v) => serde_json::json!(*v),
        ParamValue::Usize(v) => serde_json::json!(*v),
        ParamValue::F64(v) => serde_json::json!(*v),
        ParamValue::String(v) => serde_json::json!(v),
        ParamValue::StringVec(v) => serde_json::json!(v),
        ParamValue::Envelope(v) => {
            let s = serde_json::to_string(v).unwrap_or_default();
            serde_json::Value::String(s.trim_matches('"').to_string())
        }
        ParamValue::Carrier(v) => {
            let s = serde_json::to_string(v).unwrap_or_default();
            serde_json::Value::String(s.trim_matches('"').to_string())
        }
        ParamValue::Projection(v) => {
            let s = serde_json::to_string(v).unwrap_or_default();
            serde_json::Value::String(s.trim_matches('"').to_string())
        }
        ParamValue::Structure(v) => {
            let s = serde_json::to_string(v).unwrap_or_default();
            serde_json::Value::String(s.trim_matches('"').to_string())
        }
        ParamValue::Order(v) => {
            let s = serde_json::to_string(v).unwrap_or_default();
            serde_json::Value::String(s.trim_matches('"').to_string())
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{PersistTarget, PARAM_DEFS};
    use std::path::Path;

    fn config_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../config")
    }

    #[test]
    fn defaults_load_without_panic() {
        let reg = Registry::new(&config_dir());
        assert_eq!(reg.values.len(), ParamId::count());
    }

    #[test]
    fn defaults_match_toml() {
        let dir = config_dir();
        let mut reg = Registry::new(&dir);
        reg.load_rig().expect("load rig.toml");
        reg.load_experiment().expect("load experiment.toml");

        // Spot-check values from rig.toml
        assert_eq!(reg.camera_exposure_us(), 1000);
        assert_eq!(reg.camera_binning(), 4);
        assert!((reg.viewing_distance_cm() - 10.0).abs() < 1e-10);
        assert!(!reg.ring_overlay_enabled());
        assert_eq!(reg.target_stimulus_fps(), 60);
        assert!((reg.monitor_rotation_deg() - 180.0).abs() < 1e-10);
        assert!((reg.smoothing_sigma() - 2.0).abs() < 1e-10);
        assert_eq!(reg.rotation_k(), 0);
        assert!((reg.epsilon() - 0.0000000001).abs() < 1e-20);

        // Segmentation
        assert!((reg.sign_map_filter_sigma() - 9.0).abs() < 1e-10);
        assert!((reg.sign_map_threshold() - 0.35).abs() < 1e-10);
        assert_eq!(reg.open_radius(), 2);

        // System tuning
        assert_eq!(reg.camera_frame_send_interval_ms(), 33);
        assert_eq!(reg.idle_sleep_ms(), 16);

        // Experiment
        assert!((reg.horizontal_offset_deg() - 0.0).abs() < 1e-10);
        assert!((reg.contrast() - 1.0).abs() < 1e-10);
        assert!((reg.mean_luminance() - 0.5).abs() < 1e-10);
        assert_eq!(reg.repetitions(), 1);
        assert!((reg.baseline_start_sec() - 5.0).abs() < 1e-10);
    }

    #[test]
    fn rig_toml_round_trip() {
        let dir = config_dir();
        let mut reg = Registry::new(&dir);
        reg.load_rig().expect("load rig.toml");

        // Save to temp file
        let tmp = std::env::temp_dir().join("openisi_test_rig_roundtrip.toml");
        super::super::toml_io::save_rig(&reg, &tmp).expect("save rig");

        // Load into new registry and compare
        let mut reg2 = Registry::new(&dir);
        super::super::toml_io::load_rig(&mut reg2, &tmp).expect("reload rig");

        for def in PARAM_DEFS.iter() {
            if def.persist == PersistTarget::Rig {
                assert_eq!(
                    reg.get(def.id),
                    reg2.get(def.id),
                    "mismatch for {:?} (toml_path: {})",
                    def.id,
                    def.toml_path
                );
            }
        }

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn experiment_toml_round_trip() {
        let dir = config_dir();
        let mut reg = Registry::new(&dir);
        reg.load_experiment().expect("load experiment.toml");

        let tmp = std::env::temp_dir().join("openisi_test_exp_roundtrip.toml");
        super::super::toml_io::save_experiment(&reg, &tmp).expect("save experiment");

        let mut reg2 = Registry::new(&dir);
        super::super::toml_io::load_experiment(&mut reg2, &tmp).expect("reload experiment");

        for def in PARAM_DEFS.iter() {
            if def.persist == PersistTarget::Experiment {
                assert_eq!(
                    reg.get(def.id),
                    reg2.get(def.id),
                    "mismatch for {:?} (toml_path: {})",
                    def.id,
                    def.toml_path
                );
            }
        }

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn static_constraint_rejects_invalid() {
        let dir = config_dir();
        let mut reg = Registry::new(&dir);
        // Exposure must be >= 1
        let result = reg.set(ParamId::CameraExposureUs, ParamValue::U32(0));
        assert!(result.is_err(), "should reject exposure_us = 0");
    }

    #[test]
    fn static_constraint_accepts_valid() {
        let dir = config_dir();
        let mut reg = Registry::new(&dir);
        let result = reg.set(ParamId::CameraExposureUs, ParamValue::U32(5000));
        assert!(result.is_ok(), "should accept exposure_us = 5000");
        assert_eq!(reg.camera_exposure_us(), 5000);
    }

    // ── Phase 2 tests ────────────────────────────────────────────────

    #[test]
    fn hardware_injection_changes_effective_constraint() {
        let dir = config_dir();
        let mut reg = Registry::new(&dir);

        // Before hardware: effective constraint is Static (from ParamDef)
        let c = reg.effective_constraint(ParamId::CameraExposureUs);
        assert!(matches!(c, EffectiveConstraint::Static));

        // Inject camera hardware
        reg.inject_hardware(HardwareContext {
            camera_min_exposure_us: Some(100),
            camera_max_exposure_us: Some(500_000),
            camera_max_binning: Some(8),
            ..Default::default()
        });

        // Now effective constraint is the dynamic range
        let c = reg.effective_constraint(ParamId::CameraExposureUs);
        assert!(matches!(c, EffectiveConstraint::RangeU32(100, 500_000)));

        // Binning constraint should also be updated
        let c = reg.effective_constraint(ParamId::CameraBinning);
        assert!(matches!(c, EffectiveConstraint::RangeU16(1, 8)));
    }

    #[test]
    fn hardware_injection_clamps_out_of_range_values() {
        let dir = config_dir();
        let mut reg = Registry::new(&dir);

        // Set exposure to 1000 (valid for static range 1..1_000_000)
        reg.set(ParamId::CameraExposureUs, ParamValue::U32(1000)).unwrap();

        // Inject hardware that has a narrower range
        reg.inject_hardware(HardwareContext {
            camera_min_exposure_us: Some(5000),
            camera_max_exposure_us: Some(100_000),
            ..Default::default()
        });

        // Value should have been clamped up to 5000
        assert_eq!(reg.camera_exposure_us(), 5000);
    }

    #[test]
    fn dynamic_constraint_rejects_out_of_range() {
        let dir = config_dir();
        let mut reg = Registry::new(&dir);

        reg.inject_hardware(HardwareContext {
            camera_min_exposure_us: Some(100),
            camera_max_exposure_us: Some(50_000),
            ..Default::default()
        });

        // Setting outside the dynamic range should fail
        let result = reg.set(ParamId::CameraExposureUs, ParamValue::U32(100_000));
        assert!(result.is_err());

        // Setting within dynamic range should succeed
        let result = reg.set(ParamId::CameraExposureUs, ParamValue::U32(25_000));
        assert!(result.is_ok());
    }

    #[test]
    fn batch_mode_groups_changes() {
        let dir = config_dir();
        let mut reg = Registry::new(&dir);

        // In batch mode, pending_changes accumulate.
        reg.batch(|r| {
            r.set(ParamId::CameraExposureUs, ParamValue::U32(2000)).unwrap();
            r.set(ParamId::CameraBinning, ParamValue::U16(2)).unwrap();
            // Inside batch, changes should still be pending.
            // (We can't easily observe this without an app_handle, but we can
            // verify the values are set correctly.)
        });

        assert_eq!(reg.camera_exposure_us(), 2000);
        assert_eq!(reg.camera_binning(), 2);
        // After batch, pending_changes should be cleared (emitted).
        assert!(reg.pending_changes.is_empty());
    }

    #[test]
    fn snapshot_captures_current_state() {
        let dir = config_dir();
        let mut reg = Registry::new(&dir);
        reg.load_rig().expect("load rig.toml");
        reg.load_experiment().expect("load experiment.toml");

        let snap = reg.snapshot();

        // Verify snapshot matches registry
        assert_eq!(snap.camera_exposure_us(), reg.camera_exposure_us());
        assert_eq!(snap.camera_binning(), reg.camera_binning());
        assert!((snap.viewing_distance_cm() - reg.viewing_distance_cm()).abs() < 1e-10);
        assert!((snap.contrast() - reg.contrast()).abs() < 1e-10);
        assert_eq!(snap.conditions(), reg.conditions());

        // Modify registry — snapshot should not change
        reg.set(ParamId::CameraExposureUs, ParamValue::U32(9999)).unwrap();
        assert_eq!(snap.camera_exposure_us(), 1000); // original value
        assert_eq!(reg.camera_exposure_us(), 9999); // new value
    }

    #[test]
    fn computed_luminance_values() {
        let dir = config_dir();
        let mut reg = Registry::new(&dir);

        // Default: mean=0.5, contrast=1.0
        // high = 0.5 + 1.0 * 0.5 = 1.0
        // low = 0.5 - 1.0 * 0.5 = 0.0
        assert!((reg.luminance_high() - 1.0).abs() < 1e-10);
        assert!((reg.luminance_low() - 0.0).abs() < 1e-10);

        // Set mean=0.3, contrast=0.5
        reg.set(ParamId::MeanLuminance, ParamValue::F64(0.3)).unwrap();
        reg.set(ParamId::Contrast, ParamValue::F64(0.5)).unwrap();
        // high = 0.3 + 0.5 * 0.3 = 0.45
        // low = 0.3 - 0.5 * 0.3 = 0.15
        assert!((reg.luminance_high() - 0.45).abs() < 1e-10);
        assert!((reg.luminance_low() - 0.15).abs() < 1e-10);
    }

    #[test]
    fn computed_visual_field_requires_hardware() {
        let dir = config_dir();
        let reg = Registry::new(&dir);

        // Without hardware, visual field is None
        assert!(reg.visual_field_width_deg().is_none());
        assert!(reg.visual_field_height_deg().is_none());
    }

    #[test]
    fn computed_visual_field_with_hardware() {
        let dir = config_dir();
        let mut reg = Registry::new(&dir);
        reg.set(ParamId::ViewingDistanceCm, ParamValue::F64(25.0)).unwrap();

        reg.inject_hardware(HardwareContext {
            monitor_width_px: Some(1920),
            monitor_height_px: Some(1080),
            monitor_width_cm: Some(53.0),
            monitor_height_cm: Some(30.0),
            ..Default::default()
        });

        let vf_w = reg.visual_field_width_deg();
        assert!(vf_w.is_some());
        let vf_w = vf_w.unwrap();
        // 2 * atan(26.5 / 25.0) ~ 93.3 degrees
        assert!((vf_w - 93.3).abs() < 1.0, "visual field width: {vf_w}");
    }

    #[test]
    fn monitor_refresh_constrains_target_fps() {
        let dir = config_dir();
        let mut reg = Registry::new(&dir);

        // Set FPS to 120 (valid with static constraint MinU32(1))
        reg.set(ParamId::TargetStimulusFps, ParamValue::U32(120)).unwrap();

        // Inject monitor with 60 Hz refresh
        reg.inject_hardware(HardwareContext {
            monitor_refresh_hz: Some(60),
            ..Default::default()
        });

        // FPS should have been clamped to 60
        assert_eq!(reg.target_stimulus_fps(), 60);

        // Setting above 60 should fail
        let result = reg.set(ParamId::TargetStimulusFps, ParamValue::U32(90));
        assert!(result.is_err());
    }

    #[test]
    fn stimulus_width_constraint_from_geometry() {
        let dir = config_dir();
        let mut reg = Registry::new(&dir);
        reg.set(ParamId::ViewingDistanceCm, ParamValue::F64(25.0)).unwrap();

        reg.inject_hardware(HardwareContext {
            monitor_width_px: Some(1920),
            monitor_height_px: Some(1080),
            monitor_width_cm: Some(53.0),
            monitor_height_cm: Some(30.0),
            ..Default::default()
        });

        // Constraint should now be RangeF64(0.001, ~93.3)
        let c = reg.effective_constraint(ParamId::StimulusWidthDeg);
        match c {
            EffectiveConstraint::RangeF64(min, max) => {
                assert!((min - 0.001).abs() < 1e-6);
                assert!((max - 93.3).abs() < 1.0, "max stimulus width: {max}");
            }
            _ => panic!("expected RangeF64, got {c:?}"),
        }
    }
}

#[cfg(test)]
mod descriptor_count_tests {
    use super::*;
    use crate::params::GroupId;

    #[test]
    fn descriptor_counts_per_group() {
        let groups = [
            ("Stimulus", GroupId::Stimulus),
            ("Geometry", GroupId::Geometry),
            ("Timing", GroupId::Timing),
            ("Presentation", GroupId::Presentation),
            ("Retinotopy", GroupId::Retinotopy),
            ("Segmentation", GroupId::Segmentation),
            ("Camera", GroupId::Camera),
            ("Display", GroupId::Display),
            ("Ring", GroupId::Ring),
            ("System", GroupId::System),
            ("Paths", GroupId::Paths),
        ];
        let mut total = 0;
        for (name, group) in &groups {
            let count = PARAM_DEFS.iter().filter(|d| d.group == *group).count();
            eprintln!("  {name}: {count} params");
            total += count;
        }
        eprintln!("  TOTAL: {total}");
        assert!(total > 60, "Expected at least 60 total params, got {total}");
        
        // Verify specific groups
        let stimulus_count = PARAM_DEFS.iter().filter(|d| d.group == GroupId::Stimulus).count();
        assert!(stimulus_count >= 13, "Stimulus should have >= 13 params (envelope + carrier + 11 params), got {stimulus_count}");
        
        let retinotopy_count = PARAM_DEFS.iter().filter(|d| d.group == GroupId::Retinotopy).count();
        assert!(retinotopy_count >= 6, "Retinotopy should have >= 6 params, got {retinotopy_count}");
        
        let segmentation_count = PARAM_DEFS.iter().filter(|d| d.group == GroupId::Segmentation).count();
        assert!(segmentation_count >= 10, "Segmentation should have >= 10 params, got {segmentation_count}");
    }
}
