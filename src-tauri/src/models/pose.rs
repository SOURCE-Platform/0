// Data models for pose estimation, facial tracking, and hand tracking

use serde::{Deserialize, Serialize};

// ==============================================================================
// Pose Frame (Unified Result)
// ==============================================================================

/// Complete pose tracking result for a single frame
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoseFrame {
    pub session_id: String,
    pub timestamp: i64,
    pub frame_id: Option<String>, // Reference to captured screen frame
    pub body_pose: Option<BodyPose>,
    pub face_mesh: Option<FaceMesh>,
    pub hands: Vec<HandPose>,
    pub processing_time_ms: u64,
}

// ==============================================================================
// Body Pose (33 keypoints)
// ==============================================================================

/// Body pose tracking result using MediaPipe Pose (33 keypoints)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BodyPose {
    pub keypoints: Vec<Keypoint3D>,      // 33 body landmarks
    pub visibility_scores: Vec<f32>,     // Visibility confidence per keypoint
    pub world_landmarks: Option<Vec<Keypoint3D>>, // Metric 3D coordinates
    pub pose_classification: Option<PoseClassification>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoseClassification {
    Standing,
    Sitting,
    Lying,
    Leaning,
    Unknown,
}

impl PoseClassification {
    pub fn to_string(&self) -> &'static str {
        match self {
            PoseClassification::Standing => "standing",
            PoseClassification::Sitting => "sitting",
            PoseClassification::Lying => "lying",
            PoseClassification::Leaning => "leaning",
            PoseClassification::Unknown => "unknown",
        }
    }
}

/// MediaPipe Pose Landmark indices (33 total)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BodyLandmark {
    Nose = 0,
    LeftEyeInner = 1,
    LeftEye = 2,
    LeftEyeOuter = 3,
    RightEyeInner = 4,
    RightEye = 5,
    RightEyeOuter = 6,
    LeftEar = 7,
    RightEar = 8,
    MouthLeft = 9,
    MouthRight = 10,
    LeftShoulder = 11,
    RightShoulder = 12,
    LeftElbow = 13,
    RightElbow = 14,
    LeftWrist = 15,
    RightWrist = 16,
    LeftPinky = 17,
    RightPinky = 18,
    LeftIndex = 19,
    RightIndex = 20,
    LeftThumb = 21,
    RightThumb = 22,
    LeftHip = 23,
    RightHip = 24,
    LeftKnee = 25,
    RightKnee = 26,
    LeftAnkle = 27,
    RightAnkle = 28,
    LeftHeel = 29,
    RightHeel = 30,
    LeftFootIndex = 31,
    RightFootIndex = 32,
}

// ==============================================================================
// Face Mesh (468 landmarks + 52 blendshapes)
// ==============================================================================

/// Facial tracking result using MediaPipe Face Mesh
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceMesh {
    pub landmarks: Vec<Keypoint3D>,                // 468 facial landmarks
    pub blendshapes: Option<FaceBlendshapes>,      // 52 ARKit-compatible expressions
    pub transformation_matrix: Option<Vec<f32>>,   // 4x4 matrix (16 values) for face rotation/translation
}

