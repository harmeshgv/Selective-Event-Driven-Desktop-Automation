//! Task graph node definitions
//!
//! Nodes represent action types in specific app contexts.

use serde::{Deserialize, Serialize};

use crate::symbolizer::{AppIdentifier, SymbolicAction, SymbolicActionType};

/// Unique identifier for a node in the task graph
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub String);

impl NodeId {
    /// Create a node ID from a symbolic action
    pub fn from_action(action: &SymbolicAction) -> Self {
        NodeId(action.node_key())
    }

    /// Create a node ID from action type and app context
    pub fn from_type_and_app(action_type: SymbolicActionType, app: Option<&AppIdentifier>) -> Self {
        let key = match app {
            Some(app) => format!("{}:{}", action_type, app.process_name),
            None => action_type.to_string(),
        };
        NodeId(key)
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A node in the task graph
///
/// Represents an action type with optional app context.
/// Statistics are accumulated as actions are observed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    /// Unique identifier
    pub id: NodeId,
    /// The type of action this node represents
    pub action_type: SymbolicActionType,
    /// Application context (if specific to an app)
    pub app_context: Option<AppIdentifier>,
    /// Number of times this action was observed
    pub visit_count: u64,
    /// Total duration of all visits (milliseconds)
    pub total_duration_ms: u64,
    /// First observation timestamp (milliseconds since epoch)
    pub first_seen_ms: i64,
    /// Most recent observation timestamp
    pub last_seen_ms: i64,
}

impl TaskNode {
    /// Create a new node from a symbolic action
    pub fn from_action(action: &SymbolicAction, timestamp_ms: i64) -> Self {
        Self {
            id: NodeId::from_action(action),
            action_type: action.action_type(),
            app_context: Some(action.primary_app().clone()),
            visit_count: 1,
            total_duration_ms: 0,
            first_seen_ms: timestamp_ms,
            last_seen_ms: timestamp_ms,
        }
    }

    /// Create a new node from action type and app
    pub fn new(
        action_type: SymbolicActionType,
        app_context: Option<AppIdentifier>,
        timestamp_ms: i64,
    ) -> Self {
        let id = NodeId::from_type_and_app(action_type, app_context.as_ref());
        Self {
            id,
            action_type,
            app_context,
            visit_count: 1,
            total_duration_ms: 0,
            first_seen_ms: timestamp_ms,
            last_seen_ms: timestamp_ms,
        }
    }

    /// Record a visit to this node
    pub fn record_visit(&mut self, timestamp_ms: i64, duration_ms: Option<u64>) {
        self.visit_count += 1;
        self.last_seen_ms = timestamp_ms;
        if let Some(d) = duration_ms {
            self.total_duration_ms += d;
        }
    }

    /// Get the average duration per visit
    pub fn avg_duration_ms(&self) -> f64 {
        if self.visit_count == 0 {
            0.0
        } else {
            self.total_duration_ms as f64 / self.visit_count as f64
        }
    }

    /// Get the process name if there's an app context
    pub fn process_name(&self) -> Option<&str> {
        self.app_context.as_ref().map(|a| a.process_name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolizer::ContentType;

    #[test]
    fn test_node_from_action() {
        let action = SymbolicAction::CopyText {
            source_app: AppIdentifier::new("chrome.exe"),
            content_type: ContentType::PlainText,
        };

        let node = TaskNode::from_action(&action, 1000);

        assert_eq!(node.action_type, SymbolicActionType::CopyText);
        assert_eq!(node.visit_count, 1);
        assert!(node.id.0.contains("copy"));
        assert!(node.id.0.contains("chrome.exe"));
    }

    #[test]
    fn test_node_record_visit() {
        let mut node = TaskNode::new(
            SymbolicActionType::SwitchApp,
            Some(AppIdentifier::new("code.exe")),
            1000,
        );

        node.record_visit(2000, Some(100));
        node.record_visit(3000, Some(200));

        assert_eq!(node.visit_count, 3);
        assert_eq!(node.total_duration_ms, 300);
        assert_eq!(node.avg_duration_ms(), 100.0);
        assert_eq!(node.last_seen_ms, 3000);
    }
}
