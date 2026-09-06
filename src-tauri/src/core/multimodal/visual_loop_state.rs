use super::constants::MOTION_MOVING_THRESHOLD;
use crate::core::motion_detector::MotionDetector;

pub(super) struct VisualLoopState {
    pub motion_detector: MotionDetector,
    pub previous_presence: Option<String>,
    pub previous_posture: Option<String>,
    pub last_audit_evidence_at: Option<i64>,
}

impl Default for VisualLoopState {
    fn default() -> Self {
        Self {
            motion_detector: MotionDetector::new(MOTION_MOVING_THRESHOLD),
            previous_presence: None,
            previous_posture: None,
            last_audit_evidence_at: None,
        }
    }
}
