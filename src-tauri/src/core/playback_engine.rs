use crate::core::database::Database;
use crate::core::storage::RecordingStorage;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackInfo {
    pub session_id: String,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub base_layer_path: String,
    pub segments: Vec<VideoSegmentInfo>,
    pub total_duration_ms: u64,
    pub frame_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoSegmentInfo {
    pub path: String,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeekInfo {
    pub video_path: String,
    pub offset_ms: i64,
    pub segment_start: i64,
}

#[derive(sqlx::FromRow)]
struct ScreenRecordingRow {
    id: String,
    session_id: String,
    start_timestamp: i64,
    end_timestamp: Option<i64>,
    base_layer_path: String,
    display_id: String,
}

#[derive(sqlx::FromRow)]
struct VideoSegmentRow {
    id: String,
    session_id: String,
    file_path: String,
    start_timestamp: i64,
    end_timestamp: i64,
    duration_ms: i64,
}

pub struct PlaybackEngine {
    storage: Arc<RecordingStorage>,
    db: Arc<Database>,
}

impl PlaybackEngine {
    pub fn new(storage: Arc<RecordingStorage>, db: Arc<Database>) -> Self {
        Self { storage, db }
    }

    pub async fn get_playback_info(&self, session_id: Uuid) -> Result<PlaybackInfo, Box<dyn std::error::Error + Send + Sync>> {
        // Get screen recording for session
        let recording = sqlx::query_as::<_, ScreenRecordingRow>(
            r#"
            SELECT id, session_id, start_timestamp, end_timestamp, base_layer_path, display_id
            FROM screen_recordings
            WHERE session_id = ?
            "#
        )
        .bind(session_id.to_string())
        .fetch_one(&self.db.pool)
        .await?;

        // Get video segments (frames captured)
        let segments = sqlx::query_as::<_, VideoSegmentRow>(
            r#"
            SELECT id, session_id, file_path, start_timestamp, end_timestamp, duration_ms
            FROM frames
            WHERE session_id = ?
            ORDER BY start_timestamp ASC
            "#
        )
        .bind(session_id.to_string())
        .fetch_all(&self.db.pool)
        .await
        .unwrap_or_else(|_| Vec::new());

        let segment_infos: Vec<VideoSegmentInfo> = segments
            .iter()
            .map(|seg| VideoSegmentInfo {
                path: seg.file_path.clone(),
                start_timestamp: seg.start_timestamp,
                end_timestamp: seg.end_timestamp,
                duration_ms: seg.duration_ms as u64,
            })
            .collect();

        let total_duration: u64 = segment_infos
            .iter()
            .map(|s| s.duration_ms)
            .sum();

        let frame_count = segments.len() as u32;

        let end_timestamp = recording.end_timestamp
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());

        Ok(PlaybackInfo {
            session_id: recording.session_id,
            start_timestamp: recording.start_timestamp,
            end_timestamp,
            base_layer_path: recording.base_layer_path,
            segments: segment_infos,
            total_duration_ms: total_duration,
            frame_count,
        })
    }

    pub async fn get_frame_at_timestamp(
        &self,
        session_id: Uuid,
        timestamp: i64,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Find which segment/frame contains this timestamp
        let segment = sqlx::query_as::<_, VideoSegmentRow>(
            r#"
            SELECT id, session_id, file_path, start_timestamp, end_timestamp, duration_ms
            FROM frames
            WHERE session_id = ?
              AND start_timestamp <= ?
              AND end_timestamp >= ?
            LIMIT 1
            "#
        )
        .bind(session_id.to_string())
        .bind(timestamp)
        .bind(timestamp)
        .fetch_optional(&self.db.pool)
        .await?;

        if let Some(seg) = segment {
            Ok(seg.file_path)
        } else {
            // No segment at this time, return base layer
            let recording = sqlx::query_as::<_, ScreenRecordingRow>(
                r#"
                SELECT id, session_id, start_timestamp, end_timestamp, base_layer_path, display_id
                FROM screen_recordings
                WHERE session_id = ?
                "#
            )
            .bind(session_id.to_string())
            .fetch_one(&self.db.pool)
            .await?;

            Ok(recording.base_layer_path)
        }
    }

    pub async fn seek_to_timestamp(
        &self,
        session_id: Uuid,
        timestamp: i64,
    ) -> Result<SeekInfo, Box<dyn std::error::Error + Send + Sync>> {
        let frame_path = self.get_frame_at_timestamp(session_id, timestamp).await?;

        // Get segment info
        let segment_info = sqlx::query_as::<_, VideoSegmentRow>(
            r#"
            SELECT id, session_id, file_path, start_timestamp, end_timestamp, duration_ms
            FROM frames
            WHERE file_path = ?
            "#
        )
        .bind(&frame_path)
        .fetch_optional(&self.db.pool)
        .await?;

        if let Some(info) = segment_info {
            // Calculate offset within segment
            let offset_ms = timestamp - info.start_timestamp;

            Ok(SeekInfo {
                video_path: frame_path,
                offset_ms,
                segment_start: info.start_timestamp,
            })
        } else {
            // Base layer (static frame)
            Ok(SeekInfo {
                video_path: frame_path,
                offset_ms: 0,
                segment_start: timestamp,
            })
        }
    }

    pub async fn generate_thumbnail(
        &self,
        session_id: Uuid,
        timestamp: i64,
        _width: u32,
        _height: u32,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // For now, just return the frame path directly
        // TODO: Implement actual thumbnail generation with resizing
        let frame_path = self.get_frame_at_timestamp(session_id, timestamp).await?;
        Ok(frame_path)
    }
}
