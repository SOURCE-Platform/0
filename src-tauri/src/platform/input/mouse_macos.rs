#![cfg(target_os = "macos")]

use crate::core::consent::{ConsentManager, Feature};
use crate::models::input::{AppContext, MouseEvent, MouseEventType, Point, UiElement};
use std::sync::Arc;
use tokio::sync::mpsc;

#[cfg(target_os = "macos")]
use cocoa::base::id;
use cocoa::foundation::NSString;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventType};
use objc::{class, msg_send, sel, sel_impl};

pub struct MacOSMouseListener {
    event_sender: mpsc::UnboundedSender<MouseEvent>,
    consent_manager: Arc<ConsentManager>,
    last_position: Point,
    is_dragging: bool,
    drag_start: Option<Point>,
    last_click_time: i64,
    last_click_pos: Option<Point>,
}

impl MacOSMouseListener {
    pub fn new(
        consent_manager: Arc<ConsentManager>,
    ) -> Result<(Self, mpsc::UnboundedReceiver<MouseEvent>), Box<dyn std::error::Error + Send + Sync>>
    {
        let (tx, rx) = mpsc::unbounded_channel();

        Ok((
            Self {
                event_sender: tx,
                consent_manager,
                last_position: Point { x: 0, y: 0 },
                is_dragging: false,
                drag_start: None,
                last_click_time: 0,
                last_click_pos: None,
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

        // Note: Full CGEventTap implementation would go here
        // For now, this is a structural implementation
        // Real implementation requires main thread + CFRunLoop setup

        Ok(())
    }

    pub async fn stop_listening(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    {
        // Cleanup event tap if needed
        Ok(())
    }

    #[allow(dead_code)]
    fn handle_mouse_event(&mut self, event_type: CGEventType, event: &CGEvent) {
        let timestamp = chrono::Utc::now().timestamp_millis();
        let location = event.location();
        let position = Point {
            x: location.x as i32,
            y: location.y as i32,
        };

        let event_type_mapped = match event_type {
            CGEventType::MouseMoved => {
                // Only record significant movements (>50px from last position)
                if !self.should_record_movement(&position) {
                    return;
                }
                MouseEventType::Move { target: position }
            }
            CGEventType::LeftMouseDown => {
                // Check for double-click
                if self.is_double_click(timestamp, position) {
                    return; // Will be handled by LeftMouseUp
                }

                self.is_dragging = true;
                self.drag_start = Some(position);
                self.last_click_time = timestamp;
                self.last_click_pos = Some(position);
                MouseEventType::LeftClick
            }
            CGEventType::LeftMouseUp => {
                if self.is_dragging {
                    self.is_dragging = false;
                    MouseEventType::DragEnd { end_pos: position }
                } else if self.is_double_click(timestamp, position) {
                    MouseEventType::DoubleClick
                } else {
                    MouseEventType::LeftClick
                }
            }
            CGEventType::LeftMouseDragged => MouseEventType::DragMove {
                current_pos: position,
            },
            CGEventType::RightMouseDown => MouseEventType::RightClick,
            CGEventType::OtherMouseDown => MouseEventType::MiddleClick,
            CGEventType::ScrollWheel => {
                // Field 11 is ScrollWheelEventDeltaAxis1 (horizontal)
                // Field 12 is ScrollWheelEventDeltaAxis2 (vertical)
                let delta_x = event.get_integer_value_field(11) as i32;
                let delta_y = event.get_integer_value_field(12) as i32;
                MouseEventType::ScrollWheel { delta_x, delta_y }
            }
            _ => return,
        };

        let app_context = Self::get_app_context_at_position(position);
        let ui_element = Self::get_ui_element_at_position(position);

        let mouse_event = MouseEvent {
            timestamp,
            event_type: event_type_mapped,
            position,
            app_context,
            ui_element,
        };

        let _ = self.event_sender.send(mouse_event);
        self.last_position = position;
    }

    fn should_record_movement(&self, new_pos: &Point) -> bool {
        // Only record if moved >50px from last recorded position
        let distance = ((new_pos.x - self.last_position.x).pow(2)
            + (new_pos.y - self.last_position.y).pow(2)) as f32;
        distance.sqrt() > 50.0
    }

    fn is_double_click(&self, timestamp: i64, position: Point) -> bool {
        if let Some(last_pos) = self.last_click_pos {
            let time_diff = timestamp - self.last_click_time;
            let distance = ((position.x - last_pos.x).pow(2) + (position.y - last_pos.y).pow(2))
                as f32;

            // Double-click if within 500ms and 5px
            time_diff < 500 && distance.sqrt() < 5.0
        } else {
            false
        }
    }

    fn get_app_context_at_position(_position: Point) -> AppContext {
        // Get frontmost app (mouse position doesn't change this typically)
        unsafe {
            let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
            let frontmost_app: id = msg_send![workspace, frontmostApplication];

            if frontmost_app.is_null() {
                return AppContext::unknown();
            }

            // Get app name
            let name_nsstring: id = msg_send![frontmost_app, localizedName];
            let app_name = if !name_nsstring.is_null() {
                let c_str: *const i8 = msg_send![name_nsstring, UTF8String];
                if !c_str.is_null() {
                    std::ffi::CStr::from_ptr(c_str)
                        .to_string_lossy()
                        .into_owned()
                } else {
                    String::from("Unknown")
                }
            } else {
                String::from("Unknown")
            };

            // Get process ID
            let process_id: i32 = msg_send![frontmost_app, processIdentifier];

            AppContext {
                app_name,
                window_title: String::from(""), // Mouse events don't have window title easily
                process_id: process_id as u32,
            }
        }
    }

    fn get_ui_element_at_position(_position: Point) -> Option<UiElement> {
        // Accessibility API implementation would go here
        // AXUIElementCopyElementAtPosition() requires setup
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_record_movement() {
        let consent_manager = Arc::new(ConsentManager::new(Arc::new(
            crate::core::database::Database::new(":memory:").unwrap(),
        )));
        let (listener, _rx) = MacOSMouseListener::new(consent_manager).unwrap();

        // Should record movement >50px
        let new_pos = Point { x: 100, y: 0 };
        assert!(listener.should_record_movement(&new_pos));

        // Should not record small movements
        let small_move = Point { x: 10, y: 0 };
        assert!(!listener.should_record_movement(&small_move));
    }

    #[test]
    fn test_double_click_detection() {
        let consent_manager = Arc::new(ConsentManager::new(Arc::new(
            crate::core::database::Database::new(":memory:").unwrap(),
        )));
        let (mut listener, _rx) = MacOSMouseListener::new(consent_manager).unwrap();

        let pos = Point { x: 100, y: 100 };
        listener.last_click_time = chrono::Utc::now().timestamp_millis();
        listener.last_click_pos = Some(pos);

        // Double-click within 500ms and 5px
        let timestamp = listener.last_click_time + 200;
        let close_pos = Point { x: 101, y: 101 };
        assert!(listener.is_double_click(timestamp, close_pos));

        // Not a double-click if too far
        let far_pos = Point { x: 200, y: 200 };
        assert!(!listener.is_double_click(timestamp, far_pos));

        // Not a double-click if too late
        let late_timestamp = listener.last_click_time + 600;
        assert!(!listener.is_double_click(late_timestamp, close_pos));
    }
}
