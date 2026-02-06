//! SEDA Agent - Selective Event-Driven Desktop Automation
//!
//! A privacy-first local agent that observes OS-level user behavior,
//! discovers repeated action patterns, and exposes them via MCP for
//! AI-assisted automation suggestions.
//!
//! # Privacy Principles
//!
//! - **Local-first**: All processing happens locally, no cloud dependency
//! - **Structure over pixels**: Uses accessibility APIs, not screen recording
//! - **Immediate symbolization**: Raw events are converted to symbolic actions instantly
//! - **No raw text storage**: Window titles, clipboard content never stored
//!
//! # Safety Boundaries
//!
//! - LLMs can only read and plan via MCP, never execute directly
//! - Explicit allowlist of permitted operations
//! - Forbidden: arbitrary mouse movement, shell execution, file system access
//! - All actions are audited

pub mod config;
pub mod graph;
pub mod mcp;
pub mod mining;
pub mod observer;
pub mod safety;
pub mod storage;
pub mod symbolizer;

pub use config::Config;
