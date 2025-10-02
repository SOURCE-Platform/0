use sqlx::sqlite::{SqliteConnection, SqlitePool, SqlitePoolOptions};
use sqlx::{migrate::MigrateDatabase, Sqlite};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Database {
    pub(crate) pool: SqlitePool,
}

impl Database {
    /// Initialize the database with migrations
    pub async fn init() -> Result<Self, Box<dyn std::error::Error>> {
        let db_path = Self::get_db_path()?;
        let db_url = format!("sqlite://{}", db_path.display());

        // Create database directory if it doesn't exist
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Create database if it doesn't exist
        if !Sqlite::database_exists(&db_url).await? {
            Sqlite::create_database(&db_url).await?;
        }

        // Create connection pool
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&db_url)
            .await?;

        let db = Self { pool };

        // Run migrations
        db.run_migrations().await?;

        Ok(db)
    }

    /// Get a connection from the pool
    pub async fn get_connection(&self) -> Result<SqliteConnection, sqlx::Error> {
        self.pool.acquire().await.map(|conn| conn.detach())
    }

    /// Get the pool reference
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Run database migrations
    pub async fn run_migrations(&self) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await?;
        Ok(())
    }

    /// Get the database file path
    fn get_db_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| "Could not determine home directory")?;

        let mut path = PathBuf::from(home);
        path.push(".observer_data");
        path.push("database");
        path.push("observer.db");

        Ok(path)
    }
}

// Session model
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Session {
    pub id: String,
    pub start_timestamp: i64,
    pub end_timestamp: Option<i64>,
    pub device_id: String,
    pub created_at: i64,
}

impl Database {
    /// Create a new session
    pub async fn create_session(
        &self,
        id: &str,
        start_timestamp: i64,
        device_id: &str,
    ) -> Result<(), sqlx::Error> {
        let created_at = chrono::Utc::now().timestamp();

        sqlx::query(
            "INSERT INTO sessions (id, start_timestamp, end_timestamp, device_id, created_at)
             VALUES (?, ?, NULL, ?, ?)"
        )
        .bind(id)
        .bind(start_timestamp)
        .bind(device_id)
        .bind(created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get a session by ID
    pub async fn get_session(&self, id: &str) -> Result<Option<Session>, sqlx::Error> {
        sqlx::query_as::<_, Session>("SELECT * FROM sessions WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    /// Update session end timestamp
    pub async fn end_session(&self, id: &str, end_timestamp: i64) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE sessions SET end_timestamp = ? WHERE id = ?")
            .bind(end_timestamp)
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Delete a session
    pub async fn delete_session(&self, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// List all sessions
    pub async fn list_sessions(&self) -> Result<Vec<Session>, sqlx::Error> {
        sqlx::query_as::<_, Session>("SELECT * FROM sessions ORDER BY start_timestamp DESC")
            .fetch_all(&self.pool)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup_test_db() -> Database {
        // Use in-memory database for tests
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("Failed to create in-memory database");

        let db = Database { pool };

        // Run migrations
        db.run_migrations().await.expect("Failed to run migrations");

        db
    }

    #[tokio::test]
    async fn test_create_and_get_session() {
        let db = setup_test_db().await;
        let session_id = uuid::Uuid::new_v4().to_string();
        let start_time = chrono::Utc::now().timestamp();
        let device_id = "test-device";

        // Create session
        db.create_session(&session_id, start_time, device_id)
            .await
            .expect("Failed to create session");

        // Get session
        let session = db.get_session(&session_id)
            .await
            .expect("Failed to get session")
            .expect("Session not found");

        assert_eq!(session.id, session_id);
        assert_eq!(session.start_timestamp, start_time);
        assert_eq!(session.device_id, device_id);
        assert!(session.end_timestamp.is_none());
    }

    #[tokio::test]
    async fn test_end_session() {
        let db = setup_test_db().await;
        let session_id = uuid::Uuid::new_v4().to_string();
        let start_time = chrono::Utc::now().timestamp();
        let end_time = start_time + 3600;

        // Create and end session
        db.create_session(&session_id, start_time, "test-device")
            .await
            .expect("Failed to create session");

        db.end_session(&session_id, end_time)
            .await
            .expect("Failed to end session");

        // Verify end timestamp
        let session = db.get_session(&session_id)
            .await
            .expect("Failed to get session")
            .expect("Session not found");

        assert_eq!(session.end_timestamp, Some(end_time));
    }

    #[tokio::test]
    async fn test_delete_session() {
        let db = setup_test_db().await;
        let session_id = uuid::Uuid::new_v4().to_string();

        // Create and delete session
        db.create_session(&session_id, chrono::Utc::now().timestamp(), "test-device")
            .await
            .expect("Failed to create session");

        db.delete_session(&session_id)
            .await
            .expect("Failed to delete session");

        // Verify deletion
        let session = db.get_session(&session_id)
            .await
            .expect("Failed to query session");

        assert!(session.is_none());
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let db = setup_test_db().await;

        // Create multiple sessions
        for i in 0..3 {
            let session_id = uuid::Uuid::new_v4().to_string();
            let start_time = chrono::Utc::now().timestamp() + i;
            db.create_session(&session_id, start_time, "test-device")
                .await
                .expect("Failed to create session");
        }

        // List sessions
        let sessions = db.list_sessions()
            .await
            .expect("Failed to list sessions");

        assert_eq!(sessions.len(), 3);
    }
}
