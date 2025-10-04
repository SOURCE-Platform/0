#![cfg(target_os = "windows")]

use crate::core::consent::{ConsentManager, Feature};
use crate::models::input::{AppContext, KeyboardEvent, KeyEventType, ModifierState, UiElement};
use std::sync::Arc;
use tokio::sync::mpsc;

#[cfg(target_os = "windows")]
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::Input::KeyboardAndMouse::{
        GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    },
    UI::WindowsAndMessaging::{
        CallNextHookEx, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, HHOOK,
        KBDLLHOOKSTRUCT, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN,
        WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    },
};

pub struct WindowsKeyboardListener {
    event_sender: mpsc::UnboundedSender<KeyboardEvent>,
    consent_manager: Arc<ConsentManager>,
    hook: Option<HHOOK>,
}

impl WindowsKeyboardListener {
    pub fn new(
        consent_manager: Arc<ConsentManager>,
    ) -> Result<(Self, mpsc::UnboundedReceiver<KeyboardEvent>), Box<dyn std::error::Error + Send + Sync>>
    {
        let (tx, rx) = mpsc::unbounded_channel();

        Ok((
            Self {
                event_sender: tx,
                consent_manager,
                hook: None,
            },
            rx,
        ))
    }

    pub async fn start_listening(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Check consent
        let has_consent = self
            .consent_manager
            .is_consent_granted(Feature::KeyboardRecording)
            .await?;

        if !has_consent {
            return Err("KeyboardRecording consent not granted".into());
        }

        #[cfg(target_os = "windows")]
        unsafe {
            // Install low-level keyboard hook
            let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(Self::keyboard_proc), None, 0)?;

            self.hook = Some(hook);
        }

        Ok(())
    }

    pub async fn stop_listening(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    {
        #[cfg(target_os = "windows")]
        if let Some(hook) = self.hook.take() {
            unsafe {
                UnhookWindowsHookEx(hook)?;
            }
        }

        Ok(())
    }

    #[cfg(target_os = "windows")]
    unsafe extern "system" fn keyboard_proc(
        n_code: i32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        if n_code >= 0 {
            let kb_struct = *(l_param.0 as *const KBDLLHOOKSTRUCT);

            let event_type = match w_param.0 as u32 {
                WM_KEYDOWN | WM_SYSKEYDOWN => KeyEventType::KeyDown,
                WM_KEYUP | WM_SYSKEYUP => KeyEventType::KeyUp,
                _ => return CallNextHookEx(None, n_code, w_param, l_param),
            };

            let key_code = kb_struct.vkCode;

            // Get modifier state
            let modifiers = ModifierState {
                shift: GetKeyState(VK_SHIFT.0 as i32) < 0,
                ctrl: GetKeyState(VK_CONTROL.0 as i32) < 0,
                alt: GetKeyState(VK_MENU.0 as i32) < 0,
                meta: GetKeyState(VK_LWIN.0 as i32) < 0 || GetKeyState(VK_RWIN.0 as i32) < 0,
            };

            // Get foreground window info
            let hwnd = GetForegroundWindow();
            let app_context = Self::get_window_app_context(hwnd);
            let ui_element = Self::get_focused_ui_element_windows(hwnd);

            let is_sensitive = ui_element
                .as_ref()
                .map(|e| e.is_sensitive())
                .unwrap_or(false);

            let keyboard_event = KeyboardEvent {
                timestamp: chrono::Utc::now().timestamp_millis(),
                event_type,
                key_code,
                key_char: Self::vk_code_to_char(key_code, &modifiers),
                modifiers,
                app_context,
                ui_element,
                is_sensitive,
            };

            // TODO: Send event through channel (requires thread-safe global state)
            // For now, this is a structural implementation
            // Full implementation would require unsafe global state or message passing
        }

        CallNextHookEx(None, n_code, w_param, l_param)
    }

    #[cfg(target_os = "windows")]
    fn get_window_app_context(hwnd: HWND) -> AppContext {
        unsafe {
            let mut process_id = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut process_id));

            let mut window_title = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut window_title);

            let title = if len > 0 {
                String::from_utf16_lossy(&window_title[..len as usize])
            } else {
                String::from("Unknown")
            };

            let app_name = Self::get_process_name(process_id);

            AppContext {
                app_name,
                window_title: title,
                process_id: process_id as i32,
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn get_process_name(process_id: u32) -> String {
        use windows::Win32::System::ProcessStatus::K32GetModuleBaseNameW;
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION};

        unsafe {
            if let Ok(handle) = OpenProcess(PROCESS_QUERY_INFORMATION, false, process_id) {
                let mut name = [0u16; 512];
                let len = K32GetModuleBaseNameW(handle, None, &mut name);

                if len > 0 {
                    return String::from_utf16_lossy(&name[..len as usize]);
                }
            }
        }

        String::from("Unknown")
    }

    #[cfg(target_os = "windows")]
    fn get_focused_ui_element_windows(_hwnd: HWND) -> Option<UiElement> {
        // UI Automation API implementation would go here
        // This is complex and requires COM initialization
        // For now, return None - privacy filtering will still work based on window title
        None
    }

    fn vk_code_to_char(vk_code: u32, modifiers: &ModifierState) -> Option<char> {
        use windows::Win32::UI::Input::KeyboardAndMouse::*;

        // Common printable characters
        match vk_code {
            0x20 => Some(' '), // VK_SPACE
            0x30..=0x39 => {
                // 0-9
                if modifiers.shift {
                    // Shifted number keys
                    match vk_code {
                        0x30 => Some(')'),
                        0x31 => Some('!'),
                        0x32 => Some('@'),
                        0x33 => Some('#'),
                        0x34 => Some('$'),
                        0x35 => Some('%'),
                        0x36 => Some('^'),
                        0x37 => Some('&'),
                        0x38 => Some('*'),
                        0x39 => Some('('),
                        _ => None,
                    }
                } else {
                    char::from_u32(vk_code)
                }
            }
            0x41..=0x5A => {
                // A-Z
                let base = vk_code - 0x41 + 0x61; // Convert to lowercase
                if modifiers.shift {
                    char::from_u32(vk_code) // Uppercase
                } else {
                    char::from_u32(base) // Lowercase
                }
            }
            _ => None,
        }
    }
}

impl Drop for WindowsKeyboardListener {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        if let Some(hook) = self.hook.take() {
            unsafe {
                let _ = UnhookWindowsHookEx(hook);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vk_code_conversion() {
        let no_mods = ModifierState {
            shift: false,
            ctrl: false,
            alt: false,
            meta: false,
        };

        let with_shift = ModifierState {
            shift: true,
            ctrl: false,
            alt: false,
            meta: false,
        };

        // Test space
        assert_eq!(
            WindowsKeyboardListener::vk_code_to_char(0x20, &no_mods),
            Some(' ')
        );

        // Test lowercase a
        assert_eq!(
            WindowsKeyboardListener::vk_code_to_char(0x41, &no_mods),
            Some('a')
        );

        // Test uppercase A
        assert_eq!(
            WindowsKeyboardListener::vk_code_to_char(0x41, &with_shift),
            Some('A')
        );

        // Test number
        assert_eq!(
            WindowsKeyboardListener::vk_code_to_char(0x31, &no_mods),
            Some('1')
        );

        // Test shifted number
        assert_eq!(
            WindowsKeyboardListener::vk_code_to_char(0x31, &with_shift),
            Some('!')
        );
    }
}
