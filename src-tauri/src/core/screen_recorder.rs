// Screen recorder abstraction layer - unified interface for all platforms

use crate::core::consent::{ConsentManager, Feature};
use crate::core::motion_detector::{MotionDetector, MotionResult};
use crate::core::storage::RecordingStorage;
use crate::core::video_encoder::{CompressionQuality, VideoCodec, VideoEncoder};
use crate::models::capture::{CaptureError, CaptureResult, Display, RawFrame};
use crate::platform::capture::PlatformCapture;
use crate::platform::power::{PowerEvent, PowerManager};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{Duration, Instant};
use uuid::Uuid;

/// Platform-agnostic screen capture trait
#[async_trait]
pub trait ScreenCapture: Send + Sync {
    /// Get list of available displays
    async fn get_displays(&self) -> CaptureResult<Vec<Display>>;

    /// Capture a single frame from the specified display
    async fn capture_frame(&self, display_id: u32) -> CaptureResult<RawFrame>;

    /// Start continuous capture from the specified display
    async fn start_capture(&mut self, display_id: u32) -> CaptureResult<()>;

    /// Stop continuous capture
    async fn stop_capture(&mut self) -> CaptureResult<()>;

    /// Check if currently capturing
    fn is_capturing(&self) -> bool;

    /// Get the current display being captured
    fn current_display_id(&self) -> Option<u32>;
}

/// Wrapper for platform-specific capture implementation
pub struct PlatformCaptureWrapper {
    inner: PlatformCapture,
}

#[async_trait]
impl ScreenCapture for PlatformCaptureWrapper {
    async fn get_displays(&self) -> CaptureResult<Vec<Display>> {
        PlatformCapture::get_displays().await
    }

    async fn capture_frame(&self, display_id: u32) -> CaptureResult<RawFrame> {
        PlatformCapture::capture_frame(display_id).await
    }

    async fn start_capture(&mut self, display_id: u32) -> CaptureResult<()> {
        self.inner.start_capture(display_id).await
    }

    async fn stop_capture(&mut self) -> CaptureResult<()> {
        self.inner.stop_capture().await
    }

    fn is_capturing(&self) -> bool {
        self.inner.is_capturing()
    }

    fn current_display_id(&self) -> Option<u32> {
        self.inner.current_display_id()
    }
}

/// Factory function to create platform-specific screen capture
pub async fn create_screen_capture() -> CaptureResult<Box<dyn ScreenCapture>> {
    let capture = PlatformCapture::new().await?;
    Ok(Box::new(PlatformCaptureWrapper { inner: capture }))
}

/// Recording status for UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingStatus {
    pub is_recording: bool,
    pub display_id: Option<u32>,
    pub display_name: Option<String>,
    pub has_consent: bool,
    pub session_id: Option<Uuid>,
    pub segment_count: usize,
    pub total_motion_percentage: f32,
    pub is_paused: bool,
}

/// Recording configuration
#[derive(Debug, Clone)]
pub struct RecordingConfig {
    pub target_fps: u32,
    pub buffer_size: usize,
    pub no_motion_threshold: usize,
    pub motion_detection_threshold: f32,
    pub codec: VideoCodec,
    pub quality: CompressionQuality,
    pub hardware_acceleration: bool,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            target_fps: 10,
            buffer_size: 60,          // 6 seconds at 10fps
            no_motion_threshold: 20,  // ~2 seconds at 10fps
            motion_detection_threshold: 0.05, // 5% pixels changed
            codec: VideoCodec::H264,
            quality: CompressionQuality::Medium,
            hardware_acceleration: true,
        }
    }
}

/// Recording state
struct RecordingState {
    session_id: Uuid,
    display_id: u32,
    motion_detector: MotionDetector,
    video_encoder: VideoEncoder,
    frame_buffer: Vec<RawFrame>,
    base_layer: Option<RawFrame>,
    no_motion_count: usize,
    total_frames: usize,
    motion_frames: usize,
    segment_count: usize,
    is_paused: bool,
}

