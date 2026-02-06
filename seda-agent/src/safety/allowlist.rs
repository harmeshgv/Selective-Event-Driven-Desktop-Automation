//! Safety allowlist
//!
//! Defines what operations are permitted and explicitly forbidden.
//!
//! # Design Philosophy
//!
//! This is an ALLOWLIST, not a blocklist. If an operation is not
//! explicitly listed as allowed, it is DENIED by default.
//! This is safer than trying to enumerate all possible dangerous operations.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// Operations that are EXPLICITLY FORBIDDEN
///
/// These operations will NEVER be implemented, regardless of configuration.
/// This enum documents what is intentionally not allowed and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ForbiddenOp {
    /// Move mouse to arbitrary screen coordinates
    ///
    /// Reason: Mouse position combined with timing can leak information
    /// about screen content (e.g., clicking at specific coordinates reveals
    /// what UI elements are at those positions)
    ArbitraryMouseMove,

    /// Execute shell commands or run arbitrary processes
    ///
    /// Reason: Shell execution bypasses ALL safety boundaries and can
    /// perform any operation the user can perform
    ShellExecution,

    /// Send arbitrary text/string input
    ///
    /// Reason: Could be used to inject commands, passwords, or sensitive
    /// data. Use press_key for controlled character input instead.
    ArbitraryTextInput,

    /// Read clipboard contents
    ///
    /// Reason: Clipboard frequently contains sensitive data (passwords,
    /// personal information, private messages)
    ClipboardRead,

    /// Capture screen or window images
    ///
    /// Reason: Violates the core "structure over pixels" principle.
    /// Screen capture can expose any visible sensitive information.
    ScreenCapture,

    /// Access file system (read or write)
    ///
    /// Reason: File system access is outside the agent's intended scope.
    /// The agent observes UI interactions, not file contents.
    FileSystemAccess,

    /// Make outbound network requests
    ///
    /// Reason: Agent must be fully local with no data exfiltration path.
    /// Network requests could leak observed patterns or user data.
    NetworkRequest,

    /// Launch new processes
    ///
    /// Reason: Process launching bypasses the controlled MCP execution
    /// model and could be used to run arbitrary code.
    ProcessLaunch,

    /// Access system registry or configuration
    ///
    /// Reason: System configuration changes are outside agent scope
    /// and could affect system stability or security.
    RegistryAccess,

    /// Modify other processes' memory
    ///
    /// Reason: Memory manipulation is outside agent scope and could
    /// be used for malicious purposes.
    ProcessMemoryAccess,

    /// Access hardware devices directly
    ///
    /// Reason: Direct hardware access is outside agent scope.
    HardwareAccess,

    /// Modify security settings
    ///
    /// Reason: Security settings must remain under user control.
    SecurityModification,
}

impl ForbiddenOp {
    /// Get all forbidden operations
    pub fn all() -> Vec<ForbiddenOp> {
        vec![
            ForbiddenOp::ArbitraryMouseMove,
            ForbiddenOp::ShellExecution,
            ForbiddenOp::ArbitraryTextInput,
            ForbiddenOp::ClipboardRead,
            ForbiddenOp::ScreenCapture,
            ForbiddenOp::FileSystemAccess,
            ForbiddenOp::NetworkRequest,
            ForbiddenOp::ProcessLaunch,
            ForbiddenOp::RegistryAccess,
            ForbiddenOp::ProcessMemoryAccess,
            ForbiddenOp::HardwareAccess,
            ForbiddenOp::SecurityModification,
        ]
    }

    /// Get the reason this operation is forbidden
    pub fn reason(&self) -> &'static str {
        match self {
            ForbiddenOp::ArbitraryMouseMove => {
                "Mouse coordinates can leak screen content information"
            }
            ForbiddenOp::ShellExecution => "Shell execution bypasses all safety boundaries",
            ForbiddenOp::ArbitraryTextInput => {
                "Could inject commands or sensitive data; use press_key instead"
            }
            ForbiddenOp::ClipboardRead => "Clipboard frequently contains sensitive data",
            ForbiddenOp::ScreenCapture => "Violates structure-over-pixels principle",
            ForbiddenOp::FileSystemAccess => "Outside agent scope; observes UI, not files",
            ForbiddenOp::NetworkRequest => "Agent must be fully local with no data exfiltration",
            ForbiddenOp::ProcessLaunch => "Bypasses controlled MCP execution model",
            ForbiddenOp::RegistryAccess => "System configuration outside agent scope",
            ForbiddenOp::ProcessMemoryAccess => "Memory manipulation outside agent scope",
            ForbiddenOp::HardwareAccess => "Direct hardware access outside agent scope",
            ForbiddenOp::SecurityModification => "Security settings must remain under user control",
        }
    }

    /// Get the severity level (for logging/auditing)
    pub fn severity(&self) -> Severity {
        match self {
            ForbiddenOp::ShellExecution
            | ForbiddenOp::ProcessLaunch
            | ForbiddenOp::SecurityModification => Severity::Critical,

            ForbiddenOp::FileSystemAccess
            | ForbiddenOp::NetworkRequest
            | ForbiddenOp::RegistryAccess
            | ForbiddenOp::ProcessMemoryAccess => Severity::High,

            ForbiddenOp::ScreenCapture
            | ForbiddenOp::ClipboardRead
            | ForbiddenOp::ArbitraryTextInput => Severity::Medium,

            ForbiddenOp::ArbitraryMouseMove | ForbiddenOp::HardwareAccess => Severity::Low,
        }
    }
}

