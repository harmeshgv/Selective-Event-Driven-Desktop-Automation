//! Symbolic action definitions
//!
//! These are the privacy-safe representations of user actions.
//! The key privacy design decisions:
//!
//! 1. NO raw text content (window titles, clipboard text, typed text)
//! 2. Only process names, not command-line arguments
//! 3. Content types instead of actual content
//! 4. Element types instead of element content/names

use serde::{Deserialize, Serialize};

/// Categories of applications for pattern recognition
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AppCategory {
    /// Web browsers (Chrome, Firefox, Edge, etc.)
    Browser,
    /// Code editors and IDEs
    Editor,
    /// Terminal/command line applications
    Terminal,
    /// Email clients
    Email,
    /// Chat/messaging applications
    Chat,
    /// Document editors (Word, Google Docs, etc.)
    Document,
    /// Spreadsheet applications
    Spreadsheet,
    /// File managers/explorers
    FileManager,
    /// Media players
    Media,
    /// System utilities
    System,
    /// Unknown/uncategorized
    Other,
}

impl AppCategory {
    /// Categorize an application by its process name
    pub fn from_process_name(name: &str) -> Self {
        let name_lower = name.to_lowercase();

        // Browsers
        if name_lower.contains("chrome")
            || name_lower.contains("firefox")
            || name_lower.contains("edge")
            || name_lower.contains("safari")
            || name_lower.contains("opera")
            || name_lower.contains("brave")
        {
            return AppCategory::Browser;
        }

        // Editors/IDEs
        if name_lower.contains("code")
            || name_lower.contains("cursor")
            || name_lower.contains("sublime")
            || name_lower.contains("notepad")
            || name_lower.contains("vim")
            || name_lower.contains("emacs")
            || name_lower.contains("idea")
            || name_lower.contains("pycharm")
            || name_lower.contains("webstorm")
            || name_lower.contains("atom")
        {
            return AppCategory::Editor;
        }

        // Terminals
        if name_lower.contains("terminal")
            || name_lower.contains("cmd")
            || name_lower.contains("powershell")
            || name_lower.contains("wt")
            || name_lower.contains("windowsterminal")
            || name_lower.contains("alacritty")
            || name_lower.contains("iterm")
            || name_lower.contains("conhost")
        {
            return AppCategory::Terminal;
        }

        // Email
        if name_lower.contains("outlook")
            || name_lower.contains("thunderbird")
            || name_lower.contains("mail")
        {
            return AppCategory::Email;
        }

        // Chat
        if name_lower.contains("slack")
            || name_lower.contains("discord")
            || name_lower.contains("teams")
            || name_lower.contains("telegram")
            || name_lower.contains("signal")
            || name_lower.contains("whatsapp")
        {
            return AppCategory::Chat;
        }

        // Documents
        if name_lower.contains("word")
            || name_lower.contains("winword")
            || name_lower.contains("libreoffice")
            || name_lower.contains("writer")
        {
            return AppCategory::Document;
        }

        // Spreadsheets
        if name_lower.contains("excel") || name_lower.contains("calc") {
            return AppCategory::Spreadsheet;
        }

        // File managers
        if name_lower.contains("explorer") || name_lower.contains("finder") {
            return AppCategory::FileManager;
        }

        // Media
        if name_lower.contains("vlc")
            || name_lower.contains("spotify")
            || name_lower.contains("itunes")
            || name_lower.contains("wmplayer")
        {
            return AppCategory::Media;
        }

        // System
        if name_lower.contains("taskmgr")
            || name_lower.contains("control")
            || name_lower.contains("settings")
            || name_lower.contains("systemsettings")
        {
            return AppCategory::System;
        }

        AppCategory::Other
    }
}

/// Identifies an application without storing sensitive information
///
/// # Privacy Guarantee
///
/// - Only the process name is stored, not window titles or command-line args
/// - Category is derived from the process name for pattern recognition
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AppIdentifier {
    /// Process name (e.g., "chrome.exe", "Code.exe")
    pub process_name: String,
    /// Application category for pattern recognition
    pub category: AppCategory,
}

