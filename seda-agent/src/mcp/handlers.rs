//! MCP request handlers
//!
//! Implements the actual logic for each MCP tool.

use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::sync::Mutex as TokioMutex;

use super::schema::*;
use super::safety::{SafetyEnforcer, SafetyError};
use crate::mining::PatternReport;
use crate::observer::accessibility::AccessibilityTree;
use crate::observer::window_manager::WindowManager;
use crate::storage::Repository;

/// MCP request handler
/// 
/// Uses tokio::sync::Mutex for async-safe access to non-Send/Sync types.
/// Note: AccessibilityTree is created on-demand per request because UIAutomation
/// COM objects contain raw pointers that are not thread-safe.
pub struct McpHandler {
    /// Safety enforcer
    safety: Arc<SafetyEnforcer>,
    /// Repository for data access (Connection is not Sync, so we use TokioMutex)
    repository: Arc<TokioMutex<Repository>>,
    /// Window manager for window operations
    window_manager: Arc<TokioMutex<WindowManager>>,
}

impl McpHandler {
    /// Create a new MCP handler
    pub fn new(
        repository: Arc<TokioMutex<Repository>>,
        window_manager: Arc<TokioMutex<WindowManager>>,
    ) -> Self {
        Self {
            safety: Arc::new(SafetyEnforcer::new()),
            repository,
            window_manager,
        }
    }

    /// Create handler with custom safety enforcer
    pub fn with_safety(
        safety: Arc<SafetyEnforcer>,
        repository: Arc<TokioMutex<Repository>>,
        window_manager: Arc<TokioMutex<WindowManager>>,
    ) -> Self {
        Self {
            safety,
            repository,
            window_manager,
        }
    }
    

    /// Handle a JSON-RPC request
    pub async fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        let method = &request.method;
        let id = request.id.clone();

        // Check safety
        if let Err(e) = self.safety.check_operation(method) {
            return match e {
                SafetyError::OperationForbidden(_) => {
                    JsonRpcResponse::forbidden(id, e.to_string())
                }
                SafetyError::RateLimitExceeded { .. } => JsonRpcResponse::rate_limited(id),
                SafetyError::InvalidParams(msg) => JsonRpcResponse::invalid_params(id, msg),
            };
        }

        // Validate params
        if let Err(e) = self.safety.validate_params(method, &request.params) {
            return JsonRpcResponse::invalid_params(id, e.to_string());
        }

