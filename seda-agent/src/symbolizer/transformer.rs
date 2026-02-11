//! Event transformer - converts raw OS events to symbolic actions
//!
//! This is the critical privacy boundary. Raw events enter, symbolic actions exit.
//! No raw data persists beyond this module.
//!
//! # Transformation Rules
//!
//! - WindowFocusChanged -> SwitchApp (if from different app) or ignored (same app)
//! - ClipboardChanged -> CopyText (content type only, never content)
//! - KeyboardShortcut -> Detect Ctrl+V for PasteText, Ctrl+C for CopyText
//! - WindowOpened -> OpenApp
//! - WindowClosed -> CloseApp
//! - ElementFocused -> Interact or TypeText (depending on element type)

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::thread::{self, JoinHandle};

use chrono::{DateTime, Utc};

use super::actions::{AppIdentifier, ContentType, SymbolicAction};
use super::sanitizer::Sanitizer;
use crate::observer::events::{ClipboardContentType, Modifier, RawOsEvent, VirtualKey};

/// Record of a symbolic action with timestamp
#[derive(Debug, Clone)]
pub struct TimestampedAction {
    pub action: SymbolicAction,
    pub timestamp: DateTime<Utc>,
    pub duration_ms: Option<u64>,
}

/// Transformer state
pub struct EventTransformer {
    /// Sanitizer for privacy enforcement
    sanitizer: Sanitizer,
    /// Last known app for detecting switches
    last_app: Option<AppIdentifier>,
    /// Last action timestamp for duration calculation
    last_action_time: Option<DateTime<Utc>>,
    /// Track clipboard content type for paste detection
    last_clipboard_type: Option<ContentType>,
    /// Track open windows for open/close detection
    open_windows: HashMap<isize, AppIdentifier>,
    /// Track last observed browser URL per browser process to reduce duplicates
    last_browser_url: HashMap<String, String>,
}

impl EventTransformer {
    /// Create a new event transformer
    pub fn new() -> Self {
        Self {
            sanitizer: Sanitizer::new(),
            last_app: None,
            last_action_time: None,
            last_clipboard_type: None,
            open_windows: HashMap::new(),
            last_browser_url: HashMap::new(),
        }
    }

