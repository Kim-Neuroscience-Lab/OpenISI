//! Reactive parameter registry — single source of truth for all configuration.
//!
//! Every parameter is defined once in `definitions.rs` via the `define_params!` macro.
//! The registry owns all values, validates against static constraints, and serializes
//! to/from TOML files that are byte-compatible with the existing config format.

use serde::{Deserialize, Serialize};

// Re-export stimulus crate enums so the rest of the app can use `params::Envelope` etc.
pub use openisi_stimulus::dataset::EnvelopeType as Envelope;
pub use openisi_stimulus::geometry::ProjectionType as Projection;
pub use openisi_stimulus::sequencer::Order;

// Carrier and Structure are defined locally (not in the stimulus crate).
pub use self::carrier_types::Carrier;
pub use self::carrier_types::Structure;

/// Carrier and Structure enums — previously in config.rs, now canonical here.
mod carrier_types {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum Carrier {
        Solid,
        Checkerboard,
    }

    impl Carrier {
        pub fn to_shader_int(self) -> i32 {
            match self {
                Carrier::Solid => 0,
                Carrier::Checkerboard => 1,
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum Structure {
        Blocked,
        Interleaved,
    }
}

// ─── Submodules ───────────────────────────────────────────────────────────────

#[macro_use]
mod macros;
mod definitions;
pub mod registry;
pub mod toml_io;

// Phase 2 stubs
pub mod constraints;
pub mod computed;
pub mod hardware;
pub mod snapshot;
pub mod commands;

pub use definitions::ParamId;
// PARAM_DEFS is a LazyLock<Vec<ParamDef>>, re-exported for convenience.
pub use definitions::PARAM_DEFS;
pub use registry::Registry;

// ─── Core types ───────────────────────────────────────────────────────────────

/// Type-erased parameter value.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    Bool(bool),
    U16(u16),
    U32(u32),
    I32(i32),
    Usize(usize),
    F64(f64),
    String(String),
    StringVec(Vec<String>),
    Envelope(Envelope),
    Carrier(Carrier),
    Projection(Projection),
    Structure(Structure),
    Order(Order),
}

/// Which TOML file a parameter persists to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistTarget {
    Rig,
    Experiment,
}

/// Logical grouping for UI and descriptor queries.
/// Each variant maps 1:1 to a card/section in the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupId {
    Stimulus,
    Geometry,
    Timing,
    Presentation,
    Retinotopy,
    Segmentation,
    Camera,
    Display,
    Ring,
    System,
    Paths,
}

/// Static constraint for validation (Phase 1 — no dynamic constraints yet).
#[derive(Debug, Clone)]
pub enum StaticConstraint {
    None,
    RangeU16(u16, u16),
    RangeU32(u32, u32),
    RangeI32(i32, i32),
    RangeUsize(usize, usize),
    RangeF64(f64, f64),
    MinF64(f64),
    MinU32(u32),
    MinUsize(usize),
}

impl StaticConstraint {
    /// Validate a ParamValue against this constraint. Returns Ok or an error message.
    pub fn validate(&self, value: &ParamValue) -> Result<(), String> {
        match (self, value) {
            (StaticConstraint::None, _) => Ok(()),

            (StaticConstraint::RangeU16(min, max), ParamValue::U16(v)) => {
                if *v >= *min && *v <= *max {
                    Ok(())
                } else {
                    Err(format!("value {v} out of range [{min}, {max}]"))
                }
            }
            (StaticConstraint::RangeU32(min, max), ParamValue::U32(v)) => {
                if *v >= *min && *v <= *max {
                    Ok(())
                } else {
                    Err(format!("value {v} out of range [{min}, {max}]"))
                }
            }
            (StaticConstraint::RangeI32(min, max), ParamValue::I32(v)) => {
                if *v >= *min && *v <= *max {
                    Ok(())
                } else {
                    Err(format!("value {v} out of range [{min}, {max}]"))
                }
            }
            (StaticConstraint::RangeUsize(min, max), ParamValue::Usize(v)) => {
                if *v >= *min && *v <= *max {
                    Ok(())
                } else {
                    Err(format!("value {v} out of range [{min}, {max}]"))
                }
            }
            (StaticConstraint::RangeF64(min, max), ParamValue::F64(v)) => {
                if *v >= *min && *v <= *max {
                    Ok(())
                } else {
                    Err(format!("value {v} out of range [{min}, {max}]"))
                }
            }
            (StaticConstraint::MinF64(min), ParamValue::F64(v)) => {
                if *v >= *min {
                    Ok(())
                } else {
                    Err(format!("value {v} below minimum {min}"))
                }
            }
            (StaticConstraint::MinU32(min), ParamValue::U32(v)) => {
                if *v >= *min {
                    Ok(())
                } else {
                    Err(format!("value {v} below minimum {min}"))
                }
            }
            (StaticConstraint::MinUsize(min), ParamValue::Usize(v)) => {
                if *v >= *min {
                    Ok(())
                } else {
                    Err(format!("value {v} below minimum {min}"))
                }
            }

            _ => Ok(()), // type mismatch = no constraint (enum values, strings, etc.)
        }
    }
}

/// Static definition of a parameter (one per parameter, lives in PARAM_DEFS).
pub struct ParamDef {
    pub id: ParamId,
    pub label: &'static str,
    pub unit: &'static str,
    pub group: GroupId,
    pub toml_path: &'static str,
    pub persist: PersistTarget,
    pub default: ParamValue,
    pub constraint: StaticConstraint,
    /// If Some, this parameter is only active when the function returns true.
    /// Inactive parameters are hidden in the UI. None = always active.
    pub active_when: Option<fn(&Registry) -> bool>,
}

// Re-export Phase 2 types for convenience.
pub use hardware::HardwareContext;
pub use snapshot::RegistrySnapshot;

/// Metadata for a saved experiment file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExperimentMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
}
