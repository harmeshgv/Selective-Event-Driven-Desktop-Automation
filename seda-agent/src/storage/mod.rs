//! Local SQLite storage
//!
//! Persists symbolic actions and task graph data locally.
//!
//! # Safety Decision
//!
//! - Only symbolic actions are stored, never raw data
//! - Database is stored in user's local app data directory
//! - No network access from this module
//! - Schema enforces JSON validity of action data

pub mod migrations;
pub mod repository;
pub mod schema;

pub use repository::Repository;
pub use schema::*;