        // Route to handler
        match method.as_str() {
            "list_windows" => self.handle_list_windows(id, request.params).await,
            "get_window_tree" => self.handle_get_window_tree(id, request.params).await,
            "get_patterns" => self.handle_get_patterns(id, request.params).await,
            "get_transitions" => self.handle_get_transitions(id, request.params).await,
            "activate_element" => self.handle_activate_element(id, request.params).await,
            "press_key" => self.handle_press_key(id, request.params).await,
            "set_clipboard" => self.handle_set_clipboard(id, request.params).await,
            "tools/list" => self.handle_tools_list(id).await,
            _ => JsonRpcResponse::method_not_found(id, method),
        }
    }

    /// List windows
    async fn handle_list_windows(
        &self,
        id: Option<serde_json::Value>,
        params: serde_json::Value,
    ) -> JsonRpcResponse {
        let request: ListWindowsRequest = match serde_json::from_value(params) {
            Ok(r) => r,
            Err(e) => return JsonRpcResponse::invalid_params(id, e.to_string()),
        };

        let mut wm = self.window_manager.lock().await;

        let windows = match wm.enumerate_windows() {
            Ok(w) => w,
            Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
        };

        let foreground = wm.get_foreground_window();
        let focused_hwnd = foreground.as_ref().map(|w| w.hwnd);

        let window_infos: Vec<WindowInfo> = windows
            .into_iter()
            .filter(|w| request.include_hidden || w.is_visible)
            .map(|w| WindowInfo {
                hwnd: format!("{}", w.hwnd),
                process_name: w.process_name,
                // PRIVACY: Hash the title, never expose raw title
                title_hash: hash_string(&w.title),
                is_focused: Some(w.hwnd) == focused_hwnd,
                bounds: None, // Could add bounds if needed
            })
            .collect();

        drop(wm); // Release lock before acquiring another

        // Record audit
        let _ = self.repository.lock().await.record_audit(
            "list_windows",
            None,
            "success",
            None,
            None,
        );

        JsonRpcResponse::success(id, serde_json::to_value(window_infos).unwrap())
    }

    /// Get window UI tree
    async fn handle_get_window_tree(
        &self,
        id: Option<serde_json::Value>,
        params: serde_json::Value,
    ) -> JsonRpcResponse {
        let request: GetWindowTreeRequest = match serde_json::from_value(params) {
            Ok(r) => r,
            Err(e) => return JsonRpcResponse::invalid_params(id, e.to_string()),
        };

        let hwnd: isize = match request.hwnd.parse() {
            Ok(h) => h,
            Err(_) => return JsonRpcResponse::invalid_params(id, "Invalid hwnd format"),
        };

        let max_depth = request.max_depth.min(10).max(1);

        // Run UI Automation in a blocking task since it's not Send
        let tree_result = tokio::task::spawn_blocking(move || {
            let accessibility = AccessibilityTree::new()
                .map_err(|e| e.to_string())?;
            accessibility.get_element_tree(hwnd, max_depth)
                .map_err(|e| e.to_string())
        }).await;

        let tree = match tree_result {
            Ok(Ok(t)) => t,
            Ok(Err(e)) => return JsonRpcResponse::internal_error(id, e),
            Err(e) => return JsonRpcResponse::internal_error(id, format!("Task failed: {}", e)),
        };

        // Convert to privacy-safe format
        let element_node = convert_element_tree(&tree);

        // Record audit
        let _ = self.repository.lock().await.record_audit(
            "get_window_tree",
            Some(&hash_string(&request.hwnd)),
            "success",
            None,
            None,
        );

        JsonRpcResponse::success(id, serde_json::to_value(element_node).unwrap())
    }

    /// Get detected patterns
    async fn handle_get_patterns(
        &self,
        id: Option<serde_json::Value>,
        params: serde_json::Value,
    ) -> JsonRpcResponse {
        let request: GetPatternsRequest = match serde_json::from_value(params) {
            Ok(r) => r,
            Err(e) => return JsonRpcResponse::invalid_params(id, e.to_string()),
        };

        let min_freq = request.min_frequency.max(2) as i64;

        let repo = self.repository.lock().await;
        let patterns = match repo.get_patterns(min_freq) {
            Ok(p) => p,
            Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
        };

        // Convert to candidate patterns
        let candidates: Vec<_> = patterns
            .into_iter()
            .filter_map(|p| {
                let sequence = p.to_sequence().ok()?;
                Some(crate::mining::CandidatePattern::new(
                    sequence,
                    p.frequency as u32,
                    p.avg_duration_ms.unwrap_or(0) as u64,
                    p.confidence,
                ))
            })
            .collect();

        let report = PatternReport::new(
            candidates,
            request.hours_back as u64,
            0, // TODO: get actual action count
        );

        // Record audit
        let _ = repo.record_audit(
            "get_patterns",
            None,
            "success",
            None,
            None,
        );

        JsonRpcResponse::success(id, serde_json::to_value(report).unwrap())
    }

    /// Get transitions
    async fn handle_get_transitions(
        &self,
        id: Option<serde_json::Value>,
        params: serde_json::Value,
    ) -> JsonRpcResponse {
        let min_freq: i64 = params
            .get("min_frequency")
            .and_then(|v| v.as_i64())
            .unwrap_or(2);

        let repo = self.repository.lock().await;
        let transitions = match repo.get_frequent_transitions(min_freq) {
            Ok(t) => t,
            Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
        };

        // Record audit
        let _ = repo.record_audit(
            "get_transitions",
            None,
            "success",
            None,
            None,
        );

        JsonRpcResponse::success(id, serde_json::to_value(transitions).unwrap())
    }

    /// Activate a UI element
    async fn handle_activate_element(
        &self,
        id: Option<serde_json::Value>,
        params: serde_json::Value,
    ) -> JsonRpcResponse {
        let request: ActivateElementRequest = match serde_json::from_value(params) {
            Ok(r) => r,
            Err(e) => return JsonRpcResponse::invalid_params(id, e.to_string()),
        };

        let hwnd: isize = match request.hwnd.parse() {
            Ok(h) => h,
            Err(_) => return JsonRpcResponse::invalid_params(id, "Invalid hwnd format"),
        };

        let element_id = request.element_id.clone();
        let action = request.action.clone();

        // Run UI Automation in a blocking task since it's not Send
        let action_result = tokio::task::spawn_blocking(move || {
            use uiautomation::patterns::{UIInvokePattern, UIExpandCollapsePattern, UISelectionItemPattern};
            
            let accessibility = AccessibilityTree::new()
                .map_err(|e| e.to_string())?;
            
            let element = accessibility.find_element_by_id(hwnd, &element_id)
                .map_err(|e| e.to_string())?;

            // Perform the action
            match action {
                ElementAction::Click => {
                    if let Ok(pattern) = element.get_pattern::<UIInvokePattern>() {
                        pattern.invoke().map_err(|e| e.to_string())
                    } else {
                        Err("Element does not support Invoke pattern".to_string())
                    }
                }
                ElementAction::Focus => element.set_focus().map_err(|e| e.to_string()),
                ElementAction::Expand => {
                    if let Ok(pattern) = element.get_pattern::<UIExpandCollapsePattern>() {
                        pattern.expand().map_err(|e| e.to_string())
                    } else {
                        Err("Element does not support ExpandCollapse pattern".to_string())
                    }
                }
                ElementAction::Collapse => {
                    if let Ok(pattern) = element.get_pattern::<UIExpandCollapsePattern>() {
                        pattern.collapse().map_err(|e| e.to_string())
                    } else {
                        Err("Element does not support ExpandCollapse pattern".to_string())
                    }
                }
                ElementAction::Select => {
                    if let Ok(pattern) = element.get_pattern::<UISelectionItemPattern>() {
                        pattern.select().map_err(|e| e.to_string())
                    } else {
                        Err("Element does not support SelectionItem pattern".to_string())
                    }
                }
            }
        }).await;

        let (success, error) = match action_result {
            Ok(Ok(())) => (true, None),
            Ok(Err(e)) => (false, Some(e)),
            Err(e) => (false, Some(format!("Task failed: {}", e))),
        };

        let audit_id = self.repository.lock().await.record_audit(
            "activate_element",
            Some(&hash_string(&format!("{}:{:?}", request.element_id, request.action))),
            if success { "success" } else { "error" },
            error.as_deref(),
            None,
        ).unwrap_or_default();

        JsonRpcResponse::success(
            id,
            serde_json::to_value(ActionResponse {
                success,
                error,
                audit_id,
            })
            .unwrap(),
        )
    }

    /// Press a key
    async fn handle_press_key(
        &self,
        id: Option<serde_json::Value>,
        params: serde_json::Value,
    ) -> JsonRpcResponse {
        let request: PressKeyRequest = match serde_json::from_value(params) {
            Ok(r) => r,
            Err(e) => return JsonRpcResponse::invalid_params(id, e.to_string()),
        };

        // Use uiautomation's keyboard input
        let result = send_key_press(&request.key, &request.modifiers);

        let (success, error) = match result {
            Ok(_) => (true, None),
            Err(e) => (false, Some(e)),
        };

        let audit_id = self.repository.lock().await.record_audit(
            "press_key",
            Some(&hash_string(&format!("{:?}+{:?}", request.modifiers, request.key))),
            if success { "success" } else { "error" },
            error.as_deref(),
            None,
        ).unwrap_or_default();

        JsonRpcResponse::success(
            id,
            serde_json::to_value(ActionResponse {
                success,
                error,
                audit_id,
            })
            .unwrap(),
        )
    }

    /// Set clipboard
    async fn handle_set_clipboard(
        &self,
        id: Option<serde_json::Value>,
        params: serde_json::Value,
    ) -> JsonRpcResponse {
        let request: SetClipboardRequest = match serde_json::from_value(params) {
            Ok(r) => r,
            Err(e) => return JsonRpcResponse::invalid_params(id, e.to_string()),
        };

        // Check length limit
        if request.text.len() > 10240 {
            return JsonRpcResponse::invalid_params(id, "Text exceeds 10KB limit");
        }

        let result = set_clipboard_text(&request.text);

        let (success, error) = match result {
            Ok(_) => (true, None),
            Err(e) => (false, Some(e)),
        };

        let audit_id = self.repository.lock().await.record_audit(
            "set_clipboard",
            Some(&hash_string(&format!("len:{}", request.text.len()))),
            if success { "success" } else { "error" },
            error.as_deref(),
            None,
        ).unwrap_or_default();

        JsonRpcResponse::success(
            id,
            serde_json::to_value(ActionResponse {
                success,
                error,
                audit_id,
            })
            .unwrap(),
        )
    }

    /// List available tools
    async fn handle_tools_list(&self, id: Option<serde_json::Value>) -> JsonRpcResponse {
        let tools = super::tools::get_tool_definitions();
        let response = ToolsListResponse { tools };

        JsonRpcResponse::success(id, serde_json::to_value(response).unwrap())
    }

    /// Get safety statistics
    pub fn safety_stats(&self) -> crate::mcp::safety::SafetyStats {
        self.safety.stats()
    }
}

