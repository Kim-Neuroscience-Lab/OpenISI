//! Typed configuration (Phase 3) — the serde + schemars + garde replacement for
//! the `define_params!` macro registry, built strangler-fig alongside it.
//!
//! Each `PersistTarget` becomes a typed struct tree: `RigConfig` (here),
//! `ExperimentConfig`, `AnalysisConfig`. serde handles (de)serialization to JSON,
//! schemars derives the UI/validation schema, garde validates. The analysis
//! tagged enums (where `active_when` collapses into the variant structure) are
//! the canonical method+tunable types, shared with `isi-analysis` (no bridge).
//!
//! Migrated domain-by-domain; not yet wired into the live registry/IPC.

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
