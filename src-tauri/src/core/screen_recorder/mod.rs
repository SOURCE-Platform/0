use crate::core::consent::{ConsentManager, Feature};
use crate::core::motion_detector::MotionDetector;
use crate::core::ocr_processor::OcrProcessor;
use crate::core::ocr_trigger_signals::OcrTriggerSignals;
use crate::core::os_activity::OsActivityRecorder;
use crate::core::storage::RecordingStorage;
use crate::core::video_encoder::{CompressionQuality, VideoCodec, VideoEncoder};
use crate::models::capture::{CaptureError, CaptureResult, Display, RawFrame};
use crate::platform::capture::PlatformCapture;
use crate::platform::power::PowerManager;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

mod encoding;
mod lifecycle;
mod ocr;
mod tests;

const OCR_APP_SWITCH_POLL_MS: i64 = 250;
const OCR_LARGE_SCENE_CHANGE_THRESHOLD: f32 = 0.35;

#[async_trait]
pub trait ScreenCapture: Send + Sync {
    async fn get_displays(&self) -> CaptureResult<Vec<Display>>;
    async fn capture_frame(&self, display_id: u32) -> CaptureResult<RawFrame>;
    async fn start_capture(&mut self, display_id: u32) -> CaptureResult<()>;
    async fn stop_capture(&mut self) -> CaptureResult<()>;
    fn is_capturing(&self) -> bool;
    fn current_display_id(&self) -> Option<u32>;
}

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

pub async fn create_screen_capture() -> CaptureResult<Box<dyn ScreenCapture>> {
    Ok(Box::new(PlatformCaptureWrapper {
        inner: PlatformCapture::new().await?,
    }))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingStatus {
    pub is_recording: bool,
    pub display_id: Option<u32>,
    pub display_name: Option<String>,
    pub has_consent: bool,
    pub session_id: Option<Uuid>,
    pub segment_count: usize,
    pub total_frames: usize,
    pub total_motion_percentage: f32,
    pub is_paused: bool,
    pub save_directory: Option<String>,
}

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
            buffer_size: 60,
            no_motion_threshold: 20,
            motion_detection_threshold: 0.05,
            codec: VideoCodec::H264,
            quality: CompressionQuality::Medium,
            hardware_acceleration: true,
        }
    }
}

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
    last_ocr_capture_at: Option<i64>,
    last_app_poll_at: Option<i64>,
    last_frontmost_bundle_id: Option<String>,
    frontmost_is_self: bool,
}

#[derive(Debug, Clone)]
struct OcrCaptureSettings {
    enabled: bool,
    interval_seconds: u32,
}

impl Default for OcrCaptureSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_seconds: 60,
        }
    }
}

pub struct ScreenRecorder {
    capture: Arc<Mutex<Box<dyn ScreenCapture>>>,
    consent_manager: Arc<ConsentManager>,
    storage: Arc<RecordingStorage>,
    config: RecordingConfig,
    state: Arc<RwLock<Option<RecordingState>>>,
    stop_signal: Arc<RwLock<bool>>,
    power_manager: Arc<PowerManager>,
    ocr_processor: Arc<RwLock<Option<Arc<OcrProcessor>>>>,
    ocr_capture_settings: Arc<RwLock<OcrCaptureSettings>>,
    ocr_trigger_signals: Arc<OcrTriggerSignals>,
    os_activity_recorder: Arc<RwLock<Option<Arc<OsActivityRecorder>>>>,
}

impl ScreenRecorder {
    pub async fn new(
        consent_manager: Arc<ConsentManager>,
        storage: Arc<RecordingStorage>,
        ocr_trigger_signals: Arc<OcrTriggerSignals>,
    ) -> CaptureResult<Self> {
        Self::new_with_config(
            consent_manager,
            storage,
            RecordingConfig::default(),
            ocr_trigger_signals,
        )
        .await
    }

    pub async fn new_with_config(
        consent_manager: Arc<ConsentManager>,
        storage: Arc<RecordingStorage>,
        config: RecordingConfig,
        ocr_trigger_signals: Arc<OcrTriggerSignals>,
    ) -> CaptureResult<Self> {
        let recorder = Self {
            capture: Arc::new(Mutex::new(create_screen_capture().await?)),
            consent_manager,
            storage,
            config,
            state: Arc::new(RwLock::new(None)),
            stop_signal: Arc::new(RwLock::new(false)),
            power_manager: Arc::new(PowerManager::new()),
            ocr_processor: Arc::new(RwLock::new(None)),
            ocr_capture_settings: Arc::new(RwLock::new(OcrCaptureSettings::default())),
            ocr_trigger_signals,
            os_activity_recorder: Arc::new(RwLock::new(None)),
        };

        let power_manager = Arc::clone(&recorder.power_manager);
        tokio::spawn(async move {
            power_manager.start_monitoring().await;
        });

        Ok(recorder)
    }

    pub async fn attach_ocr_processor(&self, processor: Arc<OcrProcessor>) {
        *self.ocr_processor.write().await = Some(processor);
    }

    pub async fn attach_os_activity_recorder(&self, recorder: Arc<OsActivityRecorder>) {
        *self.os_activity_recorder.write().await = Some(recorder);
    }

    pub async fn configure_ocr_capture(&self, enabled: bool, interval_seconds: u32) {
        *self.ocr_capture_settings.write().await = OcrCaptureSettings {
            enabled,
            interval_seconds: interval_seconds.max(1),
        };
    }

    pub async fn get_available_displays(&self) -> CaptureResult<Vec<Display>> {
        self.capture.lock().await.get_displays().await
    }

    pub async fn is_recording(&self) -> bool {
        self.state.read().await.is_some()
    }

    async fn check_consent(&self) -> CaptureResult<bool> {
        self.consent_manager
            .is_consent_granted(Feature::ScreenRecording)
            .await
            .map_err(|e| CaptureError::CaptureFailed(format!("Failed to check consent: {}", e)))
    }

    fn clone_for_recording(&self) -> Self {
        Self {
            capture: Arc::clone(&self.capture),
            consent_manager: Arc::clone(&self.consent_manager),
            storage: Arc::clone(&self.storage),
            config: self.config.clone(),
            state: Arc::clone(&self.state),
            stop_signal: Arc::clone(&self.stop_signal),
            power_manager: Arc::clone(&self.power_manager),
            ocr_processor: Arc::clone(&self.ocr_processor),
            ocr_capture_settings: Arc::clone(&self.ocr_capture_settings),
            ocr_trigger_signals: Arc::clone(&self.ocr_trigger_signals),
            os_activity_recorder: Arc::clone(&self.os_activity_recorder),
        }
    }
}
