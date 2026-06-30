use super::types::{
    AsrSegmentDto, AsrSegmentRow, AudioChunkDto, AudioChunkRow, AudioStateSpanDto,
    AudioStateSpanRow, RawDetectorOutputDto, VideoFrameSampleRow, VisionDetectionRow,
    VisualSceneSnapshotDto, VisualSceneSnapshotRow, VisualStateSpanDto, VisualStateSpanRow,
};
use crate::core::database::Database;
use crate::core::multimodal::MultimodalQueryResult;
use serde_json::Value;
use std::sync::Arc;

pub(super) async fn visual_scene_row_to_dto(
    db: &Arc<Database>,
    row: VisualSceneSnapshotRow,
) -> MultimodalQueryResult<VisualSceneSnapshotDto> {
    let frame = if let Some(frame_id) = row.frame_id.as_ref() {
        sqlx::query_as::<_, VideoFrameSampleRow>(
            "SELECT * FROM video_frame_samples WHERE frame_id = ?",
        )
        .bind(frame_id)
        .fetch_optional(db.pool())
        .await?
    } else {
        None
    };
    let detections = if let Some(frame_id) = row.frame_id.as_ref() {
        sqlx::query_as::<_, VisionDetectionRow>(
            "SELECT * FROM vision_detections WHERE frame_id = ? ORDER BY created_at ASC",
        )
        .bind(frame_id)
        .fetch_all(db.pool())
        .await?
    } else {
        Vec::new()
    };

    Ok(VisualSceneSnapshotDto {
        visual_scene_id: row.visual_scene_id,
        session_id: row.session_id,
        timestamp: row.timestamp,
        source_id: row.source_id,
        trigger_reason: row.trigger_reason,
        frame_id: row.frame_id,
        frame_path: frame.as_ref().and_then(|value| value.frame_path.clone()),
        frame_width: frame.as_ref().map(|value| value.width as u32),
        frame_height: frame.as_ref().map(|value| value.height as u32),
        person_count: row.person_count,
        presence_label: row.presence_label,
        presence_confidence: row.presence_confidence as f32,
        posture_label: row.posture_label,
        posture_confidence: row.posture_confidence as f32,
        motion_label: row.motion_label,
        motion_confidence: row.motion_confidence as f32,
        object_labels: serde_json::from_str(&row.object_labels_json).unwrap_or_default(),
        object_boxes: serde_json::from_str(&row.object_boxes_json).unwrap_or_default(),
        fused_state: serde_json::from_str(&row.fused_state_json).unwrap_or(Value::Null),
        avg_confidence: row.avg_confidence as f32,
        processing_time_ms: row.processing_time_ms,
        detections: detections
            .into_iter()
            .map(|detection| RawDetectorOutputDto {
                detector_type: detection.detector_type,
                model_name: detection.model_name,
                model_version: detection.model_version,
                confidence: detection.confidence as f32,
                processing_time_ms: detection.processing_time_ms,
                raw_json: serde_json::from_str(&detection.raw_json).unwrap_or(Value::Null),
            })
            .collect(),
    })
}

pub(super) fn visual_span_row_to_dto(row: VisualStateSpanRow) -> VisualStateSpanDto {
    VisualStateSpanDto {
        visual_state_span_id: row.visual_state_span_id,
        session_id: row.session_id,
        source_id: row.source_id,
        state_type: row.state_type,
        label: row.label,
        first_seen_at: row.first_seen_at,
        last_seen_at: row.last_seen_at,
        duration_ms: row.duration_ms,
        scene_ids: serde_json::from_str(&row.scene_ids_json).unwrap_or_default(),
        avg_confidence: row.avg_confidence as f32,
        min_confidence: row.min_confidence as f32,
        max_confidence: row.max_confidence as f32,
        transition_in: row.transition_in,
        transition_out: row.transition_out,
    }
}

pub(super) fn audio_chunk_row_to_dto(row: AudioChunkRow) -> AudioChunkDto {
    AudioChunkDto {
        audio_chunk_id: row.audio_chunk_id,
        session_id: row.session_id,
        source_id: row.source_id,
        start_timestamp: row.start_timestamp,
        end_timestamp: row.end_timestamp,
        trigger_reason: row.trigger_reason,
        audio_path: row.audio_path,
        retained_as_evidence: row.retained_as_evidence != 0,
        vad_score: row.vad_score as f32,
        speech_detected: row.speech_detected != 0,
    }
}

pub(super) fn asr_segment_row_to_dto(row: AsrSegmentRow) -> AsrSegmentDto {
    AsrSegmentDto {
        asr_segment_id: row.asr_segment_id,
        session_id: row.session_id,
        source_id: row.source_id,
        start_timestamp: row.start_timestamp,
        end_timestamp: row.end_timestamp,
        language: row.language,
        transcript: row.transcript,
        confidence: row.confidence.map(|value| value as f32),
        model_name: row.model_name,
        model_version: row.model_version,
        audio_chunk_ids: serde_json::from_str(&row.audio_chunk_ids_json).unwrap_or_default(),
    }
}

pub(super) fn audio_state_span_row_to_dto(row: AudioStateSpanRow) -> AudioStateSpanDto {
    AudioStateSpanDto {
        audio_state_span_id: row.audio_state_span_id,
        session_id: row.session_id,
        source_id: row.source_id,
        state_type: row.state_type,
        label: row.label,
        first_seen_at: row.first_seen_at,
        last_seen_at: row.last_seen_at,
        duration_ms: row.duration_ms,
        supporting_audio_chunk_ids: serde_json::from_str(&row.supporting_audio_chunk_ids_json)
            .unwrap_or_default(),
        supporting_asr_segment_ids: serde_json::from_str(&row.supporting_asr_segment_ids_json)
            .unwrap_or_default(),
        avg_confidence: row.avg_confidence as f32,
    }
}