/// Convert accessibility tree to privacy-safe format
fn convert_element_tree(element: &crate::observer::accessibility::AccessibleElement) -> ElementNode {
    ElementNode {
        element_id: element.element_id.clone(),
        control_type: element.control_type.clone(),
        // PRIVACY: Use the pre-computed hash
        name_hash: element.name_hash.clone(),
        is_enabled: element.is_enabled,
        is_keyboard_focusable: element.is_keyboard_focusable,
        children: element.children.iter().map(convert_element_tree).collect(),
        supported_patterns: element.supported_patterns.clone(),
        bounds: element.bounds.map(|b| Rect {
            x: b.x,
            y: b.y,
            width: b.width,
            height: b.height,
        }),
    }
}

/// Hash a string for privacy
fn hash_string(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    hex::encode(hasher.finalize())[..16].to_string()
}

/// Send a key press using uiautomation's keyboard API
/// 
/// Uses the send_keys method which takes a special key syntax like:
/// - {ctrl}, {alt}, {shift}, {win} for modifiers
/// - {enter}, {tab}, {esc}, {space}, etc. for special keys
/// - (key) for grouped key combinations
fn send_key_press(key: &VirtualKeyCode, modifiers: &[Modifier]) -> Result<(), String> {
    use uiautomation::inputs::Keyboard;

    // Build the key sequence string
    let mut key_sequence = String::new();
    
    // Add modifiers
    for modifier in modifiers {
        match modifier {
            Modifier::Ctrl => key_sequence.push_str("{ctrl}"),
            Modifier::Alt => key_sequence.push_str("{alt}"),
            Modifier::Shift => key_sequence.push_str("{shift}"),
            Modifier::Win => key_sequence.push_str("{win}"),
        }
    }
    
    // Add the main key
    let key_str = map_virtual_key_to_string(key);
    
    // If we have modifiers, wrap the key in () for grouping
    if !modifiers.is_empty() {
        key_sequence.push('(');
        key_sequence.push_str(&key_str);
        key_sequence.push(')');
    } else {
        key_sequence.push_str(&key_str);
    }

    let keyboard = Keyboard::new();
    keyboard.send_keys(&key_sequence).map_err(|e| e.to_string())
}

