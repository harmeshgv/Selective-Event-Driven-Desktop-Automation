//! Windows-specific OS event observer
//!
//! Uses SetWinEventHook to capture high-level OS events:
//! - Window focus changes (EVENT_SYSTEM_FOREGROUND)
//! - Window creation/destruction
//! - Focus changes within windows
//!
//! # Safety Decision
//!
//! The observer runs in a dedicated thread with a Windows message loop.
//! Events are immediately sent to the symbolizer channel - no buffering
//! or storage of raw events.
//!
//! # Thread Safety
//!
//! SetWinEventHook requires a message loop in the calling thread.
//! We spawn a dedicated thread that runs the message pump.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use parking_lot::Mutex;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{
    SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, TranslateMessage, EVENT_OBJECT_FOCUS, EVENT_SYSTEM_FOREGROUND,
    MSG, PostThreadMessageW, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_QUIT,
};

use super::accessibility::extract_browser_url_for_window;
use super::events::RawOsEvent;
use super::window_manager::WindowManager;
use super::{ObserverError, OsObserver};

/// Global state for the event callback (required by Windows callback mechanism)
static EVENT_SENDER: once_cell::sync::OnceCell<Mutex<Option<Sender<RawOsEvent>>>> =
    once_cell::sync::OnceCell::new();

/// Global storage for hooks (must be accessed from the observer thread only)
static HOOKS: once_cell::sync::OnceCell<Mutex<Vec<isize>>> = once_cell::sync::OnceCell::new();

/// Windows-specific observer implementation
pub struct WindowsObserver {
    /// Whether the observer is currently running
    running: Arc<AtomicBool>,
    /// Native thread ID for the observer message loop thread
    thread_id: Arc<AtomicU32>,
    /// Handle to the observer thread
    thread_handle: Option<JoinHandle<()>>,
    /// Window manager for looking up window info
    window_manager: Arc<Mutex<WindowManager>>,
}

impl WindowsObserver {
    /// Create a new Windows observer
    pub fn new() -> Self {
        // Initialize global sender cell
        EVENT_SENDER.get_or_init(|| Mutex::new(None));
        HOOKS.get_or_init(|| Mutex::new(Vec::new()));

        Self {
            running: Arc::new(AtomicBool::new(false)),
            thread_id: Arc::new(AtomicU32::new(0)),
            thread_handle: None,
            window_manager: Arc::new(Mutex::new(WindowManager::new())),
        }
    }

    /// Get a reference to the window manager
    pub fn window_manager(&self) -> Arc<Mutex<WindowManager>> {
        Arc::clone(&self.window_manager)
    }
}

