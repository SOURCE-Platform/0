use super::mappers::{
    asr_segment_row_to_dto, audio_chunk_row_to_dto, audio_state_span_row_to_dto,
    visual_scene_row_to_dto, visual_span_row_to_dto,
};
use super::types::{
    AsrSegmentDto, AsrSegmentRow, AudioChunkDto, AudioChunkRow, AudioStateSpanDto,
    AudioStateSpanRow, MultimodalActivityEpisodeDto, VisualAudioSummaryDto, VisualSceneSnapshotDto,
    VisualSceneSnapshotRow, VisualStateSpanDto, VisualStateSpanRow,
};
use crate::core::database::Database;
use crate::core::multimodal::MultimodalQueryResult;
use crate::core::ocr_agent_context;
use std::collections::HashMap;
use std::sync::Arc;

pub async fn get_visual_scene_snapshot(
    db: &Arc<Database>,
    visual_scene_id: &str,
) -> MultimodalQueryResult<Option<VisualSceneSnapshotDto>> {
    let row = sqlx::query_as::<_, VisualSceneSnapshotRow>(
        "SELECT * FROM visual_scene_snapshots WHERE visual_scene_id = ?",
    )
    .bind(visual_scene_id)
    .fetch_optional(db.pool())
    .await?;

    match row {
        Some(row) => Ok(Some(visual_scene_row_to_dto(db, row).await?)),
        None => Ok(None),
    }
}

pub async fn get_visual_scene_snapshots(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
    source_filter: Option<String>,
) -> MultimodalQueryResult<Vec<VisualSceneSnapshotDto>> {
    let rows = if let Some(source_filter) = source_filter {
        sqlx::query_as::<_, VisualSceneSnapshotRow>(
            "SELECT * FROM visual_scene_snapshots
             WHERE timestamp >= ? AND timestamp <= ? AND source_id = ?
             ORDER BY timestamp ASC",
        )
        .bind(start_timestamp)
        .bind(end_timestamp)
        .bind(source_filter)
        .fetch_all(db.pool())
        .await?
    } else {
        sqlx::query_as::<_, VisualSceneSnapshotRow>(
            "SELECT * FROM visual_scene_snapshots
             WHERE timestamp >= ? AND timestamp <= ?
             ORDER BY timestamp ASC",
        )
        .bind(start_timestamp)
        .bind(end_timestamp)
        .fetch_all(db.pool())
        .await?
    };

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(visual_scene_row_to_dto(db, row).await?);
    }
    Ok(items)
}

pub async fn get_visual_state_spans(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
    state_type: Option<String>,
) -> MultimodalQueryResult<Vec<VisualStateSpanDto>> {
    let rows = if let Some(state_type) = state_type {
        sqlx::query_as::<_, VisualStateSpanRow>(
            "SELECT * FROM visual_state_spans
             WHERE first_seen_at <= ? AND last_seen_at >= ? AND state_type = ?
             ORDER BY first_seen_at ASC",
        )
        .bind(end_timestamp)
        .bind(start_timestamp)
        .bind(state_type)
        .fetch_all(db.pool())
        .await?
    } else {
        sqlx::query_as::<_, VisualStateSpanRow>(
            "SELECT * FROM visual_state_spans
             WHERE first_seen_at <= ? AND last_seen_at >= ?
             ORDER BY first_seen_at ASC",
        )
        .bind(end_timestamp)
        .bind(start_timestamp)
        .fetch_all(db.pool())
        .await?
    };
    Ok(rows.into_iter().map(visual_span_row_to_dto).collect())
}

pub async fn get_audio_chunks(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
    source_filter: Option<String>,
) -> MultimodalQueryResult<Vec<AudioChunkDto>> {
    let rows = if let Some(source_filter) = source_filter {
        sqlx::query_as::<_, AudioChunkRow>(
            "SELECT * FROM audio_chunks
             WHERE start_timestamp <= ? AND end_timestamp >= ? AND source_id = ?
             ORDER BY start_timestamp ASC",
        )
        .bind(end_timestamp)
        .bind(start_timestamp)
        .bind(source_filter)
        .fetch_all(db.pool())
        .await?
    } else {
        sqlx::query_as::<_, AudioChunkRow>(
            "SELECT * FROM audio_chunks
             WHERE start_timestamp <= ? AND end_timestamp >= ?
             ORDER BY start_timestamp ASC",
        )
        .bind(end_timestamp)
        .bind(start_timestamp)
        .fetch_all(db.pool())
        .await?
    };
    Ok(rows.into_iter().map(audio_chunk_row_to_dto).collect())
}

pub async fn get_asr_segments(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
    source_filter: Option<String>,
) -> MultimodalQueryResult<Vec<AsrSegmentDto>> {
    let rows = if let Some(source_filter) = source_filter {
        sqlx::query_as::<_, AsrSegmentRow>(
            "SELECT * FROM asr_segments
             WHERE start_timestamp <= ? AND end_timestamp >= ? AND source_id = ?
             ORDER BY start_timestamp ASC",
        )
        .bind(end_timestamp)
        .bind(start_timestamp)
        .bind(source_filter)
        .fetch_all(db.pool())
        .await?
    } else {
        sqlx::query_as::<_, AsrSegmentRow>(
            "SELECT * FROM asr_segments
             WHERE start_timestamp <= ? AND end_timestamp >= ?
             ORDER BY start_timestamp ASC",
        )
        .bind(end_timestamp)
        .bind(start_timestamp)
        .fetch_all(db.pool())
        .await?
    };
    Ok(rows.into_iter().map(asr_segment_row_to_dto).collect())
}

