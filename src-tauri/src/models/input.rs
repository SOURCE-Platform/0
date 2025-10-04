// Data models for input device events (keyboard and mouse)

use serde::{Deserialize, Serialize};

// ==============================================================================
// Keyboard Events
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardEvent {
    pub timestamp: i64,
    pub event_type: KeyEventType,
    pub key_code: u32,
    pub key_char: Option<char>,
    pub modifiers: ModifierState,
    pub app_context: AppContext,
    pub ui_element: Option<UiElement>,
    pub is_sensitive: bool, // True if in password field or other sensitive context
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyEventType {
    KeyDown,
    KeyUp,
}

impl KeyEventType {
    pub fn to_string(&self) -> &'static str {
        match self {
            KeyEventType::KeyDown => "key_down",
            KeyEventType::KeyUp => "key_up",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifierState {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool, // Cmd on macOS, Win on Windows
}

impl ModifierState {
    pub fn new() -> Self {
        Self {
            shift: false,
            ctrl: false,
            alt: false,
            meta: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.shift && !self.ctrl && !self.alt && !self.meta
    }

    pub fn to_string(&self) -> String {
        let mut parts = Vec::new();
        if self.meta {
            parts.push("Cmd");
        }
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        parts.join("+")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardShortcut {
    pub modifiers: ModifierState,
    pub key: String,
    pub display: String, // e.g., "⌘C", "Ctrl+C"
}

// ==============================================================================
// Application Context
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppContext {
    pub app_name: String,
    pub window_title: String,
    pub process_id: u32,
}

impl AppContext {
    pub fn new(app_name: String, window_title: String, process_id: u32) -> Self {
        Self {
            app_name,
            window_title,
            process_id,
        }
    }

    pub fn unknown() -> Self {
        Self {
            app_name: "Unknown".to_string(),
            window_title: "Unknown".to_string(),
            process_id: 0,
        }
    }
}

// ==============================================================================
// UI Element
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiElement {
    pub element_type: String, // "TextField", "Button", "SecureTextField", etc.
    pub label: Option<String>,
    pub role: String,
}

impl UiElement {
    pub fn new(element_type: String, label: Option<String>, role: String) -> Self {
        Self {
            element_type,
            label,
            role,
        }
    }

    /// Check if this UI element is likely a password or sensitive field
    pub fn is_sensitive(&self) -> bool {
        // Check element type
        if self.element_type.to_lowercase().contains("secure")
            || self.element_type.to_lowercase().contains("password")
        {
            return true;
        }

        // Check role
        if self.role.to_lowercase().contains("password")
            || self.role.to_lowercase().contains("secure")
        {
            return true;
        }

        // Check label
        if let Some(ref label) = self.label {
            let label_lower = label.to_lowercase();
            if label_lower.contains("password")
                || label_lower.contains("pin")
                || label_lower.contains("ssn")
                || label_lower.contains("credit card")
                || label_lower.contains("cvv")
                || label_lower.contains("security code")
            {
                return true;
            }
        }

        false
    }
}

// ==============================================================================
// Mouse Events
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseEvent {
    pub timestamp: i64,
    pub event_type: MouseEventType,
    pub position: Point,
    pub app_context: AppContext,
    pub ui_element: Option<UiElement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MouseEventType {
    Move { target: Point },
    LeftClick,
    RightClick,
    MiddleClick,
    DoubleClick,
    DragStart { start_pos: Point },
    DragMove { current_pos: Point },
    DragEnd { end_pos: Point },
    ScrollWheel { delta_x: i32, delta_y: i32 },
}

impl MouseEventType {
    pub fn to_string(&self) -> &'static str {
        match self {
            MouseEventType::Move { .. } => "move",
            MouseEventType::LeftClick => "left_click",
            MouseEventType::RightClick => "right_click",
            MouseEventType::MiddleClick => "middle_click",
            MouseEventType::DoubleClick => "double_click",
            MouseEventType::DragStart { .. } => "drag_start",
            MouseEventType::DragMove { .. } => "drag_move",
            MouseEventType::DragEnd { .. } => "drag_end",
            MouseEventType::ScrollWheel { .. } => "scroll_wheel",
        }
    }
}

// ==============================================================================
// Keyboard Statistics
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardStats {
    pub session_id: String,
    pub total_keystrokes: u64,
    pub keys_per_minute: f32,
    pub most_used_keys: Vec<(char, u32)>,
    pub shortcut_usage: Vec<(String, u32)>, // (shortcut like "Cmd+C", count)
    pub typing_speed_wpm: Option<f32>,      // Words per minute if detectable
}

// ==============================================================================
// Mouse Statistics
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseStats {
    pub session_id: String,
    pub total_clicks: u64,
    pub total_distance_pixels: u64,
    pub left_clicks: u32,
    pub right_clicks: u32,
    pub middle_clicks: u32,
    pub double_clicks: u32,
    pub scroll_events: u32,
    pub drag_operations: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modifier_state_to_string() {
        let mut mods = ModifierState::new();
        assert_eq!(mods.to_string(), "");

        mods.meta = true;
        mods.shift = true;
        assert_eq!(mods.to_string(), "Cmd+Shift");

        mods.ctrl = true;
        assert_eq!(mods.to_string(), "Cmd+Ctrl+Shift");
    }

    #[test]
    fn test_ui_element_is_sensitive() {
        let password_field = UiElement {
            element_type: "SecureTextField".to_string(),
            label: None,
            role: "text".to_string(),
        };
        assert!(password_field.is_sensitive());

        let password_labeled = UiElement {
            element_type: "TextField".to_string(),
            label: Some("Password".to_string()),
            role: "text".to_string(),
        };
        assert!(password_labeled.is_sensitive());

        let pin_field = UiElement {
            element_type: "TextField".to_string(),
            label: Some("Enter PIN".to_string()),
            role: "text".to_string(),
        };
        assert!(pin_field.is_sensitive());

        let normal_field = UiElement {
            element_type: "TextField".to_string(),
            label: Some("Name".to_string()),
            role: "text".to_string(),
        };
        assert!(!normal_field.is_sensitive());
    }

    #[test]
    fn test_key_event_type_serialization() {
        let down = KeyEventType::KeyDown;
        let json = serde_json::to_string(&down).unwrap();
        assert_eq!(json, "\"key_down\"");

        let up = KeyEventType::KeyUp;
        let json = serde_json::to_string(&up).unwrap();
        assert_eq!(json, "\"key_up\"");
    }
}