/// High-level screen recorder with consent management
pub struct ScreenRecorder {
    capture: Arc<Mutex<Box<dyn ScreenCapture>>>,
    consent_manager: Arc<ConsentManager>,
    storage: Arc<RecordingStorage>,
    config: RecordingConfig,
    state: Arc<RwLock<Option<RecordingState>>>,
    stop_signal: Arc<RwLock<bool>>,
    power_manager: Arc<PowerManager>,
}

impl ScreenRecorder {
    /// Create a new screen recorder
    pub async fn new(
        consent_manager: Arc<ConsentManager>,
        storage: Arc<RecordingStorage>,
    ) -> CaptureResult<Self> {
        let capture = create_screen_capture().await?;
        let power_manager = Arc::new(PowerManager::new());

        // Start power monitoring
        let pm = Arc::clone(&power_manager);
        tokio::spawn(async move {
            pm.start_monitoring().await;
        });

        Ok(Self {
            capture: Arc::new(Mutex::new(capture)),
            consent_manager,
            storage,
            config: RecordingConfig::default(),
            state: Arc::new(RwLock::new(None)),
            stop_signal: Arc::new(RwLock::new(false)),
            power_manager,
        })
    }

    /// Create with custom configuration
    pub async fn new_with_config(
        consent_manager: Arc<ConsentManager>,
        storage: Arc<RecordingStorage>,
        config: RecordingConfig,
    ) -> CaptureResult<Self> {
        let capture = create_screen_capture().await?;
        let power_manager = Arc::new(PowerManager::new());

        // Start power monitoring
        let pm = Arc::clone(&power_manager);
        tokio::spawn(async move {
            pm.start_monitoring().await;
        });

        Ok(Self {
            capture: Arc::new(Mutex::new(capture)),
            consent_manager,
            storage,
            config,
            state: Arc::new(RwLock::new(None)),
            stop_signal: Arc::new(RwLock::new(false)),
            power_manager,
        })
    }

    /// Get list of available displays
    pub async fn get_available_displays(&self) -> CaptureResult<Vec<Display>> {
        let capture = self.capture.lock().await;
        capture.get_displays().await
    }

    /// Check if screen recording consent is granted
    async fn check_consent(&self) -> CaptureResult<bool> {
        self.consent_manager
            .is_consent_granted(Feature::ScreenRecording)
            .await
            .map_err(|e| CaptureError::CaptureFailed(format!("Failed to check consent: {}", e)))
    }

    /// Start recording from the specified display
    pub async fn start_recording(&self, display_id: u32) -> CaptureResult<()> {
        // Check consent first
        if !self.check_consent().await? {
            return Err(CaptureError::PermissionDenied(
                "Screen recording consent not granted. Please enable it in Privacy & Consent settings.".to_string()
            ));
        }

        // Check if already recording
        {
            let state = self.state.read().await;
            if state.is_some() {
                return Err(CaptureError::AlreadyCapturing);
            }
        }

        // Verify display exists
        let displays = self.get_available_displays().await?;
        let display = displays.iter().find(|d| d.id == display_id)
            .ok_or(CaptureError::DisplayNotFound(display_id))?;

        // Create recording session
        let session_id = self.storage.create_session(display_id).await
            .map_err(|e| CaptureError::CaptureFailed(format!("Failed to create session: {}", e)))?;

        // Initialize recording state
        let motion_detector = MotionDetector::new(self.config.motion_detection_threshold);
        let video_encoder = VideoEncoder::new(
            self.config.codec,
            self.config.quality,
            self.config.hardware_acceleration,
        ).map_err(|e| CaptureError::CaptureFailed(format!("Failed to create encoder: {}", e)))?;

        let recording_state = RecordingState {
            session_id,
            display_id,
            motion_detector,
            video_encoder,
            frame_buffer: Vec::with_capacity(self.config.buffer_size),
            base_layer: None,
            no_motion_count: 0,
            total_frames: 0,
            motion_frames: 0,
            segment_count: 0,
            is_paused: false,
        };

        *self.state.write().await = Some(recording_state);
        *self.stop_signal.write().await = false;

        println!("Started recording from display: {} ({}x{})",
            display.name, display.width, display.height);
        println!("Session ID: {}", session_id);

        // Start recording loop in background
        let recorder = Arc::new(self.clone_for_recording());
        tokio::spawn(async move {
            if let Err(e) = recorder.recording_loop().await {
                eprintln!("Recording loop error: {}", e);
            }
        });

        Ok(())
    }

