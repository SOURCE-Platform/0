#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::core::database::Database;
    use crate::core::storage::RecordingStorage;
    use crate::models::capture::{PixelFormat, RawFrame};

    #[tokio::test]
    async fn test_storage_lifecycle() {
        let db = Arc::new(Database::init().await.expect("Failed to init database"));
        let temp_dir = std::env::temp_dir().join("observer_test_recordings");
        let storage = RecordingStorage::new(temp_dir.clone(), db.clone())
            .await
            .expect("Failed to create storage");

        let session_id = storage
            .create_session(0)
            .await
            .expect("Failed to create session");

        let test_frame = RawFrame {
            timestamp: chrono::Utc::now().timestamp_millis(),
            width: 100,
            height: 100,
            data: vec![255u8; 100 * 100 * 4],
            format: PixelFormat::RGBA8,
        };

        let frame_path = storage
            .save_frame(session_id, &test_frame)
            .await
            .expect("Failed to save frame");
        assert!(frame_path.exists(), "Frame file should exist");

        let frames = storage
            .get_session_frames(session_id)
            .await
            .expect("Failed to get frames");
        assert_eq!(frames.len(), 1, "Should have one frame");

        storage
            .end_session(session_id)
            .await
            .expect("Failed to end session");
        storage
            .delete_session(session_id)
            .await
            .expect("Failed to delete session");

        assert!(!frame_path.exists(), "Frame file should be deleted");
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
