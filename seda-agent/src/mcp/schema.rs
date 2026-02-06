//! MCP request/response schemas
//!
//! Defines the JSON-RPC 2.0 message types for the MCP server.
//!
//! # Safety Design
//!
//! - Window titles are HASHED before returning (may contain PII)
//! - Element names are HASHED before returning (may contain user data)
//! - No raw content is ever exposed through the API

use serde::{Deserialize, Serialize};

// ============================================================================
// JSON-RPC 2.0 Base Types
// ============================================================================

/// JSON-RPC 2.0 request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// JSON-RPC 2.0 response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    /// Create a success response
    pub fn success(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create an error response
    pub fn error(id: Option<serde_json::Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }

    /// Create a method not found error
    pub fn method_not_found(id: Option<serde_json::Value>, method: &str) -> Self {
        Self::error(id, -32601, format!("Method not found: {}", method))
    }

    /// Create an invalid params error
    pub fn invalid_params(id: Option<serde_json::Value>, message: impl Into<String>) -> Self {
        Self::error(id, -32602, message)
    }

    /// Create an internal error
    pub fn internal_error(id: Option<serde_json::Value>, message: impl Into<String>) -> Self {
        Self::error(id, -32603, message)
    }

    /// Create a forbidden error (custom code for safety violations)
    pub fn forbidden(id: Option<serde_json::Value>, message: impl Into<String>) -> Self {
        Self::error(id, -32000, message)
    }

    /// Create a rate limited error
    pub fn rate_limited(id: Option<serde_json::Value>) -> Self {
        Self::error(id, -32001, "Rate limit exceeded")
    }
}

// ============================================================================
// READ-ONLY METHODS (Safe)
// ============================================================================

/// Request to list windows
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListWindowsRequest {
    /// Whether to include hidden windows (default: false)
    #[serde(default)]
    pub include_hidden: bool,
}

/// Window information (privacy-safe)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    /// Opaque window handle (string representation)
    pub hwnd: String,
    /// Process name (e.g., "chrome.exe")
    pub process_name: String,
    /// HASH of the window title (NEVER the actual title)
    ///
    /// # Privacy Note
    /// Window titles may contain document names, URLs, or PII.
    /// We only expose a hash for identification purposes.
    pub title_hash: String,
    /// Whether this window is currently focused
    pub is_focused: bool,
    /// Window bounds (optional)
    pub bounds: Option<Rect>,
}

/// Rectangle bounds
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Request to get UI element tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetWindowTreeRequest {
    /// Window handle
    pub hwnd: String,
    /// Maximum depth to traverse (default: 3, max: 10)
    #[serde(default = "default_max_depth")]
    pub max_depth: u32,
}

fn default_max_depth() -> u32 {
    3
}

/// UI element node (privacy-safe)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementNode {
    /// Unique element ID (for subsequent operations)
    pub element_id: String,
    /// Control type (Button, Edit, Text, etc.)
    pub control_type: String,
    /// HASH of element name (NEVER the actual name)
    ///
    /// # Privacy Note
    /// Element names may contain user data.
    /// We only expose a hash for identification.
    pub name_hash: Option<String>,
    /// Whether the element is enabled
    pub is_enabled: bool,
    /// Whether the element can receive keyboard focus
    pub is_keyboard_focusable: bool,
    /// Child elements
    pub children: Vec<ElementNode>,
    /// Supported UI Automation patterns
    pub supported_patterns: Vec<String>,
    /// Element bounds (optional)
    pub bounds: Option<Rect>,
}

/// Request to get patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPatternsRequest {
    /// Minimum frequency (default: 3)
    #[serde(default = "default_min_frequency")]
    pub min_frequency: u32,
    /// Hours to look back (default: 24, max: 168)
    #[serde(default = "default_hours_back")]
    pub hours_back: u32,
}

fn default_min_frequency() -> u32 {
    3
}

fn default_hours_back() -> u32 {
    24
}

// ============================================================================
// RESTRICTED ACTION METHODS
// ============================================================================

/// Element actions that can be performed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElementAction {
    /// Click the element (Invoke pattern)
    Click,
    /// Focus the element
    Focus,
    /// Expand (for expandable controls)
    Expand,
    /// Collapse (for expandable controls)
    Collapse,
    /// Select (for selectable items)
    Select,
}

/// Request to activate a UI element
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivateElementRequest {
    /// Window handle
    pub hwnd: String,
    /// Element ID (from GetWindowTree)
    pub element_id: String,
    /// Action to perform
    pub action: ElementAction,
}

/// Keyboard modifier keys
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Win,
}

/// Virtual key codes (ENUMERATED - no arbitrary keys)
///
/// # Safety Decision
///
/// Only predefined keys are allowed. This prevents:
/// - Arbitrary string injection
/// - Unintended key sequences
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VirtualKeyCode {
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
}

/// Request to press a key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PressKeyRequest {
    /// Key to press
    pub key: VirtualKeyCode,
    /// Modifier keys to hold
    #[serde(default)]
    pub modifiers: Vec<Modifier>,
}

/// Request to set clipboard content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetClipboardRequest {
    /// Text to set (limited length for safety)
    pub text: String,
}

/// Response for action methods
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResponse {
    /// Whether the action succeeded
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// Audit ID for tracking
    pub audit_id: String,
}

// ============================================================================
// MCP TOOL DEFINITIONS
// ============================================================================

/// MCP tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool name
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// JSON Schema for input parameters
    pub input_schema: serde_json::Value,
}

/// MCP tools list response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsListResponse {
    pub tools: Vec<ToolDefinition>,
}

// ============================================================================
// EXPLICITLY FORBIDDEN OPERATIONS
// ============================================================================

/// Operations that are EXPLICITLY FORBIDDEN
///
/// These will NEVER be implemented. This enum exists to document
/// what is intentionally NOT allowed.
#[derive(Debug, Clone, Copy)]
pub enum ForbiddenOperation {
    /// Move mouse to arbitrary coordinates
    MoveMouse,
    /// Execute shell commands
    ExecuteShell,
    /// Send arbitrary text (use press_key for individual keys)
    SendText,
    /// Read clipboard content
    ReadClipboard,
    /// Capture screen or window images
    CaptureScreen,
    /// Access file system
    FileSystemAccess,
    /// Make network requests
    NetworkRequest,
    /// Launch processes
    LaunchProcess,
}

impl ForbiddenOperation {
    /// Get the reason why this operation is forbidden
    pub fn reason(&self) -> &'static str {
        match self {
            Self::MoveMouse => "Arbitrary mouse movement can leak screen content through coordinates",
            Self::ExecuteShell => "Shell execution bypasses all safety boundaries",
            Self::SendText => "Arbitrary text input could inject commands or sensitive data",
            Self::ReadClipboard => "Clipboard may contain sensitive user data",
            Self::CaptureScreen => "Screen capture violates privacy-by-design principle",
            Self::FileSystemAccess => "File system access is outside agent scope",
            Self::NetworkRequest => "Agent must be local-only with no outbound connections",
            Self::LaunchProcess => "Process launching bypasses controlled execution",
        }
    }
}
