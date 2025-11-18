use crate::core::consent::ConsentManager;
use crate::core::database::Database;
use crate::models::pose::{
    PoseFrame, PoseFrameDto, FacialExpressionEvent, FacialExpressionDto,
    PoseStatistics, PoseConfig, PoseError, PoseResult, BodyPose,
    FaceMesh, HandPose, Keypoint3D, FacialExpression,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

// ==============================================================================
// Database Models
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PoseFrameRecord {
    pub id: String,
    pub session_id: String,
    pub timestamp: i64,
    pub frame_id: Option<String>,
    pub body_keypoints_json: Option<String>,
    pub body_visibility_json: Option<String>,
    pub body_world_landmarks_json: Option<String>,
    pub pose_classification: Option<String>,
    pub face_landmarks_json: Option<String>,
    pub face_blendshapes_json: Option<String>,
    pub face_transformation_matrix_json: Option<String>,
    pub left_hand_json: Option<String>,
    pub right_hand_json: Option<String>,
    pub processing_time_ms: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct FacialExpressionRecord {
    pub id: String,
    pub session_id: String,
    pub timestamp: i64,
    pub expression_type: String,
    pub intensity: f64,
    pub duration_ms: Option<i64>,
    pub blendshapes_json: Option<String>,
    pub created_at: i64,
}

// ==============================================================================
// Pose Detector
// ==============================================================================

pub struct PoseDetector {
    db: Arc<Database>,
    consent_manager: Arc<ConsentManager>,
    config: Arc<RwLock<PoseConfig>>,
    current_session_id: Arc<RwLock<Option<String>>>,
    is_tracking: Arc<RwLock<bool>>,
    frame_tx: Arc<RwLock<Option<mpsc::Sender<PoseFrame>>>>,
}

impl PoseDetector {
    pub async fn new(
        consent_manager: Arc<ConsentManager>,
        db: Arc<Database>,
    ) -> PoseResult<Self> {
        Ok(Self {
            db,
            consent_manager,
            config: Arc::new(RwLock::new(PoseConfig::default())),
            current_session_id: Arc::new(RwLock::new(None)),
            is_tracking: Arc::new(RwLock::new(false)),
            frame_tx: Arc::new(RwLock::new(None)),
        })
    }

    /// Start pose tracking for a session
    pub async fn start_tracking(&self, session_id: String, config: PoseConfig) -> PoseResult<()> {
        // Check if already tracking
        let mut is_tracking = self.is_tracking.write().await;
        if *is_tracking {
            return Err(PoseError::AlreadyRunning);
        }

        // Check consent
        // TODO: Add ConsentFeature::PoseTracking to consent enum
        // if !self.consent_manager.has_consent(ConsentFeature::PoseTracking).await.map_err(|e| PoseError::DatabaseError(e.to_string()))? {
        //     return Err(PoseError::ConsentDenied);
        // }

        // Store session ID and config
        *self.current_session_id.write().await = Some(session_id.clone());
        *self.config.write().await = config.clone();

        // Create channel for pose frames
        let (tx, mut rx) = mpsc::channel::<PoseFrame>(100);
        *self.frame_tx.write().await = Some(tx);

        *is_tracking = true;

        // Spawn background task to process pose frames
        let db = self.db.clone();
        let is_tracking_clone = self.is_tracking.clone();

        tokio::spawn(async move {
            Self::process_pose_frames(rx, db, is_tracking_clone).await;
        });

        println!("Started pose tracking for session {}", session_id);
        Ok(())
    }

    /// Stop pose tracking
    pub async fn stop_tracking(&self) -> PoseResult<()> {
        let mut is_tracking = self.is_tracking.write().await;
        if !*is_tracking {
            return Ok(());
        }

        // Drop the sender to signal the processing task to stop
        *self.frame_tx.write().await = None;

        *is_tracking = false;
        *self.current_session_id.write().await = None;

        println!("Stopped pose tracking");
        Ok(())
    }

    /// Process a frame and detect poses
    /// This is called externally when a new frame is available
    pub async fn process_frame(&self, frame_data: &[u8], width: u32, height: u32, timestamp: i64) -> PoseResult<()> {
        let is_tracking = *self.is_tracking.read().await;
        if !is_tracking {
            return Ok(());
        }

        let session_id = self.current_session_id.read().await.clone();
        let session_id = match session_id {
            Some(id) => id,
            None => return Ok(()),
        };

        let config = self.config.read().await.clone();
        let start_time = std::time::Instant::now();

        // TODO: Actual MediaPipe inference here
        // For now, create a placeholder pose frame
        let pose_frame = Self::run_inference(frame_data, width, height, timestamp, session_id, &config)?;

        let processing_time = start_time.elapsed().as_millis() as u64;
        let mut pose_frame = pose_frame;
        pose_frame.processing_time_ms = processing_time;

        // Send to processing task
        if let Some(tx) = self.frame_tx.read().await.as_ref() {
            let _ = tx.send(pose_frame).await;
        }

        Ok(())
    }

    /// Run pose inference on a frame (placeholder for MediaPipe integration)
    fn run_inference(
        _frame_data: &[u8],
        _width: u32,
        _height: u32,
        timestamp: i64,
        session_id: String,
        config: &PoseConfig,
    ) -> PoseResult<PoseFrame> {
        // TODO: Implement actual MediaPipe inference
        // This is a placeholder that returns an empty pose frame

        let body_pose = if config.enable_body_tracking {
            Some(BodyPose {
                keypoints: vec![],
                visibility_scores: vec![],
                world_landmarks: None,
                pose_classification: None,
            })
        } else {
            None
        };

        let face_mesh = if config.enable_face_tracking {
            Some(FaceMesh {
                landmarks: vec![],
                blendshapes: None,
                transformation_matrix: None,
            })
        } else {
            None
        };

        let hands = if config.enable_hand_tracking {
            vec![]
        } else {
            vec![]
        };

        Ok(PoseFrame {
            session_id,
            timestamp,
            frame_id: None,
            body_pose,
            face_mesh,
            hands,
            processing_time_ms: 0,
        })
    }

    /// Background task to process and store pose frames
    async fn process_pose_frames(
        mut rx: mpsc::Receiver<PoseFrame>,
        db: Arc<Database>,
        is_tracking: Arc<RwLock<bool>>,
    ) {
        while *is_tracking.read().await {
            match rx.recv().await {
                Some(pose_frame) => {
                    if let Err(e) = Self::store_pose_frame(&db, &pose_frame).await {
                        eprintln!("Error storing pose frame: {}", e);
                    }

                    // Extract and store facial expressions if available
                    if let Some(ref face_mesh) = pose_frame.face_mesh {
                        if let Some(ref blendshapes) = face_mesh.blendshapes {
                            let expression = blendshapes.classify_expression();
                            let intensity = blendshapes.calculate_intensity();

                            if intensity > 0.3 { // Only store significant expressions
                                let event = FacialExpressionEvent {
                                    id: Uuid::new_v4().to_string(),
                                    session_id: pose_frame.session_id.clone(),
                                    timestamp: pose_frame.timestamp,
                                    expression_type: expression,
                                    intensity,
                                    duration_ms: None,
                                    blendshapes: Some(blendshapes.clone()),
                                };

                                if let Err(e) = Self::store_facial_expression(&db, &event).await {
                                    eprintln!("Error storing facial expression: {}", e);
                                }
                            }
                        }
                    }
                }
                None => break, // Channel closed
            }
        }
    }

    /// Store pose frame in database
    async fn store_pose_frame(db: &Arc<Database>, pose_frame: &PoseFrame) -> PoseResult<()> {
        let pool = db.pool();
        let id = Uuid::new_v4().to_string();
        let created_at = chrono::Utc::now().timestamp_millis();

        let body_keypoints_json = pose_frame.body_pose.as_ref()
            .map(|bp| serde_json::to_string(&bp.keypoints).ok())
            .flatten();
        let body_visibility_json = pose_frame.body_pose.as_ref()
            .map(|bp| serde_json::to_string(&bp.visibility_scores).ok())
            .flatten();
        let body_world_landmarks_json = pose_frame.body_pose.as_ref()
            .and_then(|bp| bp.world_landmarks.as_ref())
            .map(|wl| serde_json::to_string(wl).ok())
            .flatten();
        let pose_classification = pose_frame.body_pose.as_ref()
            .and_then(|bp| bp.pose_classification.map(|c| c.to_string().to_string()));

        let face_landmarks_json = pose_frame.face_mesh.as_ref()
            .map(|fm| serde_json::to_string(&fm.landmarks).ok())
            .flatten();
        let face_blendshapes_json = pose_frame.face_mesh.as_ref()
            .and_then(|fm| fm.blendshapes.as_ref())
            .map(|bs| serde_json::to_string(bs).ok())
            .flatten();
        let face_transformation_matrix_json = pose_frame.face_mesh.as_ref()
            .and_then(|fm| fm.transformation_matrix.as_ref())
            .map(|tm| serde_json::to_string(tm).ok())
            .flatten();

        let left_hand_json = pose_frame.hands.iter()
            .find(|h| matches!(h.handedness, crate::models::pose::Handedness::Left))
            .map(|h| serde_json::to_string(&h.landmarks).ok())
            .flatten();
        let right_hand_json = pose_frame.hands.iter()
            .find(|h| matches!(h.handedness, crate::models::pose::Handedness::Right))
            .map(|h| serde_json::to_string(&h.landmarks).ok())
            .flatten();

        sqlx::query(
            "INSERT INTO pose_frames (
                id, session_id, timestamp, frame_id,
                body_keypoints_json, body_visibility_json, body_world_landmarks_json, pose_classification,
                face_landmarks_json, face_blendshapes_json, face_transformation_matrix_json,
                left_hand_json, right_hand_json,
                processing_time_ms, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&id)
        .bind(&pose_frame.session_id)
        .bind(pose_frame.timestamp)
        .bind(&pose_frame.frame_id)
        .bind(body_keypoints_json)
        .bind(body_visibility_json)
        .bind(body_world_landmarks_json)
        .bind(pose_classification)
        .bind(face_landmarks_json)
        .bind(face_blendshapes_json)
        .bind(face_transformation_matrix_json)
        .bind(left_hand_json)
        .bind(right_hand_json)
        .bind(pose_frame.processing_time_ms as i64)
        .bind(created_at)
        .execute(pool)
        .await
        .map_err(|e| PoseError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    /// Store facial expression event in database
    async fn store_facial_expression(db: &Arc<Database>, event: &FacialExpressionEvent) -> PoseResult<()> {
        let pool = db.pool();
        let created_at = chrono::Utc::now().timestamp_millis();

        let blendshapes_json = event.blendshapes.as_ref()
            .map(|bs| serde_json::to_string(bs).ok())
            .flatten();

        sqlx::query(
            "INSERT INTO facial_expressions (
                id, session_id, timestamp, expression_type, intensity, duration_ms, blendshapes_json, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&event.id)
        .bind(&event.session_id)
        .bind(event.timestamp)
        .bind(event.expression_type.to_string())
        .bind(event.intensity as f64)
        .bind(event.duration_ms.map(|d| d as i64))
        .bind(blendshapes_json)
        .bind(created_at)
        .execute(pool)
        .await
        .map_err(|e| PoseError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    /// Get pose frames for a time range
    pub async fn get_pose_frames(
        &self,
        session_id: &str,
        start: i64,
        end: i64,
    ) -> PoseResult<Vec<PoseFrameDto>> {
        let pool = self.db.pool();

        let records: Vec<PoseFrameRecord> = sqlx::query_as(
            "SELECT * FROM pose_frames
             WHERE session_id = ? AND timestamp >= ? AND timestamp <= ?
             ORDER BY timestamp ASC"
        )
        .bind(session_id)
        .bind(start)
        .bind(end)
        .fetch_all(pool)
        .await
        .map_err(|e| PoseError::DatabaseError(e.to_string()))?;

        let dtos = records.into_iter().map(|r| {
            let body_keypoints = r.body_keypoints_json
                .and_then(|json| serde_json::from_str::<Vec<Keypoint3D>>(&json).ok());
            let face_landmarks = r.face_landmarks_json
                .and_then(|json| serde_json::from_str::<Vec<Keypoint3D>>(&json).ok());
            let face_blendshapes = r.face_blendshapes_json
                .and_then(|json| serde_json::from_str(&json).ok());
            let left_hand = r.left_hand_json
                .and_then(|json| serde_json::from_str::<Vec<Keypoint3D>>(&json).ok());
            let right_hand = r.right_hand_json
                .and_then(|json| serde_json::from_str::<Vec<Keypoint3D>>(&json).ok());

            PoseFrameDto {
                timestamp: r.timestamp,
                body_keypoints,
                face_landmarks,
                face_blendshapes,
                left_hand,
                right_hand,
                processing_time_ms: r.processing_time_ms as u64,
            }
        }).collect();

        Ok(dtos)
    }

    /// Get facial expression events
    pub async fn get_facial_expressions(
        &self,
        session_id: &str,
        expression_type: Option<String>,
        start: i64,
        end: i64,
    ) -> PoseResult<Vec<FacialExpressionDto>> {
        let pool = self.db.pool();

        let records: Vec<FacialExpressionRecord> = if let Some(expr_type) = expression_type {
            sqlx::query_as(
                "SELECT * FROM facial_expressions
                 WHERE session_id = ? AND expression_type = ? AND timestamp >= ? AND timestamp <= ?
                 ORDER BY timestamp ASC"
            )
            .bind(session_id)
            .bind(expr_type)
            .bind(start)
            .bind(end)
            .fetch_all(pool)
            .await
        } else {
            sqlx::query_as(
                "SELECT * FROM facial_expressions
                 WHERE session_id = ? AND timestamp >= ? AND timestamp <= ?
                 ORDER BY timestamp ASC"
            )
            .bind(session_id)
            .bind(start)
            .bind(end)
            .fetch_all(pool)
            .await
        }
        .map_err(|e| PoseError::DatabaseError(e.to_string()))?;

        let dtos = records.into_iter().map(|r| {
            FacialExpressionDto {
                timestamp: r.timestamp,
                expression_type: r.expression_type,
                intensity: r.intensity as f32,
                duration_ms: r.duration_ms.map(|d| d as u64),
            }
        }).collect();

        Ok(dtos)
    }

    /// Get pose statistics for a session
    pub async fn get_pose_statistics(&self, session_id: &str) -> PoseResult<PoseStatistics> {
        let pool = self.db.pool();

        let total_frames: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pose_frames WHERE session_id = ?"
        )
        .bind(session_id)
        .fetch_one(pool)
        .await
        .map_err(|e| PoseError::DatabaseError(e.to_string()))?;

        let frames_with_body: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pose_frames WHERE session_id = ? AND body_keypoints_json IS NOT NULL"
        )
        .bind(session_id)
        .fetch_one(pool)
        .await
        .map_err(|e| PoseError::DatabaseError(e.to_string()))?;

        let frames_with_face: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pose_frames WHERE session_id = ? AND face_landmarks_json IS NOT NULL"
        )
        .bind(session_id)
        .fetch_one(pool)
        .await
        .map_err(|e| PoseError::DatabaseError(e.to_string()))?;

        let frames_with_hands: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pose_frames WHERE session_id = ? AND (left_hand_json IS NOT NULL OR right_hand_json IS NOT NULL)"
        )
        .bind(session_id)
        .fetch_one(pool)
        .await
        .map_err(|e| PoseError::DatabaseError(e.to_string()))?;

        let avg_processing_time: Option<f64> = sqlx::query_scalar(
            "SELECT AVG(processing_time_ms) FROM pose_frames WHERE session_id = ?"
        )
        .bind(session_id)
        .fetch_one(pool)
        .await
        .map_err(|e| PoseError::DatabaseError(e.to_string()))?;

        Ok(PoseStatistics {
            session_id: session_id.to_string(),
            total_frames: total_frames as u64,
            frames_with_body: frames_with_body as u64,
            frames_with_face: frames_with_face as u64,
            frames_with_hands: frames_with_hands as u64,
            average_processing_time_ms: avg_processing_time.unwrap_or(0.0) as f32,
            dominant_pose: None,         // TODO: Calculate from pose_classification
            dominant_expression: None,   // TODO: Calculate from facial_expressions
            pose_changes: 0,             // TODO: Calculate pose transitions
            expression_changes: 0,       // TODO: Calculate expression transitions
        })
    }
}
