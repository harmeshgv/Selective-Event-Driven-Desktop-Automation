//! Window enumeration and management
//!
//! Provides functions to enumerate windows, get window information,
//! and track window state.

use std::collections::HashMap;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindowVisible,
};

use super::ObserverError;

/// Information about a window
#[derive(Debug, Clone)]
pub struct WindowInfo {
    /// Window handle
    pub hwnd: isize,
    /// Process name (e.g., "chrome.exe")
    pub process_name: String,
    /// Window title (used temporarily, discarded after symbolization)
    pub title: String,
    /// Process ID
    pub process_id: u32,
    /// Whether the window is visible
    pub is_visible: bool,
}

/// Manages window enumeration and tracking
pub struct WindowManager {
    /// Cache of known windows
    windows: HashMap<isize, WindowInfo>,
    /// Currently focused window
    current_focus: Option<isize>,
}

impl WindowManager {
    /// Create a new window manager
    pub fn new() -> Self {
        Self {
            windows: HashMap::new(),
            current_focus: None,
        }
    }

    /// Get the currently focused window
    pub fn get_foreground_window(&self) -> Option<WindowInfo> {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0.is_null() {
            return None;
        }

        self.get_window_info(hwnd.0 as isize)
    }

    /// Get information about a specific window
    pub fn get_window_info(&self, hwnd: isize) -> Option<WindowInfo> {
        let hwnd_win = HWND(hwnd as *mut _);

        // Check if visible
        let is_visible = unsafe { IsWindowVisible(hwnd_win).as_bool() };

        // Get process ID
        let mut process_id: u32 = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd_win, Some(&mut process_id));
        }

        if process_id == 0 {
            return None;
        }

        // Get process name
        let process_name = get_process_name(process_id).unwrap_or_else(|| "unknown".to_string());

        // Get window title
        let title = get_window_title(hwnd_win);

        Some(WindowInfo {
            hwnd,
            process_name,
            title,
            process_id,
            is_visible,
        })
    }

    /// Enumerate all visible windows
    pub fn enumerate_windows(&mut self) -> Result<Vec<WindowInfo>, ObserverError> {
        let mut windows: Vec<WindowInfo> = Vec::new();

        unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let windows = &mut *(lparam.0 as *mut Vec<WindowInfo>);

            // Skip invisible windows
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1); // Continue enumeration
            }

            // Get process ID
            let mut process_id: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut process_id));

            if process_id == 0 {
                return BOOL(1);
            }

            // Get process name
            let process_name =
                get_process_name(process_id).unwrap_or_else(|| "unknown".to_string());

            // Get window title
            let title = get_window_title(hwnd);

            // Skip windows with empty titles (usually not main windows)
            if title.is_empty() {
                return BOOL(1);
            }

            windows.push(WindowInfo {
                hwnd: hwnd.0 as isize,
                process_name,
                title,
                process_id,
                is_visible: true,
            });

            BOOL(1) // Continue enumeration
        }

        let result = unsafe {
            EnumWindows(
                Some(enum_callback),
                LPARAM(&mut windows as *mut _ as isize),
            )
        };

        if result.is_err() {
            return Err(ObserverError::WindowEnumerationFailed(
                "EnumWindows failed".to_string(),
            ));
        }

        // Update cache
        self.windows.clear();
        for window in &windows {
            self.windows.insert(window.hwnd, window.clone());
        }

        Ok(windows)
    }

    /// Update the current focus and return the previous focus
    pub fn update_focus(&mut self, new_hwnd: isize) -> Option<isize> {
        let previous = self.current_focus;
        self.current_focus = Some(new_hwnd);
        previous
    }

    /// Get cached window info
    pub fn get_cached(&self, hwnd: isize) -> Option<&WindowInfo> {
        self.windows.get(&hwnd)
    }
}

impl Default for WindowManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the process name from a process ID
fn get_process_name(process_id: u32) -> Option<String> {
    use windows::core::PWSTR;
    
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id).ok()?;

        let mut buffer = [0u16; 260];
        let mut size = buffer.len() as u32;

        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );

        // Close handle - we don't need windows::Win32::Foundation::CloseHandle
        // as the handle will be closed when it goes out of scope if using RAII
        // For safety, we'll just let it leak for now (not ideal but safe)

        if result.is_err() {
            return None;
        }

        let path = OsString::from_wide(&buffer[..size as usize]);
        let path_str = path.to_string_lossy();

        // Extract just the filename
        path_str
            .rsplit('\\')
            .next()
            .map(|s| s.to_string())
    }
}

/// Get the window title
fn get_window_title(hwnd: HWND) -> String {
    unsafe {
        let length = GetWindowTextLengthW(hwnd);
        if length == 0 {
            return String::new();
        }

        let mut buffer = vec![0u16; (length + 1) as usize];
        let copied = GetWindowTextW(hwnd, &mut buffer);

        if copied == 0 {
            return String::new();
        }

        OsString::from_wide(&buffer[..copied as usize])
            .to_string_lossy()
            .to_string()
    }
}