/// ARKit-compatible face blendshapes (52 coefficients)
/// Each value ranges from 0.0 (neutral) to 1.0 (maximum expression)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceBlendshapes {
    // Eye blinking
    pub eye_blink_left: f32,
    pub eye_blink_right: f32,
    pub eye_look_down_left: f32,
    pub eye_look_down_right: f32,
    pub eye_look_in_left: f32,
    pub eye_look_in_right: f32,
    pub eye_look_out_left: f32,
    pub eye_look_out_right: f32,
    pub eye_look_up_left: f32,
    pub eye_look_up_right: f32,
    pub eye_squint_left: f32,
    pub eye_squint_right: f32,
    pub eye_wide_left: f32,
    pub eye_wide_right: f32,

    // Jaw movement
    pub jaw_forward: f32,
    pub jaw_left: f32,
    pub jaw_right: f32,
    pub jaw_open: f32,

    // Mouth expressions
    pub mouth_close: f32,
    pub mouth_funnel: f32,
    pub mouth_pucker: f32,
    pub mouth_left: f32,
    pub mouth_right: f32,
    pub mouth_smile_left: f32,
    pub mouth_smile_right: f32,
    pub mouth_frown_left: f32,
    pub mouth_frown_right: f32,
    pub mouth_dimple_left: f32,
    pub mouth_dimple_right: f32,
    pub mouth_stretch_left: f32,
    pub mouth_stretch_right: f32,
    pub mouth_roll_lower: f32,
    pub mouth_roll_upper: f32,
    pub mouth_shrug_lower: f32,
    pub mouth_shrug_upper: f32,
    pub mouth_press_left: f32,
    pub mouth_press_right: f32,
    pub mouth_lower_down_left: f32,
    pub mouth_lower_down_right: f32,
    pub mouth_upper_up_left: f32,
    pub mouth_upper_up_right: f32,

    // Brow expressions
    pub brow_down_left: f32,
    pub brow_down_right: f32,
    pub brow_inner_up: f32,
    pub brow_outer_up_left: f32,
    pub brow_outer_up_right: f32,

    // Cheek
    pub cheek_puff: f32,
    pub cheek_squint_left: f32,
    pub cheek_squint_right: f32,

    // Nose
    pub nose_sneer_left: f32,
    pub nose_sneer_right: f32,

    // Tongue
    pub tongue_out: f32,
}

impl FaceBlendshapes {
    /// Classify the dominant facial expression from blendshapes
    pub fn classify_expression(&self) -> FacialExpression {
        // Simple rule-based classification
        let smile_intensity = (self.mouth_smile_left + self.mouth_smile_right) / 2.0;
        let frown_intensity = (self.mouth_frown_left + self.mouth_frown_right) / 2.0;
        let brow_up = self.brow_inner_up;
        let brow_down = (self.brow_down_left + self.brow_down_right) / 2.0;
        let jaw_open = self.jaw_open;

        if smile_intensity > 0.5 {
            FacialExpression::Smile
        } else if frown_intensity > 0.4 {
            FacialExpression::Frown
        } else if brow_up > 0.6 && jaw_open > 0.4 {
            FacialExpression::Surprised
        } else if brow_down > 0.5 {
            FacialExpression::Angry
        } else if jaw_open > 0.6 {
            FacialExpression::OpenMouth
        } else {
            FacialExpression::Neutral
        }
    }

    /// Calculate overall expression intensity (0.0 to 1.0)
    pub fn calculate_intensity(&self) -> f32 {
        // Average of significant expression values
        let values = vec![
            (self.mouth_smile_left + self.mouth_smile_right) / 2.0,
            (self.mouth_frown_left + self.mouth_frown_right) / 2.0,
            self.brow_inner_up,
            (self.brow_down_left + self.brow_down_right) / 2.0,
            self.jaw_open,
            (self.eye_wide_left + self.eye_wide_right) / 2.0,
        ];
        values.iter().sum::<f32>() / values.len() as f32
    }
}

/// High-level facial expression classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FacialExpression {
    Neutral,
    Smile,
    Frown,
    Surprised,
    Angry,
    OpenMouth,
    Confused,
}

impl FacialExpression {
    pub fn to_string(&self) -> &'static str {
        match self {
            FacialExpression::Neutral => "neutral",
            FacialExpression::Smile => "smile",
            FacialExpression::Frown => "frown",
            FacialExpression::Surprised => "surprised",
            FacialExpression::Angry => "angry",
            FacialExpression::OpenMouth => "open_mouth",
            FacialExpression::Confused => "confused",
        }
    }
}

// ==============================================================================
// Hand Tracking (21 keypoints per hand)
// ==============================================================================

/// Hand pose tracking result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandPose {
    pub handedness: Handedness,
    pub landmarks: Vec<Keypoint3D>,                // 21 hand landmarks
    pub world_landmarks: Option<Vec<Keypoint3D>>,  // Metric 3D coordinates
    pub confidence: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Handedness {
    Left,
    Right,
}

