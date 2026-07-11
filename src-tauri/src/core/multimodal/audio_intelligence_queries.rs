use super::audio_intelligence_types::{
    SoundEventDetectionRow, SoundEventSpanRow, SpeechEmotionSegmentRow,
};
use super::mappers::{
    sound_event_detection_row_to_dto, sound_event_span_row_to_dto,
    speech_emotion_segment_row_to_dto,
};
use crate::core::database::Database;
use crate::core::multimodal::{
    MultimodalQueryResult, SoundEventDetectionDto, SoundEventSpanDto, SpeechEmotionSegmentDto,
};
use std::sync::Arc;

pub async fn get_speech_emotion_segments(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
    source_filter: Option<String>,
) -> MultimodalQueryResult<Vec<SpeechEmotionSegmentDto>> {
    let rows = if let Some(source_filter) = source_filter {
        sqlx::query_as::<_, SpeechEmotionSegmentRow>(
            "SELECT * FROM speech_emotion_segments
             WHERE start_timestamp <= ? AND end_timestamp >= ? AND source_id = ?
             ORDER BY start_timestamp ASC",
        )
        .bind(end_timestamp)
        .bind(start_timestamp)
        .bind(source_filter)
        .fetch_all(db.pool())
        .await?
    } else {
        sqlx::query_as::<_, SpeechEmotionSegmentRow>(
            "SELECT * FROM speech_emotion_segments
             WHERE start_timestamp <= ? AND end_timestamp >= ?
             ORDER BY start_timestamp ASC",
        )
        .bind(end_timestamp)
        .bind(start_timestamp)
        .fetch_all(db.pool())
        .await?
    };
    Ok(rows
        .into_iter()
        .map(speech_emotion_segment_row_to_dto)
        .collect())
}

pub async fn get_sound_event_detections(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
    source_filter: Option<String>,
) -> MultimodalQueryResult<Vec<SoundEventDetectionDto>> {
    let rows = if let Some(source_filter) = source_filter {
        sqlx::query_as::<_, SoundEventDetectionRow>(
            "SELECT * FROM sound_event_detections
             WHERE start_timestamp <= ? AND end_timestamp >= ? AND source_id = ?
             ORDER BY start_timestamp ASC",
        )
        .bind(end_timestamp)
        .bind(start_timestamp)
        .bind(source_filter)
        .fetch_all(db.pool())
        .await?
    } else {
        sqlx::query_as::<_, SoundEventDetectionRow>(
            "SELECT * FROM sound_event_detections
             WHERE start_timestamp <= ? AND end_timestamp >= ?
             ORDER BY start_timestamp ASC",
        )
        .bind(end_timestamp)
        .bind(start_timestamp)
        .fetch_all(db.pool())
        .await?
    };
    Ok(rows
        .into_iter()
        .map(sound_event_detection_row_to_dto)
        .collect())
}

pub async fn get_sound_event_spans(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
    label_filter: Option<String>,
) -> MultimodalQueryResult<Vec<SoundEventSpanDto>> {
    let rows = if let Some(label_filter) = label_filter {
        sqlx::query_as::<_, SoundEventSpanRow>(
            "SELECT * FROM sound_event_spans
             WHERE first_seen_at <= ? AND last_seen_at >= ? AND canonical_label = ?
             ORDER BY first_seen_at ASC",
        )
        .bind(end_timestamp)
        .bind(start_timestamp)
        .bind(label_filter)
        .fetch_all(db.pool())
        .await?
    } else {
        sqlx::query_as::<_, SoundEventSpanRow>(
            "SELECT * FROM sound_event_spans
             WHERE first_seen_at <= ? AND last_seen_at >= ?
             ORDER BY first_seen_at ASC",
        )
        .bind(end_timestamp)
        .bind(start_timestamp)
        .fetch_all(db.pool())
        .await?
    };
    Ok(rows.into_iter().map(sound_event_span_row_to_dto).collect())
}
