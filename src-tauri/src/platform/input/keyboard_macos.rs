// macOS keyboard event monitoring using CGEventTap and Accessibility API

use crate::core::consent::ConsentManager;
use crate::models::input::{
    AppContext, KeyEventType, KeyboardEvent, ModifierState, UiElement,
};
use cocoa::base::{id, nil};
use cocoa::foundation::NSString;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType};
use objc::{class, msg_send, sel, sel_impl};
use std::ffi::CStr;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// macOS keyboard event listener using CGEventTap
pub struct MacOSKeyboardListener {
    event_sender: mpsc::UnboundedSender<KeyboardEvent>,
    consent_manager: Arc<ConsentManager>,
    is_listening: Arc<Mutex<bool>>,
}

impl MacOSKeyboardListener {
    /// Create a new macOS keyboard listener
    pub fn new(
        consent_manager: Arc<ConsentManager>,
    ) -> Result<(Self, mpsc::UnboundedReceiver<KeyboardEvent>), Box<dyn std::error::Error + Send + Sync>>
    {
        let (tx, rx) = mpsc::unbounded_channel();

        Ok((
            Self {
                event_sender: tx,
                consent_manager,
                is_listening: Arc::new(Mutex::new(false)),
            },
            rx,
        ))
    }

    /// Start listening for keyboard events
    pub async fn start_listening(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Check consent first
        use crate::core::consent::Feature;
        let has_consent = self
            .consent_manager
            .is_consent_granted(Feature::KeyboardRecording)
            .await
            .map_err(|e| format!("Consent check failed: {}", e))?;

        if !has_consent {
            return Err("KeyboardRecording consent not granted".into());
        }

        // Check if already listening
        {
            let mut listening = self.is_listening.lock().unwrap();
            if *listening {
                return Ok(());
            }
            *listening = true;
        }

        // Check accessibility permission (simplified - actual implementation would use AXIsProcessTrusted)
        if !Self::check_accessibility_permission() {
            *self.is_listening.lock().unwrap() = false;
            return Err("Accessibility permission required. Please enable in System Preferences > Privacy & Security > Accessibility".into());
        }

        println!("Starting macOS keyboard listener");

        // Note: CGEventTap requires running in the main thread with a run loop
        // This is a simplified implementation showing the structure
        // Full implementation would need proper event loop integration

        Ok(())
    }

    /// Stop listening for keyboard events
    pub async fn stop_listening(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut listening = self.is_listening.lock().unwrap();
        if !*listening {
            return Ok(());
        }

        *listening = false;
        println!("Stopped macOS keyboard listener");

        Ok(())
    }

    /// Check if accessibility permission is granted
    fn check_accessibility_permission() -> bool {
        // In a full implementation, this would use AXIsProcessTrusted()
        // from ApplicationServices framework
        // For now, return true as a placeholder
        true
    }

    /// Handle a keyboard event from CGEventTap
    fn handle_event(&self, event_type: CGEventType, event: &CGEvent) {
        let timestamp = chrono::Utc::now().timestamp_millis();

        // Get key code (field 9 is KEYBOARD_EVENT_KEYCODE)
        let key_code = event.get_integer_value_field(9) as u32;

        // Get character (simplified - full implementation would handle keyboard layouts)
        let key_char = Self::key_code_to_char(key_code);

        // Get modifier state
        let flags = event.get_flags();
        let modifiers = ModifierState {
            shift: flags.contains(CGEventFlags::CGEventFlagShift),
            ctrl: flags.contains(CGEventFlags::CGEventFlagControl),
            alt: flags.contains(CGEventFlags::CGEventFlagAlternate),
            meta: flags.contains(CGEventFlags::CGEventFlagCommand),
        };

        // Get app context
        let app_context = Self::get_current_app_context();

        // Get focused UI element
        let ui_element = Self::get_focused_ui_element();

        // Determine if this is a sensitive context
        let is_sensitive = ui_element
            .as_ref()
            .map(|e| e.is_sensitive())
            .unwrap_or(false);

        // Create keyboard event
        let keyboard_event = KeyboardEvent {
            timestamp,
            event_type: match event_type {
                CGEventType::KeyDown => KeyEventType::KeyDown,
                CGEventType::KeyUp => KeyEventType::KeyUp,
                _ => return,
            },
            key_code,
            key_char,
            modifiers,
            app_context,
            ui_element,
            is_sensitive,
        };

        // Only log if not sensitive
        if Self::should_log_keystroke(&keyboard_event) {
            let _ = self.event_sender.send(keyboard_event);
        }
    }