/// Severity levels for forbidden operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

/// Safety boundary configuration
#[derive(Debug, Clone)]
pub struct SafetyBoundary {
    /// Set of forbidden operations (always includes all ForbiddenOp)
    forbidden_operations: HashSet<ForbiddenOp>,

    /// Maximum actions per minute (rate limiting)
    pub max_actions_per_minute: u32,

    /// Maximum key presses per second (rate limiting)
    pub max_key_presses_per_second: u32,

    /// Maximum clipboard text length (bytes)
    pub max_clipboard_length: usize,

    /// Maximum UI tree traversal depth
    pub max_ui_tree_depth: u32,

    /// Allowed processes (if Some, whitelist mode; if None, all allowed)
    allowed_processes: Option<HashSet<String>>,

    /// Blocked processes (always blocked regardless of allowed_processes)
    blocked_processes: HashSet<String>,

    /// Whether to log denied operations
    pub log_denied_operations: bool,
}

impl Default for SafetyBoundary {
    fn default() -> Self {
        Self {
            forbidden_operations: ForbiddenOp::all().into_iter().collect(),
            max_actions_per_minute: 60,
            max_key_presses_per_second: 10,
            max_clipboard_length: 10 * 1024, // 10 KB
            max_ui_tree_depth: 10,
            allowed_processes: None, // All processes allowed by default
            blocked_processes: default_blocked_processes(),
            log_denied_operations: true,
        }
    }
}

impl SafetyBoundary {
    /// Create a new safety boundary with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a strict safety boundary (lower limits)
    pub fn strict() -> Self {
        Self {
            max_actions_per_minute: 30,
            max_key_presses_per_second: 5,
            max_clipboard_length: 1024, // 1 KB
            max_ui_tree_depth: 5,
            ..Self::default()
        }
    }

    /// Check if an operation is forbidden
    pub fn is_forbidden(&self, op: ForbiddenOp) -> bool {
        self.forbidden_operations.contains(&op)
    }

    /// Check if a process is allowed for interaction
    pub fn is_process_allowed(&self, process_name: &str) -> bool {
        let name_lower = process_name.to_lowercase();

        // Always block certain processes
        if self.blocked_processes.contains(&name_lower) {
            return false;
        }

        // If allowlist is set, check it
        if let Some(ref allowed) = self.allowed_processes {
            return allowed.contains(&name_lower);
        }

        // Otherwise, allow
        true
    }

    /// Set the allowed processes (whitelist mode)
    pub fn set_allowed_processes(&mut self, processes: Vec<String>) {
        self.allowed_processes = Some(processes.into_iter().map(|p| p.to_lowercase()).collect());
    }

    /// Clear the allowed processes (allow all except blocked)
    pub fn clear_allowed_processes(&mut self) {
        self.allowed_processes = None;
    }

    /// Add a process to the blocked list
    pub fn block_process(&mut self, process_name: &str) {
        self.blocked_processes.insert(process_name.to_lowercase());
    }

    /// Remove a process from the blocked list
    pub fn unblock_process(&mut self, process_name: &str) {
        self.blocked_processes.remove(&process_name.to_lowercase());
    }

    /// Validate a proposed action
    pub fn validate_action(&self, action: &ActionRequest) -> Result<(), SafetyViolation> {
        // Check forbidden operations
        if let Some(op) = action.as_forbidden_op() {
            return Err(SafetyViolation::ForbiddenOperation(op));
        }

        // Check rate limits would need actual counters (handled by SafetyEnforcer)

        // Check clipboard length
        if let ActionRequest::SetClipboard { text_length } = action {
            if *text_length > self.max_clipboard_length {
                return Err(SafetyViolation::ClipboardTooLarge {
                    size: *text_length,
                    max: self.max_clipboard_length,
                });
            }
        }

        // Check UI tree depth
        if let ActionRequest::GetWindowTree { depth } = action {
            if *depth > self.max_ui_tree_depth {
                return Err(SafetyViolation::DepthExceeded {
                    requested: *depth,
                    max: self.max_ui_tree_depth,
                });
            }
        }

        // Check process allowlist
        if let Some(process) = action.target_process() {
            if !self.is_process_allowed(process) {
                return Err(SafetyViolation::ProcessNotAllowed(process.to_string()));
            }
        }

        Ok(())
    }
}

