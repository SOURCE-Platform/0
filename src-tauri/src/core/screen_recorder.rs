// Screen recorder abstraction layer - unified interface for all platforms

use crate::core::consent::{ConsentManager, Feature};
use crate::models::capture::{CaptureError, CaptureResult, Display, RawFrame};
use crate::platform::capture::PlatformCapture;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

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
}

/// High-level screen recorder with consent management
pub struct ScreenRecorder {
    capture: Arc<Mutex<Box<dyn ScreenCapture>>>,
    consent_manager: Arc<ConsentManager>,
}

impl ScreenRecorder {
    /// Create a new screen recorder
    pub async fn new(consent_manager: Arc<ConsentManager>) -> CaptureResult<Self> {
        let capture = create_screen_capture().await?;

        Ok(Self {
            capture: Arc::new(Mutex::new(capture)),
            consent_manager,
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

        // Verify display exists
        let displays = self.get_available_displays().await?;
        let display = displays.iter().find(|d| d.id == display_id)
            .ok_or(CaptureError::DisplayNotFound(display_id))?;

        // Start capture
        let mut capture = self.capture.lock().await;
        capture.start_capture(display_id).await?;

        println!("Started recording from display: {} ({}x{})",
            display.name, display.width, display.height);

        Ok(())
    }

    /// Stop recording
    pub async fn stop_recording(&self) -> CaptureResult<()> {
        let mut capture = self.capture.lock().await;
        capture.stop_capture().await?;

        println!("Stopped recording");

        Ok(())
    }

    /// Check if currently recording
    pub async fn is_recording(&self) -> bool {
        let capture = self.capture.lock().await;
        capture.is_capturing()
    }

    /// Get current recording status
    pub async fn get_status(&self) -> CaptureResult<RecordingStatus> {
        let capture = self.capture.lock().await;
        let is_recording = capture.is_capturing();
        let display_id = capture.current_display_id();
        let has_consent = self.check_consent().await?;

        let display_name = if let Some(id) = display_id {
            // Get display name
            drop(capture); // Release lock before calling get_available_displays
            let displays = self.get_available_displays().await?;
            displays.iter()
                .find(|d| d.id == id)
                .map(|d| d.name.clone())
        } else {
            None
        };

        Ok(RecordingStatus {
            is_recording,
            display_id,
            display_name,
            has_consent,
        })
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
        // Initialize database and consent manager
        let db = Database::init().await.expect("Failed to init database");
        let consent_manager = Arc::new(
            ConsentManager::new(db).await.expect("Failed to create consent manager")
        );

        // Create screen recorder
        let recorder = match ScreenRecorder::new(consent_manager.clone()).await {
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
    }
}