/// Map our VirtualKeyCode to uiautomation key string format
fn map_virtual_key_to_string(key: &VirtualKeyCode) -> String {
    match key {
        VirtualKeyCode::A => "a".to_string(),
        VirtualKeyCode::B => "b".to_string(),
        VirtualKeyCode::C => "c".to_string(),
        VirtualKeyCode::D => "d".to_string(),
        VirtualKeyCode::E => "e".to_string(),
        VirtualKeyCode::F => "f".to_string(),
        VirtualKeyCode::G => "g".to_string(),
        VirtualKeyCode::H => "h".to_string(),
        VirtualKeyCode::I => "i".to_string(),
        VirtualKeyCode::J => "j".to_string(),
        VirtualKeyCode::K => "k".to_string(),
        VirtualKeyCode::L => "l".to_string(),
        VirtualKeyCode::M => "m".to_string(),
        VirtualKeyCode::N => "n".to_string(),
        VirtualKeyCode::O => "o".to_string(),
        VirtualKeyCode::P => "p".to_string(),
        VirtualKeyCode::Q => "q".to_string(),
        VirtualKeyCode::R => "r".to_string(),
        VirtualKeyCode::S => "s".to_string(),
        VirtualKeyCode::T => "t".to_string(),
        VirtualKeyCode::U => "u".to_string(),
        VirtualKeyCode::V => "v".to_string(),
        VirtualKeyCode::W => "w".to_string(),
        VirtualKeyCode::X => "x".to_string(),
        VirtualKeyCode::Y => "y".to_string(),
        VirtualKeyCode::Z => "z".to_string(),
        VirtualKeyCode::Num0 => "0".to_string(),
        VirtualKeyCode::Num1 => "1".to_string(),
        VirtualKeyCode::Num2 => "2".to_string(),
        VirtualKeyCode::Num3 => "3".to_string(),
        VirtualKeyCode::Num4 => "4".to_string(),
        VirtualKeyCode::Num5 => "5".to_string(),
        VirtualKeyCode::Num6 => "6".to_string(),
        VirtualKeyCode::Num7 => "7".to_string(),
        VirtualKeyCode::Num8 => "8".to_string(),
        VirtualKeyCode::Num9 => "9".to_string(),
        VirtualKeyCode::F1 => "{F1}".to_string(),
        VirtualKeyCode::F2 => "{F2}".to_string(),
        VirtualKeyCode::F3 => "{F3}".to_string(),
        VirtualKeyCode::F4 => "{F4}".to_string(),
        VirtualKeyCode::F5 => "{F5}".to_string(),
        VirtualKeyCode::F6 => "{F6}".to_string(),
        VirtualKeyCode::F7 => "{F7}".to_string(),
        VirtualKeyCode::F8 => "{F8}".to_string(),
        VirtualKeyCode::F9 => "{F9}".to_string(),
        VirtualKeyCode::F10 => "{F10}".to_string(),
        VirtualKeyCode::F11 => "{F11}".to_string(),
        VirtualKeyCode::F12 => "{F12}".to_string(),
        VirtualKeyCode::Enter => "{enter}".to_string(),
        VirtualKeyCode::Tab => "{tab}".to_string(),
        VirtualKeyCode::Escape => "{esc}".to_string(),
        VirtualKeyCode::Space => "{space}".to_string(),
        VirtualKeyCode::Backspace => "{backspace}".to_string(),
        VirtualKeyCode::ArrowUp => "{up}".to_string(),
        VirtualKeyCode::ArrowDown => "{down}".to_string(),
        VirtualKeyCode::ArrowLeft => "{left}".to_string(),
        VirtualKeyCode::ArrowRight => "{right}".to_string(),
        VirtualKeyCode::Home => "{home}".to_string(),
        VirtualKeyCode::End => "{end}".to_string(),
        VirtualKeyCode::PageUp => "{pageup}".to_string(),
        VirtualKeyCode::PageDown => "{pagedown}".to_string(),
        VirtualKeyCode::Insert => "{insert}".to_string(),
        VirtualKeyCode::Delete => "{delete}".to_string(),
    }
}

