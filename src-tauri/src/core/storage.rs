// Frame storage system - saves captured frames to disk and tracks in database

use crate::core::database::Database;
use crate::models::capture::{PixelFormat, RawFrame};
use image::{ImageBuffer, Rgba};
use sqlx::Row;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),

    #[error("Session not found: {0}")]
    SessionNotFound(Uuid),

    #[error("Storage error: {0}")]
    Other(String),
}

pub type StorageResult<T> = Result<T, StorageError>;

/// Recording storage manager
pub struct RecordingStorage {
    base_path: PathBuf,
    db: Arc<Database>,
}

impl RecordingStorage {
    /// Create a new recording storage instance
    pub async fn new(base_path: PathBuf, db: Arc<Database>) -> StorageResult<Self> {
        // Ensure base path exists
        std::fs::create_dir_all(&base_path)?;

        Ok(Self { base_path, db })
    }

    /// Create a new recording session
    pub async fn create_session(&self, display_id: u32) -> StorageResult<Uuid> {
        let session_id = Uuid::new_v4();
        let start_timestamp = chrono::Utc::now().timestamp();

        // Create session directory
        let session_path = self.get_session_path(&session_id);
        std::fs::create_dir_all(session_path.join("frames"))?;

        // Insert session into database
        sqlx::query(
            "INSERT INTO sessions (id, device_id, start_timestamp, recording_path)
             VALUES (?, ?, ?, ?)",
        )
        .bind(session_id.to_string())
        .bind("local") // device_id - using "local" for now
        .bind(start_timestamp)
        .bind(session_path.to_string_lossy().to_string())
        .execute(self.db.pool())
        .await?;

        println!("Created recording session: {}", session_id);
        println!("Recording path: {}", session_path.display());

        Ok(session_id)
    }

    /// Save a frame to disk and database
    pub async fn save_frame(&self, session_id: Uuid, frame: &RawFrame) -> StorageResult<PathBuf> {
        let frame_id = Uuid::new_v4();
        let filename = format!("{}.png", frame.timestamp);
        let frame_path = self
            .get_session_path(&session_id)
            .join("frames")
            .join(&filename);

        // Convert RawFrame to PNG
        self.save_frame_as_png(frame, &frame_path)?;

        // Insert frame record into database
        sqlx::query(
            "INSERT INTO frames (id, session_id, timestamp, file_path, width, height)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(frame_id.to_string())
        .bind(session_id.to_string())
        .bind(frame.timestamp)
        .bind(frame_path.to_string_lossy().to_string())
        .bind(frame.width as i64)
        .bind(frame.height as i64)
        .execute(self.db.pool())
        .await?;

        Ok(frame_path)
    }

    /// End a recording session
    pub async fn end_session(&self, session_id: Uuid) -> StorageResult<()> {
        let end_timestamp = chrono::Utc::now().timestamp();

        // Get session info
        let row = sqlx::query("SELECT start_timestamp FROM sessions WHERE id = ?")
            .bind(session_id.to_string())
            .fetch_optional(self.db.pool())
            .await?
            .ok_or(StorageError::SessionNotFound(session_id))?;

        let start_timestamp: i64 = row.get("start_timestamp");
        let duration = end_timestamp - start_timestamp;

        // Count frames
        let frame_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM frames WHERE session_id = ?")
            .bind(session_id.to_string())
            .fetch_one(self.db.pool())
            .await?;

        // Calculate total size
        let total_size = self.calculate_session_size(&session_id).await?;

        // Update session
        sqlx::query(
            "UPDATE sessions
             SET end_timestamp = ?, frame_count = ?, total_size_bytes = ?
             WHERE id = ?",
        )
        .bind(end_timestamp)
        .bind(frame_count)
        .bind(total_size as i64)
        .bind(session_id.to_string())
        .execute(self.db.pool())
        .await?;

        println!("Ended recording session: {}", session_id);
        println!("  Duration: {}s", duration);
        println!("  Frames: {}", frame_count);
        println!("  Size: {} bytes", total_size);

        Ok(())
    }

    /// Get all frame paths for a session
    pub async fn get_session_frames(&self, session_id: Uuid) -> StorageResult<Vec<PathBuf>> {
        let rows = sqlx::query("SELECT file_path FROM frames WHERE session_id = ? ORDER BY timestamp")
            .bind(session_id.to_string())
            .fetch_all(self.db.pool())
            .await?;

        let paths = rows
            .into_iter()
            .map(|row| {
                let path_str: String = row.get("file_path");
                PathBuf::from(path_str)
            })
            .collect();

        Ok(paths)
    }

    /// Load a frame from disk
    pub async fn load_frame(&self, path: PathBuf) -> StorageResult<RawFrame> {
        let img = image::open(&path)?;
        let rgba_img = img.to_rgba8();

        let width = rgba_img.width();
        let height = rgba_img.height();
        let data = rgba_img.into_raw();

        // Get timestamp from filename
        let timestamp = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);

        Ok(RawFrame {
            timestamp,
            width,
            height,
            data,
            format: PixelFormat::RGBA8,
        })
    }

