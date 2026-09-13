use super::{CaptureError, CaptureResult, RecordingState, RecordingStatus, ScreenRecorder};
use crate::core::motion_detector::MotionDetector;
use crate::core::video_encoder::VideoEncoder;
use crate::models::capture::RawFrame;
use crate::platform::capture::{request_screen_capture_permission, screen_capture_permission_granted};
use crate::platform::power::PowerEvent;
use tokio::time::{Duration, Instant};

const SCREEN_PERMISSION_MISSING: &str = "macOS Screen Recording permission is off for SOURCE, so screenshots would only show SOURCE's own window. Turn SOURCE on in System Settings > Privacy & Security > Screen & System Audio Recording, then quit and reopen SOURCE.";

impl ScreenRecorder {
    pub async fn start_recording(&self, display_id: u32) -> CaptureResult<()> {
        if !self.check_consent().await? {
            return Err(CaptureError::PermissionDenied(
                "Screen recording consent not granted. Please enable it in Privacy & Consent settings."
                    .to_string(),
            ));
        }

        if !screen_capture_permission_granted() {
            request_screen_capture_permission();
            return Err(CaptureError::PermissionDenied(SCREEN_PERMISSION_MISSING.to_string()));
        }

        if self.state.read().await.is_some() {
            return Err(CaptureError::AlreadyCapturing);
        }

        let displays = self.get_available_displays().await?;
        let display = displays
            .iter()
            .find(|display| display.id == display_id)
            .ok_or(CaptureError::DisplayNotFound(display_id))?;
        let session_id =
            self.storage.create_session(display_id).await.map_err(|e| {
                CaptureError::CaptureFailed(format!("Failed to create session: {}", e))
            })?;

        *self.state.write().await = Some(RecordingState {
            session_id,
            display_id,
            motion_detector: MotionDetector::new(self.config.motion_detection_threshold),
            video_encoder: VideoEncoder::new(
                self.config.codec,
                self.config.quality,
                self.config.hardware_acceleration,
            )
            .map_err(|e| CaptureError::CaptureFailed(format!("Failed to create encoder: {}", e)))?,
            frame_buffer: Vec::with_capacity(self.config.buffer_size),
            base_layer: None,
            no_motion_count: 0,
            total_frames: 0,
            motion_frames: 0,
            segment_count: 0,
            is_paused: false,
            last_ocr_capture_at: None,
            last_app_poll_at: None,
            last_frontmost_bundle_id: None,
            frontmost_is_self: false,
        });
        *self.stop_signal.write().await = false;

        println!(
            "Started recording from display: {} ({}x{})",
            display.name, display.width, display.height
        );
        println!("Session ID: {}", session_id);

        let recorder = std::sync::Arc::new(self.clone_for_recording());
        tokio::spawn(async move {
            if let Err(error) = recorder.recording_loop().await {
                eprintln!("Recording loop error: {}", error);
            }
        });

        Ok(())
    }

    pub async fn stop_recording(&self) -> CaptureResult<()> {
        *self.stop_signal.write().await = true;
        tokio::time::sleep(Duration::from_millis(500)).await;

        if let Some(session_id) = self
            .state
            .read()
            .await
            .as_ref()
            .map(|state| state.session_id)
        {
            self.storage.end_session(session_id).await.map_err(|e| {
                CaptureError::CaptureFailed(format!("Failed to end session: {}", e))
            })?;
            println!("Stopped recording session: {}", session_id);
        }

        *self.state.write().await = None;
        Ok(())
    }

    pub async fn pause_recording(&self) -> CaptureResult<()> {
        let mut state = self.state.write().await;
        if let Some(state) = state.as_mut() {
            state.is_paused = true;
            println!("Recording paused");
            Ok(())
        } else {
            Err(CaptureError::NotCapturing)
        }
    }

