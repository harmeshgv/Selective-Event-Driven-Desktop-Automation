//! Configuration management for the SEDA agent.
//!
//! # Safety Decision
//!
//! Configuration is loaded from environment variables and defaults.
//! No configuration file is read from disk to minimize file system access.

use std::env;
use std::path::PathBuf;

/// Agent configuration
#[derive(Debug, Clone)]
pub struct Config {
    /// Port for the MCP HTTP server (localhost only)
    pub mcp_port: u16,

    /// Path to the SQLite database
    pub database_path: PathBuf,

    /// Minimum pattern frequency to be considered a candidate
    pub min_pattern_frequency: u32,

    /// Maximum pattern length to detect
    pub max_pattern_length: usize,

    /// Time window (ms) to group related actions
    pub action_grouping_window_ms: u64,

    /// Maximum actions per minute (rate limiting)
    pub max_actions_per_minute: u32,

    /// Maximum key presses per second (rate limiting)
    pub max_key_presses_per_second: u32,

    /// Enable debug logging
    pub debug: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mcp_port: 9315,
            database_path: default_database_path(),
            min_pattern_frequency: 3,
            max_pattern_length: 10,
            action_grouping_window_ms: 5000,
            max_actions_per_minute: 60,
            max_key_presses_per_second: 10,
            debug: false,
        }
    }
}

impl Config {
    /// Load configuration from environment variables with defaults
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(port) = env::var("SEDA_MCP_PORT") {
            if let Ok(p) = port.parse() {
                config.mcp_port = p;
            }
        }

        if let Ok(path) = env::var("SEDA_DATABASE_PATH") {
            config.database_path = PathBuf::from(path);
        }

        if let Ok(freq) = env::var("SEDA_MIN_PATTERN_FREQUENCY") {
            if let Ok(f) = freq.parse() {
                config.min_pattern_frequency = f;
            }
        }

        if let Ok(len) = env::var("SEDA_MAX_PATTERN_LENGTH") {
            if let Ok(l) = len.parse() {
                config.max_pattern_length = l;
            }
        }

        if let Ok(window) = env::var("SEDA_ACTION_GROUPING_WINDOW_MS") {
            if let Ok(w) = window.parse() {
                config.action_grouping_window_ms = w;
            }
        }

        if env::var("SEDA_DEBUG").is_ok() {
            config.debug = true;
        }

        config
    }
}

/// Get the default database path in the user's local data directory
fn default_database_path() -> PathBuf {
    let base = env::var("LOCALAPPDATA")
        .or_else(|_| env::var("APPDATA"))
        .unwrap_or_else(|_| ".".to_string());

    PathBuf::from(base)
        .join("seda-agent")
        .join("seda.db")
}
