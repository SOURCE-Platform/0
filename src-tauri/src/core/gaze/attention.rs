use super::calibration::get_active_gaze_calibration;
use super::indexing::reindex_attention_spans;
use super::model::{ACTIVE_GAZE_MODEL_NAME, ACTIVE_GAZE_MODEL_VERSION};
use super::resolver::predict_gaze_point;
use super::targets::resolve_targets;
use super::types::{
    AttentionSnapshotDto, FaceFeatureSampleDto, GazeSampleDto, ATTENTION_RESOLVER_VERSION,
};
use crate::core::database::Database;
use crate::core::ocr_agent_context::{self, AgentSceneSnapshotDto};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub async fn process_gaze_frame(
    db: &Arc<Database>,
    session_id: &str,
    source_id: &str,
    features: &FaceFeatureSampleDto,
    timestamp: i64,
) -> Result<Option<AttentionSnapshotDto>, String> {
    let Some(calibration) = get_active_gaze_calibration(db).await? else {
        return Ok(None);
    };
    if features.confidence < 0.2 {
        return Ok(None);
    }
    let Some(predicted) = predict_gaze_point(&calibration, &features) else {
        return Ok(None);
    };

    let gaze_sample = persist_gaze_sample(
        db,
        session_id,
        source_id,
        timestamp,
        &calibration.calibration_id,
        &predicted,
        features,
    )
    .await?;
    let snapshot = persist_attention_snapshot(
        db,
        session_id,
        source_id,
        timestamp,
        &gaze_sample,
        calibration.screen_width,
        calibration.screen_height,
    )
    .await?;
    reindex_attention_spans(db, session_id, source_id).await?;
    Ok(Some(snapshot))
}

async fn persist_gaze_sample(
    db: &Arc<Database>,
    session_id: &str,
    source_id: &str,
    timestamp: i64,
    calibration_id: &str,
    predicted: &super::resolver::PredictedGazePoint,
    features: &super::types::FaceFeatureSampleDto,
) -> Result<GazeSampleDto, String> {
    let gaze_sample = GazeSampleDto {
        gaze_sample_id: Uuid::new_v4().to_string(),
        session_id: session_id.to_string(),
        timestamp,
        calibration_id: calibration_id.to_string(),
        source_id: source_id.to_string(),
        screen_x: predicted.screen_x,
        screen_y: predicted.screen_y,
        confidence: predicted.confidence,
        accuracy_radius_px: predicted.accuracy_radius_px,
        head_pose: predicted.head_pose.clone(),
        face_bbox: predicted.face_bbox.clone(),
        gaze_vector: features.gaze_vector.clone(),
        projected_point: features.projected_gaze.clone().or_else(|| {
            Some(json!({
                "screenX": predicted.screen_x,
                "screenY": predicted.screen_y,
            }))
        }),
        landmark_payload: Some(json!({
            "faceLandmarks": features.face_landmarks,
            "leftIrisLandmarks": features.left_iris_landmarks,
            "rightIrisLandmarks": features.right_iris_landmarks,
        })),
        raw_features_ref: Some(json!(features).to_string()),
        model_name: ACTIVE_GAZE_MODEL_NAME.to_string(),
        model_version: ACTIVE_GAZE_MODEL_VERSION.to_string(),
    };

    sqlx::query(
        "INSERT INTO gaze_samples (
            gaze_sample_id, session_id, timestamp, calibration_id, source_id, screen_x, screen_y,
            confidence, accuracy_radius_px, head_pose_json, face_bbox_json, gaze_vector_json,
            projected_point_json, landmark_payload_json, raw_features_ref, model_name,
            model_version, created_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&gaze_sample.gaze_sample_id)
    .bind(&gaze_sample.session_id)
    .bind(gaze_sample.timestamp)
    .bind(&gaze_sample.calibration_id)
    .bind(&gaze_sample.source_id)
    .bind(gaze_sample.screen_x as f64)
    .bind(gaze_sample.screen_y as f64)
    .bind(gaze_sample.confidence as f64)
    .bind(gaze_sample.accuracy_radius_px as f64)
    .bind(
        gaze_sample
            .head_pose
            .as_ref()
            .map(|value| value.to_string()),
    )
    .bind(
        gaze_sample
            .face_bbox
            .as_ref()
            .map(|value| value.to_string()),
    )
    .bind(
        gaze_sample
            .gaze_vector
            .as_ref()
            .map(|value| value.to_string()),
    )
    .bind(
        gaze_sample
            .projected_point
            .as_ref()
            .map(|value| value.to_string()),
    )
    .bind(
        gaze_sample
            .landmark_payload
            .as_ref()
            .map(|value| value.to_string()),
    )
    .bind(&gaze_sample.raw_features_ref)
    .bind(&gaze_sample.model_name)
    .bind(&gaze_sample.model_version)
    .bind(chrono::Utc::now().timestamp_millis())
    .execute(db.pool())
    .await
    .map_err(|e| format!("Failed to persist gaze sample: {e}"))?;

    Ok(gaze_sample)
}

