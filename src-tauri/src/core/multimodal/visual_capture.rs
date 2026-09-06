use super::constants::VISUAL_CAPTURE_INTERVAL_MS;
use super::indexing::reindex_visual_state_spans;
use super::media_io::{
    capture_camera_frame, load_png_as_frame, run_mediapipe_scene_inference,
    save_visual_evidence_frame, MediaPipePoseOutput,
};
use super::visual_detections::{
    classify_motion, classify_visual_trigger_reason, insert_detector_rows,
    should_retain_visual_evidence,
};
use super::visual_loop_state::VisualLoopState;
use crate::core::database::Database;
use crate::core::gaze;
use crate::core::storage::RecordingStorage;
use serde_json::json;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

pub(super) async fn run_visual_loop(
    db: Arc<Database>,
    storage: Arc<RecordingStorage>,
    generation_ref: Arc<AtomicU64>,
    generation: u64,
    session_id: String,
    source_id: String,
    display_id: Option<u32>,
    source_name: String,
    video_index: i32,
) {
    let mut loop_state = VisualLoopState::default();
    let session_uuid = match Uuid::parse_str(&session_id) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("visual loop could not parse session id: {error}");
            return;
        }
    };

    while generation_ref.load(Ordering::SeqCst) == generation {
        let started_at = chrono::Utc::now().timestamp_millis();
        let temp_dir = std::env::temp_dir().join("source_visual_samples");
        let _ = fs::create_dir_all(&temp_dir);
        let temp_path = temp_dir.join(format!("visual-{}.png", Uuid::new_v4()));

        if let Err(error) = capture_camera_frame(video_index, &temp_path).await {
            eprintln!("visual capture failed for {source_name}: {error}");
            sleep(Duration::from_millis(VISUAL_CAPTURE_INTERVAL_MS as u64)).await;
            continue;
        }

        let frame = match load_png_as_frame(&temp_path) {
            Ok(frame) => frame,
            Err(error) => {
                eprintln!("visual frame decode failed: {error}");
                let _ = fs::remove_file(&temp_path);
                sleep(Duration::from_millis(VISUAL_CAPTURE_INTERVAL_MS as u64)).await;
                continue;
            }
        };

        let motion = loop_state.motion_detector.detect_motion(&frame);
        let pose_started = chrono::Utc::now().timestamp_millis();
        let scene = match run_mediapipe_scene_inference(&temp_path).await {
            Ok(output) => output,
            Err(error) => {
                eprintln!("mediapipe scene inference failed for {source_name}: {error}");
                crate::core::multimodal::media_io::MediaPipeSceneOutput {
                    pose: MediaPipePoseOutput {
                        person_count: 0,
                        presence_label: "unknown".to_string(),
                        presence_confidence: 0.0,
                        posture_label: "unknown".to_string(),
                        posture_confidence: 0.0,
                        body_bbox: None,
                        landmarks: serde_json::Value::Null,
                        world_landmarks: serde_json::Value::Null,
                        notes: vec![error.clone()],
                    },
                    face_iris: gaze::FaceFeatureSampleDto {
                        face_bbox: None,
                        face_center_x: 0.5,
                        face_center_y: 0.5,
                        face_width: 0.0,
                        face_height: 0.0,
                        left_eye_x: None,
                        left_eye_y: None,
                        right_eye_x: None,
                        right_eye_y: None,
                        eye_mid_x: None,
                        eye_mid_y: None,
                        inter_eye_distance: None,
                        yaw: None,
                        pitch: None,
                        roll: None,
                        face_landmarks: Vec::new(),
                        left_iris_landmarks: Vec::new(),
                        right_iris_landmarks: Vec::new(),
                        head_pose: None,
                        gaze_vector: None,
                        projected_gaze: None,
                        gaze_yaw_degrees: None,
                        gaze_pitch_degrees: None,
                        gaze_model_name: None,
                        gaze_model_version: None,
                        confidence: 0.0,
                        notes: vec![error.clone()],
                    },
                    gaze: crate::core::multimodal::media_io::MediaPipeGazeOutput {
                        available: false,
                        confidence: 0.0,
                        vector: None,
                        yaw_degrees: None,
                        pitch_degrees: None,
                        model_name: None,
                        model_version: None,
                        notes: vec![error],
                    },
                }
            }
        };
        let pose_processing_time = chrono::Utc::now().timestamp_millis() - pose_started;
        let pose = &scene.pose;
        let (motion_label, motion_confidence) = classify_motion(&motion);
        let trigger_reason = classify_visual_trigger_reason(&loop_state, &pose, &motion);
        let retain_evidence = should_retain_visual_evidence(&loop_state, &trigger_reason, &pose);
        let evidence_path = if retain_evidence {
            save_visual_evidence_frame(&storage, session_uuid, &frame)
                .await
                .ok()
        } else {
            None
        };

        let frame_id = Uuid::new_v4().to_string();
        let scene_id = Uuid::new_v4().to_string();
        let timestamp = frame.timestamp;
        let created_at = chrono::Utc::now().timestamp_millis();

        let _ = sqlx::query(
            "INSERT INTO video_frame_samples (
                frame_id, session_id, timestamp, source_id, width, height, frame_path,
                retained_as_evidence, sampling_reason, motion_score, scene_delta, created_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&frame_id)
        .bind(&session_id)
        .bind(timestamp)
        .bind(&source_id)
        .bind(frame.width as i64)
        .bind(frame.height as i64)
        .bind(evidence_path.clone())
        .bind(if evidence_path.is_some() { 1 } else { 0 })
        .bind(trigger_reason.clone())
        .bind(motion.changed_percentage as f64)
        .bind(motion.changed_percentage as f64)
        .bind(created_at)
        .execute(db.pool())
        .await;

        let _ = insert_detector_rows(
            &db,
            &frame_id,
            &session_id,
            timestamp,
            pose,
            &scene.face_iris,
            &scene.gaze,
            &motion,
            motion_confidence,
            pose_processing_time,
        )
        .await;

        let avg_confidence = ((pose.presence_confidence
            + pose.posture_confidence
            + motion_confidence
            + scene.face_iris.confidence
            + scene.gaze.confidence)
            / 5.0)
            .max(0.0);
        let fused_state = json!({
            "presence_label": pose.presence_label,
            "presence_confidence": pose.presence_confidence,
            "posture_label": pose.posture_label,
            "posture_confidence": pose.posture_confidence,
            "motion_label": motion_label,
            "motion_confidence": motion_confidence,
            "head_pose": scene.face_iris.head_pose,
            "gaze_available": scene.gaze.available,
            "gaze_vector": scene.gaze.vector,
            "face_bbox": scene.face_iris.face_bbox,
            "face_landmark_count": scene.face_iris.face_landmarks.len(),
            "left_iris_count": scene.face_iris.left_iris_landmarks.len(),
            "right_iris_count": scene.face_iris.right_iris_landmarks.len(),
            "notes": pose.notes,
            "source_name": source_name,
            "face_detector_name": "mediapipe_face_iris",
            "face_detector_version": "v1",
            "gaze_model_name": scene.gaze.model_name,
            "gaze_model_version": scene.gaze.model_version,
        });

        let _ = sqlx::query(
            "INSERT INTO visual_scene_snapshots (
                visual_scene_id, session_id, timestamp, source_id, trigger_reason, frame_id,
                person_count, presence_label, presence_confidence, posture_label, posture_confidence,
                motion_label, motion_confidence, object_labels_json, object_boxes_json, fused_state_json,
                avg_confidence, processing_time_ms, created_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&scene_id)
        .bind(&session_id)
        .bind(timestamp)
        .bind(&source_id)
        .bind(trigger_reason.clone())
        .bind(&frame_id)
        .bind(pose.person_count)
        .bind(pose.presence_label.clone())
        .bind(pose.presence_confidence as f64)
        .bind(pose.posture_label.clone())
        .bind(pose.posture_confidence as f64)
        .bind(motion_label.clone())
        .bind(motion_confidence as f64)
        .bind("[]")
        .bind("[]")
        .bind(fused_state.to_string())
        .bind(avg_confidence as f64)
        .bind(chrono::Utc::now().timestamp_millis() - started_at)
        .bind(created_at)
        .execute(db.pool())
        .await;

        let _ = reindex_visual_state_spans(&db, &session_id, &source_id).await;
        if let Err(error) = gaze::process_gaze_frame(
            &db,
            &session_id,
            &source_id,
            display_id,
            &scene.face_iris,
            timestamp,
        )
        .await
        {
            eprintln!("gaze processing failed for {source_name}: {error}");
        }
        loop_state.previous_presence = Some(pose.presence_label.clone());
        loop_state.previous_posture = Some(pose.posture_label.clone());
        if retain_evidence {
            loop_state.last_audit_evidence_at = Some(timestamp);
        }

        let _ = fs::remove_file(&temp_path);
        sleep(Duration::from_millis(VISUAL_CAPTURE_INTERVAL_MS as u64)).await;
    }
}
