use super::face_capture::{capture_face_features_from_camera, choose_default_camera};
use super::model::{ACTIVE_GAZE_MODEL_NAME, ACTIVE_GAZE_MODEL_VERSION};
use super::resolver::{
    build_head_pose_range, quality_bucket_for_error, validate_calibration_points,
};
use super::types::{
    FaceFeatureSampleDto, GazeCalibrationDto, GazeCalibrationPointDto, GazeCalibrationRow,
};
use crate::core::database::Database;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

pub async fn start_gaze_calibration(
    db: &Arc<Database>,
    session_id: Option<String>,
    screen_width: i64,
    screen_height: i64,
) -> Result<GazeCalibrationDto, String> {
    let (camera_id, _) = choose_default_camera().await?;
    let calibration = GazeCalibrationDto {
        calibration_id: Uuid::new_v4().to_string(),
        session_id,
        created_at: chrono::Utc::now().timestamp_millis(),
        screen_width,
        screen_height,
        camera_id,
        model_name: ACTIVE_GAZE_MODEL_NAME.to_string(),
        model_version: ACTIVE_GAZE_MODEL_VERSION.to_string(),
        calibration_points: Vec::new(),
        validation_error_px: None,
        validation_quality: None,
        head_pose_range: None,
        active: false,
    };

    sqlx::query(
        "INSERT INTO gaze_calibrations (
            calibration_id, session_id, created_at, screen_width, screen_height, camera_id,
            model_name, model_version, calibration_points_json, validation_error_px,
            validation_quality, head_pose_range_json, active
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0)",
    )
    .bind(&calibration.calibration_id)
    .bind(&calibration.session_id)
    .bind(calibration.created_at)
    .bind(calibration.screen_width)
    .bind(calibration.screen_height)
    .bind(&calibration.camera_id)
    .bind(&calibration.model_name)
    .bind(&calibration.model_version)
    .bind("[]")
    .bind(Option::<f64>::None)
    .bind(Option::<String>::None)
    .bind(Option::<String>::None)
    .execute(db.pool())
    .await
    .map_err(|e| format!("Failed to create gaze calibration: {e}"))?;

    Ok(calibration)
}

pub async fn capture_gaze_calibration_sample(
    db: &Arc<Database>,
    calibration_id: &str,
    phase: String,
    target_x: f32,
    target_y: f32,
) -> Result<GazeCalibrationDto, String> {
    let calibration = load_calibration(db, calibration_id)
        .await?
        .ok_or_else(|| "Calibration not found.".to_string())?;
    let video_index = parse_camera_index(&calibration.camera_id)?;
    let features = capture_face_features_from_camera(video_index).await?;
    if features.confidence < 0.2 {
        return Err("Face or eye landmarks are too weak for calibration sampling.".to_string());
    }

    let mut points = calibration.calibration_points;
    points.push(GazeCalibrationPointDto {
        point_id: Uuid::new_v4().to_string(),
        phase,
        target_x,
        target_y,
        timestamp: chrono::Utc::now().timestamp_millis(),
        features,
    });
    persist_calibration_points(db, calibration_id, &points).await?;
    load_calibration(db, calibration_id)
        .await?
        .ok_or_else(|| "Calibration disappeared after update.".to_string())
}

