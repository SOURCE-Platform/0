use super::{
    AgentContextEntityDto, AgentSceneSnapshotDto, AgentTextSpanDto, ContextEntityRow,
    SceneSnapshotRow, TextSpanRow,
};

pub(super) fn scene_row_to_dto(
    row: SceneSnapshotRow,
) -> Result<AgentSceneSnapshotDto, Box<dyn std::error::Error + Send + Sync>> {
    Ok(AgentSceneSnapshotDto {
        scene_id: row.scene_id,
        session_id: row.session_id,
        timestamp: row.timestamp,
        display_id: row.display_id.map(|value| value as u32),
        frontmost_app_name: row.frontmost_app_name,
        frontmost_bundle_id: row.frontmost_bundle_id,
        window_title: row.window_title,
        trigger_reason: row.trigger_reason,
        frame_path: row.frame_path,
        frame_width: row.frame_width.map(|value| value as u32),
        frame_height: row.frame_height.map(|value| value as u32),
        full_text: row.full_text,
        avg_confidence: row.avg_confidence as f32,
        block_count: row.block_count as usize,
        text_blocks: serde_json::from_str(&row.text_blocks_json)?,
        pii_entities: serde_json::from_str(&row.pii_entities_json)?,
        linked_context: serde_json::from_str(&row.linked_context_json)?,
        raw_source: serde_json::from_str(&row.raw_source_json)?,
    })
}

pub(super) fn span_row_to_dto(
    row: TextSpanRow,
) -> Result<AgentTextSpanDto, Box<dyn std::error::Error + Send + Sync>> {
    Ok(AgentTextSpanDto {
        text_span_id: row.text_span_id,
        session_id: row.session_id,
        canonical_text: row.canonical_text,
        normalized_text: row.normalized_text,
        first_seen_at: row.first_seen_at,
        last_seen_at: row.last_seen_at,
        duration_ms: row.duration_ms,
        scene_ids: serde_json::from_str(&row.scene_ids_json)?,
        frontmost_app_name: row.frontmost_app_name,
        frontmost_bundle_id: row.frontmost_bundle_id,
        window_title: row.window_title,
        avg_confidence: row.avg_confidence as f32,
        bbox_union: serde_json::from_str(&row.bbox_union_json)?,
        occurrence_count: row.occurrence_count as usize,
        was_partial_match: row.was_partial_match != 0,
        pii_entities: serde_json::from_str(&row.pii_entities_json)?,
        raw_source: serde_json::from_str(&row.raw_source_json)?,
    })
}

pub(super) fn entity_row_to_dto(
    row: ContextEntityRow,
) -> Result<AgentContextEntityDto, Box<dyn std::error::Error + Send + Sync>> {
    Ok(AgentContextEntityDto {
        entity_id: row.entity_id,
        session_id: row.session_id,
        entity_type: row.entity_type,
        frontmost_app_name: row.frontmost_app_name,
        frontmost_bundle_id: row.frontmost_bundle_id,
        window_title: row.window_title,
        first_seen_at: row.first_seen_at,
        last_seen_at: row.last_seen_at,
        duration_ms: row.duration_ms,
        scene_ids: serde_json::from_str(&row.scene_ids_json)?,
        text_span_ids: serde_json::from_str(&row.text_span_ids_json)?,
        title_hint: row.title_hint,
        summary_text: row.summary_text,
        dominant_terms: serde_json::from_str(&row.dominant_terms_json)?,
        pii_entity_counts: serde_json::from_str(&row.pii_entity_counts_json)?,
        raw_source: serde_json::from_str(&row.raw_source_json)?,
        confidence: row.confidence as f32,
    })
}