impl Handedness {
    pub fn to_string(&self) -> &'static str {
        match self {
            Handedness::Left => "left",
            Handedness::Right => "right",
        }
    }
}

/// MediaPipe Hand Landmark indices (21 total)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HandLandmark {
    Wrist = 0,
    ThumbCmc = 1,
    ThumbMcp = 2,
    ThumbIp = 3,
    ThumbTip = 4,
    IndexFingerMcp = 5,
    IndexFingerPip = 6,
    IndexFingerDip = 7,
    IndexFingerTip = 8,
    MiddleFingerMcp = 9,
    MiddleFingerPip = 10,
    MiddleFingerDip = 11,
    MiddleFingerTip = 12,
    RingFingerMcp = 13,
    RingFingerPip = 14,
    RingFingerDip = 15,
    RingFingerTip = 16,
    PinkyMcp = 17,
    PinkyPip = 18,
    PinkyDip = 19,
    PinkyTip = 20,
}

// ==============================================================================
// Shared: 3D Keypoint
// ==============================================================================

/// A 3D keypoint with confidence score
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Keypoint3D {
    pub x: f32, // Normalized [0, 1] for image coordinates
    pub y: f32, // Normalized [0, 1] for image coordinates
    pub z: f32, // Depth (relative to reference point, e.g., hip midpoint for body)
    pub confidence: f32, // Detection confidence [0, 1]
}

impl Keypoint3D {
    pub fn new(x: f32, y: f32, z: f32, confidence: f32) -> Self {
        Self {
            x,
            y,
            z,
            confidence,
        }
    }

    pub fn is_visible(&self, threshold: f32) -> bool {
        self.confidence >= threshold
    }
}

// ==============================================================================
// Facial Expression Event (Aggregated)
// ==============================================================================

/// High-level facial expression event for database storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FacialExpressionEvent {
    pub id: String,
    pub session_id: String,
    pub timestamp: i64,
    pub expression_type: FacialExpression,
    pub intensity: f32,
    pub duration_ms: Option<u64>,
    pub blendshapes: Option<FaceBlendshapes>,
}

