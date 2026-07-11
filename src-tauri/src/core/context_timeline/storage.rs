fn pretty_json(value: serde_json::Value) -> String {
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
}

fn serialized_len(value: &serde_json::Value) -> u64 {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len() as u64)
        .unwrap_or(0)
}

fn context_event_storage_bytes(row: &ContextEventRow) -> u64 {
    serialized_len(&serde_json::json!({
        "id": row.id,
        "session_id": row.session_id,
        "timestamp": row.timestamp,
        "channel": row.channel,
        "event_type": row.event_type,
        "source": row.source,
        "confidence": row.confidence,
        "payload_json": row.payload_json,
    }))
}

fn session_storage_bytes(row: &SessionRow) -> u64 {
    serialized_len(&serde_json::json!({
        "id": row.id,
        "start_timestamp": row.start_timestamp,
        "end_timestamp": row.end_timestamp,
    }))
}

fn window_snapshot_storage_bytes(row: &WindowSnapshotRow) -> u64 {
    serialized_len(&serde_json::json!({
        "id": row.id,
        "session_id": row.session_id,
        "timestamp": row.timestamp,
        "frontmost_app_name": row.frontmost_app_name,
        "frontmost_bundle_id": row.frontmost_bundle_id,
        "visible_windows_json": row.visible_windows_json,
        "confidence": row.confidence,
        "source": row.source,
    }))
}

fn keyboard_event_storage_bytes(row: &KeyboardEventSummaryRow) -> u64 {
    serialized_len(&serde_json::json!({
        "timestamp": row.timestamp,
        "app_name": row.app_name,
        "window_title": row.window_title,
    }))
}

fn mouse_event_storage_bytes(row: &MouseEventSummaryRow) -> u64 {
    serialized_len(&serde_json::json!({
        "timestamp": row.timestamp,
        "app_name": row.app_name,
        "window_title": row.window_title,
    }))
}

fn ocr_row_storage_bytes(row: &OcrRow) -> u64 {
    serialized_len(&serde_json::json!({
        "id": row.id,
        "session_id": row.session_id,
        "timestamp": row.timestamp,
        "frame_path": row.frame_path,
        "text": row.text,
        "confidence": row.confidence,
        "bounding_box": row.bounding_box,
    }))
}

fn scene_snapshot_storage_bytes(scene: &AgentSceneSnapshotDto) -> u64 {
    serialized_len(&serde_json::json!({
        "scene_id": scene.scene_id,
        "session_id": scene.session_id,
        "timestamp": scene.timestamp,
        "display_id": scene.display_id,
        "frontmost_app_name": scene.frontmost_app_name,
        "frontmost_bundle_id": scene.frontmost_bundle_id,
        "window_title": scene.window_title,
        "trigger_reason": scene.trigger_reason,
        "frame_path": scene.frame_path,
        "frame_width": scene.frame_width,
        "frame_height": scene.frame_height,
        "full_text": scene.full_text,
        "avg_confidence": scene.avg_confidence,
        "text_blocks": scene.text_blocks,
        "pii_entities": scene.pii_entities,
        "raw_source": scene.raw_source,
    }))
}

