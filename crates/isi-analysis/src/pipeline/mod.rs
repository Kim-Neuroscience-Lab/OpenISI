//! The analysis pipeline as a directed acyclic graph of stages.
//!
//! The procedural `compute_analysis` is expressed here as a DAG: each stage
//! (`Stage`) wraps a canonical `methods/*.rs` `apply()`, reads/writes the
//! `PipelineState` blackboard, and declares its dependencies. The orchestrator
//! builds the graph (`StageGraph`), walks it in topological order, and
//! assembles the result.
//!
//! This is the substrate for incremental re-analysis (Part 3): the same graph
//! drives dirty-propagation, and `cacheable` stages persist/restore their
//! output to the `.oisi` file so a parameter tweak recomputes only the
//! affected downstream stages. In Part 2 it reproduces the procedural pipeline
//! exactly — gated by the cross-implementation equivalence test.

pub mod fingerprint;
mod graph;
mod orchestrator;
mod stage;
mod stages;
mod state;

pub use orchestrator::{run, RunOutput};
pub use stage::{CacheClass, RunEnv, StageId};

// The blackboard type is the seed interface: the I/O boundary (`analyze`) loads
// each restorable stage output into one of these and hands it to `run`. Crate-
// internal — outside callers go through `analyze`/`compute_analysis`.
pub(crate) use state::PipelineState;

/// The pipeline DAG edges, as `(stage, its dependencies)` — the single source of
/// truth (each stage's `deps()`) surfaced for the incremental cut, so the cut's
/// dependency reasoning can never drift from what the orchestrator actually runs.
pub(crate) fn stage_dependencies() -> Vec<(StageId, &'static [StageId])> {
    stages::all_stages()
        .iter()
        .map(|s| (s.id(), s.deps()))
        .collect()
}
