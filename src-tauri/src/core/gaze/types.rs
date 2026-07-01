use crate::core::ocr_agent_context::{
    AgentContextEntityDto, AgentSceneSnapshotDto, AgentTextSpanDto,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
pub(crate) const ATTENTION_RESOLVER_VERSION: &str = "v0";
pub(crate) const ATTENTION_MIN_DWELL_MS: i64 = 500;
pub(crate) const ATTENTION_SPAN_MAX_GAP_MS: i64 = 5_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceFeatureSampleDto {
    pub face_bbox: Option<Value>,
    pub face_center_x: f32,
    pub face_center_y: f32,
    pub face_width: f32,
    pub face_height: f32,
    pub left_eye_x: Option<f32>,
    pub left_eye_y: Option<f32>,
    pub right_eye_x: Option<f32>,
    pub right_eye_y: Option<f32>,
    pub eye_mid_x: Option<f32>,
    pub eye_mid_y: Option<f32>,
    pub inter_eye_distance: Option<f32>,
    pub yaw: Option<f32>,
    pub pitch: Option<f32>,
    pub roll: Option<f32>,
    #[serde(default)]
    pub face_landmarks: Vec<Value>,
    #[serde(default)]
    pub left_iris_landmarks: Vec<Value>,
    #[serde(default)]
    pub right_iris_landmarks: Vec<Value>,
    #[serde(default)]
    pub head_pose: Option<Value>,
    #[serde(default)]
    pub gaze_vector: Option<Value>,
    #[serde(default)]
    pub projected_gaze: Option<Value>,
    #[serde(default)]
    pub gaze_yaw_degrees: Option<f32>,
    #[serde(default)]
    pub gaze_pitch_degrees: Option<f32>,
    #[serde(default)]
    pub gaze_model_name: Option<String>,
    #[serde(default)]
    pub gaze_model_version: Option<String>,
    pub confidence: f32,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GazeCalibrationPointDto {
    pub point_id: String,
    pub phase: String,
    pub target_x: f32,
    pub target_y: f32,
    pub timestamp: i64,
    pub features: FaceFeatureSampleDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GazeCalibrationDto {
    pub calibration_id: String,
    pub session_id: Option<String>,
    pub created_at: i64,
    pub display_id: Option<u32>,
    pub display_name: Option<String>,
    pub display_x: i32,
    pub display_y: i32,
    pub screen_width: i64,
    pub screen_height: i64,
    pub camera_id: String,
    pub model_name: String,
    pub model_version: String,
    pub calibration_points: Vec<GazeCalibrationPointDto>,
    pub validation_error_px: Option<f32>,
    pub validation_quality: Option<String>,
    pub head_pose_range: Option<Value>,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GazeSampleDto {
    pub gaze_sample_id: String,
    pub session_id: String,
    pub timestamp: i64,
    pub calibration_id: String,
    pub source_id: String,
    pub screen_x: f32,
    pub screen_y: f32,
    pub confidence: f32,
    pub accuracy_radius_px: f32,
    pub head_pose: Option<Value>,
    pub face_bbox: Option<Value>,
    pub gaze_vector: Option<Value>,
    pub projected_point: Option<Value>,
    pub landmark_payload: Option<Value>,
    pub raw_features_ref: Option<String>,
    pub model_name: String,
    pub model_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttentionTargetDto {
    pub target_type: String,
    pub target_id: String,
    pub label: String,
    pub probability: f32,
    pub distance_px: f32,
    pub overlap_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttentionSnapshotDto {
    pub attention_snapshot_id: String,
    pub session_id: String,
    pub timestamp: i64,
    pub gaze_sample_id: String,
    pub source_id: String,
    pub screen_x: f32,
    pub screen_y: f32,
    pub accuracy_radius_px: f32,
    pub confidence: f32,
    pub frontmost_app_name: Option<String>,
    pub window_title: Option<String>,
    pub likely_targets: Vec<AttentionTargetDto>,
    pub resolver_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttentionSpanDto {
    pub attention_span_id: String,
    pub session_id: String,
    pub source_id: String,
    pub target_type: String,
    pub target_id: String,
    pub label: String,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub duration_ms: i64,
    pub supporting_attention_snapshot_ids: Vec<String>,
    pub avg_confidence: f32,
    pub max_confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttentionSearchResultDto {
    pub target_type: String,
    pub target_id: String,
    pub label: String,
    pub timestamp: i64,
    pub duration_ms: i64,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttentionSummaryDto {
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub calibration_quality: Option<String>,
    pub gaze_sample_count: usize,
    pub attention_snapshot_count: usize,
    pub attention_span_count: usize,
    pub top_targets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttentionAtTimestampDto {
    pub timestamp: i64,
    pub sample: Option<GazeSampleDto>,
    pub snapshot: Option<AttentionSnapshotDto>,
    pub scene: Option<AgentSceneSnapshotDto>,
    pub text_spans: Vec<AgentTextSpanDto>,
    pub context_entity: Option<AgentContextEntityDto>,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct GazeCalibrationRow {
    pub calibration_id: String,
    pub session_id: Option<String>,
    pub created_at: i64,
    pub display_id: Option<i64>,
    pub display_name: Option<String>,
    pub display_x: i64,
    pub display_y: i64,
    pub screen_width: i64,
    pub screen_height: i64,
    pub camera_id: String,
    pub model_name: String,
    pub model_version: String,
    pub calibration_points_json: String,
    pub validation_error_px: Option<f64>,
    pub validation_quality: Option<String>,
    pub head_pose_range_json: Option<String>,
    pub active: i64,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct GazeSampleRow {
    pub gaze_sample_id: String,
    pub session_id: String,
    pub timestamp: i64,
    pub calibration_id: String,
    pub source_id: String,
    pub screen_x: f64,
    pub screen_y: f64,
    pub confidence: f64,
    pub accuracy_radius_px: f64,
    pub head_pose_json: Option<String>,
    pub face_bbox_json: Option<String>,
    pub gaze_vector_json: Option<String>,
    pub projected_point_json: Option<String>,
    pub landmark_payload_json: Option<String>,
    pub raw_features_ref: Option<String>,
    pub model_name: String,
    pub model_version: String,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct AttentionSnapshotRow {
    pub attention_snapshot_id: String,
    pub session_id: String,
    pub timestamp: i64,
    pub gaze_sample_id: String,
    pub source_id: String,
    pub screen_x: f64,
    pub screen_y: f64,
    pub accuracy_radius_px: f64,
    pub confidence: f64,
    pub frontmost_app_name: Option<String>,
    pub window_title: Option<String>,
    pub likely_targets_json: String,
    pub resolver_version: String,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct AttentionSpanRow {
    pub attention_span_id: String,
    pub session_id: String,
    pub source_id: String,
    pub target_type: String,
    pub target_id: String,
    pub label: String,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub duration_ms: i64,
    pub supporting_attention_snapshot_ids_json: String,
    pub avg_confidence: f64,
    pub max_confidence: f64,
}
