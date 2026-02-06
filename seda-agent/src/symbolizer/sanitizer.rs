//! PII/Content sanitization
//!
//! Ensures no personally identifiable information or sensitive content
//! is stored or transmitted.
//!
//! # Sanitization Rules
//!
//! 1. Window titles are NEVER stored (may contain document names, URLs, PII)
//! 2. Clipboard content is NEVER read or stored
//! 3. Process names are allowed (public information)
//! 4. Content types are allowed (metadata only)

use sha2::{Digest, Sha256};

/// Sanitizer for privacy-sensitive data
pub struct Sanitizer {
    /// List of patterns to redact from any strings that slip through
    redaction_patterns: Vec<regex::Regex>,
}

impl Sanitizer {
    /// Create a new sanitizer with default patterns
    pub fn new() -> Self {
        // These patterns catch common PII that might slip through
        let patterns = vec![
            // Email addresses
            r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}",
            // Phone numbers (various formats)
            r"\b\d{3}[-.]?\d{3}[-.]?\d{4}\b",
            // Credit card numbers
            r"\b\d{4}[-\s]?\d{4}[-\s]?\d{4}[-\s]?\d{4}\b",
            // SSN
            r"\b\d{3}[-]?\d{2}[-]?\d{4}\b",
            // IP addresses
            r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b",
            // File paths with usernames
            r"[Cc]:\\[Uu]sers\\[^\\]+",
            r"/[Hh]ome/[^/]+",
            r"/[Uu]sers/[^/]+",
        ];

        let redaction_patterns = patterns
            .iter()
            .filter_map(|p| regex::Regex::new(p).ok())
            .collect();

        Self { redaction_patterns }
    }

    /// Hash a string for privacy-safe identification
    ///
    /// Use this when you need to compare strings without storing the original.
    pub fn hash_string(&self, s: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(s.as_bytes());
        hex::encode(hasher.finalize())[..16].to_string()
    }

    /// Sanitize a process name
    ///
    /// Process names are generally safe, but we:
    /// 1. Remove any path components
    /// 2. Normalize the extension
    pub fn sanitize_process_name(&self, name: &str) -> String {
        // Extract just the filename
        let filename = name
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(name)
            .to_lowercase();

        // Keep only alphanumeric, dots, and underscores
        filename
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '_' || *c == '-')
            .collect()
    }

    /// Redact any PII from a string (for error messages, logs, etc.)
    ///
    /// This is a fallback - ideally PII never reaches this point.
    pub fn redact_pii(&self, text: &str) -> String {
        let mut result = text.to_string();

        for pattern in &self.redaction_patterns {
            result = pattern.replace_all(&result, "[REDACTED]").to_string();
        }

        result
    }

    /// Check if a string contains potential PII
    pub fn contains_pii(&self, text: &str) -> bool {
        self.redaction_patterns.iter().any(|p| p.is_match(text))
    }

    /// Validate that a symbolic action doesn't contain PII
    ///
    /// Returns true if the action is safe, false if it contains potential PII.
    pub fn validate_action(&self, action: &super::SymbolicAction) -> bool {
        match action {
            super::SymbolicAction::SwitchApp { from_app, to_app } => {
                !self.contains_pii(&from_app.process_name)
                    && !self.contains_pii(&to_app.process_name)
            }
            super::SymbolicAction::OpenApp { app }
            | super::SymbolicAction::CloseApp { app }
            | super::SymbolicAction::PasteText { target_app: app } => {
                !self.contains_pii(&app.process_name)
            }
            super::SymbolicAction::CopyText { source_app, .. } => {
                !self.contains_pii(&source_app.process_name)
            }
            super::SymbolicAction::TypeText { target_app, .. } => {
                !self.contains_pii(&target_app.process_name)
            }
            super::SymbolicAction::Navigate { app, .. } => !self.contains_pii(&app.process_name),
            super::SymbolicAction::Interact { app, .. } => !self.contains_pii(&app.process_name),
        }
    }
}

impl Default for Sanitizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_name_sanitization() {
        let sanitizer = Sanitizer::new();

        assert_eq!(
            sanitizer.sanitize_process_name("C:\\Program Files\\Chrome\\chrome.exe"),
            "chrome.exe"
        );
        assert_eq!(
            sanitizer.sanitize_process_name("/usr/bin/firefox"),
            "firefox"
        );
        assert_eq!(
            sanitizer.sanitize_process_name("Code.exe"),
            "code.exe"
        );
    }

    #[test]
    fn test_pii_detection() {
        let sanitizer = Sanitizer::new();

        assert!(sanitizer.contains_pii("Contact me at user@example.com"));
        assert!(sanitizer.contains_pii("Call 555-123-4567"));
        assert!(sanitizer.contains_pii("C:\\Users\\JohnDoe\\Documents"));
        assert!(!sanitizer.contains_pii("chrome.exe"));
        assert!(!sanitizer.contains_pii("notepad.exe"));
    }

    #[test]
    fn test_pii_redaction() {
        let sanitizer = Sanitizer::new();

        let redacted = sanitizer.redact_pii("Email: user@example.com");
        assert!(!redacted.contains("user@example.com"));
        assert!(redacted.contains("[REDACTED]"));
    }
}
