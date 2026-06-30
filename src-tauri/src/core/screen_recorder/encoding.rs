use super::{CaptureError, CaptureResult, ScreenRecorder};
use crate::core::motion_detector::MotionResult;
use crate::models::capture::RawFrame;

impl ScreenRecorder {
    pub(super) async fn handle_motion_frame(
        &self,
        frame: RawFrame,
        motion: MotionResult,
    ) -> CaptureResult<()> {
        let (should_save_base, should_encode) = {
            let mut state = self.state.write().await;
            let state = state.as_mut().ok_or(CaptureError::NotCapturing)?;
            state.no_motion_count = 0;
            state.motion_frames += 1;
            state.frame_buffer.push(frame);

            let should_save_base = if motion.changed_percentage > 0.8 {
                if let Some(last_frame) = state.frame_buffer.last() {
                    state.base_layer = Some(last_frame.clone());
                    true
                } else {
                    false
                }
            } else {
                false
            };
            let should_encode = state.frame_buffer.len() >= self.config.buffer_size;
            (should_save_base, should_encode)
        };

        if should_save_base {
            let _ = self.save_base_layer().await;
        }
        if should_encode {
            self.encode_and_save_buffer().await?;
        }
        Ok(())
    }

    pub(super) async fn handle_static_frame(&self, frame: RawFrame) -> CaptureResult<()> {
        let should_encode = {
            let mut state = self.state.write().await;
            let state = state.as_mut().ok_or(CaptureError::NotCapturing)?;
            state.no_motion_count += 1;
            state.no_motion_count >= self.config.no_motion_threshold
                && !state.frame_buffer.is_empty()
        };
        if should_encode {
            self.encode_and_save_buffer().await?;
        }

        let should_update_base = self
            .state
            .read()
            .await
            .as_ref()
            .map(|state| state.no_motion_count == self.config.no_motion_threshold)
            .ok_or(CaptureError::NotCapturing)?;

        if should_update_base {
            if let Some(state) = self.state.write().await.as_mut() {
                state.base_layer = Some(frame);
            }
            self.save_base_layer().await?;
        }

        Ok(())
    }

    pub(super) async fn encode_and_save_buffer(&self) -> CaptureResult<()> {
        let (frames, session_id, segment_num) = {
            let mut state = self.state.write().await;
            let state = state.as_mut().ok_or(CaptureError::NotCapturing)?;
            if state.frame_buffer.is_empty() {
                return Ok(());
            }
            let frames = state.frame_buffer.drain(..).collect::<Vec<_>>();
            state.segment_count += 1;
            (frames, state.session_id, state.segment_count)
        };

        let output_path = self.storage.get_segment_path(&session_id, segment_num);
        let segment = {
            let state = self.state.read().await;
            let state = state.as_ref().ok_or(CaptureError::NotCapturing)?;
            state
                .video_encoder
                .encode_frames(frames, output_path, self.config.target_fps)
                .await
                .map_err(|e| CaptureError::CaptureFailed(format!("Encoding failed: {}", e)))?
        };

        self.storage
            .save_segment(&session_id, &segment)
            .await
            .map_err(|e| CaptureError::CaptureFailed(format!("Failed to save segment: {}", e)))?;
        println!(
            "Encoded segment {}: {} frames, {} bytes",
            segment_num, segment.frame_count, segment.file_size_bytes
        );
        Ok(())
    }

    pub(super) async fn flush_buffer(&self) -> CaptureResult<()> {
        if self
            .state
            .read()
            .await
            .as_ref()
            .map(|state| !state.frame_buffer.is_empty())
            .unwrap_or(false)
        {
            self.encode_and_save_buffer().await?;
        }
        Ok(())
    }

    pub(super) async fn save_base_layer(&self) -> CaptureResult<()> {
        let (session_id, base_layer) = {
            let state = self.state.read().await;
            let state = state.as_ref().ok_or(CaptureError::NotCapturing)?;
            (state.session_id, state.base_layer.clone())
        };

        if let Some(frame) = base_layer {
            self.storage
                .save_base_layer(&session_id, &frame)
                .await
                .map_err(|e| {
                    CaptureError::CaptureFailed(format!("Failed to save base layer: {}", e))
                })?;
            println!("Saved base layer for session {}", session_id);
        }

        Ok(())
    }
}