impl Default for WindowsObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl OsObserver for WindowsObserver {
    fn start(&mut self, event_sender: Sender<RawOsEvent>) -> Result<(), ObserverError> {
        if self.running.load(Ordering::SeqCst) {
            return Err(ObserverError::HookSetupFailed(
                "Observer already running".to_string(),
            ));
        }

        // Store sender in global state for callback access
        if let Some(sender_lock) = EVENT_SENDER.get() {
            *sender_lock.lock() = Some(event_sender);
        }

        let running = Arc::clone(&self.running);
        let thread_id = Arc::clone(&self.thread_id);
        let window_manager = Arc::clone(&self.window_manager);

        // Spawn the observer thread
        let handle = thread::spawn(move || {
            let current_thread_id = unsafe { GetCurrentThreadId() };
            thread_id.store(current_thread_id, Ordering::SeqCst);
            running.store(true, Ordering::SeqCst);

            // Set up event hooks
            // SAFETY: We use WINEVENT_OUTOFCONTEXT so the callback runs in our thread
            // WINEVENT_SKIPOWNPROCESS prevents us from observing our own windows
            let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;

            unsafe {
                // Hook for foreground window changes
                let hook_foreground = SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    None,
                    Some(win_event_callback),
                    0,
                    0,
                    flags,
                );

                if !hook_foreground.is_invalid() {
                    if let Some(hooks) = HOOKS.get() {
                        hooks.lock().push(hook_foreground.0 as isize);
                    }
                }

                // Hook for focus changes within windows
                let hook_focus = SetWinEventHook(
                    EVENT_OBJECT_FOCUS,
                    EVENT_OBJECT_FOCUS,
                    None,
                    Some(win_event_callback),
                    0,
                    0,
                    flags,
                );

                if !hook_focus.is_invalid() {
                    if let Some(hooks) = HOOKS.get() {
                        hooks.lock().push(hook_focus.0 as isize);
                    }
                }
            }

            let hook_count = HOOKS.get().map(|h| h.lock().len()).unwrap_or(0);
            tracing::info!("Windows observer started with {} hooks", hook_count);

            // Do initial window enumeration
            if let Err(e) = window_manager.lock().enumerate_windows() {
                tracing::warn!("Initial window enumeration failed: {}", e);
            }

            // Message loop - required for SetWinEventHook to work
            // SAFETY: Standard Windows message loop pattern
            unsafe {
                let mut msg = MSG::default();
                while running.load(Ordering::SeqCst) {
                    let result = GetMessageW(&mut msg, HWND::default(), 0, 0);

                    if result.0 == 0 || result.0 == -1 {
                        // WM_QUIT received or error
                        break;
                    }

                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }

            // Cleanup hooks
            if let Some(hooks) = HOOKS.get() {
                let hooks_to_remove: Vec<_> = hooks.lock().drain(..).collect();
                for hook_val in hooks_to_remove {
                    unsafe {
                        let hook = HWINEVENTHOOK(hook_val as *mut _);
                        let _ = UnhookWinEvent(hook);
                    }
                }
            }

            running.store(false, Ordering::SeqCst);
            thread_id.store(0, Ordering::SeqCst);
            tracing::info!("Windows observer stopped");
        });

        self.thread_handle = Some(handle);

        // Wait a bit for the thread to start
        thread::sleep(std::time::Duration::from_millis(100));

        if !self.running.load(Ordering::SeqCst) {
            return Err(ObserverError::HookSetupFailed(
                "Observer thread failed to start".to_string(),
            ));
        }

        Ok(())
    }