pub async fn get_audio_state_spans(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> MultimodalQueryResult<Vec<AudioStateSpanDto>> {
    let rows = sqlx::query_as::<_, AudioStateSpanRow>(
        "SELECT * FROM audio_state_spans
         WHERE first_seen_at <= ? AND last_seen_at >= ?
         ORDER BY first_seen_at ASC",
    )
    .bind(end_timestamp)
    .bind(start_timestamp)
    .fetch_all(db.pool())
    .await?;
    Ok(rows.into_iter().map(audio_state_span_row_to_dto).collect())
}

pub async fn get_visual_audio_summary(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> MultimodalQueryResult<VisualAudioSummaryDto> {
    let scenes = get_visual_scene_snapshots(db, start_timestamp, end_timestamp, None).await?;
    let visual_spans = get_visual_state_spans(db, start_timestamp, end_timestamp, None).await?;
    let audio_chunks = get_audio_chunks(db, start_timestamp, end_timestamp, None).await?;
    let audio_spans = get_audio_state_spans(db, start_timestamp, end_timestamp).await?;
    let asr_segments = get_asr_segments(db, start_timestamp, end_timestamp, None).await?;

    Ok(VisualAudioSummaryDto {
        start_timestamp,
        end_timestamp,
        visual_scene_count: scenes.len(),
        visual_span_count: visual_spans.len(),
        audio_chunk_count: audio_chunks.len(),
        audio_span_count: audio_spans.len(),
        asr_segment_count: asr_segments.len(),
        visible_duration_ms: visual_spans
            .iter()
            .filter(|span| span.state_type == "presence" && span.label == "person_visible")
            .map(|span| span.duration_ms)
            .sum(),
        speaking_duration_ms: audio_spans
            .iter()
            .filter(|span| span.label == "speaking" || span.label == "intermittent_speech")
            .map(|span| span.duration_ms)
            .sum(),
        dominant_postures: top_labels(
            visual_spans
                .iter()
                .filter(|span| span.state_type == "posture")
                .map(|span| span.label.clone())
                .collect(),
        ),
        dominant_audio_states: top_labels(
            audio_spans.iter().map(|span| span.label.clone()).collect(),
        ),
    })
}

pub async fn get_multimodal_activity_episode(
    db: &Arc<Database>,
    timestamp: i64,
) -> MultimodalQueryResult<MultimodalActivityEpisodeDto> {
    let visual_scene = get_visual_scene_snapshots(
        db,
        timestamp.saturating_sub(30_000),
        timestamp.saturating_add(30_000),
        None,
    )
    .await?
    .into_iter()
    .min_by_key(|scene| (scene.timestamp - timestamp).abs());

    Ok(MultimodalActivityEpisodeDto {
        timestamp,
        visual_scene,
        visual_spans: get_visual_state_spans(
            db,
            timestamp.saturating_sub(60_000),
            timestamp.saturating_add(60_000),
            None,
        )
        .await?,
        audio_spans: get_audio_state_spans(
            db,
            timestamp.saturating_sub(60_000),
            timestamp.saturating_add(60_000),
        )
        .await?,
        asr_segments: get_asr_segments(
            db,
            timestamp.saturating_sub(60_000),
            timestamp.saturating_add(60_000),
            None,
        )
        .await?,
        ocr_episode: ocr_agent_context::get_activity_episode(db, timestamp)
            .await
            .ok(),
    })
}

pub async fn delete_all_multimodal_derived(db: &Arc<Database>) -> MultimodalQueryResult<()> {
    sqlx::query("DELETE FROM audio_state_spans")
        .execute(db.pool())
        .await?;
    sqlx::query("DELETE FROM asr_segments")
        .execute(db.pool())
        .await?;
    sqlx::query("DELETE FROM audio_chunks")
        .execute(db.pool())
        .await?;
    sqlx::query("DELETE FROM visual_state_spans")
        .execute(db.pool())
        .await?;
    sqlx::query("DELETE FROM visual_scene_snapshots")
        .execute(db.pool())
        .await?;
    sqlx::query("DELETE FROM vision_detections")
        .execute(db.pool())
        .await?;
    sqlx::query("DELETE FROM video_frame_samples")
        .execute(db.pool())
        .await?;
    Ok(())
}

fn top_labels(labels: Vec<String>) -> Vec<String> {
    let mut counts = HashMap::<String, usize>::new();
    for label in labels {
        *counts.entry(label).or_default() += 1;
    }
    let mut items = counts.into_iter().collect::<Vec<_>>();
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    items.into_iter().take(5).map(|(label, _)| label).collect()
}