    /// Stop recording
    pub async fn stop_recording(&self) -> CaptureResult<()> {
        // Signal stop
        *self.stop_signal.write().await = true;

        // Wait for recording loop to stop
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Get session ID before clearing state
        let session_id = {
            let state = self.state.read().await;
            state.as_ref().map(|s| s.session_id)
        };

        if let Some(session_id) = session_id {
            // End the session
            self.storage.end_session(session_id).await
                .map_err(|e| CaptureError::CaptureFailed(format!("Failed to end session: {}", e)))?;

            println!("Stopped recording session: {}", session_id);
        }

        // Clear state
        *self.state.write().await = None;

        Ok(())
    }

    /// Pause recording
    pub async fn pause_recording(&self) -> CaptureResult<()> {
        let mut state = self.state.write().await;
        if let Some(ref mut s) = *state {
            s.is_paused = true;
            println!("Recording paused");
            Ok(())
        } else {
            Err(CaptureError::NotCapturing)
        }
    }

    /// Resume recording
    pub async fn resume_recording(&self) -> CaptureResult<()> {
        let mut state = self.state.write().await;
        if let Some(ref mut s) = *state {
            s.is_paused = false;
            println!("Recording resumed");
            Ok(())
        } else {
            Err(CaptureError::NotCapturing)
        }
    }

    /// Clone for recording thread
    fn clone_for_recording(&self) -> Self {
        Self {
            capture: Arc::clone(&self.capture),
            consent_manager: Arc::clone(&self.consent_manager),
            storage: Arc::clone(&self.storage),
            config: self.config.clone(),
            state: Arc::clone(&self.state),
            stop_signal: Arc::clone(&self.stop_signal),
            power_manager: Arc::clone(&self.power_manager),
        }
    }

    /// Main recording loop - runs continuously until stopped
    async fn recording_loop(&self) -> CaptureResult<()> {
        let frame_interval = Duration::from_millis(1000 / self.config.target_fps as u64);
        let mut last_frame_time = Instant::now();
        let mut power_events = self.power_manager.subscribe();

        loop {
            // Check stop signal
            if *self.stop_signal.read().await {
                // Encode any remaining frames
                self.flush_buffer().await?;
                break;
            }

            // Check for power events (non-blocking)
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

            // Check if paused
            let is_paused = {
                let state = self.state.read().await;
                state.as_ref().map(|s| s.is_paused).unwrap_or(false)
            };

            if is_paused {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }

            // Maintain frame rate
            let elapsed = last_frame_time.elapsed();
            if elapsed < frame_interval {
                tokio::time::sleep(frame_interval - elapsed).await;
            }
            last_frame_time = Instant::now();

            // Process one frame
            if let Err(e) = self.process_frame().await {
                eprintln!("Frame processing error: {}", e);
            }
        }

        Ok(())
    }

