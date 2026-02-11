//! UI Automation wrapper
//!
//! Provides access to the Windows UI Automation API for inspecting
//! UI element trees and performing accessible actions.
//!
//! # Safety Decision
//!
//! This module only provides READ access to UI elements.
//! Write operations (clicking, typing) are handled through MCP
//! with explicit safety checks.

use sha2::{Digest, Sha256};
use uiautomation::patterns::UIValuePattern;
use uiautomation::types::ControlType;
use uiautomation::types::TreeScope;
use uiautomation::types::UIProperty;
use uiautomation::UIAutomation;
use uiautomation::UIElement;

use super::ObserverError;

/// Represents a UI element in the accessibility tree
#[derive(Debug, Clone)]
pub struct AccessibleElement {
    /// Unique identifier for this element (runtime ID hash)
    pub element_id: String,
    /// Control type (Button, Edit, Text, etc.)
    pub control_type: String,
    /// Name of the element (hashed for privacy in API responses)
    pub name: Option<String>,
    /// Name hash (for API responses - privacy safe)
    pub name_hash: Option<String>,
    /// Whether the element is enabled
    pub is_enabled: bool,
    /// Whether the element can receive keyboard focus
    pub is_keyboard_focusable: bool,
    /// Bounding rectangle
    pub bounds: Option<ElementBounds>,
    /// Supported UI Automation patterns
    pub supported_patterns: Vec<String>,
    /// Child elements
    pub children: Vec<AccessibleElement>,
}

/// Bounding rectangle for an element
#[derive(Debug, Clone, Copy)]
pub struct ElementBounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Wrapper around Windows UI Automation
pub struct AccessibilityTree {
    automation: UIAutomation,
}

impl AccessibilityTree {
    /// Create a new accessibility tree wrapper
    pub fn new() -> Result<Self, ObserverError> {
        let automation = UIAutomation::new()
            .map_err(|e| ObserverError::UiAutomationError(e.to_string()))?;

        Ok(Self { automation })
    }

    /// Get the element tree for a window
    ///
    /// # Arguments
    /// * `hwnd` - Window handle
    /// * `max_depth` - Maximum depth to traverse (default 3, max 10)
    pub fn get_element_tree(
        &self,
        hwnd: isize,
        max_depth: u32,
    ) -> Result<AccessibleElement, ObserverError> {
        let max_depth = max_depth.min(10).max(1);

        // Get the root element for this window
        let handle = uiautomation::types::Handle::from(hwnd);
        let root = self
            .automation
            .element_from_handle(handle)
            .map_err(|e| ObserverError::UiAutomationError(e.to_string()))?;

        // Build the tree recursively
        self.build_element_tree(&root, max_depth, 0)
    }

    /// Build element tree recursively
    fn build_element_tree(
        &self,
        element: &uiautomation::UIElement,
        max_depth: u32,
        current_depth: u32,
    ) -> Result<AccessibleElement, ObserverError> {
        // Get basic properties
        let control_type = element
            .get_control_type()
            .map(|ct| format!("{:?}", ct))
            .unwrap_or_else(|_| "Unknown".to_string());

        let name = element.get_name().ok();
        let name_hash = name.as_ref().map(|n| hash_string(n));

        let is_enabled = element.is_enabled().unwrap_or(false);
        let is_keyboard_focusable = element.is_keyboard_focusable().unwrap_or(false);

        // Get bounding rectangle
        let bounds = element.get_bounding_rectangle().ok().map(|r| ElementBounds {
            x: r.get_left(),
            y: r.get_top(),
            width: r.get_width(),
            height: r.get_height(),
        });

        // Generate element ID from runtime ID
        let element_id = element
            .get_runtime_id()
            .map(|ids| {
                let mut hasher = Sha256::new();
                for id in ids {
                    hasher.update(id.to_le_bytes());
                }
                hex::encode(hasher.finalize())[..16].to_string()
            })
            .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());

        // Get supported patterns
        let supported_patterns = get_supported_patterns(element);

