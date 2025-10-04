use crate::core::database::Database;
use crate::models::input::{KeyboardEvent, MouseEvent};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// ==============================================================================
// Time Range for Queries
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: i64,
    pub end: i64,
}

// ==============================================================================
// Database Row Types
// ==============================================================================

#[derive(Debug, Clone, sqlx::FromRow)]
struct KeyboardEventRow {
    id: String,
    session_id: String,
    timestamp: i64,
    event_type: String,
    key_code: i64,
    key_char: Option<String>,
    modifiers: String,
    app_name: String,
    window_title: String,
    process_id: i64,
    ui_element: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct MouseEventRow {
    id: String,
    session_id: String,
    timestamp: i64,
    event_type: String,
    position_x: i64,
    position_y: i64,
    app_name: String,
    window_title: String,
    process_id: i64,
    ui_element: Option<String>,
}

// ==============================================================================
// Input Timeline
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputTimeline {
    pub keyboard_events: Vec<KeyboardEvent>,
    pub mouse_events: Vec<MouseEvent>,
}

// ==============================================================================
// Input Storage
// ==============================================================================

pub struct InputStorage {
    db: Arc<Database>,
    keyboard_buffer: Arc<RwLock<Vec<(String, KeyboardEvent)>>>, // (session_id, event)
    mouse_buffer: Arc<RwLock<Vec<(String, MouseEvent)>>>,       // (session_id, event)
    buffer_size: usize,
}

impl InputStorage {
    pub async fn new(db: Arc<Database>) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Initialize database schema
        Self::init_schema(&db).await?;

        Ok(Self {
            db,
            keyboard_buffer: Arc::new(RwLock::new(Vec::new())),
            mouse_buffer: Arc::new(RwLock::new(Vec::new())),
            buffer_size: 100, // Flush every 100 events
        })
    }