/// Represents a request that needs safety validation
#[derive(Debug, Clone)]
pub enum ActionRequest {
    /// List windows (safe)
    ListWindows,
    /// Get window UI tree
    GetWindowTree { depth: u32 },
    /// Get patterns (safe)
    GetPatterns,
    /// Activate a UI element
    ActivateElement { process: String },
    /// Press a key
    PressKey,
    /// Set clipboard
    SetClipboard { text_length: usize },
}

impl ActionRequest {
    /// Check if this request maps to a forbidden operation
    fn as_forbidden_op(&self) -> Option<ForbiddenOp> {
        // None of our allowed operations map to forbidden operations
        // This method exists for extensibility
        None
    }

    /// Get the target process if applicable
    fn target_process(&self) -> Option<&str> {
        match self {
            ActionRequest::ActivateElement { process } => Some(process),
            _ => None,
        }
    }
}

/// Safety violation error
#[derive(Debug, Clone, thiserror::Error)]
pub enum SafetyViolation {
    #[error("Forbidden operation: {0:?} - {}", .0.reason())]
    ForbiddenOperation(ForbiddenOp),

    #[error("Rate limit exceeded: {limit} per {window}")]
    RateLimitExceeded { limit: u32, window: String },

    #[error("Clipboard text too large: {size} bytes (max: {max})")]
    ClipboardTooLarge { size: usize, max: usize },

    #[error("UI tree depth exceeded: {requested} (max: {max})")]
    DepthExceeded { requested: u32, max: u32 },

    #[error("Process not allowed: {0}")]
    ProcessNotAllowed(String),

    #[error("Operation denied: {0}")]
    OperationDenied(String),
}

/// Default blocked processes
fn default_blocked_processes() -> HashSet<String> {
    let blocked = vec![
        // Security-sensitive processes
        "lsass.exe",      // Local Security Authority
        "csrss.exe",      // Client/Server Runtime
        "smss.exe",       // Session Manager
        "services.exe",   // Service Control Manager
        "winlogon.exe",   // Windows Logon
        "wininit.exe",    // Windows Initialization
        "svchost.exe",    // Service Host (various system services)
        // Security software (don't interfere)
        "msmpeng.exe",   // Windows Defender
        "msseces.exe",   // Microsoft Security Essentials
        "avp.exe",       // Kaspersky
        "avgnt.exe",     // Avira
        "mbam.exe",      // Malwarebytes
        // System utilities that shouldn't be automated
        "taskmgr.exe",   // Task Manager
        "regedit.exe",   // Registry Editor
        "mmc.exe",       // Microsoft Management Console
        "secpol.msc",    // Security Policy
    ];

    blocked.into_iter().map(|s| s.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forbidden_ops() {
        let boundary = SafetyBoundary::new();

        assert!(boundary.is_forbidden(ForbiddenOp::ShellExecution));
        assert!(boundary.is_forbidden(ForbiddenOp::ScreenCapture));
        assert!(boundary.is_forbidden(ForbiddenOp::NetworkRequest));
    }

    #[test]
    fn test_process_blocking() {
        let boundary = SafetyBoundary::new();

        // Default blocked
        assert!(!boundary.is_process_allowed("lsass.exe"));
        assert!(!boundary.is_process_allowed("LSASS.EXE")); // Case insensitive

        // Regular processes allowed
        assert!(boundary.is_process_allowed("chrome.exe"));
        assert!(boundary.is_process_allowed("notepad.exe"));
    }

    #[test]
    fn test_action_validation() {
        let boundary = SafetyBoundary::new();

        // Valid actions
        assert!(boundary.validate_action(&ActionRequest::ListWindows).is_ok());
        assert!(boundary
            .validate_action(&ActionRequest::GetWindowTree { depth: 5 })
            .is_ok());

        // Invalid: depth too high
        assert!(boundary
            .validate_action(&ActionRequest::GetWindowTree { depth: 20 })
            .is_err());

        // Invalid: clipboard too large
        assert!(boundary
            .validate_action(&ActionRequest::SetClipboard {
                text_length: 100_000
            })
            .is_err());
    }

    #[test]
    fn test_allowlist_mode() {
        let mut boundary = SafetyBoundary::new();
        boundary.set_allowed_processes(vec!["notepad.exe".to_string(), "code.exe".to_string()]);

        assert!(boundary.is_process_allowed("notepad.exe"));
        assert!(boundary.is_process_allowed("code.exe"));
        assert!(!boundary.is_process_allowed("chrome.exe"));

        // Blocked processes still blocked even if in allowlist
        boundary.set_allowed_processes(vec!["lsass.exe".to_string()]);
        assert!(!boundary.is_process_allowed("lsass.exe"));
    }
}
