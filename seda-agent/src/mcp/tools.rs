//! MCP tool definitions
//!
//! Defines the tools exposed via MCP.

use serde_json::json;

use super::schema::ToolDefinition;

/// Get all tool definitions for the MCP server
pub fn get_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        // ============== READ-ONLY TOOLS ==============
        ToolDefinition {
            name: "list_windows".to_string(),
            description: "List all visible application windows. Returns process names and window handle identifiers (titles are hashed for privacy).".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "include_hidden": {
                        "type": "boolean",
                        "default": false,
                        "description": "Whether to include hidden windows"
                    }
                }
            }),
        },
        ToolDefinition {
            name: "get_window_tree".to_string(),
            description: "Get the UI element tree for a window. Element names are hashed for privacy. Max depth is 10.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "hwnd": {
                        "type": "string",
                        "description": "Window handle from list_windows"
                    },
                    "max_depth": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 10,
                        "default": 3,
                        "description": "Maximum depth to traverse the UI tree"
                    }
                },
                "required": ["hwnd"]
            }),
        },
        ToolDefinition {
            name: "get_patterns".to_string(),
            description: "Get detected action patterns from the task graph. Returns patterns with frequency and time estimates.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "min_frequency": {
                        "type": "integer",
                        "minimum": 2,
                        "default": 3,
                        "description": "Minimum pattern frequency to include"
                    },
                    "hours_back": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 168,
                        "default": 24,
                        "description": "Hours of history to analyze"
                    }
                }
            }),
        },
        ToolDefinition {
            name: "get_transitions".to_string(),
            description: "Get action transitions from the task graph. Returns edges with frequency and duration.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "min_frequency": {
                        "type": "integer",
                        "minimum": 1,
                        "default": 2,
                        "description": "Minimum transition frequency to include"
                    }
                }
            }),
        },
        
        // ============== RESTRICTED ACTION TOOLS ==============
        ToolDefinition {
            name: "activate_element".to_string(),
            description: "Activate a UI element (click, focus, expand, collapse, or select). Requires element_id from get_window_tree.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "hwnd": {
                        "type": "string",
                        "description": "Window handle"
                    },
                    "element_id": {
                        "type": "string",
                        "description": "Element ID from get_window_tree"
                    },
                    "action": {
                        "type": "string",
                        "enum": ["Click", "Focus", "Expand", "Collapse", "Select"],
                        "description": "Action to perform on the element"
                    }
                },
                "required": ["hwnd", "element_id", "action"]
            }),
        },
        ToolDefinition {
            name: "press_key".to_string(),
            description: "Press a keyboard key with optional modifiers. Only predefined keys are allowed.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "enum": [
                            "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M",
                            "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
                            "Num0", "Num1", "Num2", "Num3", "Num4", "Num5", "Num6", "Num7", "Num8", "Num9",
                            "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
                            "Enter", "Tab", "Escape", "Space", "Backspace",
                            "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight",
                            "Home", "End", "PageUp", "PageDown",
                            "Insert", "Delete"
                        ],
                        "description": "Key to press"
                    },
                    "modifiers": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "enum": ["Ctrl", "Alt", "Shift", "Win"]
                        },
                        "default": [],
                        "description": "Modifier keys to hold"
                    }
                },
                "required": ["key"]
            }),
        },
        ToolDefinition {
            name: "set_clipboard".to_string(),
            description: "Set clipboard text content. Limited to 10KB for safety.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "maxLength": 10240,
                        "description": "Text to set on clipboard"
                    }
                },
                "required": ["text"]
            }),
        },
    ]
}

/// Check if a tool is read-only
pub fn is_read_only_tool(name: &str) -> bool {
    matches!(
        name,
        "list_windows" | "get_window_tree" | "get_patterns" | "get_transitions"
    )
}

/// Check if a tool is allowed
pub fn is_allowed_tool(name: &str) -> bool {
    let allowed = [
        "list_windows",
        "get_window_tree",
        "get_patterns",
        "get_transitions",
        "activate_element",
        "press_key",
        "set_clipboard",
    ];
    allowed.contains(&name)
}

/// Get the list of explicitly forbidden operations
pub fn get_forbidden_operations() -> Vec<(&'static str, &'static str)> {
    vec![
        ("move_mouse", "Arbitrary mouse movement can leak screen content"),
        ("execute_shell", "Shell execution bypasses all safety boundaries"),
        ("send_text", "Use press_key for controlled text input"),
        ("read_clipboard", "Clipboard may contain sensitive data"),
        ("capture_screen", "Screen capture violates privacy-by-design"),
        ("file_read", "File system access is outside agent scope"),
        ("file_write", "File system access is outside agent scope"),
        ("network_request", "Agent must be local-only"),
        ("launch_process", "Process launching bypasses controlled execution"),
    ]
}