    /// Process a single frame
    async fn process_frame(&self) -> CaptureResult<()> {
        let display_id = {
            let state = self.state.read().await;
            let s = state.as_ref().ok_or(CaptureError::NotCapturing)?;
            s.display_id
        };

        // Capture frame
        let capture = self.capture.lock().await;
        let frame = capture.capture_frame(display_id).await?;
        drop(capture);

        // Detect motion
        let motion = {
            let mut state = self.state.write().await;
            let s = state.as_mut().ok_or(CaptureError::NotCapturing)?;
            s.total_frames += 1;
            s.motion_detector.detect_motion(&frame)
        };

        // Handle based on motion
        if motion.has_motion {
            self.handle_motion_frame(frame, motion).await?;
        } else {
            self.handle_static_frame(frame).await?;
        }

        Ok(())
    }

    /// Handle a frame with motion detected
    async fn handle_motion_frame(&self, frame: RawFrame, motion: MotionResult) -> CaptureResult<()> {
        // Check if we need to update base layer and encode
        let (should_save_base, should_encode) = {
            let mut state = self.state.write().await;
            let s = state.as_mut().ok_or(CaptureError::NotCapturing)?;

            s.no_motion_count = 0;
            s.motion_frames += 1;
            s.frame_buffer.push(frame);

            // Check if major screen change (update base layer)
            let should_save_base = if motion.changed_percentage > 0.8 {
                if let Some(last_frame) = s.frame_buffer.last() {
                    s.base_layer = Some(last_frame.clone());
                    true
                } else {
                    false
                }
            } else {
                false
            };

            // Buffer full - encode segment
            let should_encode = s.frame_buffer.len() >= self.config.buffer_size;

            (should_save_base, should_encode)
        };

        // Save base layer if needed (outside lock)
        if should_save_base {
            if let Err(e) = self.save_base_layer().await {
                eprintln!("Failed to save base layer: {}", e);
            }
        }

        // Encode buffer if needed (outside lock)
        if should_encode {
            self.encode_and_save_buffer().await?;
        }

        Ok(())
    }

    /// Handle a frame without motion
    async fn handle_static_frame(&self, frame: RawFrame) -> CaptureResult<()> {
        let should_encode = {
            let mut state = self.state.write().await;
            let s = state.as_mut().ok_or(CaptureError::NotCapturing)?;

            s.no_motion_count += 1;

            // Motion stopped - encode what we have
            s.no_motion_count >= self.config.no_motion_threshold && !s.frame_buffer.is_empty()
        };

        if should_encode {
            self.encode_and_save_buffer().await?;
        }

        // Update base layer periodically during static periods
        let should_update_base = {
            let state = self.state.read().await;
            let s = state.as_ref().ok_or(CaptureError::NotCapturing)?;
            s.no_motion_count == self.config.no_motion_threshold
        };

        if should_update_base {
            let mut state = self.state.write().await;
            let s = state.as_mut().ok_or(CaptureError::NotCapturing)?;
            s.base_layer = Some(frame);
            drop(state);

            self.save_base_layer().await?;
        }

        Ok(())
    }

