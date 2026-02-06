//! Data access layer
//!
//! Provides high-level operations for storing and querying data.
//!
//! # Safety Decision
//!
//! All data stored through this repository has already been symbolized.
//! No raw user data ever reaches the database.

use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::migrations;
use super::schema::*;
use crate::symbolizer::{SymbolicAction, SymbolicActionType};

/// Repository for all data access
pub struct Repository {
    conn: Connection,
    current_session_id: String,
}

impl Repository {
    /// Open or create a repository at the given path
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RepositoryError> {
        let path = path.as_ref();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                RepositoryError::IoError(format!("Failed to create directory: {}", e))
            })?;
        }

        let conn = Connection::open(path)?;

        // Enable foreign keys and WAL mode for better performance
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;",
        )?;

        // Run migrations
        migrations::run_migrations(&conn)?;

        // Start a new session
        let session_id = Uuid::new_v4().to_string();

        let mut repo = Self {
            conn,
            current_session_id: session_id,
        };

        repo.start_session()?;

        Ok(repo)
    }

    /// Open an in-memory repository (for testing)
    pub fn open_in_memory() -> Result<Self, RepositoryError> {
        let conn = Connection::open_in_memory()?;

        conn.execute_batch(
            "PRAGMA foreign_keys = ON;",
        )?;

        migrations::run_migrations(&conn)?;

        let session_id = Uuid::new_v4().to_string();

        let mut repo = Self {
            conn,
            current_session_id: session_id,
        };

        repo.start_session()?;

        Ok(repo)
    }

    /// Get the current session ID
    pub fn session_id(&self) -> &str {
        &self.current_session_id
    }

    // ========== Session Management ==========

    /// Start a new session
    fn start_session(&mut self) -> Result<(), RepositoryError> {
        let now = Utc::now().timestamp_millis();

        self.conn.execute(
            "INSERT INTO sessions (id, started_ms, action_count) VALUES (?, ?, 0)",
            params![&self.current_session_id, now],
        )?;

        Ok(())
    }

    /// End the current session
    pub fn end_session(&mut self) -> Result<(), RepositoryError> {
        let now = Utc::now().timestamp_millis();

        self.conn.execute(
            "UPDATE sessions SET ended_ms = ? WHERE id = ?",
            params![now, &self.current_session_id],
        )?;

        // Start a new session
        self.current_session_id = Uuid::new_v4().to_string();
        self.start_session()?;

        Ok(())
    }

    // ========== Symbolic Actions ==========

    /// Store a symbolic action
    pub fn store_action(
        &self,
        action: &SymbolicAction,
        timestamp: DateTime<Utc>,
        duration_ms: Option<u64>,
    ) -> Result<i64, RepositoryError> {
        let stored = StoredAction::from_symbolic(action, timestamp, &self.current_session_id, duration_ms)?;

        self.conn.execute(
            "INSERT INTO symbolic_actions (action_type, action_data, timestamp_ms, session_id, duration_ms, source_app, target_app)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                stored.action_type,
                stored.action_data,
                stored.timestamp_ms,
                stored.session_id,
                stored.duration_ms,
                stored.source_app,
                stored.target_app,
            ],
        )?;

        let id = self.conn.last_insert_rowid();

        // Increment session action count
        self.conn.execute(
            "UPDATE sessions SET action_count = action_count + 1 WHERE id = ?",
            params![&self.current_session_id],
        )?;

        Ok(id)
    }

    /// Get actions within a time range
    pub fn get_actions(
        &self,
        from_ms: i64,
        to_ms: i64,
        limit: Option<usize>,
    ) -> Result<Vec<StoredAction>, RepositoryError> {
        let limit = limit.unwrap_or(1000);

        let mut stmt = self.conn.prepare(
            "SELECT id, action_type, action_data, timestamp_ms, session_id, duration_ms, source_app, target_app
             FROM symbolic_actions
             WHERE timestamp_ms >= ? AND timestamp_ms <= ?
             ORDER BY timestamp_ms ASC
             LIMIT ?",
        )?;

        let actions = stmt
            .query_map(params![from_ms, to_ms, limit as i64], |row| {
                Ok(StoredAction {
                    id: row.get(0)?,
                    action_type: row.get(1)?,
                    action_data: row.get(2)?,
                    timestamp_ms: row.get(3)?,
                    session_id: row.get(4)?,
                    duration_ms: row.get(5)?,
                    source_app: row.get(6)?,
                    target_app: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(actions)
    }

    /// Get the most recent actions
    pub fn get_recent_actions(&self, limit: usize) -> Result<Vec<StoredAction>, RepositoryError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, action_type, action_data, timestamp_ms, session_id, duration_ms, source_app, target_app
             FROM symbolic_actions
             ORDER BY timestamp_ms DESC
             LIMIT ?",
        )?;

        let actions = stmt
            .query_map(params![limit as i64], |row| {
                Ok(StoredAction {
                    id: row.get(0)?,
                    action_type: row.get(1)?,
                    action_data: row.get(2)?,
                    timestamp_ms: row.get(3)?,
                    session_id: row.get(4)?,
                    duration_ms: row.get(5)?,
                    source_app: row.get(6)?,
                    target_app: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(actions)
    }

    /// Count total actions
    pub fn count_actions(&self) -> Result<i64, RepositoryError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM symbolic_actions",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    // ========== Action Transitions (Graph Edges) ==========

    /// Record or update a transition between actions
    pub fn record_transition(
        &self,
        from_action: &SymbolicAction,
        to_action: &SymbolicAction,
        duration_ms: Option<u64>,
    ) -> Result<(), RepositoryError> {
        let from_type = from_action.action_type().to_string();
        let to_type = to_action.action_type().to_string();
        let from_app = from_action.primary_app().process_name.clone();
        let to_app = to_action.primary_app().process_name.clone();
        let now = Utc::now().timestamp_millis();
        let duration = duration_ms.unwrap_or(0) as i64;

        // Try to update existing transition
        let updated = self.conn.execute(
            "UPDATE action_transitions
             SET frequency = frequency + 1,
                 total_duration_ms = total_duration_ms + ?,
                 last_seen_ms = ?
             WHERE from_action_type = ? AND to_action_type = ?
               AND from_app = ? AND to_app = ?",
            params![duration, now, from_type, to_type, from_app, to_app],
        )?;

        // If no row was updated, insert a new one
        if updated == 0 {
            self.conn.execute(
                "INSERT INTO action_transitions (from_action_type, to_action_type, from_app, to_app, frequency, total_duration_ms, last_seen_ms)
                 VALUES (?, ?, ?, ?, 1, ?, ?)",
                params![from_type, to_type, from_app, to_app, duration, now],
            )?;
        }

        Ok(())
    }

    /// Get all transitions
    pub fn get_transitions(&self) -> Result<Vec<StoredTransition>, RepositoryError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, from_action_type, to_action_type, from_app, to_app, frequency, total_duration_ms, last_seen_ms
             FROM action_transitions
             ORDER BY frequency DESC",
        )?;

        let transitions = stmt
            .query_map([], |row| {
                Ok(StoredTransition {
                    id: row.get(0)?,
                    from_action_type: row.get(1)?,
                    to_action_type: row.get(2)?,
                    from_app: row.get(3)?,
                    to_app: row.get(4)?,
                    frequency: row.get(5)?,
                    total_duration_ms: row.get(6)?,
                    last_seen_ms: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(transitions)
    }

    /// Get transitions above a frequency threshold
    pub fn get_frequent_transitions(&self, min_frequency: i64) -> Result<Vec<StoredTransition>, RepositoryError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, from_action_type, to_action_type, from_app, to_app, frequency, total_duration_ms, last_seen_ms
             FROM action_transitions
             WHERE frequency >= ?
             ORDER BY frequency DESC",
        )?;

        let transitions = stmt
            .query_map(params![min_frequency], |row| {
                Ok(StoredTransition {
                    id: row.get(0)?,
                    from_action_type: row.get(1)?,
                    to_action_type: row.get(2)?,
                    from_app: row.get(3)?,
                    to_app: row.get(4)?,
                    frequency: row.get(5)?,
                    total_duration_ms: row.get(6)?,
                    last_seen_ms: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(transitions)
    }

    // ========== Patterns ==========

    /// Store or update a detected pattern
    pub fn store_pattern(
        &self,
        sequence: &[SymbolicActionType],
        frequency: i64,
        avg_duration_ms: Option<i64>,
        confidence: f64,
    ) -> Result<i64, RepositoryError> {
        let sequence_json = serde_json::to_string(sequence)?;
        let pattern_hash = {
            let mut hasher = Sha256::new();
            hasher.update(sequence_json.as_bytes());
            hex::encode(hasher.finalize())
        };
        let now = Utc::now().timestamp_millis();

        // Try to update existing pattern
        let updated = self.conn.execute(
            "UPDATE detected_patterns
             SET frequency = ?,
                 avg_duration_ms = ?,
                 confidence = ?,
                 last_seen_ms = ?
             WHERE pattern_hash = ?",
            params![frequency, avg_duration_ms, confidence, now, pattern_hash],
        )?;

        if updated == 0 {
            // Insert new pattern
            self.conn.execute(
                "INSERT INTO detected_patterns (pattern_hash, sequence, frequency, avg_duration_ms, confidence, first_seen_ms, last_seen_ms)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
                params![pattern_hash, sequence_json, frequency, avg_duration_ms, confidence, now, now],
            )?;
        }

        let id = self.conn.last_insert_rowid();
        Ok(id)
    }

    /// Get patterns above a frequency threshold
    pub fn get_patterns(&self, min_frequency: i64) -> Result<Vec<StoredPattern>, RepositoryError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, pattern_hash, sequence, frequency, avg_duration_ms, confidence, first_seen_ms, last_seen_ms, user_dismissed, user_accepted
             FROM detected_patterns
             WHERE frequency >= ? AND user_dismissed = 0
             ORDER BY frequency DESC",
        )?;

        let patterns = stmt
            .query_map(params![min_frequency], |row| {
                Ok(StoredPattern {
                    id: row.get(0)?,
                    pattern_hash: row.get(1)?,
                    sequence: row.get(2)?,
                    frequency: row.get(3)?,
                    avg_duration_ms: row.get(4)?,
                    confidence: row.get(5)?,
                    first_seen_ms: row.get(6)?,
                    last_seen_ms: row.get(7)?,
                    user_dismissed: row.get(8)?,
                    user_accepted: row.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(patterns)
    }

    /// Mark a pattern as dismissed by the user
    pub fn dismiss_pattern(&self, pattern_hash: &str) -> Result<(), RepositoryError> {
        self.conn.execute(
            "UPDATE detected_patterns SET user_dismissed = 1 WHERE pattern_hash = ?",
            params![pattern_hash],
        )?;
        Ok(())
    }

    /// Mark a pattern as accepted by the user
    pub fn accept_pattern(&self, pattern_hash: &str) -> Result<(), RepositoryError> {
        self.conn.execute(
            "UPDATE detected_patterns SET user_accepted = 1 WHERE pattern_hash = ?",
            params![pattern_hash],
        )?;
        Ok(())
    }

    // ========== Audit Log ==========

    /// Record an audit entry
    pub fn record_audit(
        &self,
        operation: &str,
        parameters_hash: Option<&str>,
        result: &str,
        error_message: Option<&str>,
        caller: Option<&str>,
    ) -> Result<String, RepositoryError> {
        let audit_id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp_millis();

        self.conn.execute(
            "INSERT INTO audit_log (audit_id, timestamp_ms, operation, parameters_hash, result, error_message, caller)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![audit_id, now, operation, parameters_hash, result, error_message, caller],
        )?;

        Ok(audit_id)
    }

    /// Get recent audit entries
    pub fn get_recent_audits(&self, limit: usize) -> Result<Vec<StoredAuditEntry>, RepositoryError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, audit_id, timestamp_ms, operation, parameters_hash, result, error_message, caller
             FROM audit_log
             ORDER BY timestamp_ms DESC
             LIMIT ?",
        )?;

        let entries = stmt
            .query_map(params![limit as i64], |row| {
                Ok(StoredAuditEntry {
                    id: row.get(0)?,
                    audit_id: row.get(1)?,
                    timestamp_ms: row.get(2)?,
                    operation: row.get(3)?,
                    parameters_hash: row.get(4)?,
                    result: row.get(5)?,
                    error_message: row.get(6)?,
                    caller: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(entries)
    }

    // ========== Data Management ==========

    /// Delete all data older than the given timestamp
    pub fn cleanup_old_data(&self, before_ms: i64) -> Result<CleanupStats, RepositoryError> {
        let actions_deleted = self.conn.execute(
            "DELETE FROM symbolic_actions WHERE timestamp_ms < ?",
            params![before_ms],
        )?;

        let transitions_deleted = self.conn.execute(
            "DELETE FROM action_transitions WHERE last_seen_ms < ?",
            params![before_ms],
        )?;

        let audits_deleted = self.conn.execute(
            "DELETE FROM audit_log WHERE timestamp_ms < ?",
            params![before_ms],
        )?;

        Ok(CleanupStats {
            actions_deleted,
            transitions_deleted,
            audits_deleted,
        })
    }

    /// Delete all data (for privacy reset)
    pub fn delete_all_data(&self) -> Result<(), RepositoryError> {
        self.conn.execute_batch(
            "DELETE FROM symbolic_actions;
             DELETE FROM action_transitions;
             DELETE FROM detected_patterns;
             DELETE FROM audit_log;
             DELETE FROM sessions;",
        )?;
        Ok(())
    }
}

/// Statistics from a cleanup operation
#[derive(Debug, Clone)]
pub struct CleanupStats {
    pub actions_deleted: usize,
    pub transitions_deleted: usize,
    pub audits_deleted: usize,
}

/// Repository errors
#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("SQLite error: {0}")]
    SqliteError(#[from] rusqlite::Error),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    IoError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbolizer::{AppIdentifier, ContentType};

    #[test]
    fn test_store_and_retrieve_action() {
        let repo = Repository::open_in_memory().unwrap();

        let action = SymbolicAction::SwitchApp {
            from_app: AppIdentifier::new("chrome.exe"),
            to_app: AppIdentifier::new("code.exe"),
        };

        let id = repo.store_action(&action, Utc::now(), Some(100)).unwrap();
        assert!(id > 0);

        let actions = repo.get_recent_actions(10).unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action_type, "SWITCH_APP");
    }

    #[test]
    fn test_record_transition() {
        let repo = Repository::open_in_memory().unwrap();

        let action1 = SymbolicAction::CopyText {
            source_app: AppIdentifier::new("chrome.exe"),
            content_type: ContentType::PlainText,
        };

        let action2 = SymbolicAction::PasteText {
            target_app: AppIdentifier::new("code.exe"),
        };

        // Record same transition multiple times
        repo.record_transition(&action1, &action2, Some(50)).unwrap();
        repo.record_transition(&action1, &action2, Some(60)).unwrap();
        repo.record_transition(&action1, &action2, Some(70)).unwrap();

        let transitions = repo.get_frequent_transitions(2).unwrap();
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0].frequency, 3);
        assert_eq!(transitions[0].total_duration_ms, 180);
    }

    #[test]
    fn test_store_pattern() {
        let repo = Repository::open_in_memory().unwrap();

        let sequence = vec![
            SymbolicActionType::CopyText,
            SymbolicActionType::SwitchApp,
            SymbolicActionType::PasteText,
        ];

        repo.store_pattern(&sequence, 5, Some(500), 0.9).unwrap();

        let patterns = repo.get_patterns(3).unwrap();
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].frequency, 5);
    }

    #[test]
    fn test_audit_log() {
        let repo = Repository::open_in_memory().unwrap();

        let audit_id = repo.record_audit(
            "list_windows",
            Some("abc123"),
            "success",
            None,
            Some("python-planner"),
        ).unwrap();

        assert!(!audit_id.is_empty());

        let audits = repo.get_recent_audits(10).unwrap();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].operation, "list_windows");
        assert_eq!(audits[0].result, "success");
    }
}