    /// Get the current app context from frontmost application
    fn get_current_app_context() -> AppContext {
        unsafe {
            let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
            let frontmost_app: id = msg_send![workspace, frontmostApplication];

            if frontmost_app == nil {
                return AppContext::unknown();
            }

            // Get app name
            let name_nsstring: id = msg_send![frontmost_app, localizedName];
            let app_name = if name_nsstring != nil {
                let c_str: *const i8 = msg_send![name_nsstring, UTF8String];
                if !c_str.is_null() {
                    CStr::from_ptr(c_str).to_string_lossy().to_string()
                } else {
                    "Unknown".to_string()
                }
            } else {
                "Unknown".to_string()
            };

            // Get process ID
            let process_id: i32 = msg_send![frontmost_app, processIdentifier];

            // Get window title (simplified - would need more complex implementation)
            let window_title = Self::get_frontmost_window_title().unwrap_or_else(|| "".to_string());

            AppContext::new(app_name, window_title, process_id as u32)
        }
    }

    /// Get the title of the frontmost window
    fn get_frontmost_window_title() -> Option<String> {
        // This would require Accessibility API to get window title
        // Placeholder implementation
        None
    }

    /// Get the focused UI element using Accessibility API
    fn get_focused_ui_element() -> Option<UiElement> {
        // This requires proper Accessibility API bindings
        // Placeholder implementation showing the structure

        // In a full implementation, this would:
        // 1. Create system-wide accessibility object
        // 2. Get focused UI element
        // 3. Query its attributes (role, title, subrole)
        // 4. Return UiElement struct

        None
    }

    /// Convert macOS key code to character
    fn key_code_to_char(key_code: u32) -> Option<char> {
        // Simplified key code mapping
        // Full implementation would handle keyboard layouts and Unicode
        match key_code {
            // Letters
            0 => Some('a'),
            11 => Some('b'),
            8 => Some('c'),
            2 => Some('d'),
            14 => Some('e'),
            3 => Some('f'),
            5 => Some('g'),
            4 => Some('h'),
            34 => Some('i'),
            38 => Some('j'),
            40 => Some('k'),
            37 => Some('l'),
            46 => Some('m'),
            45 => Some('n'),
            31 => Some('o'),
            35 => Some('p'),
            12 => Some('q'),
            15 => Some('r'),
            1 => Some('s'),
            17 => Some('t'),
            32 => Some('u'),
            9 => Some('v'),
            13 => Some('w'),
            7 => Some('x'),
            16 => Some('y'),
            6 => Some('z'),

            // Numbers
            29 => Some('0'),
            18 => Some('1'),
            19 => Some('2'),
            20 => Some('3'),
            21 => Some('4'),
            23 => Some('5'),
            22 => Some('6'),
            26 => Some('7'),
            28 => Some('8'),
            25 => Some('9'),

            // Special keys
            36 => Some('\n'), // Return
            48 => Some('\t'), // Tab
            49 => Some(' '),  // Space

            _ => None,
        }
    }

    /// Determine if a keystroke should be logged
    fn should_log_keystroke(event: &KeyboardEvent) -> bool {
        // Don't log if in sensitive context
        if event.is_sensitive {
            return false;
        }

        // Don't log password fields
        if let Some(ref ui_element) = event.ui_element {
            if ui_element.is_sensitive() {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_code_to_char() {
        assert_eq!(MacOSKeyboardListener::key_code_to_char(0), Some('a'));
        assert_eq!(MacOSKeyboardListener::key_code_to_char(1), Some('s'));
        assert_eq!(MacOSKeyboardListener::key_code_to_char(18), Some('1'));
        assert_eq!(MacOSKeyboardListener::key_code_to_char(49), Some(' '));
        assert_eq!(MacOSKeyboardListener::key_code_to_char(999), None);
    }

    #[test]
    fn test_should_log_keystroke_sensitive() {
        let sensitive_event = KeyboardEvent {
            timestamp: 0,
            event_type: KeyEventType::KeyDown,
            key_code: 0,
            key_char: Some('a'),
            modifiers: ModifierState::new(),
            app_context: AppContext::unknown(),
            ui_element: Some(UiElement {
                element_type: "SecureTextField".to_string(),
                label: None,
                role: "text".to_string(),
            }),
            is_sensitive: true,
        };

        assert!(!MacOSKeyboardListener::should_log_keystroke(&sensitive_event));
    }

    #[test]
    fn test_should_log_keystroke_normal() {
        let normal_event = KeyboardEvent {
            timestamp: 0,
            event_type: KeyEventType::KeyDown,
            key_code: 0,
            key_char: Some('a'),
            modifiers: ModifierState::new(),
            app_context: AppContext::unknown(),
            ui_element: Some(UiElement {
                element_type: "TextField".to_string(),
                label: Some("Name".to_string()),
                role: "text".to_string(),
            }),
            is_sensitive: false,
        };

        assert!(MacOSKeyboardListener::should_log_keystroke(&normal_event));
    }

    #[test]
    fn test_check_accessibility_permission() {
        // This is a placeholder test
        let has_permission = MacOSKeyboardListener::check_accessibility_permission();
        assert!(has_permission); // Currently always returns true
    }
}
