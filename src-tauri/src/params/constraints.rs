//! Dynamic constraints — recomputed when hardware context or dependent params change.
//!
//! Each `DynamicConstraint` targets one parameter and specifies which param IDs
//! and/or hardware fields trigger recomputation. The constraint engine caches
//! the computed `EffectiveConstraint` and clamps values that violate new bounds.

use super::hardware::HardwareContext;
use super::{ParamId, ParamValue, StaticConstraint};

/// An effective constraint computed from hardware context and/or other params.
#[derive(Debug, Clone)]
pub enum EffectiveConstraint {
    /// No override — use the static constraint from ParamDef.
    Static,
    /// Override with a dynamic range.
    RangeU16(u16, u16),
    RangeU32(u32, u32),
    RangeF64(f64, f64),
    MinF64(f64),
}

impl EffectiveConstraint {
    /// Validate a ParamValue against this effective constraint.
    pub fn validate(&self, value: &ParamValue, static_constraint: &StaticConstraint) -> Result<(), String> {
        match self {
            EffectiveConstraint::Static => static_constraint.validate(value),
            EffectiveConstraint::RangeU16(min, max) => {
                if let ParamValue::U16(v) = value {
                    if *v >= *min && *v <= *max {
                        Ok(())
                    } else {
                        Err(format!("value {v} out of range [{min}, {max}]"))
                    }
                } else {
                    Ok(())
                }
            }
            EffectiveConstraint::RangeU32(min, max) => {
                if let ParamValue::U32(v) = value {
                    if *v >= *min && *v <= *max {
                        Ok(())
                    } else {
                        Err(format!("value {v} out of range [{min}, {max}]"))
                    }
                } else {
                    Ok(())
                }
            }
            EffectiveConstraint::RangeF64(min, max) => {
                if let ParamValue::F64(v) = value {
                    if *v >= *min && *v <= *max {
                        Ok(())
                    } else {
                        Err(format!("value {v} out of range [{min}, {max}]"))
                    }
                } else {
                    Ok(())
                }
            }
            EffectiveConstraint::MinF64(min) => {
                if let ParamValue::F64(v) = value {
                    if *v >= *min {
                        Ok(())
                    } else {
                        Err(format!("value {v} below minimum {min}"))
                    }
                } else {
                    Ok(())
                }
            }
        }
    }

