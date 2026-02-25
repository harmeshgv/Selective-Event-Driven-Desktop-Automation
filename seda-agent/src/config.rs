//! Configuration management for the SEDA agent.
//!
//! # Safety Decision
//!
//! Configuration is loaded from environment variables and defaults.
//! No configuration file is read from disk to minimize file system access.

use std::env;
use std::fs;
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

    /// LLM provider: disabled, groq, or ollama
    pub llm_provider: String,

    /// Model name for the configured LLM provider
    pub llm_model: String,

    /// Optional provider endpoint override
    pub llm_base_url: Option<String>,

    /// LLM request timeout in seconds
    pub llm_timeout_seconds: u64,

    /// Minimum repeated task length to enable AI assist
    pub automation_min_steps: usize,
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
            llm_provider: "disabled".to_string(),
            llm_model: String::new(),
            llm_base_url: None,
            llm_timeout_seconds: 30,
            automation_min_steps: 15,
        }
    }
}

impl Config {
    /// Load configuration from environment variables with defaults
    pub fn from_env() -> Self {
        load_dotenv_files();

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

        if let Ok(provider) = env::var("SEDA_LLM_PROVIDER") {
            let normalized = provider.trim().to_ascii_lowercase();
            if matches!(normalized.as_str(), "disabled" | "groq" | "ollama") {
                config.llm_provider = normalized;
            }
        }

        if let Ok(model) = env::var("SEDA_LLM_MODEL") {
            let trimmed = model.trim();
            if !trimmed.is_empty() {
                config.llm_model = trimmed.to_string();
            }
        }

        if let Ok(base_url) = env::var("SEDA_LLM_BASE_URL") {
            let trimmed = base_url.trim();
            config.llm_base_url = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
        }

        if let Ok(timeout) = env::var("SEDA_LLM_TIMEOUT_SECONDS") {
            if let Ok(seconds) = timeout.parse::<u64>() {
                config.llm_timeout_seconds = seconds.max(5);
            }
        }

        if let Ok(min_steps) = env::var("SEDA_AUTOMATION_MIN_STEPS") {
            if let Ok(steps) = min_steps.parse::<usize>() {
                config.automation_min_steps = steps.max(2);
            }
        }

        config
    }
}

/// Load `.env` values into process environment if present.
///
/// Resolution order:
/// 1. `.env` in current working directory
/// 2. `.env` in parent directory
///
/// Existing environment variables are never overwritten.
fn load_dotenv_files() {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd.join(".env"));
        if let Some(parent) = cwd.parent() {
            candidates.push(parent.join(".env"));
        }
    }

    for path in candidates {
        if path.exists() {
            let _ = load_dotenv_file(&path);
        }
    }
}

fn load_dotenv_file(path: &PathBuf) -> Result<(), std::io::Error> {
    let content = fs::read_to_string(path)?;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let line = if let Some(rest) = line.strip_prefix("export ") {
            rest.trim_start()
        } else {
            line
        };

        let Some((key_raw, value_raw)) = line.split_once('=') else {
            continue;
        };

        let key = key_raw.trim();
        if key.is_empty() || env::var_os(key).is_some() {
            continue;
        }

        let value = parse_dotenv_value(value_raw.trim());
        env::set_var(key, value);
    }

    Ok(())
}

fn parse_dotenv_value(raw: &str) -> String {
    let value = raw.trim();

    if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
        return parse_double_quoted(&value[1..value.len() - 1]);
    }

    if value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2 {
        return value[1..value.len() - 1].to_string();
    }

    // Strip inline comments for unquoted values: KEY=value # comment
    let mut unquoted = value.to_string();
    if let Some(idx) = unquoted.find(" #") {
        unquoted.truncate(idx);
    }
    unquoted.trim_end().to_string()
}

fn parse_double_quoted(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => output.push('\n'),
                Some('r') => output.push('\r'),
                Some('t') => output.push('\t'),
                Some('"') => output.push('"'),
                Some('\\') => output.push('\\'),
                Some(other) => {
                    output.push('\\');
                    output.push(other);
                }
                None => output.push('\\'),
            }
        } else {
            output.push(ch);
        }
    }

    output
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