async fn persist_attention_snapshot(
    db: &Arc<Database>,
    session_id: &str,
    source_id: &str,
    timestamp: i64,
    sample: &GazeSampleDto,
    screen_width: i64,
    screen_height: i64,
) -> Result<AttentionSnapshotDto, String> {
    let scene = nearest_scene_snapshot(db, timestamp).await?;
    let text_spans = ocr_agent_context::get_text_spans(
        db,
        timestamp.saturating_sub(90_000),
        timestamp.saturating_add(90_000),
        scene
            .as_ref()
            .and_then(|item| item.frontmost_app_name.clone()),
    )
    .await
    .map_err(|e| format!("Failed to load OCR text spans for attention resolution: {e}"))?;
    let entities = ocr_agent_context::get_context_entities(
        db,
        timestamp.saturating_sub(180_000),
        timestamp.saturating_add(180_000),
        scene
            .as_ref()
            .and_then(|item| item.frontmost_app_name.clone()),
        None,
    )
    .await
    .map_err(|e| format!("Failed to load OCR context entities for attention resolution: {e}"))?;

    let likely_targets = resolve_targets(
        scene.as_ref(),
        &text_spans,
        &entities,
        sample.screen_x,
        sample.screen_y,
        sample.accuracy_radius_px,
        screen_width as f32,
        screen_height as f32,
    );

    let snapshot = AttentionSnapshotDto {
        attention_snapshot_id: Uuid::new_v4().to_string(),
        session_id: session_id.to_string(),
        timestamp,
        gaze_sample_id: sample.gaze_sample_id.clone(),
        source_id: source_id.to_string(),
        screen_x: sample.screen_x,
        screen_y: sample.screen_y,
        accuracy_radius_px: sample.accuracy_radius_px,
        confidence: sample.confidence,
        frontmost_app_name: scene
            .as_ref()
            .and_then(|item| item.frontmost_app_name.clone()),
        window_title: scene.as_ref().and_then(|item| item.window_title.clone()),
        likely_targets,
        resolver_version: ATTENTION_RESOLVER_VERSION.to_string(),
    };

    sqlx::query(
        "INSERT INTO attention_snapshots (
            attention_snapshot_id, session_id, timestamp, gaze_sample_id, source_id, screen_x,
            screen_y, accuracy_radius_px, confidence, frontmost_app_name, window_title,
            likely_targets_json, resolver_version, created_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&snapshot.attention_snapshot_id)
    .bind(&snapshot.session_id)
    .bind(snapshot.timestamp)
    .bind(&snapshot.gaze_sample_id)
    .bind(&snapshot.source_id)
    .bind(snapshot.screen_x as f64)
    .bind(snapshot.screen_y as f64)
    .bind(snapshot.accuracy_radius_px as f64)
    .bind(snapshot.confidence as f64)
    .bind(&snapshot.frontmost_app_name)
    .bind(&snapshot.window_title)
    .bind(json!(snapshot.likely_targets).to_string())
    .bind(&snapshot.resolver_version)
    .bind(chrono::Utc::now().timestamp_millis())
    .execute(db.pool())
    .await
    .map_err(|e| format!("Failed to persist attention snapshot: {e}"))?;

    Ok(snapshot)
}

async fn nearest_scene_snapshot(
    db: &Arc<Database>,
    timestamp: i64,
) -> Result<Option<AgentSceneSnapshotDto>, String> {
    ocr_agent_context::get_scene_snapshots(
        db,
        timestamp.saturating_sub(30_000),
        timestamp.saturating_add(30_000),
        None,
    )
    .await
    .map_err(|e| format!("Failed to load OCR scene snapshots for attention resolution: {e}"))
    .map(|items| {
        items
            .into_iter()
            .min_by_key(|item| (item.timestamp - timestamp).abs())
    })
}