    /// Encode buffered frames and save segment
    async fn encode_and_save_buffer(&self) -> CaptureResult<()> {
        let (frames, session_id, segment_num) = {
            let mut state = self.state.write().await;
            let s = state.as_mut().ok_or(CaptureError::NotCapturing)?;

            if s.frame_buffer.is_empty() {
                return Ok(());
            }

            let frames = s.frame_buffer.drain(..).collect::<Vec<_>>();
            s.segment_count += 1;
            (frames, s.session_id, s.segment_count)
        };

        // Get output path
        let output_path = self.storage.get_segment_path(&session_id, segment_num);

        // Encode frames
        let segment = {
            let state = self.state.read().await;
            let s = state.as_ref().ok_or(CaptureError::NotCapturing)?;

            s.video_encoder
                .encode_frames(frames, output_path, self.config.target_fps)
                .await
                .map_err(|e| CaptureError::CaptureFailed(format!("Encoding failed: {}", e)))?
        };

        // Save segment to database
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

    /// Flush any remaining frames in buffer
    async fn flush_buffer(&self) -> CaptureResult<()> {
        let has_frames = {
            let state = self.state.read().await;
            state.as_ref().map(|s| !s.frame_buffer.is_empty()).unwrap_or(false)
        };

        if has_frames {
            self.encode_and_save_buffer().await?;
        }

        Ok(())
    }

    /// Save base layer to disk
    async fn save_base_layer(&self) -> CaptureResult<()> {
        let (session_id, base_layer) = {
            let state = self.state.read().await;
            let s = state.as_ref().ok_or(CaptureError::NotCapturing)?;
            (s.session_id, s.base_layer.clone())
        };

        if let Some(frame) = base_layer {
            self.storage
                .save_base_layer(&session_id, &frame)
                .await
                .map_err(|e| CaptureError::CaptureFailed(format!("Failed to save base layer: {}", e)))?;

            println!("Saved base layer for session {}", session_id);
        }

        Ok(())
    }

    /// Check if currently recording
    pub async fn is_recording(&self) -> bool {
        self.state.read().await.is_some()
    }

    /// Get current recording status
    pub async fn get_status(&self) -> CaptureResult<RecordingStatus> {
        let state = self.state.read().await;
        let has_consent = self.check_consent().await?;

        if let Some(ref s) = *state {
            let display_id = Some(s.display_id);
            let displays = self.get_available_displays().await?;
            let display_name = displays
                .iter()
                .find(|d| d.id == s.display_id)
                .map(|d| d.name.clone());

            let total_motion_percentage = if s.total_frames > 0 {
                (s.motion_frames as f32 / s.total_frames as f32) * 100.0
            } else {
                0.0
            };

            Ok(RecordingStatus {
                is_recording: true,
                display_id,
                display_name,
                has_consent,
                session_id: Some(s.session_id),
                segment_count: s.segment_count,
                total_motion_percentage,
                is_paused: s.is_paused,
            })
        } else {
            Ok(RecordingStatus {
                is_recording: false,
                display_id: None,
                display_name: None,
                has_consent,
                session_id: None,
                segment_count: 0,
                total_motion_percentage: 0.0,
                is_paused: false,
            })
        }
    }

    /// Capture a single frame (for testing)
    pub async fn capture_frame(&self, display_id: u32) -> CaptureResult<RawFrame> {
        // Check consent first
        if !self.check_consent().await? {
            return Err(CaptureError::PermissionDenied(
                "Screen recording consent not granted".to_string()
            ));
        }

        let capture = self.capture.lock().await;
        capture.capture_frame(display_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::database::Database;

    #[tokio::test]
    async fn test_create_screen_capture() {
        let result = create_screen_capture().await;
        match result {
            Ok(capture) => {
                println!("Successfully created screen capture");
                // Try to get displays
                match capture.get_displays().await {
                    Ok(displays) => {
                        println!("Found {} display(s)", displays.len());
                        assert!(!displays.is_empty());
                    }
                    Err(e) => {
                        eprintln!("Failed to get displays: {}", e);
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to create screen capture: {}", e);
                // This is acceptable on some platforms (e.g., Wayland, headless)
            }
        }
    }

    #[tokio::test]
    async fn test_screen_recorder() {
        // Initialize database, storage, and consent manager
        let db = Arc::new(Database::init().await.expect("Failed to init database"));
        let consent_manager = Arc::new(
            ConsentManager::new(db.clone()).await.expect("Failed to create consent manager")
        );

        let temp_dir = std::env::temp_dir().join("observer_test_recordings");
        let storage = Arc::new(
            RecordingStorage::new(temp_dir.clone(), db.clone())
                .await
                .expect("Failed to create storage")
        );

        // Create screen recorder
        let recorder = match ScreenRecorder::new(consent_manager.clone(), storage.clone()).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Failed to create recorder: {}", e);
                return; // Skip test on unsupported platforms
            }
        };

        // Get displays
        match recorder.get_available_displays().await {
            Ok(displays) => {
                println!("Found {} display(s)", displays.len());
                assert!(!displays.is_empty());

                // Get status
                let status = recorder.get_status().await.expect("Failed to get status");
                assert!(!status.is_recording);
                println!("Has consent: {}", status.has_consent);
            }
            Err(e) => {
                eprintln!("Failed to get displays: {}", e);
            }
        }

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_recording_lifecycle() {
        use tokio::time::{sleep, Duration};

        // Setup
        let db = Arc::new(Database::init().await.expect("Failed to init database"));
        let consent_manager = Arc::new(
            ConsentManager::new(db.clone()).await.expect("Failed to create consent manager")
        );

        let temp_dir = std::env::temp_dir().join("observer_test_lifecycle");
        let storage = Arc::new(
            RecordingStorage::new(temp_dir.clone(), db.clone())
                .await
                .expect("Failed to create storage")
        );

        // Grant consent for testing
        consent_manager
            .grant_consent(Feature::ScreenRecording)
            .await
            .expect("Failed to grant consent");

        let recorder = match ScreenRecorder::new(consent_manager, storage).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Failed to create recorder: {}", e);
                let _ = std::fs::remove_dir_all(&temp_dir);
                return;
            }
        };

        // Get first display
        let displays = match recorder.get_available_displays().await {
            Ok(d) if !d.is_empty() => d,
            _ => {
                eprintln!("No displays available for testing");
                let _ = std::fs::remove_dir_all(&temp_dir);
                return;
            }
        };

        let display_id = displays[0].id;

        // Start recording
        if let Err(e) = recorder.start_recording(display_id).await {
            eprintln!("Failed to start recording: {}", e);
            let _ = std::fs::remove_dir_all(&temp_dir);
            return;
        }

        // Verify recording started
        assert!(recorder.is_recording().await);

        let status = recorder.get_status().await.expect("Failed to get status");
        assert!(status.is_recording);
        assert_eq!(status.display_id, Some(display_id));
        assert!(status.session_id.is_some());

        // Record for a bit
        sleep(Duration::from_secs(2)).await;

        // Stop recording
        recorder.stop_recording().await.expect("Failed to stop");

        // Verify recording stopped
        assert!(!recorder.is_recording().await);

        let status = recorder.get_status().await.expect("Failed to get status");
        assert!(!status.is_recording);

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_pause_resume() {
        let db = Arc::new(Database::init().await.expect("Failed to init database"));
        let consent_manager = Arc::new(
            ConsentManager::new(db.clone()).await.expect("Failed to create consent manager")
        );

        let temp_dir = std::env::temp_dir().join("observer_test_pause");
        let storage = Arc::new(
            RecordingStorage::new(temp_dir.clone(), db.clone())
                .await
                .expect("Failed to create storage")
        );

        // Grant consent
        consent_manager
            .grant_consent(Feature::ScreenRecording)
            .await
            .expect("Failed to grant consent");

        let recorder = match ScreenRecorder::new(consent_manager, storage).await {
            Ok(r) => r,
            Err(_) => {
                let _ = std::fs::remove_dir_all(&temp_dir);
                return;
            }
        };

        let displays = match recorder.get_available_displays().await {
            Ok(d) if !d.is_empty() => d,
            _ => {
                let _ = std::fs::remove_dir_all(&temp_dir);
                return;
            }
        };

        // Start recording
        if recorder.start_recording(displays[0].id).await.is_err() {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return;
        }

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Pause
        recorder.pause_recording().await.expect("Failed to pause");
        let status = recorder.get_status().await.expect("Failed to get status");
        assert!(status.is_paused);

        // Resume
        recorder.resume_recording().await.expect("Failed to resume");
        let status = recorder.get_status().await.expect("Failed to get status");
        assert!(!status.is_paused);

        // Stop
        recorder.stop_recording().await.expect("Failed to stop");

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
