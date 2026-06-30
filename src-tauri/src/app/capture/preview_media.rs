use crate::app::state::{AppState, CapturePreviewRowDto};
use crate::core::gaze;
use crate::core::multimodal;

pub(super) async fn preview_visual(
    limit: i64,
    state: &AppState,
) -> Result<Vec<CapturePreviewRowDto>, String> {
    let scenes = multimodal::get_visual_scene_snapshots(&state.db, 0, i64::MAX, None)
        .await
        .map_err(|e| format!("Failed to load vision preview: {}", e))?;
    let attention = gaze::get_attention_snapshots(&state.db, 0, i64::MAX, None, None)
        .await
        .map_err(|e| format!("Failed to load attention preview: {}", e))?;
    let samples = gaze::get_gaze_samples(&state.db, 0, i64::MAX, None, None)
        .await
        .map_err(|e| format!("Failed to load gaze sample preview: {}", e))?;

    let mut rows = scenes
        .into_iter()
        .map(|item| CapturePreviewRowDto {
            timestamp: Some(item.timestamp),
            summary: format!(
                "Scene • {} • {} • {}",
                item.presence_label, item.posture_label, item.motion_label
            ),
            raw_json: super::pretty_json(serde_json::json!({
                "kind": "visual_scene_snapshot",
                "visual_scene_id": item.visual_scene_id,
                "timestamp": item.timestamp,
                "trigger_reason": item.trigger_reason,
                "presence_label": item.presence_label,
                "posture_label": item.posture_label,
                "motion_label": item.motion_label,
                "source_id": item.source_id,
                "frame_path": item.frame_path,
                "detections": item.detections,
            })),
        })
        .collect::<Vec<_>>();

    rows.extend(attention.into_iter().map(|item| CapturePreviewRowDto {
        timestamp: Some(item.timestamp),
        summary: format!(
            "Attention • {:.0}% • {} targets",
            item.confidence * 100.0,
            item.likely_targets.len()
        ),
        raw_json: super::pretty_json(serde_json::json!({
            "kind": "attention_snapshot",
            "attention_snapshot_id": item.attention_snapshot_id,
            "timestamp": item.timestamp,
            "source_id": item.source_id,
            "screen_x": item.screen_x,
            "screen_y": item.screen_y,
            "accuracy_radius_px": item.accuracy_radius_px,
            "confidence": item.confidence,
            "frontmost_app_name": item.frontmost_app_name,
            "window_title": item.window_title,
            "likely_targets": item.likely_targets,
            "resolver_version": item.resolver_version,
        })),
    }));

    rows.extend(samples.into_iter().map(|item| CapturePreviewRowDto {
        timestamp: Some(item.timestamp),
        summary: format!(
            "Gaze sample • ({:.0}, {:.0}) • {:.0}%",
            item.screen_x,
            item.screen_y,
            item.confidence * 100.0
        ),
        raw_json: super::pretty_json(serde_json::json!({
            "kind": "gaze_sample",
            "gaze_sample_id": item.gaze_sample_id,
            "timestamp": item.timestamp,
            "source_id": item.source_id,
            "calibration_id": item.calibration_id,
            "screen_x": item.screen_x,
            "screen_y": item.screen_y,
            "confidence": item.confidence,
            "accuracy_radius_px": item.accuracy_radius_px,
            "head_pose": item.head_pose,
            "face_bbox": item.face_bbox,
            "model_name": item.model_name,
            "model_version": item.model_version,
        })),
    }));

    rows.sort_by_key(|row| row.timestamp.unwrap_or_default());
    rows.reverse();
    rows.truncate(limit as usize);
    Ok(rows)
}

pub(super) async fn preview_audio(
    limit: i64,
    state: &AppState,
) -> Result<Vec<CapturePreviewRowDto>, String> {
    let items = multimodal::get_audio_chunks(&state.db, 0, i64::MAX, None)
        .await
        .map_err(|e| format!("Failed to load audio preview: {}", e))?;

    Ok(items
        .into_iter()
        .rev()
        .take(limit as usize)
        .map(|item| CapturePreviewRowDto {
            timestamp: Some(item.start_timestamp),
            summary: if item.speech_detected {
                format!("Speech chunk • {:.2} VAD", item.vad_score)
            } else {
                format!("Silent chunk • {:.2} VAD", item.vad_score)
            },
            raw_json: super::pretty_json(serde_json::json!({
                "audio_chunk_id": item.audio_chunk_id,
                "start_timestamp": item.start_timestamp,
                "end_timestamp": item.end_timestamp,
                "trigger_reason": item.trigger_reason,
                "vad_score": item.vad_score,
                "speech_detected": item.speech_detected,
                "audio_path": item.audio_path,
                "retained_as_evidence": item.retained_as_evidence,
            })),
        })
        .collect())
}
