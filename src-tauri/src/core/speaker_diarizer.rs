use crate::core::database::Database;
use crate::models::audio::{
    SpeakerSegment, SpeakerInfo, AudioError, AudioResult,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

// ==============================================================================
// Database Models
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SpeakerSegmentRecord {
    pub id: String,
    pub recording_id: String,
    pub speaker_id: String,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub confidence: f64,
    pub embedding_json: Option<String>,
    pub created_at: i64,
}

// ==============================================================================
// Speaker Diarizer
// ==============================================================================

pub struct SpeakerDiarizer {
    db: Arc<Database>,
}

impl SpeakerDiarizer {
    pub async fn new(db: Arc<Database>) -> AudioResult<Self> {
        Ok(Self { db })
    }

    /// Perform speaker diarization on an audio file
    pub async fn diarize_audio(
        &self,
        recording_id: &str,
        audio_file_path: &str,
    ) -> AudioResult<Vec<SpeakerSegment>> {
        // TODO: Implement actual pyannote.audio diarization
        // This is a placeholder implementation

        // In a real implementation:
        // 1. Load pyannote.audio diarization model
        // 2. Process audio file to detect speaker segments
        // 3. Extract voice embeddings for each speaker
        // 4. Cluster speakers and assign IDs (SPEAKER_00, SPEAKER_01, etc.)
        // 5. Return segments with speaker attributions

        println!("Diarizing audio file: {}", audio_file_path);

        // Placeholder: return empty segments
        Ok(vec![])
    }

    /// Store speaker segment in database
    pub async fn store_speaker_segment(&self, segment: &SpeakerSegment) -> AudioResult<()> {
        let pool = self.db.pool();
        let created_at = chrono::Utc::now().timestamp_millis();

        let embedding_json = segment.embedding.as_ref()
            .map(|emb| serde_json::to_string(emb).ok())
            .flatten();

        sqlx::query(
            "INSERT INTO speaker_segments (
                id, recording_id, speaker_id, start_timestamp, end_timestamp,
                confidence, embedding_json, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&segment.id)
        .bind(&segment.recording_id)
        .bind(&segment.speaker_id)
        .bind(segment.start_timestamp)
        .bind(segment.end_timestamp)
        .bind(segment.confidence as f64)
        .bind(embedding_json)
        .bind(created_at)
        .execute(pool)
        .await
        .map_err(|e| AudioError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    /// Get speaker segments for a recording
    pub async fn get_speaker_segments(
        &self,
        recording_id: &str,
        speaker_id: Option<String>,
    ) -> AudioResult<Vec<SpeakerSegment>> {
        let pool = self.db.pool();

        let records: Vec<SpeakerSegmentRecord> = if let Some(spk_id) = speaker_id {
            sqlx::query_as(
                "SELECT * FROM speaker_segments
                 WHERE recording_id = ? AND speaker_id = ?
                 ORDER BY start_timestamp ASC"
            )
            .bind(recording_id)
            .bind(spk_id)
            .fetch_all(pool)
            .await
        } else {
            sqlx::query_as(
                "SELECT * FROM speaker_segments
                 WHERE recording_id = ?
                 ORDER BY start_timestamp ASC"
            )
            .bind(recording_id)
            .fetch_all(pool)
            .await
        }
        .map_err(|e| AudioError::DatabaseError(e.to_string()))?;

        let segments = records.into_iter().map(|r| {
            let embedding = r.embedding_json
                .and_then(|json| serde_json::from_str::<Vec<f32>>(&json).ok());

            SpeakerSegment {
                id: r.id,
                recording_id: r.recording_id,
                speaker_id: r.speaker_id,
                start_timestamp: r.start_timestamp,
                end_timestamp: r.end_timestamp,
                confidence: r.confidence as f32,
                embedding,
            }
        }).collect();

        Ok(segments)
    }

    /// Get speaker information for a session
    pub async fn get_speakers(&self, session_id: &str) -> AudioResult<Vec<SpeakerInfo>> {
        let pool = self.db.pool();

        // Get all recordings for this session
        let recording_ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM audio_recordings WHERE session_id = ?"
        )
        .bind(session_id)
        .fetch_all(pool)
        .await
        .map_err(|e| AudioError::DatabaseError(e.to_string()))?;

        if recording_ids.is_empty() {
            return Ok(vec![]);
        }

        // Get speaker segments for all recordings
        let mut speaker_stats: HashMap<String, (u64, u32, f32)> = HashMap::new();

        for recording_id in &recording_ids {
            let segments = self.get_speaker_segments(recording_id, None).await?;

            for segment in segments {
                let duration = (segment.end_timestamp - segment.start_timestamp) as u64;
                let entry = speaker_stats.entry(segment.speaker_id.clone()).or_insert((0, 0, 0.0));
                entry.0 += duration;
                entry.1 += 1;
                entry.2 += segment.confidence;
            }
        }

        // Determine primary user (speaker with most speaking time)
        let primary_speaker = speaker_stats.iter()
            .max_by_key(|(_, (duration, _, _))| duration)
            .map(|(id, _)| id.clone());

        let mut speaker_infos: Vec<SpeakerInfo> = speaker_stats.into_iter().map(|(speaker_id, (total_time, count, total_conf))| {
            SpeakerInfo {
                speaker_id: speaker_id.clone(),
                session_id: session_id.to_string(),
                total_speaking_time_ms: total_time,
                segment_count: count,
                average_confidence: total_conf / count as f32,
                is_primary_user: primary_speaker.as_ref() == Some(&speaker_id),
            }
        }).collect();

        // Sort by speaking time (descending)
        speaker_infos.sort_by_key(|s| std::cmp::Reverse(s.total_speaking_time_ms));

        Ok(speaker_infos)
    }
}
