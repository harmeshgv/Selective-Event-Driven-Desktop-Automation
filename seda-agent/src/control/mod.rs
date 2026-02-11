//! Collector lifecycle control
//!
//! Provides start/stop control over OS observers and tracks collection sessions
//! for UI visibility.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;

use crate::observer::events::RawOsEvent;
use crate::observer::windows::{
    BrowserUrlObserver, ClipboardObserver, KeyboardObserver, WindowsObserver,
};
use crate::observer::OsObserver;
use crate::storage::Repository;

#[derive(Debug, Clone, Serialize)]
pub struct CompletedCollectionSession {
    pub id: String,
    pub started_ms: i64,
    pub stopped_ms: i64,
    pub actions_collected: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActiveCollectionSession {
    pub id: String,
    pub started_ms: i64,
    pub actions_collected: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CollectorSnapshot {
    pub collecting: bool,
    pub total_actions: i64,
    pub active_session: Option<ActiveCollectionSession>,
    pub completed_sessions: Vec<CompletedCollectionSession>,
}

#[derive(Debug, Clone)]
struct PendingSession {
    id: String,
    started_ms: i64,
    actions_before: i64,
}

/// Controls observer lifecycle and keeps lightweight collection history.
pub struct CollectionController {
    windows_observer: WindowsObserver,
    clipboard_observer: ClipboardObserver,
    keyboard_observer: KeyboardObserver,
    browser_url_observer: BrowserUrlObserver,
    event_sender: Sender<RawOsEvent>,
    repository: Arc<Mutex<Repository>>,
    collecting: bool,
    pending_session: Option<PendingSession>,
    completed_sessions: Vec<CompletedCollectionSession>,
}

impl CollectionController {
    pub fn new(event_sender: Sender<RawOsEvent>, repository: Arc<Mutex<Repository>>) -> Self {
        Self {
            windows_observer: WindowsObserver::new(),
            clipboard_observer: ClipboardObserver::new(),
            keyboard_observer: KeyboardObserver::new(),
            browser_url_observer: BrowserUrlObserver::new(),
            event_sender,
            repository,
            collecting: false,
            pending_session: None,
            completed_sessions: Vec::new(),
        }
    }

    pub fn start_collection(&mut self) -> Result<CollectorSnapshot, String> {
        if self.collecting {
            return Ok(self.snapshot());
        }

        let sender = self.event_sender.clone();

        if let Err(e) = self.windows_observer.start(sender.clone()) {
            return Err(format!("Failed to start Windows observer: {}", e));
        }

        if let Err(e) = self.clipboard_observer.start(sender.clone()) {
            let _ = self.windows_observer.stop();
            return Err(format!("Failed to start clipboard observer: {}", e));
        }

        if let Err(e) = self.keyboard_observer.start(sender) {
            let _ = self.clipboard_observer.stop();
            let _ = self.windows_observer.stop();
            return Err(format!("Failed to start keyboard observer: {}", e));
        }

        if let Err(e) = self.browser_url_observer.start(self.event_sender.clone()) {
            let _ = self.keyboard_observer.stop();
            let _ = self.clipboard_observer.stop();
            let _ = self.windows_observer.stop();
            return Err(format!("Failed to start browser URL observer: {}", e));
        }

        let now_ms = Utc::now().timestamp_millis();
        let action_count = self.count_actions_safe();

        self.pending_session = Some(PendingSession {
            id: Uuid::new_v4().to_string(),
            started_ms: now_ms,
            actions_before: action_count,
        });
        self.collecting = true;

        Ok(self.snapshot())
    }

    pub fn stop_collection(&mut self) -> Result<CollectorSnapshot, String> {
        if !self.collecting {
            return Ok(self.snapshot());
        }

        let mut errors = Vec::new();

        if let Err(e) = self.windows_observer.stop() {
            errors.push(format!("Windows observer stop failed: {}", e));
        }
        if let Err(e) = self.clipboard_observer.stop() {
            errors.push(format!("Clipboard observer stop failed: {}", e));
        }
        if let Err(e) = self.keyboard_observer.stop() {
            errors.push(format!("Keyboard observer stop failed: {}", e));
        }
        if let Err(e) = self.browser_url_observer.stop() {
            errors.push(format!("Browser URL observer stop failed: {}", e));
        }

        self.collecting = false;

        let actions_after = self.count_actions_safe();
        let now_ms = Utc::now().timestamp_millis();

        if let Some(pending) = self.pending_session.take() {
            let actions_collected = actions_after.saturating_sub(pending.actions_before);
            self.completed_sessions.push(CompletedCollectionSession {
                id: pending.id,
                started_ms: pending.started_ms,
                stopped_ms: now_ms,
                actions_collected,
            });
        }

        match self.repository.lock() {
            Ok(mut repo) => {
                if let Err(e) = repo.end_session() {
                    errors.push(format!("Failed to end session in repository: {}", e));
                }
            }
            Err(e) => {
                errors.push(format!("Failed to lock repository: {}", e));
            }
        }

        if errors.is_empty() {
            Ok(self.snapshot())
        } else {
            Err(errors.join("; "))
        }
    }

    pub fn clear_collected_data(&mut self) -> Result<CollectorSnapshot, String> {
        let now_ms = Utc::now().timestamp_millis();

        match self.repository.lock() {
            Ok(mut repo) => {
                if let Err(e) = repo.clear_collected_data() {
                    return Err(format!("Failed to clear collected data: {}", e));
                }
            }
            Err(e) => {
                return Err(format!("Failed to lock repository for clear operation: {}", e));
            }
        }

        self.completed_sessions.clear();

        if self.collecting {
            self.pending_session = Some(PendingSession {
                id: Uuid::new_v4().to_string(),
                started_ms: now_ms,
                actions_before: 0,
            });
        } else {
            self.pending_session = None;
        }

        Ok(self.snapshot())
    }

    pub fn snapshot(&self) -> CollectorSnapshot {
        let total_actions = self.count_actions_safe();
        let active_session = self.pending_session.as_ref().map(|pending| ActiveCollectionSession {
            id: pending.id.clone(),
            started_ms: pending.started_ms,
            actions_collected: total_actions.saturating_sub(pending.actions_before),
        });

        CollectorSnapshot {
            collecting: self.collecting,
            total_actions,
            active_session,
            completed_sessions: self.completed_sessions.clone(),
        }
    }

    fn count_actions_safe(&self) -> i64 {
        match self.repository.lock() {
            Ok(repo) => match repo.count_actions() {
                Ok(count) => count,
                Err(e) => {
                    tracing::warn!("Failed to count actions: {}", e);
                    0
                }
            },
            Err(e) => {
                tracing::warn!("Failed to lock repository for counting actions: {}", e);
                0
            }
        }
    }
}
