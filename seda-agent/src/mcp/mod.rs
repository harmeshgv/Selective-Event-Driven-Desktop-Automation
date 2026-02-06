//! Model Context Protocol (MCP) Server
//!
//! Exposes a local HTTP server on localhost that provides:
//! - Read-only methods for window/element inspection
//! - Restricted action methods with explicit safety boundaries
//!
//! # Safety Design
//!
//! MCP is the boundary between the local agent and external LLMs.
//! The protocol explicitly forbids:
//! - Arbitrary mouse movement
//! - Shell/command execution
//! - File system access
//! - Arbitrary text input
//! - Screenshot capture
//!
//! All actions are:
//! - Rate limited
//! - Audited
//! - Verified against an allowlist

pub mod handlers;
pub mod safety;
pub mod schema;
pub mod server;
pub mod tools;

pub use safety::SafetyEnforcer;
pub use schema::*;
pub use server::McpServer;
pub use tools::get_tool_definitions;