impl AppIdentifier {
    /// Create a new app identifier from a process name
    pub fn new(process_name: impl Into<String>) -> Self {
        let name = process_name.into();
        let category = AppCategory::from_process_name(&name);
        Self {
            process_name: name,
            category,
        }
    }

    /// Create an unknown/empty app identifier
    pub fn unknown() -> Self {
        Self {
            process_name: "unknown".to_string(),
            category: AppCategory::Other,
        }
    }
}

/// Types of content (not the actual content)
///
/// # Privacy Guarantee
///
/// We store the TYPE of content, never the content itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContentType {
    /// Plain text
    PlainText,
    /// Rich/formatted text
    RichText,
    /// HTML content
    Html,
    /// Image data
    Image,
    /// File reference(s)
    Files,
    /// Code/source
    Code,
    /// URL/link
    Url,
    /// Unknown content type
    Unknown,
}

/// Types of UI elements (for interaction patterns)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ElementType {
    Button,
    TextField,
    Link,
    Menu,
    MenuItem,
    Tab,
    ListItem,
    TreeItem,
    Checkbox,
    RadioButton,
    Dropdown,
    Slider,
    ScrollBar,
    Dialog,
    Window,
    Other,
}

/// Types of interactions with UI elements
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InteractionType {
    Click,
    DoubleClick,
    RightClick,
    Focus,
    Select,
    Expand,
    Collapse,
    Scroll,
    Drag,
    Drop,
}

/// Navigation actions within an application
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NavigationAction {
    Forward,
    Back,
    Up,
    Down,
    Home,
    End,
    Search,
    NewTab,
    CloseTab,
    SwitchTab,
}

/// Types of field content (without the actual content)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FieldType {
    /// General text input
    Text,
    /// Password field
    Password,
    /// Email address field
    Email,
    /// URL/address bar
    Url,
    /// Search box
    Search,
    /// Numeric input
    Number,
    /// Code/source input
    Code,
    /// Other/unknown
    Other,
}

/// Simple symbolic action type for pattern matching
///
/// This is a simplified representation for sequence mining.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SymbolicActionType {
    SwitchApp,
    OpenApp,
    CloseApp,
    CopyText,
    PasteText,
    TypeText,
    Navigate,
    Interact,
    VisitWebsite,
    SearchWeb,
}

impl std::fmt::Display for SymbolicActionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SymbolicActionType::SwitchApp => write!(f, "SWITCH_APP"),
            SymbolicActionType::OpenApp => write!(f, "OPEN_APP"),
            SymbolicActionType::CloseApp => write!(f, "CLOSE_APP"),
            SymbolicActionType::CopyText => write!(f, "COPY_TEXT"),
            SymbolicActionType::PasteText => write!(f, "PASTE_TEXT"),
            SymbolicActionType::TypeText => write!(f, "TYPE_TEXT"),
            SymbolicActionType::Navigate => write!(f, "NAVIGATE"),
            SymbolicActionType::Interact => write!(f, "INTERACT"),
            SymbolicActionType::VisitWebsite => write!(f, "VISIT_WEBSITE"),
            SymbolicActionType::SearchWeb => write!(f, "SEARCH_WEB"),
        }
    }
}

/// Symbolic representation of a user action
///
/// # Privacy By Design
///
/// This enum is carefully designed to be privacy-safe:
/// - No raw text content is ever stored
/// - Window titles are NOT included
/// - Only process names and content types are captured
/// - All personal data is stripped before reaching this point
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SymbolicAction {
    /// User switched from one application to another
    SwitchApp {
        from_app: AppIdentifier,
        to_app: AppIdentifier,
    },

    /// User opened a new application
    OpenApp {
        app: AppIdentifier,
    },

    /// User closed an application
    CloseApp {
        app: AppIdentifier,
    },

    /// User copied content to clipboard
    ///
    /// # Privacy Note
    /// Only the SOURCE app and content TYPE are stored.
    /// The actual copied content is NEVER captured.
    CopyText {
        source_app: AppIdentifier,
        content_type: ContentType,
    },

    /// User pasted content from clipboard
    ///
    /// # Privacy Note
    /// Only the TARGET app is stored.
    /// The pasted content is NEVER captured.
    PasteText {
        target_app: AppIdentifier,
    },

    /// User typed text in a field
    ///
    /// # Privacy Note
    /// Only the TARGET app and field TYPE are stored.
    /// The actual typed text is NEVER captured.
    TypeText {
        target_app: AppIdentifier,
        field_type: FieldType,
    },

    /// User performed navigation within an app
    Navigate {
        app: AppIdentifier,
        action: NavigationAction,
    },

    /// User interacted with a UI element
    Interact {
        app: AppIdentifier,
        element_type: ElementType,
        interaction: InteractionType,
    },

    /// User visited a website in a browser
    VisitWebsite {
        browser_app: AppIdentifier,
        url: String,
        domain: String,
    },

    /// User performed a web search
    SearchWeb {
        browser_app: AppIdentifier,
        engine: Option<String>,
        query: String,
        url: String,
        domain: String,
    },
}

