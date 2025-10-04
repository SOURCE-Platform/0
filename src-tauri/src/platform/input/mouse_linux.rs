#![cfg(target_os = "linux")]

use crate::core::consent::{ConsentManager, Feature};
use crate::models::input::{AppContext, MouseEvent, MouseEventType, Point, UiElement};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

#[cfg(target_os = "linux")]
use evdev::{Device, InputEventKind, Key, RelativeAxisType};

pub struct LinuxMouseListener {
    event_sender: mpsc::UnboundedSender<MouseEvent>,
    consent_manager: Arc<ConsentManager>,
    devices: Vec<Device>,
    current_position: Point,
    button_states: HashMap<u32, bool>,
    last_recorded_position: Point,
}

impl LinuxMouseListener {
    pub fn new(
        consent_manager: Arc<ConsentManager>,
    ) -> Result<(Self, mpsc::UnboundedReceiver<MouseEvent>), Box<dyn std::error::Error + Send + Sync>>
    {
        let (tx, rx) = mpsc::unbounded_channel();

        Ok((
            Self {
                event_sender: tx,
                consent_manager,
                devices: Vec::new(),
                current_position: Point { x: 0, y: 0 },
                button_states: HashMap::new(),
                last_recorded_position: Point { x: 0, y: 0 },
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

        // Check permissions
        Self::check_input_permissions()?;

        // Find mouse devices in /dev/input/
        #[cfg(target_os = "linux")]
        {
            let mut devices = Vec::new();

            for entry in std::fs::read_dir("/dev/input")? {
                let entry = entry?;
                let path = entry.path();

                if let Ok(device) = Device::open(&path) {
                    // Check if it's a mouse by looking for relative axes
                    if device.supported_relative_axes().map_or(false, |axes| {
                        axes.contains(RelativeAxisType::REL_X)
                            && axes.contains(RelativeAxisType::REL_Y)
                    }) {
                        devices.push(device);
                    }
                }
            }

            if devices.is_empty() {
                return Err("No mouse devices found in /dev/input".into());
            }

            // Start position polling (X11/Wayland)
            self.start_position_polling().await?;

            // Spawn task to read from each device
            for mut device in devices.clone() {
                let sender = self.event_sender.clone();

                tokio::spawn(async move {
                    loop {
                        match device.fetch_events() {
                            Ok(events) => {
                                for event in events {
                                    Self::process_event(&event, &sender);
                                }
                            }
                            Err(e) => {
                                eprintln!("Error reading mouse events: {}", e);
                                break;
                            }
                        }

                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                });
            }

            self.devices = devices;
        }

        Ok(())
    }

    pub async fn stop_listening(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    {
        // Clear devices - tasks will naturally stop
        self.devices.clear();
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn process_event(
        event: &evdev::InputEvent,
        sender: &mpsc::UnboundedSender<MouseEvent>,
    ) {
        match event.kind() {
            InputEventKind::RelAxis(_axis) => {
                // Relative mouse movement - position is tracked separately via X11
                // Could update current_position here if tracking relative movements
            }
            InputEventKind::Key(key) => {
                let event_type = match (key, event.value()) {
                    (Key::BTN_LEFT, 1) => MouseEventType::LeftClick,
                    (Key::BTN_RIGHT, 1) => MouseEventType::RightClick,
                    (Key::BTN_MIDDLE, 1) => MouseEventType::MiddleClick,
                    _ => return,
                };

                let position = Self::get_cursor_position_x11();
                let app_context = Self::get_current_app_context();

                let mouse_event = MouseEvent {
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    event_type,
                    position,
                    app_context,
                    ui_element: None,
                };

                let _ = sender.send(mouse_event);
            }
            _ => {}
        }
    }

    fn get_cursor_position_x11() -> Point {
        #[cfg(feature = "x11")]
        unsafe {
            use x11::xlib::*;

            let display = XOpenDisplay(std::ptr::null());
            if display.is_null() {
                return Point { x: 0, y: 0 };
            }

            let root = XDefaultRootWindow(display);

            let mut root_return = 0;
            let mut child_return = 0;
            let mut root_x = 0;
            let mut root_y = 0;
            let mut win_x = 0;
            let mut win_y = 0;
            let mut mask = 0;

            XQueryPointer(
                display,
                root,
                &mut root_return,
                &mut child_return,
                &mut root_x,
                &mut root_y,
                &mut win_x,
                &mut win_y,
                &mut mask,
            );

            XCloseDisplay(display);

            Point {
                x: root_x,
                y: root_y,
            }
        }

        #[cfg(not(feature = "x11"))]
        Point { x: 0, y: 0 }
    }

    fn get_current_app_context() -> AppContext {
        // Try X11 first
        #[cfg(feature = "x11")]
        {
            if let Ok(context) = Self::get_app_context_x11() {
                return context;
            }
        }

        // Fallback
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
            let mut process_id = 0u32;
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
                process_id = *(prop as *const u32);
                XFree(prop as *mut _);
            }

            // Get process name
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

    async fn start_position_polling(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    {
        // Poll cursor position every 100ms for movement tracking
        let sender = self.event_sender.clone();
        let mut last_position = Point { x: 0, y: 0 };

        tokio::spawn(async move {
            loop {
                let current_position = Self::get_cursor_position_x11();

                if current_position.x != last_position.x || current_position.y != last_position.y {
                    let distance = ((current_position.x - last_position.x).pow(2)
                        + (current_position.y - last_position.y).pow(2))
                        as f32;

                    // Only record significant movements (>50px)
                    if distance.sqrt() > 50.0 {
                        let _ = sender.send(MouseEvent {
                            timestamp: chrono::Utc::now().timestamp_millis(),
                            event_type: MouseEventType::Move {
                                target: current_position,
                            },
                            position: current_position,
                            app_context: Self::get_current_app_context(),
                            ui_element: None,
                        });

                        last_position = current_position;
                    }
                }

                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });

        Ok(())
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
    fn test_mouse_listener_creation() {
        let consent_manager = Arc::new(ConsentManager::new(Arc::new(
            crate::core::database::Database::new(":memory:").unwrap(),
        )));

        let result = LinuxMouseListener::new(consent_manager);
        assert!(result.is_ok());
    }
}