pub async fn finalize_gaze_calibration(
    db: &Arc<Database>,
    calibration_id: &str,
) -> Result<GazeCalibrationDto, String> {
    let calibration = load_calibration(db, calibration_id)
        .await?
        .ok_or_else(|| "Calibration not found.".to_string())?;
    if calibration.calibration_points.len() < 5 {
        return Err("At least 5 calibration points are required before finishing.".to_string());
    }

    let validation_error_px = validate_calibration_points(&calibration);
    let validation_quality = quality_bucket_for_error(validation_error_px).to_string();
    let head_pose_range = build_head_pose_range(&calibration.calibration_points);
    let should_activate = validation_quality != "failed";

    sqlx::query("UPDATE gaze_calibrations SET active = 0")
        .execute(db.pool())
        .await
        .map_err(|e| format!("Failed to clear active gaze calibration: {e}"))?;
    sqlx::query(
        "UPDATE gaze_calibrations
         SET validation_error_px = ?, validation_quality = ?, head_pose_range_json = ?, active = ?
         WHERE calibration_id = ?",
    )
    .bind(validation_error_px as f64)
    .bind(validation_quality.clone())
    .bind(head_pose_range.as_ref().map(Value::to_string))
    .bind(if should_activate { 1 } else { 0 })
    .bind(calibration_id)
    .execute(db.pool())
    .await
    .map_err(|e| format!("Failed to finalize gaze calibration: {e}"))?;

    load_calibration(db, calibration_id)
        .await?
        .ok_or_else(|| "Calibration missing after finalize.".to_string())
}

pub async fn get_active_gaze_calibration(
    db: &Arc<Database>,
) -> Result<Option<GazeCalibrationDto>, String> {
    let row = sqlx::query_as::<_, GazeCalibrationRow>(
        "SELECT * FROM gaze_calibrations WHERE active = 1 ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_optional(db.pool())
    .await
    .map_err(|e| format!("Failed to load active gaze calibration: {e}"))?;
    row.map(row_to_dto).transpose()
}

pub(crate) async fn load_calibration(
    db: &Arc<Database>,
    calibration_id: &str,
) -> Result<Option<GazeCalibrationDto>, String> {
    let row = sqlx::query_as::<_, GazeCalibrationRow>(
        "SELECT * FROM gaze_calibrations WHERE calibration_id = ?",
    )
    .bind(calibration_id)
    .fetch_optional(db.pool())
    .await
    .map_err(|e| format!("Failed to load gaze calibration: {e}"))?;
    row.map(row_to_dto).transpose()
}

fn parse_camera_index(camera_id: &str) -> Result<i32, String> {
    camera_id
        .strip_prefix("camera:")
        .ok_or_else(|| "Unknown camera identifier.".to_string())?
        .parse::<i32>()
        .map_err(|e| format!("Failed to parse camera identifier: {e}"))
}

async fn persist_calibration_points(
    db: &Arc<Database>,
    calibration_id: &str,
    points: &[GazeCalibrationPointDto],
) -> Result<(), String> {
    sqlx::query(
        "UPDATE gaze_calibrations SET calibration_points_json = ? WHERE calibration_id = ?",
    )
    .bind(json!(points).to_string())
    .bind(calibration_id)
    .execute(db.pool())
    .await
    .map_err(|e| format!("Failed to update gaze calibration points: {e}"))?;
    Ok(())
}

fn row_to_dto(row: GazeCalibrationRow) -> Result<GazeCalibrationDto, String> {
    let calibration_points =
        serde_json::from_str::<Vec<GazeCalibrationPointDto>>(&row.calibration_points_json)
            .map_err(|e| format!("Failed to parse gaze calibration points: {e}"))?;
    let head_pose_range = row
        .head_pose_range_json
        .as_deref()
        .map(serde_json::from_str::<Value>)
        .transpose()
        .map_err(|e| format!("Failed to parse gaze head-pose range: {e}"))?;

    Ok(GazeCalibrationDto {
        calibration_id: row.calibration_id,
        session_id: row.session_id,
        created_at: row.created_at,
        screen_width: row.screen_width,
        screen_height: row.screen_height,
        camera_id: row.camera_id,
        model_name: row.model_name,
        model_version: row.model_version,
        calibration_points,
        validation_error_px: row.validation_error_px.map(|value| value as f32),
        validation_quality: row.validation_quality,
        head_pose_range,
        active: row.active == 1,
    })
}

pub(crate) fn calibration_points_features(
    points: &[GazeCalibrationPointDto],
) -> Vec<FaceFeatureSampleDto> {
    points.iter().map(|point| point.features.clone()).collect()
}
