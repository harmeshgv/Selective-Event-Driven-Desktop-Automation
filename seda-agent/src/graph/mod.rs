//! Personal Task Graph
//!
//! Builds and maintains a graph where:
//! - Nodes represent symbolic action types
//! - Edges represent transitions between actions
//! - Edge weights encode frequency and duration
//!
//! # Privacy Preservation
//!
//! The graph is built exclusively from SymbolicAction data.
//! No raw user data (text, window content) is ever part of the graph.
//! This allows pattern discovery while maintaining privacy.

pub mod builder;
pub mod edge;
pub mod node;

pub use builder::TaskGraphBuilder;
pub use edge::TaskEdge;
pub use node::{NodeId, TaskNode};

use std::collections::HashMap;

/// The Personal Task Graph
#[derive(Debug, Clone)]
pub struct TaskGraph {
    /// Nodes indexed by their ID
    pub nodes: HashMap<NodeId, TaskNode>,
    /// Edges as adjacency list (from_node -> list of edges)
    pub edges: HashMap<NodeId, Vec<TaskEdge>>,
}

impl TaskGraph {
    /// Create a new empty task graph
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
        }
    }

    /// Get all edges with frequency above threshold
    pub fn frequent_transitions(&self, min_freq: u64) -> Vec<&TaskEdge> {
        self.edges
            .values()
            .flatten()
            .filter(|e| e.frequency >= min_freq)
            .collect()
    }

    /// Get total number of nodes
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get total number of edges
    pub fn edge_count(&self) -> usize {
        self.edges.values().map(|v| v.len()).sum()
    }
}

impl Default for TaskGraph {
    fn default() -> Self {
        Self::new()
    }
}
