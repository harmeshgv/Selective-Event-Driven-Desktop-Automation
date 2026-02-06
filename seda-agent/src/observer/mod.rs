//! OS-level observation layer
//!
//! This module captures high-level OS events using Windows APIs:
//! - Window focus changes via SetWinEventHook
//! - Clipboard changes
//! - Application switches
//!
//! # Safety Decision
//!
//! Events are passed immediately to the symbolizer. The observer NEVER:
//! - Stores raw event data
//! - Records window content
//! - Captures screenshots
//! - Logs keystrokes (except shortcuts)

pub mod accessibility;
pub mod events;
pub mod window_manager;
pub mod windows;

pub use events::{ClipboardContentType, RawOsEvent};
pub use window_manager::WindowManager;
pub use windows::{ClipboardObserver, KeyboardObserver, WindowsObserver};

use std::sync::mpsc;
use thiserror::Error;

/// Errors that can occur during observation
#[derive(Error, Debug)]
pub enum ObserverError {
    #[error("Failed to set up Windows event hook: {0}")]
    HookSetupFailed(String),

    #[error("Failed to enumerate windows: {0}")]
    WindowEnumerationFailed(String),

    #[error("UI Automation error: {0}")]
    UiAutomationError(String),

    #[error("Observer thread error: {0}")]
    ThreadError(String),
}

/// Trait for platform-specific observer implementations
pub trait OsObserver: Send {
    /// Start observing OS events
    fn start(&mut self, event_sender: mpsc::Sender<RawOsEvent>) -> Result<(), ObserverError>;

    /// Stop observing
    fn stop(&mut self) -> Result<(), ObserverError>;

    /// Check if observer is running
    fn is_running(&self) -> bool;
}
