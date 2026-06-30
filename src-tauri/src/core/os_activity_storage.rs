use crate::core::database::Database;
use crate::models::activity::AppEvent;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AppUsage {
    pub id: String,
    pub session_id: String,
    pub app_name: String,
    pub bundle_id: String,
    pub process_id: i64,
    pub start_timestamp: i64,
    pub end_timestamp: Option<i64>,
    pub focus_duration_ms: i64,
    pub background_duration_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AppUsageStats {
    pub app_name: String,
    pub bundle_id: String,
    pub total_focus_duration_ms: i64,
    pub total_background_duration_ms: i64,
    pub launch_count: i64,
    pub first_launch: i64,
    pub last_terminate: Option<i64>,
}

#[derive(Clone)]
pub struct ActivityStorage {
    db: Arc<Database>,
}

impl ActivityStorage {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub async fn init_schema(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let pool = self.db.pool();

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS app_usage (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                app_name TEXT NOT NULL,
                bundle_id TEXT NOT NULL,
                process_id INTEGER NOT NULL,
                start_timestamp INTEGER NOT NULL,
                end_timestamp INTEGER,
                focus_duration_ms INTEGER DEFAULT 0,
                background_duration_ms INTEGER DEFAULT 0,
                FOREIGN KEY (session_id) REFERENCES sessions(id)
            )",
        )
        .execute(pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_app_usage_session ON app_usage(session_id)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_app_usage_app ON app_usage(app_name)")
            .execute(pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_app_usage_time ON app_usage(start_timestamp)")
            .execute(pool)
            .await?;

        Ok(())
    }

    pub async fn record_app_launch(
        &self,
        session_id: &str,
        event: AppEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        sqlx::query(
            "INSERT INTO app_usage (id, session_id, app_name, bundle_id, process_id, start_timestamp)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(session_id)
        .bind(event.app_info.name)
        .bind(event.app_info.bundle_id)
        .bind(event.app_info.process_id as i64)
        .bind(event.timestamp)
        .execute(self.db.pool())
        .await?;

        Ok(())
    }

    pub async fn record_app_terminate(
        &self,
        event: AppEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        sqlx::query(
            "UPDATE app_usage SET end_timestamp = ?
             WHERE process_id = ? AND end_timestamp IS NULL",
        )
        .bind(event.timestamp)
        .bind(event.app_info.process_id as i64)
        .execute(self.db.pool())
        .await?;

        Ok(())
    }

    pub async fn record_focus_duration(
        &self,
        process_id: u32,
        duration_ms: i64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        sqlx::query(
            "UPDATE app_usage
             SET focus_duration_ms = focus_duration_ms + ?
             WHERE process_id = ? AND end_timestamp IS NULL",
        )
        .bind(duration_ms)
        .bind(process_id as i64)
        .execute(self.db.pool())
        .await?;

        Ok(())
    }

    pub async fn get_app_usage_for_session(
        &self,
        session_id: String,
    ) -> Result<Vec<AppUsage>, Box<dyn std::error::Error + Send + Sync>> {
        let results = sqlx::query_as::<_, AppUsage>(
            "SELECT id, session_id, app_name, bundle_id, process_id,
                    start_timestamp, end_timestamp, focus_duration_ms, background_duration_ms
             FROM app_usage
             WHERE session_id = ?
             ORDER BY start_timestamp DESC",
        )
        .bind(session_id)
        .fetch_all(self.db.pool())
        .await?;

        Ok(results)
    }

    pub async fn get_app_usage_stats(
        &self,
        session_id: String,
    ) -> Result<Vec<AppUsageStats>, Box<dyn std::error::Error + Send + Sync>> {
        let results = sqlx::query_as::<_, AppUsageStats>(
            "SELECT
                app_name,
                bundle_id,
                SUM(focus_duration_ms) as total_focus_duration_ms,
                SUM(background_duration_ms) as total_background_duration_ms,
                COUNT(*) as launch_count,
                MIN(start_timestamp) as first_launch,
                MAX(end_timestamp) as last_terminate
             FROM app_usage
             WHERE session_id = ?
             GROUP BY app_name, bundle_id
             ORDER BY total_focus_duration_ms DESC",
        )
        .bind(session_id)
        .fetch_all(self.db.pool())
        .await?;

        Ok(results)
    }
}
