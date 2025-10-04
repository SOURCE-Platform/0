#![cfg(target_os = "windows")]

use crate::core::consent::{ConsentManager, Feature};
use crate::models::input::{AppContext, MouseEvent, MouseEventType, Point, UiElement};
use std::sync::Arc;
use tokio::sync::mpsc;

#[cfg(target_os = "windows")]
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
    UI::WindowsAndMessaging::{
        CallNextHookEx, GetWindowTextW, GetWindowThreadProcessId, SetWindowsHookExW,
        UnhookWindowsHookEx, WindowFromPoint, HHOOK, MSLLHOOKSTRUCT, WH_MOUSE_LL,
        WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEHWHEEL,
        WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN,
    },
};

pub struct WindowsMouseListener {
    event_sender: mpsc::UnboundedSender<MouseEvent>,
    consent_manager: Arc<ConsentManager>,
    hook: Option<HHOOK>,
    last_position: Point,
    is_dragging: bool,
    drag_start: Option<Point>,
}

impl WindowsMouseListener {
    pub fn new(
        consent_manager: Arc<ConsentManager>,
    ) -> Result<(Self, mpsc::UnboundedReceiver<MouseEvent>), Box<dyn std::error::Error + Send + Sync>>
    {
        let (tx, rx) = mpsc::unbounded_channel();

        Ok((
            Self {
                event_sender: tx,
                consent_manager,
                hook: None,
                last_position: Point { x: 0, y: 0 },
                is_dragging: false,
                drag_start: None,
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
            .is_consent_granted(Feature::MouseRecording)
            .await
            .map_err(|e| format!("Consent check failed: {}", e))?;

        if !has_consent {
            return Err("MouseRecording consent not granted".into());
        }

        #[cfg(target_os = "windows")]
        unsafe {
            // Install low-level mouse hook
            let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(Self::mouse_proc), None, 0)?;
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
    unsafe extern "system" fn mouse_proc(
        n_code: i32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        if n_code >= 0 {
            let mouse_struct = *(l_param.0 as *const MSLLHOOKSTRUCT);
            let position = Point {
                x: mouse_struct.pt.x,
                y: mouse_struct.pt.y,
            };

            let event_type = match w_param.0 as u32 {
                WM_MOUSEMOVE => MouseEventType::Move { target: position },
                WM_LBUTTONDOWN => MouseEventType::LeftClick,
                WM_LBUTTONUP => MouseEventType::LeftClick,
                WM_LBUTTONDBLCLK => MouseEventType::DoubleClick,
                WM_RBUTTONDOWN => MouseEventType::RightClick,
                WM_MBUTTONDOWN => MouseEventType::MiddleClick,
                WM_MOUSEWHEEL => {
                    let wheel_delta = (mouse_struct.mouseData >> 16) as i16;
                    MouseEventType::ScrollWheel {
                        delta_x: 0,
                        delta_y: wheel_delta as i32,
                    }
                }
                WM_MOUSEHWHEEL => {
                    let wheel_delta = (mouse_struct.mouseData >> 16) as i16;
                    MouseEventType::ScrollWheel {
                        delta_x: wheel_delta as i32,
                        delta_y: 0,
                    }
                }
                _ => return CallNextHookEx(None, n_code, w_param, l_param),
            };

            // Get window at position
            let hwnd = WindowFromPoint(POINT {
                x: mouse_struct.pt.x,
                y: mouse_struct.pt.y,
            });

            let app_context = Self::get_window_app_context(hwnd);
            let ui_element = Self::get_element_at_point(position);

            let mouse_event = MouseEvent {
                timestamp: chrono::Utc::now().timestamp_millis(),
                event_type,
                position,
                app_context,
                ui_element,
            };

            // TODO: Send event through channel (requires thread-safe global state)
            // For now, this is a structural implementation
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
                process_id,
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
    fn get_element_at_point(_position: Point) -> Option<UiElement> {
        // UI Automation API implementation would go here
        // Requires COM initialization: CoCreateInstance(&CUIAutomation, ...)
        // Then: automation.ElementFromPoint(POINT { x, y })
        None
    }
}

impl Drop for WindowsMouseListener {
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
    fn test_mouse_listener_creation() {
        let consent_manager = Arc::new(ConsentManager::new(Arc::new(
            crate::core::database::Database::new(":memory:").unwrap(),
        )));

        let result = WindowsMouseListener::new(consent_manager);
        assert!(result.is_ok());
    }
}
