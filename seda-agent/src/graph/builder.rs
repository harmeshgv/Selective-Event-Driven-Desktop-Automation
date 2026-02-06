//! Task graph builder
//!
//! Constructs and maintains the Personal Task Graph from symbolic actions.
//!
//! # Privacy Preservation
//!
//! The graph is built exclusively from SymbolicAction data.
//! No raw user data (text, window content) is ever part of the graph.

use chrono::{DateTime, Utc};

use super::edge::{GraphPath, TaskEdge};
use super::node::{NodeId, TaskNode};
use super::TaskGraph;
use crate::symbolizer::SymbolicAction;

/// Builder for constructing and updating the task graph
pub struct TaskGraphBuilder {
    /// The graph being built
    graph: TaskGraph,
    /// Last observed action (for tracking transitions)
    last_action: Option<(NodeId, DateTime<Utc>)>,
    /// Window of recent actions for pattern detection
    recent_actions: Vec<(NodeId, DateTime<Utc>)>,
    /// Maximum number of recent actions to track
    max_recent: usize,
}

impl TaskGraphBuilder {
    /// Create a new task graph builder
    pub fn new() -> Self {
        Self {
            graph: TaskGraph::new(),
            last_action: None,
            recent_actions: Vec::new(),
            max_recent: 100,
        }
    }

    /// Create a builder with a custom recent actions window size
    pub fn with_window_size(max_recent: usize) -> Self {
        Self {
            graph: TaskGraph::new(),
            last_action: None,
            recent_actions: Vec::new(),
            max_recent,
        }
    }

    /// Get the current graph
    pub fn graph(&self) -> &TaskGraph {
        &self.graph
    }

    /// Get a mutable reference to the graph
    pub fn graph_mut(&mut self) -> &mut TaskGraph {
        &mut self.graph
    }

    /// Take ownership of the graph
    pub fn into_graph(self) -> TaskGraph {
        self.graph
    }

    /// Observe a new symbolic action and update the graph
    ///
    /// # Privacy Guarantee
    ///
    /// This method only accepts SymbolicAction, which is already privacy-safe.
    /// No raw events or user data can reach the graph.
    pub fn observe(&mut self, action: &SymbolicAction, timestamp: DateTime<Utc>) {
        let timestamp_ms = timestamp.timestamp_millis();
        let node_id = NodeId::from_action(action);

        // Update or create node
        if let Some(node) = self.graph.nodes.get_mut(&node_id) {
            let duration = self.last_action.as_ref().map(|(_, t)| {
                timestamp.signed_duration_since(*t).num_milliseconds().max(0) as u64
            });
            node.record_visit(timestamp_ms, duration);
        } else {
            let node = TaskNode::from_action(action, timestamp_ms);
            self.graph.nodes.insert(node_id.clone(), node);
        }

        // Update edge from last action
        if let Some((last_id, last_time)) = &self.last_action {
            let duration_ms = timestamp
                .signed_duration_since(*last_time)
                .num_milliseconds()
                .max(0) as u64;

            // Get or create edge list for source node
            let edges = self.graph.edges.entry(last_id.clone()).or_default();

            // Find existing edge or create new one
            if let Some(edge) = edges.iter_mut().find(|e| e.to == node_id) {
                edge.record_transition(timestamp_ms, Some(duration_ms));
            } else {
                let edge = TaskEdge::with_duration(
                    last_id.clone(),
                    node_id.clone(),
                    timestamp_ms,
                    duration_ms,
                );
                edges.push(edge);
            }
        }

        // Update recent actions
        self.recent_actions.push((node_id.clone(), timestamp));
        if self.recent_actions.len() > self.max_recent {
            self.recent_actions.remove(0);
        }

        // Update last action
        self.last_action = Some((node_id, timestamp));
    }

    /// Get the most frequent transitions
    pub fn frequent_transitions(&self, min_freq: u64) -> Vec<&TaskEdge> {
        self.graph.frequent_transitions(min_freq)
    }

    /// Get the most visited nodes
    pub fn most_visited_nodes(&self, limit: usize) -> Vec<&TaskNode> {
        let mut nodes: Vec<_> = self.graph.nodes.values().collect();
        nodes.sort_by(|a, b| b.visit_count.cmp(&a.visit_count));
        nodes.truncate(limit);
        nodes
    }

    /// Find common paths of a given length
    pub fn find_common_paths(&self, length: usize, min_freq: u64) -> Vec<GraphPath> {
        let mut paths = Vec::new();

        // Start from each node
        for node_id in self.graph.nodes.keys() {
            let mut path = GraphPath::new(node_id.clone());
            self.extend_path(&mut path, length - 1, min_freq, &mut paths);
        }

        // Sort by frequency
        paths.sort_by(|a, b| b.min_frequency.cmp(&a.min_frequency));
        paths
    }

    /// Recursively extend a path
    fn extend_path(
        &self,
        current_path: &mut GraphPath,
        remaining_length: usize,
        min_freq: u64,
        results: &mut Vec<GraphPath>,
    ) {
        if remaining_length == 0 {
            if current_path.min_frequency >= min_freq && current_path.len() > 1 {
                results.push(current_path.clone());
            }
            return;
        }

        let last_node = current_path.nodes.last().unwrap();

        if let Some(edges) = self.graph.edges.get(last_node) {
            for edge in edges {
                if edge.frequency >= min_freq {
                    let mut new_path = current_path.clone();
                    new_path.extend(edge.to.clone(), edge);
                    self.extend_path(&mut new_path, remaining_length - 1, min_freq, results);
                }
            }
        }
    }

