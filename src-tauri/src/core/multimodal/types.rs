use super::audio_intelligence_types::{SoundEventSpanDto, SpeechEmotionSegmentDto};
use crate::core::motion_detector::MotionDetector;
use crate::core::ocr_agent_context::ActivityEpisodeDto;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawDetectorOutputDto {
    pub detector_type: String,
    pub model_name: String,
    pub model_version: String,
    pub confidence: f32,
    pub processing_time_ms: i64,
    pub raw_json: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualSceneSnapshotDto {
    pub visual_scene_id: String,
    pub session_id: String,
    pub timestamp: i64,
    pub source_id: String,
    pub trigger_reason: String,
    pub frame_id: Option<String>,
    pub frame_path: Option<String>,
    pub frame_width: Option<u32>,
    pub frame_height: Option<u32>,
    pub person_count: i64,
    pub presence_label: String,
    pub presence_confidence: f32,
    pub posture_label: String,
    pub posture_confidence: f32,
    pub motion_label: String,
    pub motion_confidence: f32,
    pub object_labels: Vec<String>,
    pub object_boxes: Vec<Value>,
    pub fused_state: Value,
    pub avg_confidence: f32,
    pub processing_time_ms: i64,
    pub detections: Vec<RawDetectorOutputDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualStateSpanDto {
    pub visual_state_span_id: String,
    pub session_id: String,
    pub source_id: String,
    pub state_type: String,
    pub label: String,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub duration_ms: i64,
    pub scene_ids: Vec<String>,
    pub avg_confidence: f32,
    pub min_confidence: f32,
    pub max_confidence: f32,
    pub transition_in: Option<String>,
    pub transition_out: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioChunkDto {
    pub audio_chunk_id: String,
    pub session_id: String,
    pub source_id: String,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub trigger_reason: String,
    pub audio_path: Option<String>,
    pub retained_as_evidence: bool,
    pub vad_score: f32,
    pub speech_detected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AsrSegmentDto {
    pub asr_segment_id: String,
    pub session_id: String,
    pub source_id: String,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub language: Option<String>,
    pub transcript: String,
    pub confidence: Option<f32>,
    pub model_name: String,
    pub model_version: String,
    pub audio_chunk_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStateSpanDto {
    pub audio_state_span_id: String,
    pub session_id: String,
    pub source_id: String,
    pub state_type: String,
    pub label: String,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub duration_ms: i64,
    pub supporting_audio_chunk_ids: Vec<String>,
    pub supporting_asr_segment_ids: Vec<String>,
    pub avg_confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualAudioSummaryDto {
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub visual_scene_count: usize,
    pub visual_span_count: usize,
    pub audio_chunk_count: usize,
    pub audio_span_count: usize,
    pub asr_segment_count: usize,
    pub speech_emotion_segment_count: usize,
    pub sound_event_detection_count: usize,
    pub sound_event_span_count: usize,
    pub visible_duration_ms: i64,
    pub speaking_duration_ms: i64,
    pub dominant_postures: Vec<String>,
    pub dominant_audio_states: Vec<String>,
    pub dominant_speech_emotions: Vec<String>,
    pub dominant_sound_events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultimodalActivityEpisodeDto {
    pub timestamp: i64,
    pub visual_scene: Option<VisualSceneSnapshotDto>,
    pub visual_spans: Vec<VisualStateSpanDto>,
    pub audio_spans: Vec<AudioStateSpanDto>,
    pub asr_segments: Vec<AsrSegmentDto>,
    pub speech_emotion_segments: Vec<SpeechEmotionSegmentDto>,
    pub sound_event_spans: Vec<SoundEventSpanDto>,
    pub ocr_episode: Option<ActivityEpisodeDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MultimodalStartReport {
    pub visual_started: bool,
    pub audio_started: bool,
    pub visual_source_name: Option<String>,
    pub audio_source_name: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MultimodalCaptureOptions {
    pub enable_visual: bool,
    pub enable_audio: bool,
    pub display_id: Option<u32>,
    pub audio_source_id: Option<String>,
    pub enable_microphone_audio: bool,
    pub enable_desktop_audio: bool,
    pub desktop_audio_gain_db: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct VisionSceneDetailPayload {
    pub scene: VisualSceneSnapshotDto,
}

#[derive(Default)]
pub(super) struct MultimodalRuntimeState {
    pub generation: u64,
    pub visual_handle: Option<JoinHandle<()>>,
    pub audio_handles: Vec<JoinHandle<()>>,
}

#[derive(Debug, Clone)]
pub(crate) struct AvFoundationSource {
    pub index: i32,
    pub name: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AvFoundationSources {
    pub video: Vec<AvFoundationSource>,
    pub audio: Vec<AvFoundationSource>,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct VisualSceneSnapshotRow {
    pub visual_scene_id: String,
    pub session_id: String,
    pub timestamp: i64,
    pub source_id: String,
    pub trigger_reason: String,
    pub frame_id: Option<String>,
    pub person_count: i64,
    pub presence_label: String,
    pub presence_confidence: f64,
    pub posture_label: String,
    pub posture_confidence: f64,
    pub motion_label: String,
    pub motion_confidence: f64,
    pub object_labels_json: String,
    pub object_boxes_json: String,
    pub fused_state_json: String,
    pub avg_confidence: f64,
    pub processing_time_ms: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct VideoFrameSampleRow {
    pub frame_id: String,
    pub session_id: String,
    pub timestamp: i64,
    pub source_id: String,
    pub width: i64,
    pub height: i64,
    pub frame_path: Option<String>,
    pub retained_as_evidence: i64,
    pub sampling_reason: String,
    pub motion_score: f64,
    pub scene_delta: f64,
    pub created_at: i64,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct VisionDetectionRow {
    pub detection_id: String,
    pub frame_id: String,
    pub session_id: String,
    pub timestamp: i64,
    pub detector_type: String,
    pub model_name: String,
    pub model_version: String,
    pub raw_json: String,
    pub confidence: f64,
    pub processing_time_ms: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct VisualStateSpanRow {
    pub visual_state_span_id: String,
    pub session_id: String,
    pub source_id: String,
    pub state_type: String,
    pub label: String,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub duration_ms: i64,
    pub scene_ids_json: String,
    pub avg_confidence: f64,
    pub min_confidence: f64,
    pub max_confidence: f64,
    pub transition_in: Option<String>,
    pub transition_out: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct AudioChunkRow {
    pub audio_chunk_id: String,
    pub session_id: String,
    pub source_id: String,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub trigger_reason: String,
    pub audio_path: Option<String>,
    pub retained_as_evidence: i64,
    pub vad_score: f64,
    pub speech_detected: i64,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct AsrSegmentRow {
    pub asr_segment_id: String,
    pub session_id: String,
    pub source_id: String,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub language: Option<String>,
    pub transcript: String,
    pub confidence: Option<f64>,
    pub model_name: String,
    pub model_version: String,
    pub audio_chunk_ids_json: String,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct AudioStateSpanRow {
    pub audio_state_span_id: String,
    pub session_id: String,
    pub source_id: String,
    pub state_type: String,
    pub label: String,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub duration_ms: i64,
    pub supporting_audio_chunk_ids_json: String,
    pub supporting_asr_segment_ids_json: String,
    pub avg_confidence: f64,
}

#[derive(Debug, Clone)]
pub(super) struct VisualSpanSeed {
    pub state_type: String,
    pub label: String,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub scene_ids: Vec<String>,
    pub confidences: Vec<f32>,
    pub transition_in: Option<String>,
    pub transition_out: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct AudioSpanSeed {
    pub label: String,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub supporting_audio_chunk_ids: Vec<String>,
    pub supporting_asr_segment_ids: Vec<String>,
    pub confidences: Vec<f32>,
}

pub(super) struct VisualLoopState {
    pub motion_detector: MotionDetector,
    pub previous_presence: Option<String>,
    pub previous_posture: Option<String>,
    pub last_audit_evidence_at: Option<i64>,
}

impl Default for VisualLoopState {
    fn default() -> Self {
        Self {
            motion_detector: MotionDetector::new(
                crate::core::multimodal::constants::MOTION_MOVING_THRESHOLD,
            ),
            previous_presence: None,
            previous_posture: None,
            last_audit_evidence_at: None,
        }
    }
}
