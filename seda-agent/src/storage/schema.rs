//! Database schema types
//!
//! These types represent the data stored in SQLite.
//! All types are designed to be privacy-safe.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::symbolizer::{SymbolicAction, SymbolicActionType};

/// A stored symbolic action record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAction {
    /// Database ID
    pub id: i64,
    /// Action type (e.g., "SWITCH_APP", "COPY_TEXT")
    pub action_type: String,
    /// JSON-serialized SymbolicAction
    pub action_data: String,
    /// When the action occurred (milliseconds since Unix epoch)
    pub timestamp_ms: i64,
    /// Session ID for grouping actions
    pub session_id: String,
    /// Duration of the action in milliseconds
    pub duration_ms: Option<i64>,
    /// Source application (process name)
    pub source_app: Option<String>,
    /// Target application (process name)
    pub target_app: Option<String>,
}

impl StoredAction {
    /// Create a new stored action from a symbolic action
    pub fn from_symbolic(
        action: &SymbolicAction,
        timestamp: DateTime<Utc>,
        session_id: &str,
        duration_ms: Option<u64>,
    ) -> Result<Self, serde_json::Error> {
        let action_type = action.action_type().to_string();
        let action_data = serde_json::to_string(action)?;

        let (source_app, target_app) = match action {
            SymbolicAction::SwitchApp { from_app, to_app } => {
                (Some(from_app.process_name.clone()), Some(to_app.process_name.clone()))
            }
            SymbolicAction::OpenApp { app } | SymbolicAction::CloseApp { app } => {
                (None, Some(app.process_name.clone()))
            }
            SymbolicAction::CopyText { source_app, .. } => {
                (Some(source_app.process_name.clone()), None)
            }
            SymbolicAction::PasteText { target_app } => {
                (None, Some(target_app.process_name.clone()))
            }
            SymbolicAction::TypeText { target_app, .. } => {
                (None, Some(target_app.process_name.clone()))
            }
            SymbolicAction::Navigate { app, .. } | SymbolicAction::Interact { app, .. } => {
                (None, Some(app.process_name.clone()))
            }
            SymbolicAction::VisitWebsite { browser_app, .. }
            | SymbolicAction::SearchWeb { browser_app, .. } => {
                (None, Some(browser_app.process_name.clone()))
            }
        };

        Ok(Self {
            id: 0, // Will be set by database
            action_type,
            action_data,
            timestamp_ms: timestamp.timestamp_millis(),
            session_id: session_id.to_string(),
            duration_ms: duration_ms.map(|d| d as i64),
            source_app,
            target_app,
        })
    }

    /// Parse the stored action data back to a SymbolicAction
    pub fn to_symbolic(&self) -> Result<SymbolicAction, serde_json::Error> {
        serde_json::from_str(&self.action_data)
    }

    /// Get the timestamp as a DateTime
    pub fn timestamp(&self) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(self.timestamp_ms)
            .unwrap_or_else(|| Utc::now())
    }
}

/// A stored action transition (graph edge)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredTransition {
    /// Database ID
    pub id: i64,
    /// Source action type
    pub from_action_type: String,
    /// Target action type
    pub to_action_type: String,
    /// Source application (process name)
    pub from_app: Option<String>,
    /// Target application (process name)
    pub to_app: Option<String>,
    /// How often this transition occurred
    pub frequency: i64,
    /// Total duration of all occurrences (milliseconds)
    pub total_duration_ms: i64,
    /// Last time this transition was observed
    pub last_seen_ms: i64,
}

impl StoredTransition {
    /// Calculate average duration
    pub fn avg_duration_ms(&self) -> f64 {
        if self.frequency == 0 {
            0.0
        } else {
            self.total_duration_ms as f64 / self.frequency as f64
        }
    }
}

/// A stored pattern from sequence mining
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPattern {
    /// Database ID
    pub id: i64,
    /// SHA256 hash of the pattern sequence
    pub pattern_hash: String,
    /// JSON array of action types in the sequence
    pub sequence: String,
    /// How often pattern was observed
    pub frequency: i64,
    /// Average time to complete pattern (milliseconds)
    pub avg_duration_ms: Option<i64>,
    /// Confidence score (0-1)
    pub confidence: f64,
    /// First observation time
    pub first_seen_ms: i64,
    /// Most recent observation
    pub last_seen_ms: i64,
    /// Whether user dismissed this pattern
    pub user_dismissed: bool,
    /// Whether user accepted this pattern
    pub user_accepted: bool,
}

impl StoredPattern {
    /// Parse the sequence back to action types
    pub fn to_sequence(&self) -> Result<Vec<SymbolicActionType>, serde_json::Error> {
        serde_json::from_str(&self.sequence)
    }
}

/// Audit log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAuditEntry {
    /// Database ID
    pub id: i64,
    /// Unique audit ID (UUID)
    pub audit_id: String,
    /// When the action occurred
    pub timestamp_ms: i64,
    /// MCP operation name
    pub operation: String,
    /// SHA256 hash of parameters
    pub parameters_hash: Option<String>,
    /// Result: "success", "error", or "denied"
    pub result: String,
    /// Error message if result is "error"
    pub error_message: Option<String>,
    /// MCP client identifier
    pub caller: Option<String>,
}

/// Session information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSession {
    /// Session ID (UUID)
    pub id: String,
    /// Session start time
    pub started_ms: i64,
    /// Session end time (if ended)
    pub ended_ms: Option<i64>,
    /// Number of actions in this session
    pub action_count: i64,
}

impl StoredSession {
    /// Check if the session is currently active
    pub fn is_active(&self) -> bool {
        self.ended_ms.is_none()
    }

    /// Get session duration in milliseconds
    pub fn duration_ms(&self) -> Option<i64> {
        self.ended_ms.map(|end| end - self.started_ms)
    }
}
