//! Cross-cutting safety concerns
//!
//! This module enforces safety boundaries across the entire agent:
//! - Allowlist of permitted operations
//! - Rate limiting
//! - Audit logging
//!
//! # Core Principle
//!
//! If an operation is not explicitly allowed, it is forbidden.
//! This is the opposite of a blocklist approach - safer by default.

pub mod allowlist;
pub mod audit;

pub use allowlist::{ForbiddenOp, SafetyBoundary};
pub use audit::{AuditLog, AuditResult};