impl SymbolicAction {
    /// Get the action type for pattern matching
    pub fn action_type(&self) -> SymbolicActionType {
        match self {
            SymbolicAction::SwitchApp { .. } => SymbolicActionType::SwitchApp,
            SymbolicAction::OpenApp { .. } => SymbolicActionType::OpenApp,
            SymbolicAction::CloseApp { .. } => SymbolicActionType::CloseApp,
            SymbolicAction::CopyText { .. } => SymbolicActionType::CopyText,
            SymbolicAction::PasteText { .. } => SymbolicActionType::PasteText,
            SymbolicAction::TypeText { .. } => SymbolicActionType::TypeText,
            SymbolicAction::Navigate { .. } => SymbolicActionType::Navigate,
            SymbolicAction::Interact { .. } => SymbolicActionType::Interact,
            SymbolicAction::VisitWebsite { .. } => SymbolicActionType::VisitWebsite,
            SymbolicAction::SearchWeb { .. } => SymbolicActionType::SearchWeb,
        }
    }

    /// Get the primary app involved in this action
    pub fn primary_app(&self) -> &AppIdentifier {
        match self {
            SymbolicAction::SwitchApp { to_app, .. } => to_app,
            SymbolicAction::OpenApp { app } => app,
            SymbolicAction::CloseApp { app } => app,
            SymbolicAction::CopyText { source_app, .. } => source_app,
            SymbolicAction::PasteText { target_app } => target_app,
            SymbolicAction::TypeText { target_app, .. } => target_app,
            SymbolicAction::Navigate { app, .. } => app,
            SymbolicAction::Interact { app, .. } => app,
            SymbolicAction::VisitWebsite { browser_app, .. } => browser_app,
            SymbolicAction::SearchWeb { browser_app, .. } => browser_app,
        }
    }

    /// Get a unique key for this action type + context (for graph nodes)
    pub fn node_key(&self) -> String {
        match self {
            SymbolicAction::SwitchApp { from_app, to_app } => {
                format!("switch:{}:{}", from_app.process_name, to_app.process_name)
            }
            SymbolicAction::OpenApp { app } => format!("open:{}", app.process_name),
            SymbolicAction::CloseApp { app } => format!("close:{}", app.process_name),
            SymbolicAction::CopyText {
                source_app,
                content_type,
            } => format!("copy:{}:{:?}", source_app.process_name, content_type),
            SymbolicAction::PasteText { target_app } => {
                format!("paste:{}", target_app.process_name)
            }
            SymbolicAction::TypeText {
                target_app,
                field_type,
            } => format!("type:{}:{:?}", target_app.process_name, field_type),
            SymbolicAction::Navigate { app, action } => {
                format!("nav:{}:{:?}", app.process_name, action)
            }
            SymbolicAction::Interact {
                app,
                element_type,
                interaction,
            } => format!(
                "interact:{}:{:?}:{:?}",
                app.process_name, element_type, interaction
            ),
            SymbolicAction::VisitWebsite {
                browser_app,
                domain,
                ..
            } => format!("visit:{}:{}", browser_app.process_name, domain),
            SymbolicAction::SearchWeb {
                browser_app,
                engine,
                ..
            } => format!(
                "search:{}:{}",
                browser_app.process_name,
                engine.as_deref().unwrap_or("unknown")
            ),
        }
    }
}