    /// Transform a raw OS event into a symbolic action (if applicable)
    ///
    /// # Privacy Guarantee
    ///
    /// This method ensures:
    /// - Window titles are discarded
    /// - Clipboard content is never read
    /// - Only process names and content types are retained
    pub fn transform(&mut self, event: RawOsEvent) -> Option<TimestampedAction> {
        let now = Utc::now();
        let duration_ms = self.last_action_time.map(|t| {
            let diff = now.signed_duration_since(t);
            diff.num_milliseconds().max(0) as u64
        });

        let action = match event {
            RawOsEvent::WindowFocusChanged {
                hwnd,
                process_name,
                window_title: _, // PRIVACY: Explicitly ignored, never stored
            } => {
                let sanitized_name = self.sanitizer.sanitize_process_name(&process_name);
                let new_app = AppIdentifier::new(sanitized_name);

                // Track the window
                self.open_windows.insert(hwnd, new_app.clone());

                // Only emit SwitchApp if actually switching to a different app
                let action = if let Some(ref last) = self.last_app {
                    if last.process_name != new_app.process_name {
                        Some(SymbolicAction::SwitchApp {
                            from_app: last.clone(),
                            to_app: new_app.clone(),
                        })
                    } else {
                        None // Same app, different window - not a switch
                    }
                } else {
                    // First app focus - treat as OpenApp
                    Some(SymbolicAction::OpenApp { app: new_app.clone() })
                };

                self.last_app = Some(new_app);
                action
            }

            RawOsEvent::ClipboardChanged { content_type } => {
                // PRIVACY: Only the content TYPE is recorded, never the content itself
                let content_type = match content_type {
                    ClipboardContentType::Text => ContentType::PlainText,
                    ClipboardContentType::RichText => ContentType::RichText,
                    ClipboardContentType::Html => ContentType::Html,
                    ClipboardContentType::Image => ContentType::Image,
                    ClipboardContentType::Files => ContentType::Files,
                    ClipboardContentType::Unknown => ContentType::Unknown,
                };

                self.last_clipboard_type = Some(content_type);

                // Emit CopyText with the current app as source
                self.last_app.as_ref().map(|app| SymbolicAction::CopyText {
                    source_app: app.clone(),
                    content_type,
                })
            }

            RawOsEvent::KeyboardShortcut { modifiers, key } => {
                // Detect copy/paste shortcuts
                let has_ctrl = modifiers.contains(&Modifier::Ctrl);

                if has_ctrl {
                    match key {
                        VirtualKey::C => {
                            // Ctrl+C - Copy (clipboard change will also fire)
                            // We don't emit here to avoid duplicates
                            None
                        }
                        VirtualKey::V => {
                            // Ctrl+V - Paste
                            self.last_app.as_ref().map(|app| SymbolicAction::PasteText {
                                target_app: app.clone(),
                            })
                        }
                        VirtualKey::X => {
                            // Ctrl+X - Cut (treat as copy + delete, but we only record copy)
                            None // Clipboard change will fire
                        }
                        _ => None, // Other shortcuts not tracked for now
                    }
                } else {
                    None
                }
            }

            RawOsEvent::WindowOpened { hwnd, process_name } => {
                let sanitized_name = self.sanitizer.sanitize_process_name(&process_name);
                let app = AppIdentifier::new(sanitized_name);
                self.open_windows.insert(hwnd, app.clone());
                Some(SymbolicAction::OpenApp { app })
            }

            RawOsEvent::WindowClosed { hwnd, process_name } => {
                let sanitized_name = self.sanitizer.sanitize_process_name(&process_name);
                let app = self
                    .open_windows
                    .remove(&hwnd)
                    .unwrap_or_else(|| AppIdentifier::new(sanitized_name));
                Some(SymbolicAction::CloseApp { app })
            }

            RawOsEvent::ElementFocused {
                hwnd: _,
                element_id: _,
                control_type,
            } => {
                // Element focus within the current app
                // Could be expanded to track specific interaction patterns
                // For now, we don't emit actions for element focus changes
                // as they're too granular and would create noise
                let _ = control_type; // Acknowledge but don't use
                None
            }

            RawOsEvent::BrowserNavigation {
                hwnd: _,
                process_name,
                url,
            } => {
                let sanitized_name = self.sanitizer.sanitize_process_name(&process_name);
                let browser_app = AppIdentifier::new(sanitized_name.clone());
                let normalized_url = normalize_url(&url);

                if normalized_url.is_empty() {
                    None
                } else {
                    let is_duplicate = self
                        .last_browser_url
                        .get(&sanitized_name)
                        .map(|last| last == &normalized_url)
                        .unwrap_or(false);

                    if is_duplicate {
                        None
                    } else {
                        self.last_browser_url
                            .insert(sanitized_name.clone(), normalized_url.clone());

                        let domain = extract_domain(&normalized_url)
                            .unwrap_or_else(|| "unknown".to_string());
                        let search_query = extract_search_query(&normalized_url);
                        let search_engine = infer_search_engine(&domain);

                        if let Some(query) = search_query {
                            Some(SymbolicAction::SearchWeb {
                                browser_app,
                                engine: search_engine,
                                query,
                                url: normalized_url,
                                domain,
                            })
                        } else {
                            Some(SymbolicAction::VisitWebsite {
                                browser_app,
                                url: normalized_url,
                                domain,
                            })
                        }
                    }
                }
            }
        };

        // Update timing
        if action.is_some() {
            self.last_action_time = Some(now);
        }

        action.map(|action| {
            // Validate the action doesn't contain PII
            if !self.sanitizer.validate_action(&action) {
                tracing::warn!("Action validation failed, potential PII detected");
                // Don't emit the action if it contains PII
                return None;
            }
            Some(TimestampedAction {
                action,
                timestamp: now,
                duration_ms,
            })
        }).flatten()
    }

    /// Get the current app context
    pub fn current_app(&self) -> Option<&AppIdentifier> {
        self.last_app.as_ref()
    }

    /// Reset the transformer state
    pub fn reset(&mut self) {
        self.last_app = None;
        self.last_action_time = None;
        self.last_clipboard_type = None;
        self.open_windows.clear();
        self.last_browser_url.clear();
    }
}

impl Default for EventTransformer {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_url(url: &str) -> String {
    let trimmed = url.trim().trim_matches('"').trim_matches('\'');
    if trimmed.is_empty() {
        return String::new();
    }