    fn stop(&mut self) -> Result<(), ObserverError> {
        self.running.store(false, Ordering::SeqCst);

        // Post WM_QUIT to the observer thread's message queue.
        let tid = self.thread_id.load(Ordering::SeqCst);
        if tid != 0 {
            unsafe {
                let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }

        // Wait for thread to finish
        if let Some(handle) = self.thread_handle.take() {
            handle
                .join()
                .map_err(|_| ObserverError::ThreadError("Failed to join observer thread".into()))?;
        }
        self.thread_id.store(0, Ordering::SeqCst);

        // Clear the global sender
        if let Some(sender_lock) = EVENT_SENDER.get() {
            *sender_lock.lock() = None;
        }

        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Drop for WindowsObserver {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Windows event callback
///
/// SAFETY: This is called by Windows on our observer thread.
/// We immediately convert the event to a RawOsEvent and send it to the channel.
/// No raw data is stored.
unsafe extern "system" fn win_event_callback(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _id_event_thread: u32,
    _dwms_event_time: u32,
) {
    // Skip invalid window handles
    if hwnd.0.is_null() {
        return;
    }

    let hwnd_val = hwnd.0 as isize;

    // Get window info
    let window_manager = WindowManager::new();
    let window_info = match window_manager.get_window_info(hwnd_val) {
        Some(info) => info,
        None => return,
    };

    // Create the appropriate event
    let raw_event = match event {
        e if e == EVENT_SYSTEM_FOREGROUND => RawOsEvent::WindowFocusChanged {
            hwnd: hwnd_val,
            process_name: window_info.process_name,
            window_title: window_info.title, // Will be discarded during symbolization
        },
        e if e == EVENT_OBJECT_FOCUS => {
            // For object focus, we treat it as a focus change if it's a different window
            RawOsEvent::ElementFocused {
                hwnd: hwnd_val,
                element_id: format!("focus_{}", hwnd_val),
                control_type: "Unknown".to_string(),
            }
        }
        _ => return,
    };

    // Send event to channel
    if let Some(sender_lock) = EVENT_SENDER.get() {
        if let Some(sender) = sender_lock.lock().as_ref() {
            let _ = sender.send(raw_event);
        }
    }
}

/// Clipboard observer - monitors clipboard changes
///
/// Note: This is a separate component that can be optionally enabled.
pub struct ClipboardObserver {
    running: Arc<AtomicBool>,
    thread_handle: Option<JoinHandle<()>>,
}

impl ClipboardObserver {
    /// Create a new clipboard observer
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
        }
    }

    /// Start observing clipboard changes
    pub fn start(&mut self, event_sender: Sender<RawOsEvent>) -> Result<(), ObserverError> {
        use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;

        if self.running.load(Ordering::SeqCst) {
            return Err(ObserverError::HookSetupFailed(
                "Clipboard observer already running".to_string(),
            ));
        }

        let running = Arc::clone(&self.running);

        let handle = thread::spawn(move || {
            running.store(true, Ordering::SeqCst);

            // We'll use a simple polling approach for clipboard changes
            // A more sophisticated approach would use AddClipboardFormatListener
            // but that requires a message-only window

            let mut last_sequence = unsafe { GetClipboardSequenceNumber() };

            while running.load(Ordering::SeqCst) {
                thread::sleep(std::time::Duration::from_millis(500));

                let current_sequence = unsafe { GetClipboardSequenceNumber() };

                if current_sequence != last_sequence {
                    last_sequence = current_sequence;

                    // Determine clipboard content type without reading actual content
                    let content_type = get_clipboard_content_type();

                    let event = RawOsEvent::ClipboardChanged { content_type };

                    if event_sender.send(event).is_err() {
                        break;
                    }
                }
            }

            running.store(false, Ordering::SeqCst);
            tracing::info!("Clipboard observer stopped");
        });

        self.thread_handle = Some(handle);
        Ok(())
    }

    /// Stop observing clipboard changes
    pub fn stop(&mut self) -> Result<(), ObserverError> {
        self.running.store(false, Ordering::SeqCst);

        if let Some(handle) = self.thread_handle.take() {
            handle
                .join()
                .map_err(|_| ObserverError::ThreadError("Failed to join clipboard thread".into()))?;
        }

        Ok(())
    }

    /// Check if observer is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Default for ClipboardObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ClipboardObserver {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Browser URL observer - polls the foreground browser window for URL changes
///
/// This observer complements WinEvent hooks by tracking in-window browser navigation
/// where focus does not change between windows.
pub struct BrowserUrlObserver {
    running: Arc<AtomicBool>,
    thread_handle: Option<JoinHandle<()>>,
}

impl BrowserUrlObserver {
    /// Create a new browser URL observer
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
        }
    }

    /// Start polling foreground browser URL changes
    pub fn start(&mut self, event_sender: Sender<RawOsEvent>) -> Result<(), ObserverError> {
        if self.running.load(Ordering::SeqCst) {
            return Err(ObserverError::HookSetupFailed(
                "Browser URL observer already running".to_string(),
            ));
        }

        let running = Arc::clone(&self.running);

        let handle = thread::spawn(move || {
            running.store(true, Ordering::SeqCst);
            let mut last_emitted: Option<(isize, String)> = None;

            while running.load(Ordering::SeqCst) {
                thread::sleep(std::time::Duration::from_millis(1200));

                let window_manager = WindowManager::new();
                let Some(window) = window_manager.get_foreground_window() else {
                    continue;
                };

                if !is_browser_process(&window.process_name) {
                    continue;
                }

                let Some(url) = extract_browser_url_for_window(window.hwnd) else {
                    continue;
                };

                let should_emit = match &last_emitted {
                    Some((last_hwnd, last_url)) => *last_hwnd != window.hwnd || *last_url != url,
                    None => true,
                };

                if !should_emit {
                    continue;
                }

                let event = RawOsEvent::BrowserNavigation {
                    hwnd: window.hwnd,
                    process_name: window.process_name.clone(),
                    url: url.clone(),
                };

                if event_sender.send(event).is_err() {
                    break;
                }

                last_emitted = Some((window.hwnd, url));
            }

            running.store(false, Ordering::SeqCst);
            tracing::info!("Browser URL observer stopped");
        });

        self.thread_handle = Some(handle);
        Ok(())
    }

    /// Stop polling browser URLs
    pub fn stop(&mut self) -> Result<(), ObserverError> {
        self.running.store(false, Ordering::SeqCst);

        if let Some(handle) = self.thread_handle.take() {
            handle
                .join()
                .map_err(|_| ObserverError::ThreadError("Failed to join browser URL thread".into()))?;
        }

        Ok(())
    }

    /// Check if running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Default for BrowserUrlObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for BrowserUrlObserver {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Get the type of content on the clipboard without reading the actual content
///
/// SAFETY: This only checks format availability, never reads clipboard data
fn get_clipboard_content_type() -> super::events::ClipboardContentType {
    use super::events::ClipboardContentType;
    use windows::Win32::System::DataExchange::IsClipboardFormatAvailable;

    // Standard clipboard format constants (as raw u32 values)
    // CF_TEXT = 1, CF_BITMAP = 2, CF_UNICODETEXT = 13, CF_HDROP = 15
    
    unsafe {
        // Check formats in order of preference
        // CF_HDROP for files
        if IsClipboardFormatAvailable(15).is_ok() {
            return ClipboardContentType::Files;
        }
        // CF_BITMAP for images
        if IsClipboardFormatAvailable(2).is_ok() {
            return ClipboardContentType::Image;
        }
        // CF_UNICODETEXT or CF_TEXT for text
        if IsClipboardFormatAvailable(13).is_ok()
            || IsClipboardFormatAvailable(1).is_ok()
        {
            return ClipboardContentType::Text;
        }

        ClipboardContentType::Unknown
    }
}

// ============================================================================
// Keyboard Observer - Low-level keyboard hook for shortcut detection
// ============================================================================

use super::events::{Modifier, VirtualKey};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LCONTROL, VK_RCONTROL,
    VK_MENU, VK_LMENU, VK_RMENU, VK_SHIFT, VK_LSHIFT, VK_RSHIFT,
    VK_LWIN, VK_RWIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowsHookExW, UnhookWindowsHookEx, CallNextHookEx,
    WH_KEYBOARD_LL, KBDLLHOOKSTRUCT, HHOOK,
};

/// Global state for the keyboard callback
static KEYBOARD_SENDER: once_cell::sync::OnceCell<Mutex<Option<Sender<RawOsEvent>>>> =
    once_cell::sync::OnceCell::new();

/// Global storage for keyboard hook
static KEYBOARD_HOOK: once_cell::sync::OnceCell<Mutex<Option<isize>>> =
    once_cell::sync::OnceCell::new();

/// Keyboard observer - monitors keyboard shortcuts (Ctrl+C, Ctrl+V, etc.)
///
/// # Safety
///
/// This observer ONLY captures specific shortcut combinations.
/// Regular typing is NEVER recorded - we filter out everything except
/// modifier+key combinations that indicate copy/paste/cut operations.
pub struct KeyboardObserver {
    running: Arc<AtomicBool>,
    thread_id: Arc<AtomicU32>,
    thread_handle: Option<JoinHandle<()>>,
}

fn is_browser_process(process_name: &str) -> bool {
    let p = process_name.to_lowercase();
    p.contains("chrome")
        || p.contains("msedge")
        || p.contains("firefox")
        || p.contains("brave")
        || p.contains("opera")
        || p.contains("vivaldi")
}

impl KeyboardObserver {
    /// Create a new keyboard observer
    pub fn new() -> Self {
        KEYBOARD_SENDER.get_or_init(|| Mutex::new(None));
        KEYBOARD_HOOK.get_or_init(|| Mutex::new(None));

        Self {
            running: Arc::new(AtomicBool::new(false)),
            thread_id: Arc::new(AtomicU32::new(0)),
            thread_handle: None,
        }
    }

