use super::constants::{
    EVIDENCE_AUDIT_INTERVAL_MS, MOTION_HIGH_THRESHOLD, MOTION_MODEL_NAME, MOTION_MODEL_VERSION,
    MOTION_MOVING_THRESHOLD, OBJECT_MODEL_NAME, OBJECT_MODEL_VERSION, VISION_MODEL_NAME,
    VISION_MODEL_VERSION, VISUAL_MIN_POSTURE_CONFIDENCE,
};
use super::media_io::{MediaPipeGazeOutput, MediaPipePoseOutput};
use super::visual_loop_state::VisualLoopState;
use crate::core::database::Database;
use crate::core::gaze::FaceFeatureSampleDto;
use crate::core::motion_detector::MotionResult;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

pub(super) fn classify_motion(motion: &MotionResult) -> (String, f32) {
    if motion.changed_percentage >= MOTION_HIGH_THRESHOLD {
        ("high_motion".to_string(), 0.9)
    } else if motion.changed_percentage >= MOTION_MOVING_THRESHOLD {
        ("moving".to_string(), 0.76)
    } else {
        ("still".to_string(), 0.82)
    }
}

pub(super) fn classify_visual_trigger_reason(
    state: &VisualLoopState,
    pose: &MediaPipePoseOutput,
    motion: &MotionResult,
) -> String {
    match (&state.previous_presence, pose.presence_label.as_str()) {
        (Some(previous), current) if previous != current && current == "person_visible" => {
            return "person_entered".to_string()
        }
        (Some(previous), current) if previous != current && current == "no_person_visible" => {
            return "person_left".to_string()
        }
        _ => {}
    }

    if state
        .previous_posture
        .as_ref()
        .map(|previous| previous != &pose.posture_label)
        .unwrap_or(false)
    {
        return "posture_change".to_string();
    }
    if motion.changed_percentage >= MOTION_HIGH_THRESHOLD {
        return "motion_spike".to_string();
    }
    if motion.changed_percentage >= MOTION_MOVING_THRESHOLD {
        return "object_change".to_string();
    }
    "periodic_sample".to_string()
}

pub(super) fn should_retain_visual_evidence(
    state: &VisualLoopState,
    trigger_reason: &str,
    pose: &MediaPipePoseOutput,
) -> bool {
    if matches!(
        trigger_reason,
        "person_entered" | "person_left" | "posture_change" | "motion_spike" | "manual_marker"
    ) {
        return true;
    }
    if pose.posture_confidence < VISUAL_MIN_POSTURE_CONFIDENCE {
        return true;
    }
    state
        .last_audit_evidence_at
        .map(|last| chrono::Utc::now().timestamp_millis() - last >= EVIDENCE_AUDIT_INTERVAL_MS)
        .unwrap_or(true)
}

pub(super) async fn insert_detector_rows(
    db: &Arc<Database>,
    frame_id: &str,
    session_id: &str,
    timestamp: i64,
    pose: &MediaPipePoseOutput,
    face_iris: &FaceFeatureSampleDto,
    gaze: &MediaPipeGazeOutput,
    motion: &MotionResult,
    motion_confidence: f32,
    pose_processing_time: i64,
) -> Result<(), sqlx::Error> {
    let pose_raw = json!({
        "person_count": pose.person_count,
        "presence_label": pose.presence_label,
        "presence_confidence": pose.presence_confidence,
        "posture_label": pose.posture_label,
        "posture_confidence": pose.posture_confidence,
        "body_bbox": pose.body_bbox,
        "landmarks": pose.landmarks,
        "world_landmarks": pose.world_landmarks,
        "notes": pose.notes,
    });
    let face_raw = json!({
        "face_bbox": face_iris.face_bbox,
        "face_center_x": face_iris.face_center_x,
        "face_center_y": face_iris.face_center_y,
        "face_width": face_iris.face_width,
        "face_height": face_iris.face_height,
        "yaw": face_iris.yaw,
        "pitch": face_iris.pitch,
        "roll": face_iris.roll,
        "confidence": face_iris.confidence,
        "head_pose": face_iris.head_pose,
        "face_landmarks": face_iris.face_landmarks,
        "left_iris_landmarks": face_iris.left_iris_landmarks,
        "right_iris_landmarks": face_iris.right_iris_landmarks,
        "notes": face_iris.notes,
    });
    let gaze_raw = json!({
        "available": gaze.available,
        "confidence": gaze.confidence,
        "vector": gaze.vector,
        "yaw_degrees": gaze.yaw_degrees,
        "pitch_degrees": gaze.pitch_degrees,
        "notes": gaze.notes,
    });
    let motion_raw = json!({
        "changed_percentage": motion.changed_percentage,
        "has_motion": motion.has_motion,
        "bounding_boxes": motion.bounding_boxes.iter().map(|bbox| json!({
            "x": bbox.x,
            "y": bbox.y,
            "width": bbox.width,
            "height": bbox.height,
        })).collect::<Vec<_>>(),
    });

    insert_visual_detection(
        db,
        frame_id,
        session_id,
        timestamp,
        "mediapipe_pose",
        VISION_MODEL_NAME,
        VISION_MODEL_VERSION,
        &pose_raw,
        pose.posture_confidence.max(pose.presence_confidence),
        pose_processing_time,
    )
    .await?;
    insert_visual_detection(
        db,
        frame_id,
        session_id,
        timestamp,
        "mediapipe_face_iris",
        OBJECT_MODEL_NAME,
        OBJECT_MODEL_VERSION,
        &face_raw,
        face_iris.confidence,
        pose_processing_time,
    )
    .await?;
    insert_visual_detection(
        db,
        frame_id,
        session_id,
        timestamp,
        "gaze_3d",
        face_iris
            .projected_gaze
            .as_ref()
            .and(Some("mediapipe_face_iris_gaze"))
            .unwrap_or("gaze_unavailable"),
        "v1",
        &gaze_raw,
        gaze.confidence,
        pose_processing_time,
    )
    .await?;
    insert_visual_detection(
        db,
        frame_id,
        session_id,
        timestamp,
        "motion_detector",
        MOTION_MODEL_NAME,
        MOTION_MODEL_VERSION,
        &motion_raw,
        motion_confidence,
        0,
    )
    .await?;
    Ok(())
}

async fn insert_visual_detection(
    db: &Arc<Database>,
    frame_id: &str,
    session_id: &str,
    timestamp: i64,
    detector_type: &str,
    model_name: &str,
    model_version: &str,
    raw_json: &Value,
    confidence: f32,
    processing_time_ms: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO vision_detections (
            detection_id, frame_id, session_id, timestamp, detector_type, model_name,
            model_version, raw_json, confidence, processing_time_ms, created_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(frame_id)
    .bind(session_id)
    .bind(timestamp)
    .bind(detector_type)
    .bind(model_name)
    .bind(model_version)
    .bind(raw_json.to_string())
    .bind(confidence as f64)
    .bind(processing_time_ms)
    .bind(chrono::Utc::now().timestamp_millis())
    .execute(db.pool())
    .await?;
    Ok(())
}