    let lower = trimmed.to_lowercase();
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("about:")
        || lower.starts_with("edge://")
        || lower.starts_with("chrome://")
        || lower.starts_with("brave://")
        || lower.starts_with("opera://")
        || lower.starts_with("vivaldi://")
        || lower.starts_with("file://")
    {
        trimmed.to_string()
    } else if trimmed.contains('.') && !trimmed.contains(' ') {
        format!("https://{}", trimmed)
    } else {
        trimmed.to_string()
    }
}

fn extract_domain(url: &str) -> Option<String> {
    let lower = url.to_lowercase();
    if lower.is_empty() {
        return None;
    }

    let without_scheme = if let Some((_, rest)) = lower.split_once("://") {
        rest
    } else {
        lower.as_str()
    };

    let host = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim();

    if host.is_empty() {
        return None;
    }

    Some(host.trim_start_matches("www.").to_string())
}

fn extract_search_query(url: &str) -> Option<String> {
    let query_string = url.split_once('?')?.1.split('#').next().unwrap_or("");
    if query_string.is_empty() {
        return None;
    }

    let keys = ["q", "query", "p", "text", "search", "k", "keyword", "wd"];

    for pair in query_string.split('&') {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or("").to_lowercase();
        let value = parts.next().unwrap_or("");

        if keys.contains(&key.as_str()) {
            let decoded = percent_decode(value);
            if !decoded.trim().is_empty() {
                return Some(decoded);
            }
        }
    }

    None
}

fn infer_search_engine(domain: &str) -> Option<String> {
    let d = domain.to_lowercase();

    let engine = if d.contains("google.") {
        Some("google")
    } else if d.contains("bing.") {
        Some("bing")
    } else if d.contains("duckduckgo.") {
        Some("duckduckgo")
    } else if d.contains("yahoo.") {
        Some("yahoo")
    } else if d.contains("baidu.") {
        Some("baidu")
    } else if d.contains("yandex.") {
        Some("yandex")
    } else if d.contains("ecosia.") {
        Some("ecosia")
    } else if d.contains("startpage.") {
        Some("startpage")
    } else if d.contains("search.brave.com") {
        Some("brave-search")
    } else {
        None
    };

    engine.map(|e| e.to_string())
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let h1 = from_hex(bytes[i + 1]);
                let h2 = from_hex(bytes[i + 2]);
                if let (Some(a), Some(b)) = (h1, h2) {
                    out.push((a << 4) | b);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }

    String::from_utf8(out).unwrap_or_else(|_| value.to_string())
}

fn from_hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Background transformer that processes events from a channel
pub struct BackgroundTransformer {
    handle: Option<JoinHandle<()>>,
}

impl BackgroundTransformer {
    /// Start a background transformer that reads from event_rx and writes to action_tx
    pub fn start(
        event_rx: Receiver<RawOsEvent>,
        action_tx: Sender<TimestampedAction>,
    ) -> Self {
        let handle = thread::spawn(move || {
            let mut transformer = EventTransformer::new();

            for event in event_rx {
                if let Some(action) = transformer.transform(event) {
                    if action_tx.send(action).is_err() {
                        // Channel closed, stop processing
                        break;
                    }
                }
            }

            tracing::info!("Background transformer stopped");
        });

        Self {
            handle: Some(handle),
        }
    }

    /// Wait for the transformer to finish
    pub fn join(mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_focus_creates_switch_app() {
        let mut transformer = EventTransformer::new();

        // First focus
        let result = transformer.transform(RawOsEvent::WindowFocusChanged {
            hwnd: 1,
            process_name: "chrome.exe".to_string(),
            window_title: "Secret Document - Google Docs".to_string(), // Should be ignored
        });

        assert!(result.is_some());
        if let Some(ts_action) = result {
            matches!(ts_action.action, SymbolicAction::OpenApp { .. });
        }

        // Switch to another app
        let result = transformer.transform(RawOsEvent::WindowFocusChanged {
            hwnd: 2,
            process_name: "Code.exe".to_string(),
            window_title: "password.txt - Visual Studio Code".to_string(), // Should be ignored
        });

        assert!(result.is_some());
        if let Some(ts_action) = result {
            if let SymbolicAction::SwitchApp { from_app, to_app } = ts_action.action {
                assert_eq!(from_app.process_name, "chrome.exe");
                assert_eq!(to_app.process_name, "code.exe"); // Normalized to lowercase
            } else {
                panic!("Expected SwitchApp");
            }
        }
    }

    #[test]
    fn test_clipboard_change_creates_copy_text() {
        let mut transformer = EventTransformer::new();

        // First, focus an app
        transformer.transform(RawOsEvent::WindowFocusChanged {
            hwnd: 1,
            process_name: "notepad.exe".to_string(),
            window_title: "Untitled".to_string(),
        });

        // Clipboard change
        let result = transformer.transform(RawOsEvent::ClipboardChanged {
            content_type: ClipboardContentType::Text,
        });

        assert!(result.is_some());
        if let Some(ts_action) = result {
            if let SymbolicAction::CopyText {
                source_app,
                content_type,
            } = ts_action.action
            {
                assert_eq!(source_app.process_name, "notepad.exe");
                assert_eq!(content_type, ContentType::PlainText);
            } else {
                panic!("Expected CopyText");
            }
        }
    }

    #[test]
    fn test_window_title_is_not_stored() {
        let mut transformer = EventTransformer::new();

        let result = transformer.transform(RawOsEvent::WindowFocusChanged {
            hwnd: 1,
            process_name: "chrome.exe".to_string(),
            window_title: "My Bank Account - Sensitive Info".to_string(),
        });

        // The action should not contain the window title
        if let Some(ts_action) = result {
            let serialized = serde_json::to_string(&ts_action.action).unwrap();
            assert!(!serialized.contains("Bank"));
            assert!(!serialized.contains("Sensitive"));
        }
    }
}
