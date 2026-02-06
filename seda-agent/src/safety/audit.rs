//! Audit logging
//!
//! Records all MCP actions for accountability and debugging.
//!
//! # Privacy Design
//!
//! - Parameters are HASHED, not stored directly
//! - Only operation names and results are recorded
//! - Audit logs can be cleared by user

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Result of an audited action
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditResult {
    /// Action completed successfully
    Success,
    /// Action failed with an error
    Error,
    /// Action was denied by safety checks
    Denied,
    /// Action was rate limited
    RateLimited,
}

impl std::fmt::Display for AuditResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditResult::Success => write!(f, "success"),
            AuditResult::Error => write!(f, "error"),
            AuditResult::Denied => write!(f, "denied"),
            AuditResult::RateLimited => write!(f, "rate_limited"),
        }
    }
}

impl std::str::FromStr for AuditResult {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "success" => Ok(AuditResult::Success),
            "error" => Ok(AuditResult::Error),
            "denied" => Ok(AuditResult::Denied),
            "rate_limited" => Ok(AuditResult::RateLimited),
            _ => Err(format!("Unknown audit result: {}", s)),
        }
    }
}

/// An audit log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLog {
    /// Unique identifier for this audit entry
    pub id: String,
    /// When the action occurred
    pub timestamp: DateTime<Utc>,
    /// Name of the operation
    pub operation: String,
    /// Hash of the parameters (NEVER the actual parameters)
    ///
    /// # Privacy Note
    /// We hash parameters instead of storing them directly to:
    /// 1. Allow correlation of identical requests
    /// 2. Not expose potentially sensitive parameter values
    pub parameters_hash: Option<String>,
    /// Result of the operation
    pub result: AuditResult,
    /// Error message if result is Error
    pub error_message: Option<String>,
    /// Identifier of the caller (e.g., "python-planner")
    pub caller: Option<String>,
}

impl AuditLog {
    /// Create a new audit log entry
    pub fn new(
        operation: impl Into<String>,
        parameters: Option<&serde_json::Value>,
        result: AuditResult,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            operation: operation.into(),
            parameters_hash: parameters.map(hash_params),
            result,
            error_message: None,
            caller: None,
        }
    }

    /// Create a success audit entry
    pub fn success(operation: impl Into<String>, parameters: Option<&serde_json::Value>) -> Self {
        Self::new(operation, parameters, AuditResult::Success)
    }

    /// Create an error audit entry
    pub fn error(
        operation: impl Into<String>,
        parameters: Option<&serde_json::Value>,
        message: impl Into<String>,
    ) -> Self {
        let mut entry = Self::new(operation, parameters, AuditResult::Error);
        entry.error_message = Some(message.into());
        entry
    }

    /// Create a denied audit entry
    pub fn denied(operation: impl Into<String>, parameters: Option<&serde_json::Value>) -> Self {
        Self::new(operation, parameters, AuditResult::Denied)
    }

    /// Create a rate limited audit entry
    pub fn rate_limited(
        operation: impl Into<String>,
        parameters: Option<&serde_json::Value>,
    ) -> Self {
        Self::new(operation, parameters, AuditResult::RateLimited)
    }

    /// Set the caller
    pub fn with_caller(mut self, caller: impl Into<String>) -> Self {
        self.caller = Some(caller.into());
        self
    }

    /// Check if this was a successful operation
    pub fn is_success(&self) -> bool {
        self.result == AuditResult::Success
    }

    /// Check if this was a denied operation
    pub fn is_denied(&self) -> bool {
        self.result == AuditResult::Denied
    }

    /// Get a human-readable summary
    pub fn summary(&self) -> String {
        let caller_str = self
            .caller
            .as_ref()
            .map(|c| format!(" (caller: {})", c))
            .unwrap_or_default();

        let error_str = self
            .error_message
            .as_ref()
            .map(|e| format!(" - {}", e))
            .unwrap_or_default();

        format!(
            "[{}] {} -> {}{}{}",
            self.timestamp.format("%Y-%m-%d %H:%M:%S"),
            self.operation,
            self.result,
            error_str,
            caller_str
        )
    }
}

/// In-memory audit buffer
pub struct AuditBuffer {
    /// Maximum number of entries to keep
    max_entries: usize,
    /// Audit entries (newest first)
    entries: Vec<AuditLog>,
    /// Count of denied operations (for monitoring)
    denied_count: u64,
    /// Count of rate limited operations
    rate_limited_count: u64,
}

