use super::{
    CaptureError, CaptureResult, ScreenRecorder, OCR_APP_SWITCH_POLL_MS,
    OCR_LARGE_SCENE_CHANGE_THRESHOLD,
};
use crate::core::motion_detector::MotionResult;
use crate::models::capture::RawFrame;
use crate::models::ocr::BoundingBox as OcrBoundingBox;

impl ScreenRecorder {
    pub(super) async fn maybe_enqueue_ocr_frame(
        &self,
        frame: &RawFrame,
        motion: &MotionResult,
    ) -> CaptureResult<()> {
        let settings = self.ocr_capture_settings.read().await.clone();
        if !settings.enabled {
            return Ok(());
        }

        let Some(processor) = self.ocr_processor.read().await.clone() else {
            return Ok(());
        };

        if motion.has_motion && motion.changed_percentage >= OCR_LARGE_SCENE_CHANGE_THRESHOLD {
            self.ocr_trigger_signals
                .mark_large_scene_change(frame.timestamp)
                .await;
        }

        let trigger_outcome = self.ocr_trigger_signals.consume_due(frame.timestamp).await;
        let (session_id, display_id, trigger_reason) = {
            let mut state = self.state.write().await;
            let state = state.as_mut().ok_or(CaptureError::NotCapturing)?;
            let base_interval_ms = settings.interval_seconds as i64 * 1000;
            let should_capture_now = trigger_outcome.app_switch
                || trigger_outcome.large_scene_change
                || trigger_outcome.scroll_settled;

            if !should_capture_now
                && state
                    .last_ocr_capture_at
                    .map(|last| frame.timestamp - last < base_interval_ms)
                    .unwrap_or(false)
            {
                return Ok(());
            }

            state.last_ocr_capture_at = Some(frame.timestamp);
            let reason = if trigger_outcome.app_switch {
                "app_switch"
            } else if trigger_outcome.large_scene_change {
                "scene_change"
            } else if trigger_outcome.scroll_settled {
                "scroll_settled"
            } else {
                "static_fallback"
            };
            (state.session_id, Some(state.display_id), reason.to_string())
        };

        let frame_path = self
            .storage
            .save_ocr_frame(session_id, frame)
            .await
            .map_err(|e| CaptureError::CaptureFailed(format!("Failed to save OCR frame: {}", e)))?;

        let motion_regions = if motion.has_motion && !motion.bounding_boxes.is_empty() {
            motion
                .bounding_boxes
                .iter()
                .map(|region| OcrBoundingBox::new(region.x, region.y, region.width, region.height))
                .collect()
        } else {
            vec![OcrBoundingBox::new(0, 0, frame.width, frame.height)]
        };

        processor
            .enqueue_frame(
                session_id,
                frame_path,
                frame.timestamp,
                display_id,
                trigger_reason,
                motion_regions,
            )
            .await
            .map_err(|e| CaptureError::CaptureFailed(format!("Failed to enqueue OCR frame: {}", e)))
    }

    pub(super) async fn maybe_detect_app_switch(&self, timestamp: i64) {
        let should_poll = {
            let mut state = self.state.write().await;
            let Some(state) = state.as_mut() else {
                return;
            };
            let should_poll = state
                .last_app_poll_at
                .map(|last| timestamp - last >= OCR_APP_SWITCH_POLL_MS)
                .unwrap_or(true);
            if should_poll {
                state.last_app_poll_at = Some(timestamp);
            }
            should_poll
        };

        if !should_poll {
            return;
        }

        let Some(recorder) = self.os_activity_recorder.read().await.clone() else {
            return;
        };
        let current_bundle_id = recorder
            .get_current_app()
            .await
            .ok()
            .flatten()
            .map(|app| app.bundle_id);

        let switched = {
            let mut state = self.state.write().await;
            let Some(state) = state.as_mut() else {
                return;
            };

            match (&state.last_frontmost_bundle_id, &current_bundle_id) {
                (Some(previous), Some(current)) if previous != current => {
                    state.last_frontmost_bundle_id = Some(current.clone());
                    true
                }
                (None, Some(current)) => {
                    state.last_frontmost_bundle_id = Some(current.clone());
                    false
                }
                (Some(_), None) => {
                    state.last_frontmost_bundle_id = None;
                    false
                }
                _ => false,
            }
        };

        if switched {
            self.ocr_trigger_signals.mark_app_switch(timestamp).await;
        }
    }
}
