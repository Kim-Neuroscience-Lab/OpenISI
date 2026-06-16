//! Typed configuration — the serde + schemars + garde parameter system.
//!
//! Each persist target is a typed struct tree: `RigConfig`, `ExperimentConfig`,
//! `AnalysisConfig`. serde handles (de)serialization to JSON, schemars derives the
//! UI/validation schema, garde validates. The analysis tagged enums — where a
//! method's tunables exist only inside its selected variant, so variant activation
//! is a type-level guarantee — are the canonical method+tunable types, shared
//! directly with `isi-analysis` (no bridge).

pub mod analysis;
pub mod experiment;
pub mod loader;
pub mod rig;
pub mod store;
pub mod ui_state;

pub use analysis::AnalysisConfig;
pub use experiment::ExperimentConfig;
pub use loader::{load_merged, load_target_from_dir, to_json};
pub use rig::RigConfig;
pub use ui_state::UiStateConfig;
pub use store::{ConfigSnapshot, ConfigStore};