    pub async fn resume_recording(&self) -> CaptureResult<()> {
        let mut state = self.state.write().await;
        if let Some(state) = state.as_mut() {
            state.is_paused = false;
            println!("Recording resumed");
            Ok(())
        } else {
            Err(CaptureError::NotCapturing)
        }
    }

    pub async fn get_status(&self) -> CaptureResult<RecordingStatus> {
        let has_consent = self.check_consent().await?;
        if let Some(state) = self.state.read().await.as_ref() {
            let displays = self.get_available_displays().await?;
            let display_name = displays
                .iter()
                .find(|display| display.id == state.display_id)
                .map(|display| display.name.clone());
            let total_motion_percentage = if state.total_frames > 0 {
                (state.motion_frames as f32 / state.total_frames as f32) * 100.0
            } else {
                0.0
            };

            return Ok(RecordingStatus {
                is_recording: true,
                display_id: Some(state.display_id),
                display_name,
                has_consent,
                session_id: Some(state.session_id),
                segment_count: state.segment_count,
                total_frames: state.total_frames,
                total_motion_percentage,
                is_paused: state.is_paused,
                save_directory: Some(
                    self.storage
                        .get_session_dir(&state.session_id)
                        .to_string_lossy()
                        .to_string(),
                ),
            });
        }

        Ok(RecordingStatus {
            is_recording: false,
            display_id: None,
            display_name: None,
            has_consent,
            session_id: None,
            segment_count: 0,
            total_frames: 0,
            total_motion_percentage: 0.0,
            is_paused: false,
            save_directory: None,
        })
    }

    pub async fn capture_frame(&self, display_id: u32) -> CaptureResult<RawFrame> {
        if !self.check_consent().await? {
            return Err(CaptureError::PermissionDenied(
                "Screen recording consent not granted".to_string(),
            ));
        }
        self.capture.lock().await.capture_frame(display_id).await
    }

    async fn recording_loop(&self) -> CaptureResult<()> {
        let frame_interval = Duration::from_millis(1000 / self.config.target_fps as u64);
        let mut last_frame_time = Instant::now();
        let mut power_events = self.power_manager.subscribe();

        loop {
            if *self.stop_signal.read().await {
                self.flush_buffer().await?;
                break;
            }

            if let Ok(event) = power_events.try_recv() {
                match event {
                    PowerEvent::Sleep => {
                        println!("System going to sleep - pausing recording");
                        let _ = self.pause_recording().await;
                    }
                    PowerEvent::Wake => {
                        println!("System waking up - resuming recording");
                        let _ = self.resume_recording().await;
                    }
                }
            }

            if self
                .state
                .read()
                .await
                .as_ref()
                .map(|state| state.is_paused)
                .unwrap_or(false)
            {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }

            let elapsed = last_frame_time.elapsed();
            if elapsed < frame_interval {
                tokio::time::sleep(frame_interval - elapsed).await;
            }
            last_frame_time = Instant::now();

            if let Err(error) = self.process_frame().await {
                eprintln!("Frame processing error: {}", error);
            }
        }

        Ok(())
    }

    async fn process_frame(&self) -> CaptureResult<()> {
        let display_id = self
            .state
            .read()
            .await
            .as_ref()
            .ok_or(CaptureError::NotCapturing)?
            .display_id;

        let frame = self.capture.lock().await.capture_frame(display_id).await?;
        self.maybe_detect_app_switch(frame.timestamp).await;

        let motion = {
            let mut state = self.state.write().await;
            let state = state.as_mut().ok_or(CaptureError::NotCapturing)?;
            state.total_frames += 1;
            state.motion_detector.detect_motion(&frame)
        };

        if let Err(error) = self.maybe_enqueue_ocr_frame(&frame, &motion).await {
            eprintln!("OCR enqueue error: {}", error);
        }

        if motion.has_motion {
            self.handle_motion_frame(frame, motion).await?;
        } else {
            self.handle_static_frame(frame).await?;
        }

        Ok(())
    }
}
