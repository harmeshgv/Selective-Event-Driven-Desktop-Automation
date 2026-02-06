//! MCP safety enforcement
//!
//! Enforces safety boundaries for all MCP operations.
//!
//! # Core Principle
//!
//! If an operation is not explicitly allowed, it is DENIED.
//! This is an allowlist approach, not a blocklist.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

/// Safety enforcer for MCP operations
pub struct SafetyEnforcer {
    /// Maximum actions per minute
    max_actions_per_minute: u32,
    /// Maximum key presses per second
    max_key_presses_per_second: u32,
    /// Action counts per minute window
    action_counts: Mutex<RateLimitWindow>,
    /// Key press counts per second window
    key_counts: Mutex<RateLimitWindow>,
    /// Total operations performed
    total_operations: AtomicU64,
    /// Operations denied
    operations_denied: AtomicU64,
}

/// Sliding window for rate limiting
struct RateLimitWindow {
    counts: HashMap<u64, u32>,
    window_size_ms: u64,
}

impl RateLimitWindow {
    fn new(window_size_ms: u64) -> Self {
        Self {
            counts: HashMap::new(),
            window_size_ms,
        }
    }

    fn record(&mut self, now_ms: u64) -> u32 {
        // Clean old entries
        let cutoff = now_ms.saturating_sub(self.window_size_ms);
        self.counts.retain(|&ts, _| ts > cutoff);

        // Record new entry
        let bucket = now_ms / 1000; // 1-second buckets
        *self.counts.entry(bucket).or_insert(0) += 1;

        // Return total count in window
        self.counts.values().sum()
    }

    fn count(&self, now_ms: u64) -> u32 {
        let cutoff = now_ms.saturating_sub(self.window_size_ms);
        self.counts
            .iter()
            .filter(|(&ts, _)| ts > cutoff / 1000)
            .map(|(_, &c)| c)
            .sum()
    }
}

impl SafetyEnforcer {
    /// Create a new safety enforcer with default limits
    pub fn new() -> Self {
        Self::with_limits(60, 10) // 60 actions/min, 10 keys/sec
    }

    /// Create a safety enforcer with custom limits
    pub fn with_limits(max_actions_per_minute: u32, max_key_presses_per_second: u32) -> Self {
        Self {
            max_actions_per_minute,
            max_key_presses_per_second,
            action_counts: Mutex::new(RateLimitWindow::new(60_000)), // 1 minute
            key_counts: Mutex::new(RateLimitWindow::new(1_000)),     // 1 second
            total_operations: AtomicU64::new(0),
            operations_denied: AtomicU64::new(0),
        }
    }