    /// Delete a recording session
    pub async fn delete_session(&self, session_id: Uuid) -> StorageResult<()> {
        // Delete frames from database
        sqlx::query("DELETE FROM frames WHERE session_id = ?")
            .bind(session_id.to_string())
            .execute(self.db.pool())
            .await?;

        // Delete session from database
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(session_id.to_string())
            .execute(self.db.pool())
            .await?;

        // Delete session directory
        let session_path = self.get_session_path(&session_id);
        if session_path.exists() {
            std::fs::remove_dir_all(&session_path)?;
        }

        println!("Deleted recording session: {}", session_id);

        Ok(())
    }

    /// Get the path for a session
    fn get_session_path(&self, session_id: &Uuid) -> PathBuf {
        self.base_path.join(session_id.to_string())
    }

    /// Calculate total size of all frames in a session
    async fn calculate_session_size(&self, session_id: &Uuid) -> StorageResult<u64> {
        let session_path = self.get_session_path(session_id);
        let frames_path = session_path.join("frames");

        let mut total_size = 0u64;

        if frames_path.exists() {
            for entry in std::fs::read_dir(frames_path)? {
                let entry = entry?;
                if entry.path().extension().and_then(|s| s.to_str()) == Some("png") {
                    total_size += entry.metadata()?.len();
                }
            }
        }

        Ok(total_size)
    }

    /// Save a RawFrame as PNG
    fn save_frame_as_png(&self, frame: &RawFrame, path: &PathBuf) -> StorageResult<()> {
        // Convert to RGBA if needed
        let rgba_data = match frame.format {
            PixelFormat::BGRA8 => {
                // Convert BGRA to RGBA
                let mut rgba = Vec::with_capacity(frame.data.len());
                for chunk in frame.data.chunks_exact(4) {
                    rgba.push(chunk[2]); // R
                    rgba.push(chunk[1]); // G
                    rgba.push(chunk[0]); // B
                    rgba.push(chunk[3]); // A
                }
                rgba
            }
            PixelFormat::RGBA8 => frame.data.clone(),
        };

        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_raw(frame.width, frame.height, rgba_data)
                .ok_or_else(|| StorageError::Other("Failed to create image buffer".to_string()))?;

        img.save(path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_storage_lifecycle() {
        // Initialize database
        let db = Arc::new(Database::init().await.expect("Failed to init database"));

        // Create storage
        let temp_dir = std::env::temp_dir().join("observer_test_recordings");
        let storage = RecordingStorage::new(temp_dir.clone(), db.clone())
            .await
            .expect("Failed to create storage");

        // Create session
        let session_id = storage
            .create_session(0)
            .await
            .expect("Failed to create session");

        // Create a test frame
        let test_frame = RawFrame {
            timestamp: chrono::Utc::now().timestamp_millis(),
            width: 100,
            height: 100,
            data: vec![255u8; 100 * 100 * 4], // White image
            format: PixelFormat::RGBA8,
        };

        // Save frame
        let frame_path = storage
            .save_frame(session_id, &test_frame)
            .await
            .expect("Failed to save frame");

        assert!(frame_path.exists(), "Frame file should exist");

        // Get session frames
        let frames = storage
            .get_session_frames(session_id)
            .await
            .expect("Failed to get frames");

        assert_eq!(frames.len(), 1, "Should have one frame");

        // End session
        storage
            .end_session(session_id)
            .await
            .expect("Failed to end session");

        // Delete session
        storage
            .delete_session(session_id)
            .await
            .expect("Failed to delete session");

        assert!(!frame_path.exists(), "Frame file should be deleted");

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