        // Get children if we haven't reached max depth
        let children = if current_depth < max_depth {
            let walker = self
                .automation
                .create_tree_walker()
                .map_err(|e| ObserverError::UiAutomationError(e.to_string()))?;

            let mut children = Vec::new();

            if let Ok(child_elements) = element.find_all(
                TreeScope::Children,
                &self
                    .automation
                    .create_true_condition()
                    .map_err(|e| ObserverError::UiAutomationError(e.to_string()))?,
            ) {
                for child in child_elements {
                    if let Ok(child_tree) =
                        self.build_element_tree(&child, max_depth, current_depth + 1)
                    {
                        children.push(child_tree);
                    }
                }
            }

            // Fallback to walker if find_all doesn't work well
            if children.is_empty() {
                if let Ok(first_child) = walker.get_first_child(element) {
                    if let Ok(child_tree) =
                        self.build_element_tree(&first_child, max_depth, current_depth + 1)
                    {
                        children.push(child_tree);
                    }

                    let mut current = first_child;
                    while let Ok(sibling) = walker.get_next_sibling(&current) {
                        if let Ok(sibling_tree) =
                            self.build_element_tree(&sibling, max_depth, current_depth + 1)
                        {
                            children.push(sibling_tree);
                        }
                        current = sibling;

                        // Safety limit
                        if children.len() > 100 {
                            break;
                        }
                    }
                }
            }

            children
        } else {
            Vec::new()
        };

        Ok(AccessibleElement {
            element_id,
            control_type,
            name,
            name_hash,
            is_enabled,
            is_keyboard_focusable,
            bounds,
            supported_patterns,
            children,
        })
    }

    /// Find an element by its ID in a window
    pub fn find_element_by_id(
        &self,
        hwnd: isize,
        element_id: &str,
    ) -> Result<uiautomation::UIElement, ObserverError> {
        let handle = uiautomation::types::Handle::from(hwnd);
        let root = self
            .automation
            .element_from_handle(handle)
            .map_err(|e| ObserverError::UiAutomationError(e.to_string()))?;

        // Search for the element with matching ID
        self.find_element_recursive(&root, element_id, 10, 0)
    }

    fn find_element_recursive(
        &self,
        element: &uiautomation::UIElement,
        target_id: &str,
        max_depth: u32,
        current_depth: u32,
    ) -> Result<uiautomation::UIElement, ObserverError> {
        // Check if this is the element
        let element_id = element
            .get_runtime_id()
            .map(|ids| {
                let mut hasher = Sha256::new();
                for id in ids {
                    hasher.update(id.to_le_bytes());
                }
                hex::encode(hasher.finalize())[..16].to_string()
            })
            .unwrap_or_default();

        if element_id == target_id {
            return Ok(element.clone());
        }

        if current_depth >= max_depth {
            return Err(ObserverError::UiAutomationError(
                "Element not found".to_string(),
            ));
        }

        // Search children
        let walker = self
            .automation
            .create_tree_walker()
            .map_err(|e| ObserverError::UiAutomationError(e.to_string()))?;

        if let Ok(first_child) = walker.get_first_child(element) {
            if let Ok(found) =
                self.find_element_recursive(&first_child, target_id, max_depth, current_depth + 1)
            {
                return Ok(found);
            }

            let mut current = first_child;
            while let Ok(sibling) = walker.get_next_sibling(&current) {
                if let Ok(found) =
                    self.find_element_recursive(&sibling, target_id, max_depth, current_depth + 1)
                {
                    return Ok(found);
                }
                current = sibling;
            }
        }

        Err(ObserverError::UiAutomationError(
            "Element not found".to_string(),
        ))
    }

    /// Get the focused element
    pub fn get_focused_element(&self) -> Result<AccessibleElement, ObserverError> {
        let focused = self
            .automation
            .get_focused_element()
            .map_err(|e| ObserverError::UiAutomationError(e.to_string()))?;

        self.build_element_tree(&focused, 1, 0)
    }
}