    /// Check if an operation is allowed
    pub fn check_operation(&self, operation: &str) -> Result<(), SafetyError> {
        // Check if operation is in allowlist
        if !is_operation_allowed(operation) {
            self.operations_denied.fetch_add(1, Ordering::Relaxed);
            return Err(SafetyError::OperationForbidden(operation.to_string()));
        }

        // Check rate limits for action operations
        if is_action_operation(operation) {
            let now_ms = current_time_ms();
            let count = self.action_counts.lock().record(now_ms);

            if count > self.max_actions_per_minute {
                self.operations_denied.fetch_add(1, Ordering::Relaxed);
                return Err(SafetyError::RateLimitExceeded {
                    limit: self.max_actions_per_minute,
                    window: "minute",
                });
            }
        }

        // Extra rate limit for key presses
        if operation == "press_key" {
            let now_ms = current_time_ms();
            let count = self.key_counts.lock().record(now_ms);

            if count > self.max_key_presses_per_second {
                self.operations_denied.fetch_add(1, Ordering::Relaxed);
                return Err(SafetyError::RateLimitExceeded {
                    limit: self.max_key_presses_per_second,
                    window: "second",
                });
            }
        }

        self.total_operations.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Validate parameters for an operation
    pub fn validate_params(
        &self,
        operation: &str,
        params: &serde_json::Value,
    ) -> Result<(), SafetyError> {
        match operation {
            "set_clipboard" => {
                // Limit clipboard text length
                if let Some(text) = params.get("text").and_then(|t| t.as_str()) {
                    if text.len() > 10240 {
                        return Err(SafetyError::InvalidParams(
                            "Clipboard text exceeds 10KB limit".to_string(),
                        ));
                    }
                }
            }
            "get_window_tree" => {
                // Enforce max depth
                if let Some(depth) = params.get("max_depth").and_then(|d| d.as_u64()) {
                    if depth > 10 {
                        return Err(SafetyError::InvalidParams(
                            "max_depth cannot exceed 10".to_string(),
                        ));
                    }
                }
            }
            "get_patterns" => {
                // Enforce max hours_back
                if let Some(hours) = params.get("hours_back").and_then(|h| h.as_u64()) {
                    if hours > 168 {
                        return Err(SafetyError::InvalidParams(
                            "hours_back cannot exceed 168 (1 week)".to_string(),
                        ));
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Get statistics about safety enforcement
    pub fn stats(&self) -> SafetyStats {
        SafetyStats {
            total_operations: self.total_operations.load(Ordering::Relaxed),
            operations_denied: self.operations_denied.load(Ordering::Relaxed),
            current_action_rate: self.action_counts.lock().count(current_time_ms()),
            current_key_rate: self.key_counts.lock().count(current_time_ms()),
            max_actions_per_minute: self.max_actions_per_minute,
            max_key_presses_per_second: self.max_key_presses_per_second,
        }
    }

    /// Reset rate limit counters
    pub fn reset_counters(&self) {
        self.action_counts.lock().counts.clear();
        self.key_counts.lock().counts.clear();
    }
}

impl Default for SafetyEnforcer {
    fn default() -> Self {
        Self::new()
    }
}

/// Safety enforcement statistics
#[derive(Debug, Clone)]
pub struct SafetyStats {
    pub total_operations: u64,
    pub operations_denied: u64,
    pub current_action_rate: u32,
    pub current_key_rate: u32,
    pub max_actions_per_minute: u32,
    pub max_key_presses_per_second: u32,
}

/// Safety errors
#[derive(Debug, thiserror::Error)]
pub enum SafetyError {
    #[error("Operation forbidden: {0}")]
    OperationForbidden(String),

    #[error("Rate limit exceeded: {limit} per {window}")]
    RateLimitExceeded { limit: u32, window: &'static str },

    #[error("Invalid parameters: {0}")]
    InvalidParams(String),
}

/// Check if an operation is in the allowlist
fn is_operation_allowed(operation: &str) -> bool {
    const ALLOWED: &[&str] = &[
        // Read-only operations (always safe)
        "list_windows",
        "get_window_tree",
        "get_patterns",
        "get_transitions",
        "tools/list",
        // Restricted action operations (rate limited)
        "activate_element",
        "press_key",
        "set_clipboard",
    ];

    ALLOWED.contains(&operation)
}

/// Check if an operation is an action (vs read-only)
fn is_action_operation(operation: &str) -> bool {
    const ACTIONS: &[&str] = &["activate_element", "press_key", "set_clipboard"];
    ACTIONS.contains(&operation)
}

/// Get current time in milliseconds
fn current_time_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allowed_operations() {
        let enforcer = SafetyEnforcer::new();

        assert!(enforcer.check_operation("list_windows").is_ok());
        assert!(enforcer.check_operation("get_window_tree").is_ok());
        assert!(enforcer.check_operation("get_patterns").is_ok());
        assert!(enforcer.check_operation("activate_element").is_ok());
        assert!(enforcer.check_operation("press_key").is_ok());
    }

    #[test]
    fn test_forbidden_operations() {
        let enforcer = SafetyEnforcer::new();

        assert!(enforcer.check_operation("move_mouse").is_err());
        assert!(enforcer.check_operation("execute_shell").is_err());
        assert!(enforcer.check_operation("send_text").is_err());
        assert!(enforcer.check_operation("read_clipboard").is_err());
        assert!(enforcer.check_operation("capture_screen").is_err());
    }

    #[test]
    fn test_rate_limiting() {
        let enforcer = SafetyEnforcer::with_limits(5, 2);

        // Should allow up to limit
        for _ in 0..5 {
            assert!(enforcer.check_operation("activate_element").is_ok());
        }

        // Should deny after limit
        let result = enforcer.check_operation("activate_element");
        assert!(matches!(
            result,
            Err(SafetyError::RateLimitExceeded { .. })
        ));
    }

    #[test]
    fn test_param_validation() {
        let enforcer = SafetyEnforcer::new();

        // Valid params
        let params = serde_json::json!({ "max_depth": 5 });
        assert!(enforcer.validate_params("get_window_tree", &params).is_ok());

        // Invalid params (depth too high)
        let params = serde_json::json!({ "max_depth": 20 });
        assert!(enforcer
            .validate_params("get_window_tree", &params)
            .is_err());

        // Invalid params (text too long)
        let long_text = "x".repeat(20000);
        let params = serde_json::json!({ "text": long_text });
        assert!(enforcer.validate_params("set_clipboard", &params).is_err());
    }
}
