//! Task graph edge definitions
//!
//! Edges represent transitions between actions with
//! frequency and duration statistics.

use serde::{Deserialize, Serialize};

use super::node::NodeId;

/// An edge in the task graph
///
/// Represents a transition from one action to another.
/// Statistics help identify common patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEdge {
    /// Source node
    pub from: NodeId,
    /// Target node
    pub to: NodeId,
    /// Number of times this transition was observed
    pub frequency: u64,
    /// Total duration of all transitions (milliseconds)
    pub total_duration_ms: u64,
    /// First observation timestamp
    pub first_seen_ms: i64,
    /// Most recent observation timestamp
    pub last_seen_ms: i64,
}

impl TaskEdge {
    /// Create a new edge between two nodes
    pub fn new(from: NodeId, to: NodeId, timestamp_ms: i64) -> Self {
        Self {
            from,
            to,
            frequency: 1,
            total_duration_ms: 0,
            first_seen_ms: timestamp_ms,
            last_seen_ms: timestamp_ms,
        }
    }

    /// Create an edge with an initial duration
    pub fn with_duration(from: NodeId, to: NodeId, timestamp_ms: i64, duration_ms: u64) -> Self {
        Self {
            from,
            to,
            frequency: 1,
            total_duration_ms: duration_ms,
            first_seen_ms: timestamp_ms,
            last_seen_ms: timestamp_ms,
        }
    }

    /// Record another occurrence of this transition
    pub fn record_transition(&mut self, timestamp_ms: i64, duration_ms: Option<u64>) {
        self.frequency += 1;
        self.last_seen_ms = timestamp_ms;
        if let Some(d) = duration_ms {
            self.total_duration_ms += d;
        }
    }

    /// Get the average transition time
    pub fn avg_transition_time_ms(&self) -> f64 {
        if self.frequency == 0 {
            0.0
        } else {
            self.total_duration_ms as f64 / self.frequency as f64
        }
    }

    /// Calculate a weight for this edge (higher = more important)
    ///
    /// Weight is based on frequency and recency.
    pub fn weight(&self, current_time_ms: i64) -> f64 {
        // Frequency component (log scale to prevent dominance)
        let freq_weight = (self.frequency as f64).ln_1p();

        // Recency component (decay over time)
        let age_ms = (current_time_ms - self.last_seen_ms).max(0) as f64;
        let age_hours = age_ms / (1000.0 * 60.0 * 60.0);
        let recency_weight = (-age_hours / 24.0).exp(); // Half-life of ~24 hours

        freq_weight * recency_weight
    }

    /// Check if this edge represents a common transition
    pub fn is_frequent(&self, threshold: u64) -> bool {
        self.frequency >= threshold
    }
}

/// A weighted path through the graph
#[derive(Debug, Clone)]
pub struct GraphPath {
    /// Sequence of node IDs
    pub nodes: Vec<NodeId>,
    /// Total frequency (minimum edge frequency)
    pub min_frequency: u64,
    /// Total duration estimate
    pub total_duration_ms: u64,
}

impl GraphPath {
    /// Create a new path starting from a single node
    pub fn new(start: NodeId) -> Self {
        Self {
            nodes: vec![start],
            min_frequency: u64::MAX,
            total_duration_ms: 0,
        }
    }

    /// Extend the path with a new node via an edge
    pub fn extend(&mut self, node: NodeId, edge: &TaskEdge) {
        self.nodes.push(node);
        self.min_frequency = self.min_frequency.min(edge.frequency);
        self.total_duration_ms += edge.total_duration_ms / edge.frequency.max(1);
    }

    /// Get the length of the path (number of nodes)
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Check if the path is empty
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Get the path as a sequence of transitions
    pub fn transitions(&self) -> impl Iterator<Item = (&NodeId, &NodeId)> {
        self.nodes.windows(2).map(|w| (&w[0], &w[1]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edge_creation() {
        let edge = TaskEdge::new(
            NodeId("copy:chrome.exe".to_string()),
            NodeId("paste:code.exe".to_string()),
            1000,
        );

        assert_eq!(edge.frequency, 1);
        assert_eq!(edge.total_duration_ms, 0);
    }

    #[test]
    fn test_edge_record_transition() {
        let mut edge = TaskEdge::new(
            NodeId("copy:chrome.exe".to_string()),
            NodeId("paste:code.exe".to_string()),
            1000,
        );

        edge.record_transition(2000, Some(100));
        edge.record_transition(3000, Some(150));

        assert_eq!(edge.frequency, 3);
        assert_eq!(edge.total_duration_ms, 250);
        assert_eq!(edge.avg_transition_time_ms(), 250.0 / 3.0);
    }

    #[test]
    fn test_edge_weight() {
        let edge = TaskEdge {
            from: NodeId("a".to_string()),
            to: NodeId("b".to_string()),
            frequency: 10,
            total_duration_ms: 1000,
            first_seen_ms: 0,
            last_seen_ms: 1000,
        };

        // Recent edge should have higher weight
        let weight_now = edge.weight(1000);
        let weight_later = edge.weight(1000 + 24 * 60 * 60 * 1000); // 24 hours later

        assert!(weight_now > weight_later);
    }

    #[test]
    fn test_graph_path() {
        let mut path = GraphPath::new(NodeId("start".to_string()));

        let edge1 = TaskEdge {
            from: NodeId("start".to_string()),
            to: NodeId("middle".to_string()),
            frequency: 5,
            total_duration_ms: 500,
            first_seen_ms: 0,
            last_seen_ms: 1000,
        };

        let edge2 = TaskEdge {
            from: NodeId("middle".to_string()),
            to: NodeId("end".to_string()),
            frequency: 3,
            total_duration_ms: 300,
            first_seen_ms: 0,
            last_seen_ms: 1000,
        };

        path.extend(NodeId("middle".to_string()), &edge1);
        path.extend(NodeId("end".to_string()), &edge2);

        assert_eq!(path.len(), 3);
        assert_eq!(path.min_frequency, 3); // Minimum of 5 and 3
    }
}
