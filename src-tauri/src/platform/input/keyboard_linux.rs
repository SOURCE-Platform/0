#![cfg(target_os = "linux")]

use crate::core::consent::{ConsentManager, Feature};
use crate::models::input::{AppContext, KeyboardEvent, KeyEventType, ModifierState, UiElement};
use std::sync::Arc;
use tokio::sync::mpsc;

#[cfg(target_os = "linux")]
use evdev::{Device, InputEventKind, Key};

pub struct LinuxKeyboardListener {
    event_sender: mpsc::UnboundedSender<KeyboardEvent>,
    consent_manager: Arc<ConsentManager>,
    devices: Vec<Device>,
}

impl LinuxKeyboardListener {
    pub fn new(
        consent_manager: Arc<ConsentManager>,
    ) -> Result<(Self, mpsc::UnboundedReceiver<KeyboardEvent>), Box<dyn std::error::Error + Send + Sync>>
    {
        let (tx, rx) = mpsc::unbounded_channel();

        Ok((
            Self {
                event_sender: tx,
                consent_manager,
                devices: Vec::new(),
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

        // Check permissions
        Self::check_input_permissions()?;

        // Find keyboard devices in /dev/input/
        #[cfg(target_os = "linux")]
        {
            let mut devices = Vec::new();

            for entry in std::fs::read_dir("/dev/input")? {
                let entry = entry?;
                let path = entry.path();

                if let Ok(device) = Device::open(&path) {
                    // Check if it's a keyboard by looking for common keyboard keys
                    if device.supported_keys().map_or(false, |keys| {
                        keys.contains(Key::KEY_A)
                            && keys.contains(Key::KEY_ENTER)
                            && keys.contains(Key::KEY_SPACE)
                    }) {
                        devices.push(device);
                    }
                }
            }

            if devices.is_empty() {
                return Err("No keyboard devices found in /dev/input".into());
            }

            // Spawn task to read from each device
            for mut device in devices.clone() {
                let sender = self.event_sender.clone();

                tokio::spawn(async move {
                    loop {
                        match device.fetch_events() {
                            Ok(events) => {
                                for event in events {
                                    if let InputEventKind::Key(key) = event.kind() {
                                        if let Some(keyboard_event) =
                                            Self::convert_event(&event, key)
                                        {
                                            let _ = sender.send(keyboard_event);
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("Error reading keyboard events: {}", e);
                                break;
                            }
                        }

                        // Small sleep to prevent busy waiting
                        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                    }
                });
            }

            self.devices = devices;
        }

        Ok(())
    }

    pub async fn stop_listening(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    {
        // Clear devices - tasks will naturally stop when trying to read
        self.devices.clear();
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn convert_event(event: &evdev::InputEvent, key: Key) -> Option<KeyboardEvent> {
        let event_type = match event.value() {
            1 => KeyEventType::KeyDown,
            0 => KeyEventType::KeyUp,
            _ => return None, // Ignore repeat events (value 2)
        };

        let app_context = Self::get_current_app_context_linux();
        let ui_element = Self::get_focused_ui_element_linux();

        let is_sensitive = ui_element
            .as_ref()
            .map(|e| e.is_sensitive())
            .unwrap_or(false);

        Some(KeyboardEvent {
            timestamp: chrono::Utc::now().timestamp_millis(),
            event_type,
            key_code: key.code(),
            key_char: Self::key_to_char(key),
            modifiers: Self::get_modifier_state(),
            app_context,
            ui_element,
            is_sensitive,
        })
    }

    fn get_current_app_context_linux() -> AppContext {
        // Try X11 first, fall back to process detection
        #[cfg(feature = "x11")]
        {
            if let Ok(context) = Self::get_app_context_x11() {
                return context;
            }
        }

        // Fallback: Use /proc to find active GUI process
        AppContext {
            app_name: String::from("Unknown"),
            window_title: String::from("Unknown"),
            process_id: 0,
        }
    }

    #[cfg(feature = "x11")]
    fn get_app_context_x11() -> Result<AppContext, Box<dyn std::error::Error>> {
        use x11::xlib::*;

        unsafe {
            let display = XOpenDisplay(std::ptr::null());
            if display.is_null() {
                return Err("Failed to open X11 display".into());
            }

            let root = XDefaultRootWindow(display);

            // Get active window
            let mut actual_type = 0;
            let mut actual_format = 0;
            let mut nitems = 0;
            let mut bytes_after = 0;
            let mut prop: *mut u8 = std::ptr::null_mut();

            let net_active_window =
                XInternAtom(display, b"_NET_ACTIVE_WINDOW\0".as_ptr() as *const i8, 0);

            XGetWindowProperty(
                display,
                root,
                net_active_window,
                0,
                1,
                0,
                33, // XA_WINDOW
                &mut actual_type,
                &mut actual_format,
                &mut nitems,
                &mut bytes_after,
                &mut prop,
            );

            if prop.is_null() {
                XCloseDisplay(display);
                return Err("Failed to get active window".into());
            }

            let window = *(prop as *const u64);
            XFree(prop as *mut _);

            // Get window title
            let mut window_title = String::from("Unknown");
            let mut text_prop = std::mem::zeroed();
            if XGetWMName(display, window, &mut text_prop) != 0 && !text_prop.value.is_null() {
                window_title = std::ffi::CStr::from_ptr(text_prop.value as *const i8)
                    .to_string_lossy()
                    .into_owned();
                XFree(text_prop.value as *mut _);
            }

            // Get PID
            let mut process_id = 0i32;
            let net_wm_pid = XInternAtom(display, b"_NET_WM_PID\0".as_ptr() as *const i8, 0);

            XGetWindowProperty(
                display,
                window,
                net_wm_pid,
                0,
                1,
                0,
                19, // XA_CARDINAL
                &mut actual_type,
                &mut actual_format,
                &mut nitems,
                &mut bytes_after,
                &mut prop,
            );

            if !prop.is_null() {
                process_id = *(prop as *const i32);
                XFree(prop as *mut _);
            }

            // Get process name from PID
            let app_name = if process_id > 0 {
                std::fs::read_to_string(format!("/proc/{}/comm", process_id))
                    .unwrap_or_else(|_| String::from("Unknown"))
                    .trim()
                    .to_string()
            } else {
                String::from("Unknown")
            };

            XCloseDisplay(display);

            Ok(AppContext {
                app_name,
                window_title,
                process_id,
            })
        }
    }

    fn get_focused_ui_element_linux() -> Option<UiElement> {
        // AT-SPI2 implementation would go here
        // This is complex and optional for now
        None
    }

    fn get_modifier_state() -> ModifierState {
        // TODO: Implement proper modifier state detection
        // Could use X11 XQueryKeymap or read from /dev/input
        ModifierState {
            shift: false,
            ctrl: false,
            alt: false,
            meta: false,
        }
    }

    #[cfg(target_os = "linux")]
    fn key_to_char(key: Key) -> Option<char> {
        use evdev::Key::*;

        match key {
            KEY_SPACE => Some(' '),
            KEY_A => Some('a'),
            KEY_B => Some('b'),
            KEY_C => Some('c'),
            KEY_D => Some('d'),
            KEY_E => Some('e'),
            KEY_F => Some('f'),
            KEY_G => Some('g'),
            KEY_H => Some('h'),
            KEY_I => Some('i'),
            KEY_J => Some('j'),
            KEY_K => Some('k'),
            KEY_L => Some('l'),
            KEY_M => Some('m'),
            KEY_N => Some('n'),
            KEY_O => Some('o'),
            KEY_P => Some('p'),
            KEY_Q => Some('q'),
            KEY_R => Some('r'),
            KEY_S => Some('s'),
            KEY_T => Some('t'),
            KEY_U => Some('u'),
            KEY_V => Some('v'),
            KEY_W => Some('w'),
            KEY_X => Some('x'),
            KEY_Y => Some('y'),
            KEY_Z => Some('z'),
            KEY_0 => Some('0'),
            KEY_1 => Some('1'),
            KEY_2 => Some('2'),
            KEY_3 => Some('3'),
            KEY_4 => Some('4'),
            KEY_5 => Some('5'),
            KEY_6 => Some('6'),
            KEY_7 => Some('7'),
            KEY_8 => Some('8'),
            KEY_9 => Some('9'),
            _ => None,
        }
    }

    fn check_input_permissions() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Check if user can read /dev/input/event*
        let test_paths = ["/dev/input/event0", "/dev/input/event1", "/dev/input"];

        let mut accessible = false;
        for path in &test_paths {
            if std::fs::metadata(path).is_ok() {
                accessible = true;
                break;
            }
        }

        if !accessible {
            return Err(
                "Cannot access /dev/input. User must be in 'input' group.\n\
                 Run: sudo usermod -a -G input $USER\n\
                 Then log out and log back in."
                    .into(),
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "linux")]
    fn test_key_conversion() {
        use evdev::Key::*;

        assert_eq!(LinuxKeyboardListener::key_to_char(KEY_A), Some('a'));
        assert_eq!(LinuxKeyboardListener::key_to_char(KEY_SPACE), Some(' '));
        assert_eq!(LinuxKeyboardListener::key_to_char(KEY_1), Some('1'));
        assert_eq!(LinuxKeyboardListener::key_to_char(KEY_ENTER), None);
    }
}
