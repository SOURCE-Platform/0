use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechEmotionSegmentDto {
    pub speech_emotion_segment_id: String,
    pub session_id: String,
    pub source_id: String,
    pub audio_chunk_id: String,
    pub asr_segment_id: Option<String>,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub trigger_reason: String,
    pub emotion_label: String,
    pub canonical_label: String,
    pub confidence: f32,
    pub model_name: String,
    pub model_version: String,
    pub raw_json: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundEventDetectionDto {
    pub sound_event_detection_id: String,
    pub session_id: String,
    pub source_id: String,
    pub audio_chunk_id: String,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub trigger_reason: String,
    pub event_label: String,
    pub canonical_label: String,
    pub confidence: f32,
    pub model_name: String,
    pub model_version: String,
    pub raw_json: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundEventSpanDto {
    pub sound_event_span_id: String,
    pub session_id: String,
    pub source_id: String,
    pub canonical_label: String,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub duration_ms: i64,
    pub supporting_detection_ids: Vec<String>,
    pub supporting_audio_chunk_ids: Vec<String>,
    pub avg_confidence: f32,
    pub max_confidence: f32,
    pub model_name: String,
    pub model_version: String,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct SpeechEmotionSegmentRow {
    pub speech_emotion_segment_id: String,
    pub session_id: String,
    pub source_id: String,
    pub audio_chunk_id: String,
    pub asr_segment_id: Option<String>,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub trigger_reason: String,
    pub emotion_label: String,
    pub canonical_label: String,
    pub confidence: f64,
    pub model_name: String,
    pub model_version: String,
    pub raw_json: String,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct SoundEventDetectionRow {
    pub sound_event_detection_id: String,
    pub session_id: String,
    pub source_id: String,
    pub audio_chunk_id: String,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub trigger_reason: String,
    pub event_label: String,
    pub canonical_label: String,
    pub confidence: f64,
    pub model_name: String,
    pub model_version: String,
    pub raw_json: String,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct SoundEventSpanRow {
    pub sound_event_span_id: String,
    pub session_id: String,
    pub source_id: String,
    pub canonical_label: String,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub duration_ms: i64,
    pub supporting_detection_ids_json: String,
    pub supporting_audio_chunk_ids_json: String,
    pub avg_confidence: f64,
    pub max_confidence: f64,
    pub model_name: String,
    pub model_version: String,
}

#[derive(Debug, Clone)]
pub(super) struct SoundEventSpanSeed {
    pub canonical_label: String,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub supporting_detection_ids: Vec<String>,
    pub supporting_audio_chunk_ids: Vec<String>,
    pub confidences: Vec<f32>,
    pub model_name: String,
    pub model_version: String,
}
