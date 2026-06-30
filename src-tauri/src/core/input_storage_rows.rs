use serde::{Deserialize, Serialize};

use crate::models::input::{KeyboardEvent, MouseEvent};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: i64,
    pub end: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputTimeline {
    pub keyboard_events: Vec<KeyboardEvent>,
    pub mouse_events: Vec<MouseEvent>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub(super) struct KeyboardEventRow {
    pub id: String,
    pub session_id: String,
    pub timestamp: i64,
    pub event_type: String,
    pub key_code: i64,
    pub key_char: Option<String>,
    pub modifiers: String,
    pub app_name: String,
    pub window_title: String,
    pub process_id: i64,
    pub ui_element: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub(super) struct MouseEventRow {
    pub id: String,
    pub session_id: String,
    pub timestamp: i64,
    pub event_type: String,
    pub position_x: i64,
    pub position_y: i64,
    pub app_name: String,
    pub window_title: String,
    pub process_id: i64,
    pub ui_element: Option<String>,
}

pub(super) fn row_to_keyboard_event(
    row: KeyboardEventRow,
) -> Result<KeyboardEvent, Box<dyn std::error::Error + Send + Sync>> {
    use crate::models::input::{AppContext, KeyEventType, ModifierState, UiElement};

    let event_type = match row.event_type.as_str() {
        "key_down" => KeyEventType::KeyDown,
        "key_up" => KeyEventType::KeyUp,
        _ => KeyEventType::KeyDown,
    };

    let modifiers: ModifierState = serde_json::from_str(&row.modifiers)?;
    let ui_element: Option<UiElement> = row
        .ui_element
        .as_ref()
        .map(|s| serde_json::from_str(s))
        .transpose()?;

    Ok(KeyboardEvent {
        timestamp: row.timestamp,
        event_type,
        key_code: row.key_code as u32,
        key_char: row.key_char.and_then(|s| s.chars().next()),
        modifiers,
        app_context: AppContext {
            app_name: row.app_name,
            window_title: row.window_title,
            process_id: row.process_id as u32,
        },
        ui_element,
        is_sensitive: false,
    })
}

pub(super) fn row_to_mouse_event(
    row: MouseEventRow,
) -> Result<MouseEvent, Box<dyn std::error::Error + Send + Sync>> {
    use crate::models::input::{AppContext, MouseEventType, Point, UiElement};

    let event_type: MouseEventType = serde_json::from_str(&row.event_type)?;
    let ui_element: Option<UiElement> = row
        .ui_element
        .as_ref()
        .map(|s| serde_json::from_str(s))
        .transpose()?;

    Ok(MouseEvent {
        timestamp: row.timestamp,
        event_type,
        position: Point {
            x: row.position_x as i32,
            y: row.position_y as i32,
        },
        app_context: AppContext {
            app_name: row.app_name,
            window_title: row.window_title,
            process_id: row.process_id as u32,
        },
        ui_element,
    })
}