    /// Clamp a ParamValue to fit within this constraint. Returns the clamped value
    /// if it changed, or None if it was already in bounds.
    pub fn clamp(&self, value: &ParamValue, static_constraint: &StaticConstraint) -> Option<ParamValue> {
        if self.validate(value, static_constraint).is_ok() {
            return None;
        }
        match self {
            EffectiveConstraint::Static => {
                // Static constraint clamping — handled per type
                clamp_static(value, static_constraint)
            }
            EffectiveConstraint::RangeU16(min, max) => {
                if let ParamValue::U16(v) = value {
                    Some(ParamValue::U16((*v).clamp(*min, *max)))
                } else {
                    None
                }
            }
            EffectiveConstraint::RangeU32(min, max) => {
                if let ParamValue::U32(v) = value {
                    Some(ParamValue::U32((*v).clamp(*min, *max)))
                } else {
                    None
                }
            }
            EffectiveConstraint::RangeF64(min, max) => {
                if let ParamValue::F64(v) = value {
                    Some(ParamValue::F64(v.clamp(*min, *max)))
                } else {
                    None
                }
            }
            EffectiveConstraint::MinF64(min) => {
                if let ParamValue::F64(v) = value {
                    if *v < *min {
                        Some(ParamValue::F64(*min))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        }
    }
}

/// Clamp a value against a static constraint.
fn clamp_static(value: &ParamValue, constraint: &StaticConstraint) -> Option<ParamValue> {
    match (constraint, value) {
        (StaticConstraint::RangeU16(min, max), ParamValue::U16(v)) => {
            Some(ParamValue::U16((*v).clamp(*min, *max)))
        }
        (StaticConstraint::RangeU32(min, max), ParamValue::U32(v)) => {
            Some(ParamValue::U32((*v).clamp(*min, *max)))
        }
        (StaticConstraint::RangeI32(min, max), ParamValue::I32(v)) => {
            Some(ParamValue::I32((*v).clamp(*min, *max)))
        }
        (StaticConstraint::RangeUsize(min, max), ParamValue::Usize(v)) => {
            Some(ParamValue::Usize((*v).clamp(*min, *max)))
        }
        (StaticConstraint::RangeF64(min, max), ParamValue::F64(v)) => {
            Some(ParamValue::F64(v.clamp(*min, *max)))
        }
        (StaticConstraint::MinF64(min), ParamValue::F64(v)) => {
            if *v < *min { Some(ParamValue::F64(*min)) } else { None }
        }
        (StaticConstraint::MinU32(min), ParamValue::U32(v)) => {
            if *v < *min { Some(ParamValue::U32(*min)) } else { None }
        }
        (StaticConstraint::MinUsize(min), ParamValue::Usize(v)) => {
            if *v < *min { Some(ParamValue::Usize(*min)) } else { None }
        }
        _ => None,
    }
}

/// Which inputs a dynamic constraint depends on.
#[derive(Debug, Clone)]
pub enum ConstraintDependency {
    /// Depends on a parameter value.
    Param(ParamId),
    /// Depends on hardware context (recomputed on inject_hardware).
    Hardware,
}

/// A dynamic constraint edge: when dependencies change, recompute the
/// effective constraint for the target parameter.
pub struct DynamicConstraint {
    /// The parameter whose constraint is being overridden.
    pub target: ParamId,
    /// What triggers recomputation.
    pub dependencies: Vec<ConstraintDependency>,
    /// Compute the effective constraint given current registry values and hardware.
    pub compute: fn(&[ParamValue], &HardwareContext) -> EffectiveConstraint,
}

/// Build the 5 dynamic constraint edges.
///
/// Edge numbering from the plan:
/// 1. exposure_us from camera
/// 2. binning from camera
/// 3. target_stimulus_fps from monitor
/// 4. strobe_frequency_hz from monitor (measured)
/// 5. stimulus_width_deg from geometry (viewing_distance + monitor + projection)
///
/// Edge 6 (contrast from mean_luminance) is unnecessary: luminance_low = mean*(1-contrast),
/// and since mean >= 0 and contrast in [0,1], luminance_low is always >= 0.
///
/// Edge 7 (active_when) is handled separately via ParamDef active_when function pointers.
pub fn build_dynamic_constraints() -> Vec<DynamicConstraint> {
    vec![
        // 1. Camera exposure range
        DynamicConstraint {
            target: ParamId::CameraExposureUs,
            dependencies: vec![ConstraintDependency::Hardware],
            compute: |_values, hw| {
                match (hw.camera_min_exposure_us, hw.camera_max_exposure_us) {
                    (Some(min), Some(max)) => EffectiveConstraint::RangeU32(min, max),
                    _ => EffectiveConstraint::Static,
                }
            },
        },
        // 2. Camera binning max
        DynamicConstraint {
            target: ParamId::CameraBinning,
            dependencies: vec![ConstraintDependency::Hardware],
            compute: |_values, hw| {
                match hw.camera_max_binning {
                    Some(max) => EffectiveConstraint::RangeU16(1, max),
                    None => EffectiveConstraint::Static,
                }
            },
        },
        // 3. Target stimulus FPS from monitor refresh
        DynamicConstraint {
            target: ParamId::TargetStimulusFps,
            dependencies: vec![ConstraintDependency::Hardware],
            compute: |_values, hw| {
                match hw.monitor_refresh_hz {
                    Some(hz) => EffectiveConstraint::RangeU32(1, hz),
                    None => EffectiveConstraint::Static,
                }
            },
        },
        // 4. Strobe frequency max from measured refresh / 2
        DynamicConstraint {
            target: ParamId::StrobeFrequencyHz,
            dependencies: vec![ConstraintDependency::Hardware],
            compute: |_values, hw| {
                match hw.measured_refresh_hz {
                    Some(hz) => EffectiveConstraint::RangeF64(0.0, hz / 2.0),
                    None => EffectiveConstraint::Static,
                }
            },
        },
        // 5. Stimulus width max from visual field width
        //    Depends on viewing_distance (param) + monitor dims (hardware) + projection (param)
        DynamicConstraint {
            target: ParamId::StimulusWidthDeg,
            dependencies: vec![
                ConstraintDependency::Param(ParamId::ViewingDistanceCm),
                ConstraintDependency::Param(ParamId::ExperimentProjection),
                ConstraintDependency::Hardware,
            ],
            compute: |values, hw| {
                let (width_cm, height_cm) = match (hw.monitor_width_cm, hw.monitor_height_cm) {
                    (Some(w), Some(h)) if w > 0.0 && h > 0.0 => (w, h),
                    _ => return EffectiveConstraint::Static,
                };

                let viewing_distance = match &values[ParamId::ViewingDistanceCm as usize] {
                    super::ParamValue::F64(v) => *v,
                    _ => return EffectiveConstraint::Static,
                };

                if viewing_distance <= 0.0 {
                    return EffectiveConstraint::Static;
                }

                let projection = match &values[ParamId::ExperimentProjection as usize] {
                    super::ParamValue::Projection(p) => *p,
                    _ => return EffectiveConstraint::Static,
                };

                let geom = openisi_stimulus::geometry::DisplayGeometry::new(
                    projection,
                    viewing_distance,
                    0.0, 0.0,
                    width_cm, height_cm,
                    hw.monitor_width_px.unwrap_or(1920),
                    hw.monitor_height_px.unwrap_or(1080),
                );

                let vf_width = geom.visual_field_width_deg();
                EffectiveConstraint::RangeF64(0.001, vf_width)
            },
        },
    ]
}