    async fn init_schema(db: &Arc<Database>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let pool = db.pool();

        // Create keyboard_events table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS keyboard_events (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                event_type TEXT NOT NULL,
                key_code INTEGER NOT NULL,
                key_char TEXT,
                modifiers TEXT NOT NULL,
                app_name TEXT NOT NULL,
                window_title TEXT NOT NULL,
                process_id INTEGER NOT NULL,
                ui_element TEXT,
                FOREIGN KEY (session_id) REFERENCES sessions(id)
            )
            "#,
        )
        .execute(pool)
        .await?;

        // Create indexes for keyboard_events
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_keyboard_session ON keyboard_events(session_id)")
            .execute(pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_keyboard_timestamp ON keyboard_events(timestamp)")
            .execute(pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_keyboard_app ON keyboard_events(app_name)")
            .execute(pool)
            .await?;

        // Create mouse_events table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS mouse_events (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                event_type TEXT NOT NULL,
                position_x INTEGER NOT NULL,
                position_y INTEGER NOT NULL,
                app_name TEXT NOT NULL,
                window_title TEXT NOT NULL,
                process_id INTEGER NOT NULL,
                ui_element TEXT,
                FOREIGN KEY (session_id) REFERENCES sessions(id)
            )
            "#,
        )
        .execute(pool)
        .await?;

        // Create indexes for mouse_events
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_mouse_session ON mouse_events(session_id)")
            .execute(pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_mouse_timestamp ON mouse_events(timestamp)")
            .execute(pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_mouse_position ON mouse_events(position_x, position_y)")
            .execute(pool)
            .await?;

        Ok(())
    }

    // ==============================================================================
    // Keyboard Event Storage
    // ==============================================================================

    pub async fn store_keyboard_event(
        &self,
        session_id: String,
        event: KeyboardEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut buffer = self.keyboard_buffer.write().await;
        buffer.push((session_id, event));

        if buffer.len() >= self.buffer_size {
            drop(buffer); // Release lock before flushing
            self.flush_keyboard_buffer().await?;
        }

        Ok(())
    }

    pub async fn flush_keyboard_buffer(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut buffer = self.keyboard_buffer.write().await;

        if buffer.is_empty() {
            return Ok(());
        }

        let pool = self.db.pool();

        // Begin transaction for batch insert
        let mut tx = pool.begin().await?;

        for (session_id, event) in buffer.drain(..) {
            let modifiers_json = serde_json::to_string(&event.modifiers)?;
            let ui_element_json = event
                .ui_element
                .as_ref()
                .map(|e| serde_json::to_string(e))
                .transpose()?;

            sqlx::query(
                r#"
                INSERT INTO keyboard_events (
                    id, session_id, timestamp, event_type, key_code, key_char,
                    modifiers, app_name, window_title, process_id, ui_element
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(Uuid::new_v4().to_string())
            .bind(session_id)
            .bind(event.timestamp)
            .bind(event.event_type.to_string())
            .bind(event.key_code as i64)
            .bind(event.key_char.map(|c| c.to_string()))
            .bind(modifiers_json)
            .bind(event.app_context.app_name)
            .bind(event.app_context.window_title)
            .bind(event.app_context.process_id as i64)
            .bind(ui_element_json)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        Ok(())
    }

    // ==============================================================================
    // Mouse Event Storage
    // ==============================================================================

    pub async fn store_mouse_event(
        &self,
        session_id: String,
        event: MouseEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut buffer = self.mouse_buffer.write().await;
        buffer.push((session_id, event));

        if buffer.len() >= self.buffer_size {
            drop(buffer); // Release lock before flushing
            self.flush_mouse_buffer().await?;
        }

        Ok(())
    }

    pub async fn flush_mouse_buffer(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut buffer = self.mouse_buffer.write().await;

        if buffer.is_empty() {
            return Ok(());
        }

        let pool = self.db.pool();

        // Begin transaction for batch insert
        let mut tx = pool.begin().await?;

        for (session_id, event) in buffer.drain(..) {
            let ui_element_json = event
                .ui_element
                .as_ref()
                .map(|e| serde_json::to_string(e))
                .transpose()?;

            sqlx::query(
                r#"
                INSERT INTO mouse_events (
                    id, session_id, timestamp, event_type,
                    position_x, position_y, app_name, window_title, process_id, ui_element
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(Uuid::new_v4().to_string())
            .bind(session_id)
            .bind(event.timestamp)
            .bind(event.event_type.to_string())
            .bind(event.position.x as i64)
            .bind(event.position.y as i64)
            .bind(event.app_context.app_name)
            .bind(event.app_context.window_title)
            .bind(event.app_context.process_id as i64)
            .bind(ui_element_json)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        Ok(())
    }

    // ==============================================================================
    // Flush All Buffers
    // ==============================================================================

    pub async fn flush_buffers(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.flush_keyboard_buffer().await?;
        self.flush_mouse_buffer().await?;
        Ok(())
    }

    // ==============================================================================
    // Querying
    // ==============================================================================

    pub async fn get_keyboard_events(
        &self,
        session_id: String,
        time_range: Option<TimeRange>,
    ) -> Result<Vec<KeyboardEvent>, Box<dyn std::error::Error + Send + Sync>> {
        let pool = self.db.pool();

        let rows: Vec<KeyboardEventRow> = if let Some(range) = time_range {
            sqlx::query_as(
                r#"
                SELECT * FROM keyboard_events
                WHERE session_id = ?
                  AND timestamp >= ?
                  AND timestamp <= ?
                ORDER BY timestamp ASC
                "#,
            )
            .bind(session_id)
            .bind(range.start)
            .bind(range.end)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as(
                r#"
                SELECT * FROM keyboard_events
                WHERE session_id = ?
                ORDER BY timestamp ASC
                "#,
            )
            .bind(session_id)
            .fetch_all(pool)
            .await?
        };

        rows.into_iter()
            .map(|row| self.row_to_keyboard_event(row))
            .collect()
    }

    pub async fn get_mouse_events(
        &self,
        session_id: String,
        time_range: Option<TimeRange>,
    ) -> Result<Vec<MouseEvent>, Box<dyn std::error::Error + Send + Sync>> {
        let pool = self.db.pool();

        let rows: Vec<MouseEventRow> = if let Some(range) = time_range {
            sqlx::query_as(
                r#"
                SELECT * FROM mouse_events
                WHERE session_id = ?
                  AND timestamp >= ?
                  AND timestamp <= ?
                ORDER BY timestamp ASC
                "#,
            )
            .bind(session_id)
            .bind(range.start)
            .bind(range.end)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as(
                r#"
                SELECT * FROM mouse_events
                WHERE session_id = ?
                ORDER BY timestamp ASC
                "#,
            )
            .bind(session_id)
            .fetch_all(pool)
            .await?
        };

        rows.into_iter()
            .map(|row| self.row_to_mouse_event(row))
            .collect()
    }

    pub async fn get_input_timeline(
        &self,
        session_id: String,
    ) -> Result<InputTimeline, Box<dyn std::error::Error + Send + Sync>> {
        let keyboard_events = self.get_keyboard_events(session_id.clone(), None).await?;
        let mouse_events = self.get_mouse_events(session_id, None).await?;

        Ok(InputTimeline {
            keyboard_events,
            mouse_events,
        })
    }

    // ==============================================================================
    // Row Conversion
    // ==============================================================================

    fn row_to_keyboard_event(
        &self,
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
            is_sensitive: false, // Set based on UI element if needed
        })
    }

    fn row_to_mouse_event(
        &self,
        row: MouseEventRow,
    ) -> Result<MouseEvent, Box<dyn std::error::Error + Send + Sync>> {
        use crate::models::input::{AppContext, MouseEventType, Point, UiElement};

        // Parse event_type JSON
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

    // ==============================================================================
    // Retention Policy
    // ==============================================================================

    pub async fn cleanup_old_events(
        &self,
        retention_days: u32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let cutoff_timestamp = chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::days(retention_days as i64))
            .unwrap()
            .timestamp_millis();

        let pool = self.db.pool();

        // Delete old keyboard events
        sqlx::query("DELETE FROM keyboard_events WHERE timestamp < ?")
            .bind(cutoff_timestamp)
            .execute(pool)
            .await?;

        // Delete old mouse events
        sqlx::query("DELETE FROM mouse_events WHERE timestamp < ?")
            .bind(cutoff_timestamp)
            .execute(pool)
            .await?;

        // Vacuum database to reclaim space
        sqlx::query("VACUUM").execute(pool).await?;

        Ok(())
    }
}
