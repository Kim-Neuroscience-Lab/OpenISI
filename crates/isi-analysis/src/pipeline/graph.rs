//! The static pipeline DAG. Stages declare their dependencies via
//! [`Stage::deps`]; this builds a `petgraph` `DiGraph` from those edges,
//! verifies it's acyclic, and yields the topological execution order.
//!
//! For Part 2 the topological order *is* the execution plan (every stage runs,
//! in order — reproducing the procedural `compute_analysis`). Part 3 reuses the
//! same graph for dirty-propagation: mark the changed stages, then every stage
//! reachable downstream, and recompute only those from the restore frontier.

use std::collections::HashMap;

use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};

use crate::AnalysisError;

use super::stage::{Stage, StageId};

/// The pipeline dependency graph over `StageId`.
pub struct StageGraph {
    graph: DiGraph<StageId, ()>,
}

impl StageGraph {
    /// Build the DAG from the stages' declared `deps()`. An edge `dep → stage`
    /// means `stage` consumes `dep`'s output.
    pub fn build(stages: &[Box<dyn Stage>]) -> Self {
        let mut graph = DiGraph::<StageId, ()>::new();
        let mut index: HashMap<StageId, NodeIndex> = HashMap::new();
        for stage in stages {
            let id = stage.id();
            let node = graph.add_node(id);
            index.insert(id, node);
        }
        for stage in stages {
            let to = index[&stage.id()];
            for dep in stage.deps() {
                let from = index[dep];
                graph.add_edge(from, to, ());
            }
        }
        Self { graph }
    }

    /// Topological execution order. Errors if the declared deps form a cycle
    /// (a programming error — the analysis pipeline is acyclic by construction).
    pub fn topo_order(&self) -> Result<Vec<StageId>, AnalysisError> {
        match toposort(&self.graph, None) {
            Ok(order) => Ok(order.into_iter().map(|n| self.graph[n]).collect()),
            Err(cycle) => Err(AnalysisError::Compute(format!(
                "pipeline DAG has a cycle at stage {:?}",
                self.graph[cycle.node_id()]
            ))),
        }
    }
}