    /// Get recent action sequence for pattern analysis
    pub fn recent_sequence(&self) -> Vec<&NodeId> {
        self.recent_actions.iter().map(|(id, _)| id).collect()
    }

    /// Reset the builder state (but keep the graph)
    pub fn reset_state(&mut self) {
        self.last_action = None;
        self.recent_actions.clear();
    }

    /// Clear the entire graph
    pub fn clear(&mut self) {
        self.graph = TaskGraph::new();
        self.last_action = None;
        self.recent_actions.clear();
    }

    /// Load graph state from storage
    pub fn load_from_transitions(
        &mut self,
        transitions: Vec<crate::storage::StoredTransition>,
    ) {
        for t in transitions {
            let from_app_str = t.from_app.clone().unwrap_or_default();
            let to_app_str = t.to_app.clone().unwrap_or_default();
            let from_id = NodeId(format!("{}:{}", t.from_action_type, from_app_str));
            let to_id = NodeId(format!("{}:{}", t.to_action_type, to_app_str));

            // Ensure nodes exist
            if !self.graph.nodes.contains_key(&from_id) {
                // Create a placeholder node - we don't have full info from transitions
                self.graph.nodes.insert(from_id.clone(), TaskNode {
                    id: from_id.clone(),
                    action_type: crate::symbolizer::SymbolicActionType::SwitchApp, // Placeholder
                    app_context: t.from_app.clone().map(|a| crate::symbolizer::AppIdentifier::new(a)),
                    visit_count: t.frequency as u64,
                    total_duration_ms: 0,
                    first_seen_ms: t.last_seen_ms,
                    last_seen_ms: t.last_seen_ms,
                });
            }

            if !self.graph.nodes.contains_key(&to_id) {
                self.graph.nodes.insert(to_id.clone(), TaskNode {
                    id: to_id.clone(),
                    action_type: crate::symbolizer::SymbolicActionType::SwitchApp, // Placeholder
                    app_context: t.to_app.clone().map(|a| crate::symbolizer::AppIdentifier::new(a)),
                    visit_count: t.frequency as u64,
                    total_duration_ms: 0,
                    first_seen_ms: t.last_seen_ms,
                    last_seen_ms: t.last_seen_ms,
                });
            }

            // Create edge
            let edge = TaskEdge {
                from: from_id.clone(),
                to: to_id,
                frequency: t.frequency as u64,
                total_duration_ms: t.total_duration_ms as u64,
                first_seen_ms: t.last_seen_ms,
                last_seen_ms: t.last_seen_ms,
            };

            self.graph.edges.entry(from_id).or_default().push(edge);
        }
    }
}

impl Default for TaskGraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolizer::{AppIdentifier, ContentType};

    #[test]
    fn test_observe_actions() {
        let mut builder = TaskGraphBuilder::new();

        let action1 = SymbolicAction::CopyText {
            source_app: AppIdentifier::new("chrome.exe"),
            content_type: ContentType::PlainText,
        };

        let action2 = SymbolicAction::SwitchApp {
            from_app: AppIdentifier::new("chrome.exe"),
            to_app: AppIdentifier::new("code.exe"),
        };

        let action3 = SymbolicAction::PasteText {
            target_app: AppIdentifier::new("code.exe"),
        };

        builder.observe(&action1, Utc::now());
        builder.observe(&action2, Utc::now());
        builder.observe(&action3, Utc::now());

        assert_eq!(builder.graph().node_count(), 3);
        assert_eq!(builder.graph().edge_count(), 2);
    }

    #[test]
    fn test_frequent_transitions() {
        let mut builder = TaskGraphBuilder::new();

        let copy = SymbolicAction::CopyText {
            source_app: AppIdentifier::new("chrome.exe"),
            content_type: ContentType::PlainText,
        };

        let paste = SymbolicAction::PasteText {
            target_app: AppIdentifier::new("code.exe"),
        };

        // Repeat the pattern multiple times
        for _ in 0..5 {
            builder.observe(&copy, Utc::now());
            builder.observe(&paste, Utc::now());
        }

        let frequent = builder.frequent_transitions(3);
        assert!(!frequent.is_empty());
        assert!(frequent[0].frequency >= 3);
    }

    #[test]
    fn test_find_common_paths() {
        let mut builder = TaskGraphBuilder::new();

        let copy = SymbolicAction::CopyText {
            source_app: AppIdentifier::new("browser.exe"),
            content_type: ContentType::PlainText,
        };

        let switch = SymbolicAction::SwitchApp {
            from_app: AppIdentifier::new("browser.exe"),
            to_app: AppIdentifier::new("editor.exe"),
        };

        let paste = SymbolicAction::PasteText {
            target_app: AppIdentifier::new("editor.exe"),
        };

        // Repeat the pattern
        for _ in 0..5 {
            builder.observe(&copy, Utc::now());
            builder.observe(&switch, Utc::now());
            builder.observe(&paste, Utc::now());
        }

        let paths = builder.find_common_paths(3, 3);
        assert!(!paths.is_empty());
        assert_eq!(paths[0].len(), 3);
    }
}