impl AuditBuffer {
    /// Create a new audit buffer
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries,
            entries: Vec::with_capacity(max_entries),
            denied_count: 0,
            rate_limited_count: 0,
        }
    }

    /// Add an audit entry
    pub fn record(&mut self, entry: AuditLog) {
        // Update counters
        match entry.result {
            AuditResult::Denied => self.denied_count += 1,
            AuditResult::RateLimited => self.rate_limited_count += 1,
            _ => {}
        }

        // Add entry
        self.entries.insert(0, entry);

        // Trim if needed
        if self.entries.len() > self.max_entries {
            self.entries.truncate(self.max_entries);
        }
    }

    /// Get recent entries
    pub fn recent(&self, limit: usize) -> &[AuditLog] {
        let end = limit.min(self.entries.len());
        &self.entries[..end]
    }

    /// Get entries for a specific operation
    pub fn for_operation(&self, operation: &str) -> Vec<&AuditLog> {
        self.entries
            .iter()
            .filter(|e| e.operation == operation)
            .collect()
    }

    /// Get denied entries
    pub fn denied_entries(&self) -> Vec<&AuditLog> {
        self.entries
            .iter()
            .filter(|e| e.result == AuditResult::Denied)
            .collect()
    }

    /// Get statistics
    pub fn stats(&self) -> AuditStats {
        let total = self.entries.len();
        let success = self
            .entries
            .iter()
            .filter(|e| e.result == AuditResult::Success)
            .count();
        let error = self
            .entries
            .iter()
            .filter(|e| e.result == AuditResult::Error)
            .count();

        AuditStats {
            total_entries: total,
            success_count: success,
            error_count: error,
            denied_count: self.denied_count,
            rate_limited_count: self.rate_limited_count,
        }
    }

    /// Clear all entries
    pub fn clear(&mut self) {
        self.entries.clear();
        self.denied_count = 0;
        self.rate_limited_count = 0;
    }

    /// Find entry by ID
    pub fn find(&self, id: &str) -> Option<&AuditLog> {
        self.entries.iter().find(|e| e.id == id)
    }
}

impl Default for AuditBuffer {
    fn default() -> Self {
        Self::new(1000) // Keep last 1000 entries
    }
}

/// Audit statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditStats {
    pub total_entries: usize,
    pub success_count: usize,
    pub error_count: usize,
    pub denied_count: u64,
    pub rate_limited_count: u64,
}

impl AuditStats {
    /// Get the success rate as a percentage
    pub fn success_rate(&self) -> f64 {
        if self.total_entries == 0 {
            100.0
        } else {
            (self.success_count as f64 / self.total_entries as f64) * 100.0
        }
    }

    /// Get a summary string
    pub fn summary(&self) -> String {
        format!(
            "Audit: {} total, {} success ({:.1}%), {} errors, {} denied, {} rate limited",
            self.total_entries,
            self.success_count,
            self.success_rate(),
            self.error_count,
            self.denied_count,
            self.rate_limited_count
        )
    }
}

/// Hash parameters for privacy-safe storage
fn hash_params(params: &serde_json::Value) -> String {
    let json = serde_json::to_string(params).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    hex::encode(hasher.finalize())[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_log_creation() {
        let entry = AuditLog::success("list_windows", None);
        assert!(entry.is_success());
        assert!(!entry.is_denied());
        assert!(entry.error_message.is_none());
    }

    #[test]
    fn test_audit_log_with_params() {
        let params = serde_json::json!({ "hwnd": "12345", "depth": 3 });
        let entry = AuditLog::success("get_window_tree", Some(&params));

        assert!(entry.parameters_hash.is_some());
        // Hash should be consistent
        let hash1 = hash_params(&params);
        let hash2 = hash_params(&params);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_audit_buffer() {
        let mut buffer = AuditBuffer::new(5);

        for i in 0..10 {
            buffer.record(AuditLog::success(format!("op_{}", i), None));
        }

        // Should only keep last 5
        assert_eq!(buffer.entries.len(), 5);

        // Most recent should be first
        assert_eq!(buffer.entries[0].operation, "op_9");
    }

    #[test]
    fn test_audit_stats() {
        let mut buffer = AuditBuffer::new(100);

        buffer.record(AuditLog::success("op1", None));
        buffer.record(AuditLog::success("op2", None));
        buffer.record(AuditLog::error("op3", None, "failed"));
        buffer.record(AuditLog::denied("op4", None));

        let stats = buffer.stats();
        assert_eq!(stats.total_entries, 4);
        assert_eq!(stats.success_count, 2);
        assert_eq!(stats.error_count, 1);
        assert_eq!(stats.denied_count, 1);
        assert_eq!(stats.success_rate(), 50.0);
    }

    #[test]
    fn test_audit_result_parsing() {
        assert_eq!("success".parse::<AuditResult>().unwrap(), AuditResult::Success);
        assert_eq!("error".parse::<AuditResult>().unwrap(), AuditResult::Error);
        assert_eq!("denied".parse::<AuditResult>().unwrap(), AuditResult::Denied);
    }
}