/// Set clipboard text using Windows APIs
fn set_clipboard_text(text: &str) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

    // CF_UNICODETEXT = 13
    const CF_UNICODETEXT: u32 = 13;

    unsafe {
        // Open clipboard
        OpenClipboard(None).map_err(|e| format!("Failed to open clipboard: {}", e))?;

        // Empty it
        if let Err(e) = EmptyClipboard() {
            let _ = CloseClipboard();
            return Err(format!("Failed to empty clipboard: {}", e));
        }

        // Convert text to wide string
        let wide: Vec<u16> = OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let size = wide.len() * 2;

        // Allocate global memory
        let hmem = GlobalAlloc(GMEM_MOVEABLE, size).map_err(|e| {
            let _ = CloseClipboard();
            format!("Failed to allocate memory: {}", e)
        })?;

        // Copy data
        let ptr = GlobalLock(hmem);
        if ptr.is_null() {
            let _ = CloseClipboard();
            return Err("Failed to lock memory".to_string());
        }

        std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr as *mut u16, wide.len());
        let _ = GlobalUnlock(hmem);

        // Set clipboard data
        let result = SetClipboardData(CF_UNICODETEXT, HANDLE(hmem.0 as *mut _));

        let _ = CloseClipboard();

        result.map_err(|e| format!("Failed to set clipboard data: {}", e))?;

        Ok(())
    }
}