/// Best-effort extraction of a browser URL from a window using UI Automation.
///
/// This scans editable controls in the window subtree and returns the first value
/// that looks like a URL or domain.
pub fn extract_browser_url_for_window(hwnd: isize) -> Option<String> {
    let automation = UIAutomation::new().ok()?;
    let handle = uiautomation::types::Handle::from(hwnd);
    let root = automation.element_from_handle(handle).ok()?;

    let edits = automation
        .create_matcher()
        .from(root)
        .control_type(ControlType::Edit)
        .depth(10)
        .timeout(0)
        .find_all()
        .ok()?;

    let mut fallback_candidate: Option<String> = None;

    for edit in edits {
        if let Some(raw_value) = extract_text_value(&edit) {
            let normalized = normalize_url_like(raw_value.trim());
            if looks_like_url(&normalized) {
                return Some(normalized);
            }

            if fallback_candidate.is_none() && looks_like_domain(&normalized) {
                fallback_candidate = Some(normalized);
            }
        }
    }

    fallback_candidate
}

fn extract_text_value(element: &UIElement) -> Option<String> {
    if let Ok(pattern) = element.get_pattern::<UIValuePattern>() {
        if let Ok(value) = pattern.get_value() {
            if !value.trim().is_empty() {
                return Some(value);
            }
        }
    }

    if let Ok(value_variant) = element.get_property_value(UIProperty::ValueValue) {
        if let Ok(value) = value_variant.get_string() {
            if !value.trim().is_empty() {
                return Some(value);
            }
        }
    }

    if let Ok(name) = element.get_name() {
        if !name.trim().is_empty() {
            return Some(name);
        }
    }

    None
}

fn normalize_url_like(value: &str) -> String {
    let trimmed = value.trim().trim_matches('"').trim_matches('\'');
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
        || lower.starts_with("vivaldi://")
        || lower.starts_with("opera://")
        || lower.starts_with("file://")
    {
        trimmed.to_string()
    } else if trimmed.contains('.') && !trimmed.contains(' ') {
        format!("https://{}", trimmed)
    } else {
        trimmed.to_string()
    }
}

fn looks_like_url(value: &str) -> bool {
    let lower = value.to_lowercase();
    if lower.is_empty() {
        return false;
    }

    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("about:")
        || lower.starts_with("edge://")
        || lower.starts_with("chrome://")
        || lower.starts_with("brave://")
        || lower.starts_with("vivaldi://")
        || lower.starts_with("opera://")
    {
        return true;
    }

    looks_like_domain(value)
}

fn looks_like_domain(value: &str) -> bool {
    let val = value.trim();
    !val.is_empty()
        && val.contains('.')
        && !val.contains(' ')
        && !val.starts_with('/')
        && !val.starts_with('\\')
}

/// Get the list of supported UI Automation patterns for an element
fn get_supported_patterns(element: &uiautomation::UIElement) -> Vec<String> {
    use uiautomation::patterns::UIInvokePattern;
    use uiautomation::patterns::UISelectionPattern;
    use uiautomation::patterns::UIValuePattern;
    use uiautomation::patterns::UIExpandCollapsePattern;
    use uiautomation::patterns::UITogglePattern;
    use uiautomation::patterns::UIScrollPattern;
    
    let mut patterns = Vec::new();

    // Check common patterns using get_pattern::<T>()
    if element.get_pattern::<UIInvokePattern>().is_ok() {
        patterns.push("Invoke".to_string());
    }
    if element.get_pattern::<UISelectionPattern>().is_ok() {
        patterns.push("Selection".to_string());
    }
    if element.get_pattern::<UIValuePattern>().is_ok() {
        patterns.push("Value".to_string());
    }
    if element.get_pattern::<UIExpandCollapsePattern>().is_ok() {
        patterns.push("ExpandCollapse".to_string());
    }
    if element.get_pattern::<UITogglePattern>().is_ok() {
        patterns.push("Toggle".to_string());
    }
    if element.get_pattern::<UIScrollPattern>().is_ok() {
        patterns.push("Scroll".to_string());
    }

    patterns
}

/// Hash a string for privacy-safe storage/transmission
fn hash_string(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    hex::encode(hasher.finalize())[..16].to_string()
}
