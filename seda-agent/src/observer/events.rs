//! Raw OS event types
//!
//! These are the events captured from the OS before symbolization.
//!
//! # Safety Decision
//!
//! RawOsEvent contains the minimum information needed for symbolization.
//! - Window titles ARE captured here but will be discarded during symbolization
//! - Clipboard CONTENT is never captured, only the content type
//! - Keystroke content is never captured, only modifier+key for shortcuts

use serde::{Deserialize, Serialize};

/// Types of clipboard content (not the content itself)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClipboardContentType {
    /// Plain text
    Text,
    /// Rich text (RTF)
    RichText,
    /// HTML content
    Html,
    /// Image data
    Image,
    /// File(s)
    Files,
    /// Unknown or unsupported format
    Unknown,
}

/// Keyboard modifier keys
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Win,
}

/// Virtual key codes for keyboard shortcuts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VirtualKey {
    // Letters
    A, B, C, D, E, F, G, H, I, J, K, L, M,
    N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    // Numbers
    Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9,
    // Function keys
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    // Navigation
    Enter, Tab, Escape, Space, Backspace,
    ArrowUp, ArrowDown, ArrowLeft, ArrowRight,
    Home, End, PageUp, PageDown,
    Insert, Delete,
    // Other
    PrintScreen,
}

/// Raw OS events captured before symbolization
///
/// # Privacy Note
///
/// This enum is designed to capture the minimum information needed.
/// Window titles are included here but will be stripped during symbolization.
/// Clipboard and keystroke content is NEVER captured.
#[derive(Debug, Clone)]
pub enum RawOsEvent {
    /// Window focus changed
    ///
    /// # Fields
    /// - `hwnd`: Window handle (for UI Automation queries)
    /// - `process_name`: Name of the process (e.g., "chrome.exe")
    /// - `window_title`: Title of the window (WILL BE DISCARDED after symbolization)
    WindowFocusChanged {
        hwnd: isize,
        process_name: String,
        window_title: String, // Privacy: Discarded during symbolization
    },

    /// Clipboard content changed
    ///
    /// # Safety
    /// Only the content TYPE is captured, never the actual content.
    ClipboardChanged {
        content_type: ClipboardContentType,
    },

    /// Keyboard shortcut detected
    ///
    /// # Safety
    /// Only system shortcuts (Ctrl+C, Ctrl+V, etc.) are captured.
    /// Regular typing is NEVER recorded.
    KeyboardShortcut {
        modifiers: Vec<Modifier>,
        key: VirtualKey,
    },

    /// A new window/application was opened
    WindowOpened {
        hwnd: isize,
        process_name: String,
    },

    /// A window/application was closed
    WindowClosed {
        hwnd: isize,
        process_name: String,
    },

    /// UI element focus changed within a window
    ElementFocused {
        hwnd: isize,
        element_id: String,
        control_type: String,
    },

    /// Browser navigation detected from URL bar value
    ///
    /// # Privacy Note
    ///
    /// This event intentionally captures URL text for explicit user-requested
    /// web activity tracking. This is not enabled by default in most privacy-first systems.
    BrowserNavigation {
        hwnd: isize,
        process_name: String,
        url: String,
    },
}

impl RawOsEvent {
    /// Get the process name associated with this event (if any)
    pub fn process_name(&self) -> Option<&str> {
        match self {
            RawOsEvent::WindowFocusChanged { process_name, .. } => Some(process_name),
            RawOsEvent::WindowOpened { process_name, .. } => Some(process_name),
            RawOsEvent::WindowClosed { process_name, .. } => Some(process_name),
            RawOsEvent::BrowserNavigation { process_name, .. } => Some(process_name),
            _ => None,
        }
    }

    /// Get the window handle associated with this event (if any)
    pub fn hwnd(&self) -> Option<isize> {
        match self {
            RawOsEvent::WindowFocusChanged { hwnd, .. } => Some(*hwnd),
            RawOsEvent::WindowOpened { hwnd, .. } => Some(*hwnd),
            RawOsEvent::WindowClosed { hwnd, .. } => Some(*hwnd),
            RawOsEvent::ElementFocused { hwnd, .. } => Some(*hwnd),
            RawOsEvent::BrowserNavigation { hwnd, .. } => Some(*hwnd),
            _ => None,
        }
    }
}
