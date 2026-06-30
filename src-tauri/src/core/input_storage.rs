use crate::core::database::Database;
use crate::core::input_storage_rows::{
    row_to_keyboard_event, row_to_mouse_event, InputTimeline, KeyboardEventRow, MouseEventRow,
    TimeRange,
};
use crate::core::input_storage_schema::init_schema;
use crate::models::input::{KeyboardEvent, MouseEvent};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

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
        init_schema(&db).await?;

        Ok(Self {
            db,
            keyboard_buffer: Arc::new(RwLock::new(Vec::new())),
            mouse_buffer: Arc::new(RwLock::new(Vec::new())),
            buffer_size: 100, // Flush every 100 events
        })
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

    pub async fn flush_keyboard_buffer(
        &self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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

        rows.into_iter().map(row_to_keyboard_event).collect()
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

        rows.into_iter().map(row_to_mouse_event).collect()
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