    /// Start observing keyboard shortcuts
    pub fn start(&mut self, event_sender: Sender<RawOsEvent>) -> Result<(), ObserverError> {
        if self.running.load(Ordering::SeqCst) {
            return Err(ObserverError::HookSetupFailed(
                "Keyboard observer already running".to_string(),
            ));
        }

        // Store sender in global state for callback access
        if let Some(sender_lock) = KEYBOARD_SENDER.get() {
            *sender_lock.lock() = Some(event_sender);
        }

        let running = Arc::clone(&self.running);
        let thread_id = Arc::clone(&self.thread_id);

        let handle = thread::spawn(move || {
            let current_thread_id = unsafe { GetCurrentThreadId() };
            thread_id.store(current_thread_id, Ordering::SeqCst);
            running.store(true, Ordering::SeqCst);

            // Set up low-level keyboard hook
            // SAFETY: Standard Windows hook pattern
            unsafe {
                let hook = SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(keyboard_hook_callback),
                    None,
                    0,
                );

                match hook {
                    Ok(h) => {
                        if let Some(hook_storage) = KEYBOARD_HOOK.get() {
                            *hook_storage.lock() = Some(h.0 as isize);
                        }
                        tracing::info!("Keyboard hook installed successfully");
                    }
                    Err(e) => {
                        tracing::error!("Failed to install keyboard hook: {:?}", e);
                        running.store(false, Ordering::SeqCst);
                        return;
                    }
                }
            }

            // Message loop - required for the hook to work
            unsafe {
                let mut msg = MSG::default();
                while running.load(Ordering::SeqCst) {
                    let result = GetMessageW(&mut msg, HWND::default(), 0, 0);

                    if result.0 == 0 || result.0 == -1 {
                        break;
                    }

                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }

            // Cleanup hook
            if let Some(hook_storage) = KEYBOARD_HOOK.get() {
                if let Some(hook_val) = hook_storage.lock().take() {
                    unsafe {
                        let hook = HHOOK(hook_val as *mut _);
                        let _ = UnhookWindowsHookEx(hook);
                    }
                }
            }

            running.store(false, Ordering::SeqCst);
            thread_id.store(0, Ordering::SeqCst);
            tracing::info!("Keyboard observer stopped");
        });

        self.thread_handle = Some(handle);

        // Wait a bit for the thread to start
        thread::sleep(std::time::Duration::from_millis(100));

        if !self.running.load(Ordering::SeqCst) {
            return Err(ObserverError::HookSetupFailed(
                "Keyboard observer thread failed to start".to_string(),
            ));
        }

        Ok(())
    }