fn file_size(path: &str) -> u64 {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

fn get_frame_dimensions(path: &str) -> Option<(u32, u32)> {
    image::open(path).ok().map(|image| image.dimensions())
}


fn group_ocr_rows(ocr_rows: &[OcrRow]) -> Vec<OcrEventGroup> {
    let mut groups: Vec<OcrEventGroup> = Vec::new();

    for row in ocr_rows {
        let frame_key = row.frame_path.clone().unwrap_or_default();
        if let Some(group) = groups.iter_mut().find(|group| {
            group.session_id == row.session_id
                && group.timestamp == row.timestamp
                && group.frame_path.clone().unwrap_or_default() == frame_key
        }) {
            group.blocks.push(row.clone());
            continue;
        }

        groups.push(OcrEventGroup {
            id: format!("ocr-event-{}-{}", row.session_id, row.timestamp),
            session_id: row.session_id.clone(),
            timestamp: row.timestamp,
            frame_path: row.frame_path.clone(),
            blocks: vec![row.clone()],
        });
    }

    groups
}

fn format_duration_short(ms: i64) -> String {
    let seconds = (ms.max(0) as f32 / 1000.0).round() as i64;
    if seconds >= 60 {
        format!("{}m", seconds / 60)
    } else {
        format!("{}s", seconds)
    }
}

fn estimate_visual_scene_storage_bytes(scene: &VisualSceneSnapshotDto) -> u64 {
    serialized_len(&serde_json::json!({
        "visual_scene_id": scene.visual_scene_id,
        "session_id": scene.session_id,
        "timestamp": scene.timestamp,
        "source_id": scene.source_id,
        "trigger_reason": scene.trigger_reason,
        "frame_id": scene.frame_id,
        "frame_path": scene.frame_path,
        "frame_width": scene.frame_width,
        "frame_height": scene.frame_height,
        "person_count": scene.person_count,
        "presence_label": scene.presence_label,
        "presence_confidence": scene.presence_confidence,
        "posture_label": scene.posture_label,
        "posture_confidence": scene.posture_confidence,
        "motion_label": scene.motion_label,
        "motion_confidence": scene.motion_confidence,
        "object_labels": scene.object_labels,
        "object_boxes": scene.object_boxes,
        "avg_confidence": scene.avg_confidence,
        "processing_time_ms": scene.processing_time_ms,
        "detections": scene.detections,
    })) + scene.frame_path.as_deref().map(file_size).unwrap_or(0)
}

fn estimate_visual_span_storage_bytes(span: &VisualStateSpanDto) -> u64 {
    serialized_len(&serde_json::json!({
        "visual_state_span_id": span.visual_state_span_id,
        "session_id": span.session_id,
        "source_id": span.source_id,
        "state_type": span.state_type,
        "label": span.label,
        "first_seen_at": span.first_seen_at,
        "last_seen_at": span.last_seen_at,
        "duration_ms": span.duration_ms,
        "scene_ids": span.scene_ids,
        "avg_confidence": span.avg_confidence,
        "min_confidence": span.min_confidence,
        "max_confidence": span.max_confidence,
        "transition_in": span.transition_in,
        "transition_out": span.transition_out,
    }))
}

fn estimate_audio_span_storage_bytes(span: &AudioStateSpanDto) -> u64 {
    serialized_len(&serde_json::json!({
        "audio_state_span_id": span.audio_state_span_id,
        "session_id": span.session_id,
        "source_id": span.source_id,
        "state_type": span.state_type,
        "label": span.label,
        "first_seen_at": span.first_seen_at,
        "last_seen_at": span.last_seen_at,
        "duration_ms": span.duration_ms,
        "supporting_audio_chunk_ids": span.supporting_audio_chunk_ids,
        "supporting_asr_segment_ids": span.supporting_asr_segment_ids,
        "avg_confidence": span.avg_confidence,
    }))
}

fn estimate_asr_segment_storage_bytes(segment: &AsrSegmentDto) -> u64 {
    serialized_len(&serde_json::json!({
        "asr_segment_id": segment.asr_segment_id,
        "session_id": segment.session_id,
        "source_id": segment.source_id,
        "start_timestamp": segment.start_timestamp,
        "end_timestamp": segment.end_timestamp,
        "language": segment.language,
        "transcript": segment.transcript,
        "confidence": segment.confidence,
        "model_name": segment.model_name,
        "model_version": segment.model_version,
        "audio_chunk_ids": segment.audio_chunk_ids,
    }))
}

fn estimate_speech_emotion_storage_bytes(segment: &SpeechEmotionSegmentDto) -> u64 {
    serialized_len(&serde_json::json!({
        "speech_emotion_segment_id": segment.speech_emotion_segment_id,
        "session_id": segment.session_id,
        "source_id": segment.source_id,
        "audio_chunk_id": segment.audio_chunk_id,
        "asr_segment_id": segment.asr_segment_id,
        "start_timestamp": segment.start_timestamp,
        "end_timestamp": segment.end_timestamp,
        "trigger_reason": segment.trigger_reason,
        "emotion_label": segment.emotion_label,
        "canonical_label": segment.canonical_label,
        "confidence": segment.confidence,
        "model_name": segment.model_name,
        "model_version": segment.model_version,
        "raw_json": segment.raw_json,
    }))
}

fn estimate_sound_event_detection_storage_bytes(detection: &SoundEventDetectionDto) -> u64 {
    serialized_len(&serde_json::json!({
        "sound_event_detection_id": detection.sound_event_detection_id,
        "session_id": detection.session_id,
        "source_id": detection.source_id,
        "audio_chunk_id": detection.audio_chunk_id,
        "start_timestamp": detection.start_timestamp,
        "end_timestamp": detection.end_timestamp,
        "trigger_reason": detection.trigger_reason,
        "event_label": detection.event_label,
        "canonical_label": detection.canonical_label,
        "confidence": detection.confidence,
        "model_name": detection.model_name,
        "model_version": detection.model_version,
        "raw_json": detection.raw_json,
    }))
}

fn estimate_sound_event_span_storage_bytes(span: &SoundEventSpanDto) -> u64 {
    serialized_len(&serde_json::json!({
        "sound_event_span_id": span.sound_event_span_id,
        "session_id": span.session_id,
        "source_id": span.source_id,
        "canonical_label": span.canonical_label,
        "first_seen_at": span.first_seen_at,
        "last_seen_at": span.last_seen_at,
        "duration_ms": span.duration_ms,
        "supporting_detection_ids": span.supporting_detection_ids,
        "supporting_audio_chunk_ids": span.supporting_audio_chunk_ids,
        "avg_confidence": span.avg_confidence,
        "max_confidence": span.max_confidence,
        "model_name": span.model_name,
        "model_version": span.model_version,
    }))
}

fn parse_visible_windows(
    row: &WindowSnapshotRow,
) -> Result<Vec<WindowSnapshotDto>, Box<dyn std::error::Error + Send + Sync>> {
    let windows = serde_json::from_str::<Vec<WindowSnapshotDto>>(&row.visible_windows_json)?;
    Ok(windows)
}