// ==============================================================================
// DTOs (Data Transfer Objects for Tauri commands)
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoseFrameDto {
    pub timestamp: i64,
    pub body_keypoints: Option<Vec<Keypoint3D>>,
    pub face_landmarks: Option<Vec<Keypoint3D>>,
    pub face_blendshapes: Option<FaceBlendshapes>,
    pub left_hand: Option<Vec<Keypoint3D>>,
    pub right_hand: Option<Vec<Keypoint3D>>,
    pub processing_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FacialExpressionDto {
    pub timestamp: i64,
    pub expression_type: String,
    pub intensity: f32,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoseStatistics {
    pub session_id: String,
    pub total_frames: u64,
    pub frames_with_body: u64,
    pub frames_with_face: u64,
    pub frames_with_hands: u64,
    pub average_processing_time_ms: f32,
    pub dominant_pose: Option<String>,         // "sitting", "standing", etc.
    pub dominant_expression: Option<String>,   // "smile", "neutral", etc.
    pub pose_changes: u32,                     // Number of pose transitions
    pub expression_changes: u32,               // Number of expression transitions
}

// ==============================================================================
// Configuration
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoseConfig {
    pub enable_body_tracking: bool,
    pub enable_face_tracking: bool,
    pub enable_hand_tracking: bool,
    pub target_fps: u32,                        // Frames per second to process (default: 15)
    pub min_detection_confidence: f32,          // Minimum confidence for detection (default: 0.5)
    pub min_tracking_confidence: f32,           // Minimum confidence for tracking (default: 0.5)
    pub model_complexity: ModelComplexity,      // Model complexity (0=lite, 1=full, 2=heavy)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelComplexity {
    Lite = 0,   // Fastest, less accurate
    Full = 1,   // Balanced
    Heavy = 2,  // Slowest, most accurate
}

impl Default for PoseConfig {
    fn default() -> Self {
        Self {
            enable_body_tracking: true,
            enable_face_tracking: true,
            enable_hand_tracking: true,
            target_fps: 15,
            min_detection_confidence: 0.5,
            min_tracking_confidence: 0.5,
            model_complexity: ModelComplexity::Full,
        }
    }
}

// ==============================================================================
// Error Types
// ==============================================================================

#[derive(Debug, thiserror::Error)]
pub enum PoseError {
    #[error("Pose detection not initialized")]
    NotInitialized,

    #[error("Pose detection already running")]
    AlreadyRunning,

    #[error("Model loading failed: {0}")]
    ModelLoadFailed(String),

    #[error("Inference failed: {0}")]
    InferenceFailed(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Not supported on this platform")]
    NotSupported,
}

pub type PoseResult<T> = Result<T, PoseError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypoint3d_visibility() {
        let keypoint = Keypoint3D::new(0.5, 0.5, 0.0, 0.8);
        assert!(keypoint.is_visible(0.5));
        assert!(keypoint.is_visible(0.7));
        assert!(!keypoint.is_visible(0.9));
    }

    #[test]
    fn test_facial_expression_classification() {
        let mut blendshapes = FaceBlendshapes {
            eye_blink_left: 0.0,
            eye_blink_right: 0.0,
            eye_look_down_left: 0.0,
            eye_look_down_right: 0.0,
            eye_look_in_left: 0.0,
            eye_look_in_right: 0.0,
            eye_look_out_left: 0.0,
            eye_look_out_right: 0.0,
            eye_look_up_left: 0.0,
            eye_look_up_right: 0.0,
            eye_squint_left: 0.0,
            eye_squint_right: 0.0,
            eye_wide_left: 0.0,
            eye_wide_right: 0.0,
            jaw_forward: 0.0,
            jaw_left: 0.0,
            jaw_right: 0.0,
            jaw_open: 0.0,
            mouth_close: 0.0,
            mouth_funnel: 0.0,
            mouth_pucker: 0.0,
            mouth_left: 0.0,
            mouth_right: 0.0,
            mouth_smile_left: 0.8,
            mouth_smile_right: 0.8,
            mouth_frown_left: 0.0,
            mouth_frown_right: 0.0,
            mouth_dimple_left: 0.0,
            mouth_dimple_right: 0.0,
            mouth_stretch_left: 0.0,
            mouth_stretch_right: 0.0,
            mouth_roll_lower: 0.0,
            mouth_roll_upper: 0.0,
            mouth_shrug_lower: 0.0,
            mouth_shrug_upper: 0.0,
            mouth_press_left: 0.0,
            mouth_press_right: 0.0,
            mouth_lower_down_left: 0.0,
            mouth_lower_down_right: 0.0,
            mouth_upper_up_left: 0.0,
            mouth_upper_up_right: 0.0,
            brow_down_left: 0.0,
            brow_down_right: 0.0,
            brow_inner_up: 0.0,
            brow_outer_up_left: 0.0,
            brow_outer_up_right: 0.0,
            cheek_puff: 0.0,
            cheek_squint_left: 0.0,
            cheek_squint_right: 0.0,
            nose_sneer_left: 0.0,
            nose_sneer_right: 0.0,
            tongue_out: 0.0,
        };

        assert_eq!(blendshapes.classify_expression(), FacialExpression::Smile);

        blendshapes.mouth_smile_left = 0.0;
        blendshapes.mouth_smile_right = 0.0;
        blendshapes.mouth_frown_left = 0.6;
        blendshapes.mouth_frown_right = 0.6;

        assert_eq!(blendshapes.classify_expression(), FacialExpression::Frown);
    }

    #[test]
    fn test_pose_config_default() {
        let config = PoseConfig::default();
        assert_eq!(config.target_fps, 15);
        assert_eq!(config.min_detection_confidence, 0.5);
        assert!(config.enable_body_tracking);
        assert!(config.enable_face_tracking);
        assert!(config.enable_hand_tracking);
    }
}