    /// Stop observing keyboard shortcuts
    pub fn stop(&mut self) -> Result<(), ObserverError> {
        self.running.store(false, Ordering::SeqCst);

        // Post WM_QUIT to the keyboard observer thread's message queue.
        let tid = self.thread_id.load(Ordering::SeqCst);
        if tid != 0 {
            unsafe {
                let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }

        if let Some(handle) = self.thread_handle.take() {
            handle
                .join()
                .map_err(|_| ObserverError::ThreadError("Failed to join keyboard thread".into()))?;
        }
        self.thread_id.store(0, Ordering::SeqCst);

        // Clear the global sender
        if let Some(sender_lock) = KEYBOARD_SENDER.get() {
            *sender_lock.lock() = None;
        }

        Ok(())
    }

    /// Check if observer is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Default for KeyboardObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for KeyboardObserver {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Windows keyboard hook message constants
const WM_KEYDOWN: u32 = 0x0100;
const WM_SYSKEYDOWN: u32 = 0x0104;

/// Check if a modifier key is currently pressed
fn is_modifier_pressed(vk: i32) -> bool {
    unsafe {
        // GetAsyncKeyState returns negative if key is pressed
        GetAsyncKeyState(vk) < 0
    }
}

/// Get currently pressed modifiers
fn get_current_modifiers() -> Vec<Modifier> {
    let mut modifiers = Vec::new();

    // Check Ctrl
    if is_modifier_pressed(VK_CONTROL.0 as i32)
        || is_modifier_pressed(VK_LCONTROL.0 as i32)
        || is_modifier_pressed(VK_RCONTROL.0 as i32)
    {
        modifiers.push(Modifier::Ctrl);
    }

    // Check Alt
    if is_modifier_pressed(VK_MENU.0 as i32)
        || is_modifier_pressed(VK_LMENU.0 as i32)
        || is_modifier_pressed(VK_RMENU.0 as i32)
    {
        modifiers.push(Modifier::Alt);
    }

    // Check Shift
    if is_modifier_pressed(VK_SHIFT.0 as i32)
        || is_modifier_pressed(VK_LSHIFT.0 as i32)
        || is_modifier_pressed(VK_RSHIFT.0 as i32)
    {
        modifiers.push(Modifier::Shift);
    }

    // Check Win
    if is_modifier_pressed(VK_LWIN.0 as i32) || is_modifier_pressed(VK_RWIN.0 as i32) {
        modifiers.push(Modifier::Win);
    }

    modifiers
}

/// Convert Windows virtual key code to our VirtualKey enum
fn vk_to_virtual_key(vk_code: u32) -> Option<VirtualKey> {
    // We only care about specific keys for shortcuts
    // Virtual key codes: A-Z = 0x41-0x5A, 0-9 = 0x30-0x39
    match vk_code {
        // Letters (for Ctrl+C, Ctrl+V, Ctrl+X, etc.)
        0x41 => Some(VirtualKey::A),
        0x42 => Some(VirtualKey::B),
        0x43 => Some(VirtualKey::C), // Ctrl+C = Copy
        0x44 => Some(VirtualKey::D),
        0x45 => Some(VirtualKey::E),
        0x46 => Some(VirtualKey::F),
        0x47 => Some(VirtualKey::G),
        0x48 => Some(VirtualKey::H),
        0x49 => Some(VirtualKey::I),
        0x4A => Some(VirtualKey::J),
        0x4B => Some(VirtualKey::K),
        0x4C => Some(VirtualKey::L),
        0x4D => Some(VirtualKey::M),
        0x4E => Some(VirtualKey::N),
        0x4F => Some(VirtualKey::O),
        0x50 => Some(VirtualKey::P),
        0x51 => Some(VirtualKey::Q),
        0x52 => Some(VirtualKey::R),
        0x53 => Some(VirtualKey::S),
        0x54 => Some(VirtualKey::T),
        0x55 => Some(VirtualKey::U),
        0x56 => Some(VirtualKey::V), // Ctrl+V = Paste
        0x57 => Some(VirtualKey::W),
        0x58 => Some(VirtualKey::X), // Ctrl+X = Cut
        0x59 => Some(VirtualKey::Y),
        0x5A => Some(VirtualKey::Z),
        // Numbers (0-9)
        0x30 => Some(VirtualKey::Num0),
        0x31 => Some(VirtualKey::Num1),
        0x32 => Some(VirtualKey::Num2),
        0x33 => Some(VirtualKey::Num3),
        0x34 => Some(VirtualKey::Num4),
        0x35 => Some(VirtualKey::Num5),
        0x36 => Some(VirtualKey::Num6),
        0x37 => Some(VirtualKey::Num7),
        0x38 => Some(VirtualKey::Num8),
        0x39 => Some(VirtualKey::Num9),
        // Function keys
        0x70 => Some(VirtualKey::F1),
        0x71 => Some(VirtualKey::F2),
        0x72 => Some(VirtualKey::F3),
        0x73 => Some(VirtualKey::F4),
        0x74 => Some(VirtualKey::F5),
        0x75 => Some(VirtualKey::F6),
        0x76 => Some(VirtualKey::F7),
        0x77 => Some(VirtualKey::F8),
        0x78 => Some(VirtualKey::F9),
        0x79 => Some(VirtualKey::F10),
        0x7A => Some(VirtualKey::F11),
        0x7B => Some(VirtualKey::F12),
        // Navigation keys
        0x0D => Some(VirtualKey::Enter),
        0x09 => Some(VirtualKey::Tab),
        0x1B => Some(VirtualKey::Escape),
        0x20 => Some(VirtualKey::Space),
        0x08 => Some(VirtualKey::Backspace),
        0x26 => Some(VirtualKey::ArrowUp),
        0x28 => Some(VirtualKey::ArrowDown),
        0x25 => Some(VirtualKey::ArrowLeft),
        0x27 => Some(VirtualKey::ArrowRight),
        0x24 => Some(VirtualKey::Home),
        0x23 => Some(VirtualKey::End),
        0x21 => Some(VirtualKey::PageUp),
        0x22 => Some(VirtualKey::PageDown),
        0x2D => Some(VirtualKey::Insert),
        0x2E => Some(VirtualKey::Delete),
        0x2C => Some(VirtualKey::PrintScreen),
        _ => None,
    }
}

/// Low-level keyboard hook callback
///
/// # Safety
///
/// This callback is called by Windows for every keyboard event.
/// We ONLY emit events for shortcut combinations (Ctrl+key).
/// Regular typing is NEVER captured - privacy is paramount.
unsafe extern "system" fn keyboard_hook_callback(
    code: i32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    // Only process if code is 0 (HC_ACTION)
    if code == 0 {
        let wparam_val = wparam.0 as u32;

        // Only process key down events
        if wparam_val == WM_KEYDOWN || wparam_val == WM_SYSKEYDOWN {
            let kb_struct = *(lparam.0 as *const KBDLLHOOKSTRUCT);
            let vk_code = kb_struct.vkCode;

            // Get current modifiers
            let modifiers = get_current_modifiers();

            // PRIVACY: Only capture if at least Ctrl is pressed (shortcuts only)
            // This ensures we NEVER log regular typing
            if modifiers.contains(&Modifier::Ctrl) {
                if let Some(key) = vk_to_virtual_key(vk_code) {
                    // Only capture specific shortcuts we care about
                    let is_relevant_shortcut = matches!(
                        key,
                        VirtualKey::C | VirtualKey::V | VirtualKey::X | VirtualKey::A | VirtualKey::Z
                    );

                    if is_relevant_shortcut {
                        let event = RawOsEvent::KeyboardShortcut {
                            modifiers: modifiers.clone(),
                            key,
                        };

                        // Send event to channel
                        if let Some(sender_lock) = KEYBOARD_SENDER.get() {
                            if let Some(sender) = sender_lock.lock().as_ref() {
                                let _ = sender.send(event);
                            }
                        }

                        tracing::debug!(
                            "Keyboard shortcut detected: {:?}+{:?}",
                            modifiers,
                            key
                        );
                    }
                }
            }
        }
    }

    // Always call next hook
    CallNextHookEx(None, code, wparam, lparam)
}
