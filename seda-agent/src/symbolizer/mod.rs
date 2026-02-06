//! Event symbolization - the privacy boundary
//!
//! This module transforms raw OS events into symbolic actions.
//! This is the critical privacy boundary where:
//! - Window titles are discarded (may contain PII)
//! - Clipboard content is reduced to content type only
//! - Only process names are retained (not arguments)
//!
//! # Privacy Guarantee
//!
//! After symbolization, no raw user data exists in the system.
//! The SymbolicAction enum is designed to be privacy-safe by construction.

pub mod actions;
pub mod sanitizer;
pub mod transformer;

pub use actions::{AppCategory, AppIdentifier, ContentType, SymbolicAction, SymbolicActionType};
pub use sanitizer::Sanitizer;
pub use transformer::EventTransformer;
