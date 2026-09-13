use super::app_context::infer_app_context;
use super::mappings::scene_row_to_dto;
use super::pii::detect_pii_entities;
use super::spans::rebuild_session_spans_and_entities;
use super::{AgentLinkedContextDto, AgentRawSourceDto, AgentSceneSnapshotDto, AgentTextBlockDto};
use crate::core::database::Database;
use crate::core::ocr_storage::ProcessedOcrResult;
use std::sync::Arc;

pub async fn reindex_processed_result(
    db: &Arc<Database>,
    result: &ProcessedOcrResult,
    raw_row_ids: &[String],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let scene = build_scene_snapshot(db, result, raw_row_ids).await?;
    upsert_scene_snapshot(db, &scene).await?;
    rebuild_session_spans_and_entities(db, &result.session_id.to_string()).await
}

pub async fn delete_derived_for_session(
    db: &Arc<Database>,
    session_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    sqlx::query("DELETE FROM ocr_context_entities WHERE session_id = ?")
        .bind(session_id)
        .execute(db.pool())
        .await?;
    sqlx::query("DELETE FROM ocr_text_spans WHERE session_id = ?")
        .bind(session_id)
        .execute(db.pool())
        .await?;
    sqlx::query("DELETE FROM ocr_scene_snapshots WHERE session_id = ?")
        .bind(session_id)
        .execute(db.pool())
        .await?;
    Ok(())
}

pub async fn delete_all_derived(
    db: &Arc<Database>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    sqlx::query("DELETE FROM ocr_context_entities")
        .execute(db.pool())
        .await?;
    sqlx::query("DELETE FROM ocr_text_spans")
        .execute(db.pool())
        .await?;
    sqlx::query("DELETE FROM ocr_scene_snapshots")
        .execute(db.pool())
        .await?;
    Ok(())
}

pub(super) async fn build_scene_snapshot(
    db: &Arc<Database>,
    result: &ProcessedOcrResult,
    raw_row_ids: &[String],
) -> Result<AgentSceneSnapshotDto, Box<dyn std::error::Error + Send + Sync>> {
    let session_id = result.session_id.to_string();
    let scene_id = format!("scene-{}-{}", session_id, result.timestamp);
    let mut context = infer_app_context(db, result.timestamp, Some(&session_id)).await?;
    // Inferred context is sampled every few seconds and lags app switches;
    // the recorder saw the front app at the moment of capture.
    if let Some(app) = &result.frontmost_app {
        if context.app_name.as_deref() != Some(app.name.as_str()) {
            context.window_title = None;
        }
        context.app_name = Some(app.name.clone());
        context.bundle_id = Some(app.bundle_id.clone());
    }
    let frame_path_string = result
        .frame_path
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());

    let text_blocks = result
        .ocr_result
        .text_blocks
        .iter()
        .enumerate()
        .map(|(index, block)| AgentTextBlockDto {
            block_id: raw_row_ids
                .get(index)
                .cloned()
                .unwrap_or_else(|| format!("{}-block-{}", scene_id, index + 1)),
            text: block.text.clone(),
            confidence: block.confidence,
            bbox: block.bounding_box.clone(),
            language: block.language.clone(),
        })
        .collect::<Vec<_>>();

    let avg_confidence = if text_blocks.is_empty() {
        0.0
    } else {
        text_blocks
            .iter()
            .map(|block| block.confidence)
            .sum::<f32>()
            / text_blocks.len() as f32
    };

    let pii_entities = text_blocks
        .iter()
        .flat_map(|block| detect_pii_entities(&block.text, Some(block.bbox.clone())))
        .collect::<Vec<_>>();

    Ok(AgentSceneSnapshotDto {
        scene_id,
        session_id: session_id.clone(),
        timestamp: result.timestamp,
        display_id: result.display_id,
        frontmost_app_name: context.app_name.clone(),
        frontmost_bundle_id: context.bundle_id.clone(),
        window_title: context.window_title.clone(),
        trigger_reason: result.trigger_reason.clone(),
        frame_path: frame_path_string.clone(),
        frame_width: Some(result.frame_width),
        frame_height: Some(result.frame_height),
        full_text: result.ocr_result.total_text.clone(),
        avg_confidence,
        block_count: text_blocks.len(),
        text_blocks,
        pii_entities,
        linked_context: AgentLinkedContextDto {
            focused_app: context.app_name,
            focused_bundle_id: context.bundle_id,
            window_title: context.window_title,
            visible_window_snapshot_id: context.visible_window_snapshot_id,
        },
        raw_source: AgentRawSourceDto {
            raw_row_ids: raw_row_ids.to_vec(),
            frame_path: frame_path_string,
        },
    })
}

pub(super) async fn upsert_scene_snapshot(
    db: &Arc<Database>,
    scene: &AgentSceneSnapshotDto,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        r#"
        INSERT INTO ocr_scene_snapshots (
            scene_id, session_id, timestamp, display_id, frontmost_app_name, frontmost_bundle_id,
            window_title, trigger_reason, frame_path, frame_width, frame_height, full_text,
            avg_confidence, block_count, text_blocks_json, pii_entities_json, linked_context_json,
            raw_source_json, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(scene_id) DO UPDATE SET
            session_id = excluded.session_id,
            timestamp = excluded.timestamp,
            display_id = excluded.display_id,
            frontmost_app_name = excluded.frontmost_app_name,
            frontmost_bundle_id = excluded.frontmost_bundle_id,
            window_title = excluded.window_title,
            trigger_reason = excluded.trigger_reason,
            frame_path = excluded.frame_path,
            frame_width = excluded.frame_width,
            frame_height = excluded.frame_height,
            full_text = excluded.full_text,
            avg_confidence = excluded.avg_confidence,
            block_count = excluded.block_count,
            text_blocks_json = excluded.text_blocks_json,
            pii_entities_json = excluded.pii_entities_json,
            linked_context_json = excluded.linked_context_json,
            raw_source_json = excluded.raw_source_json,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(&scene.scene_id)
    .bind(&scene.session_id)
    .bind(scene.timestamp)
    .bind(scene.display_id.map(|value| value as i64))
    .bind(&scene.frontmost_app_name)
    .bind(&scene.frontmost_bundle_id)
    .bind(&scene.window_title)
    .bind(&scene.trigger_reason)
    .bind(&scene.frame_path)
    .bind(scene.frame_width.map(|value| value as i64))
    .bind(scene.frame_height.map(|value| value as i64))
    .bind(&scene.full_text)
    .bind(scene.avg_confidence as f64)
    .bind(scene.block_count as i64)
    .bind(serde_json::to_string(&scene.text_blocks)?)
    .bind(serde_json::to_string(&scene.pii_entities)?)
    .bind(serde_json::to_string(&scene.linked_context)?)
    .bind(serde_json::to_string(&scene.raw_source)?)
    .bind(now)
    .bind(now)
    .execute(db.pool())
    .await?;
    let _ = scene_row_to_dto;
    Ok(())
}
