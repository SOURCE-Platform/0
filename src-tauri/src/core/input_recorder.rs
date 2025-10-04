use crate::core::consent::{ConsentManager, Feature};
use crate::core::database::Database;
use crate::core::input_storage::InputStorage;
use crate::models::input::{KeyboardEvent, MouseEvent};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::RwLock;

// Platform-specific keyboard listener
#[cfg(target_os = "macos")]
use crate::platform::input::MacOSKeyboardListener as PlatformKeyboardListener;
#[cfg(target_os = "windows")]
use crate::platform::input::WindowsKeyboardListener as PlatformKeyboardListener;
#[cfg(target_os = "linux")]
use crate::platform::input::LinuxKeyboardListener as PlatformKeyboardListener;

// Platform-specific mouse listener
#[cfg(target_os = "macos")]
use crate::platform::input::MacOSMouseListener as PlatformMouseListener;
#[cfg(target_os = "windows")]
use crate::platform::input::WindowsMouseListener as PlatformMouseListener;
#[cfg(target_os = "linux")]
use crate::platform::input::LinuxMouseListener as PlatformMouseListener;

// ==============================================================================
// Input Recorder
// ==============================================================================

pub struct InputRecorder {
    consent_manager: Arc<ConsentManager>,
    storage: Arc<InputStorage>,
    keyboard_listener: Arc<RwLock<Option<PlatformKeyboardListener>>>,
    mouse_listener: Arc<RwLock<Option<PlatformMouseListener>>>,
    current_session_id: Arc<RwLock<Option<String>>>,
    is_recording: Arc<RwLock<bool>>,
}

impl InputRecorder {
    pub async fn new(
        consent_manager: Arc<ConsentManager>,
        db: Arc<Database>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let storage = Arc::new(InputStorage::new(db).await?);

        Ok(Self {
            consent_manager,
            storage,
            keyboard_listener: Arc::new(RwLock::new(None)),
            mouse_listener: Arc::new(RwLock::new(None)),
            current_session_id: Arc::new(RwLock::new(None)),
            is_recording: Arc::new(RwLock::new(false)),
        })
    }

    pub async fn start_recording(
        &self,
        session_id: String,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Check if already recording
        let mut is_recording = self.is_recording.write().await;
        if *is_recording {
            return Err("Already recording input events".into());
        }

        // Check consents
        let has_keyboard_consent = self
            .consent_manager
            .is_consent_granted(Feature::KeyboardRecording)
            .await
            .unwrap_or(false);

        let has_mouse_consent = self
            .consent_manager
            .is_consent_granted(Feature::MouseRecording)
            .await
            .unwrap_or(false);

        if !has_keyboard_consent && !has_mouse_consent {
            return Err("No input recording consent granted (need keyboard or mouse)".into());
        }

        // Store session ID
        *self.current_session_id.write().await = Some(session_id.clone());

        // Start keyboard listener if consented
        if has_keyboard_consent {
            let (listener, keyboard_rx) =
                PlatformKeyboardListener::new(self.consent_manager.clone())?;

            // Start listening
            // Note: This is async but we're not awaiting to avoid blocking
            // The actual implementation would handle this properly

            *self.keyboard_listener.write().await = Some(listener);

            // Spawn task to process keyboard events
            let storage = self.storage.clone();
            let session_id_clone = session_id.clone();
            let is_recording_clone = self.is_recording.clone();

            tokio::spawn(async move {
                Self::process_keyboard_events(
                    keyboard_rx,
                    storage,
                    session_id_clone,
                    is_recording_clone,
                )
                .await;
            });
        }

        // Start mouse listener if consented
        if has_mouse_consent {
            let (listener, mouse_rx) = PlatformMouseListener::new(self.consent_manager.clone())?;

            // Start listening
            // Note: Similar to keyboard, actual implementation would handle async properly

            *self.mouse_listener.write().await = Some(listener);

            // Spawn task to process mouse events
            let storage = self.storage.clone();
            let session_id_clone = session_id.clone();
            let is_recording_clone = self.is_recording.clone();

            tokio::spawn(async move {
                Self::process_mouse_events(mouse_rx, storage, session_id_clone, is_recording_clone)
                    .await;
            });
        }

        // Start periodic buffer flush
        let storage_clone = self.storage.clone();
        let is_recording_clone = self.is_recording.clone();

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;

                // Check if still recording
                if !*is_recording_clone.read().await {
                    break;
                }

                let _ = storage_clone.flush_buffers().await;
            }
        });

        *is_recording = true;

        Ok(())
    }

    pub async fn stop_recording(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Mark as not recording
        *self.is_recording.write().await = false;

        // Stop keyboard listener
        if let Some(mut listener) = self.keyboard_listener.write().await.take() {
            listener.stop_listening().await?;
        }

        // Stop mouse listener
        if let Some(mut listener) = self.mouse_listener.write().await.take() {
            listener.stop_listening().await?;
        }

        // Final buffer flush
        self.storage.flush_buffers().await?;

        // Clear session ID
        *self.current_session_id.write().await = None;

        Ok(())
    }

    pub async fn is_recording(&self) -> bool {
        *self.is_recording.read().await
    }

    async fn process_keyboard_events(
        mut rx: mpsc::UnboundedReceiver<KeyboardEvent>,
        storage: Arc<InputStorage>,
        session_id: String,
        is_recording: Arc<RwLock<bool>>,
    ) {
        while let Some(event) = rx.recv().await {
            // Check if still recording
            if !*is_recording.read().await {
                break;
            }

            // Store event (ignore errors to prevent blocking)
            let _ = storage
                .store_keyboard_event(session_id.clone(), event)
                .await;
        }
    }

    async fn process_mouse_events(
        mut rx: mpsc::UnboundedReceiver<MouseEvent>,
        storage: Arc<InputStorage>,
        session_id: String,
        is_recording: Arc<RwLock<bool>>,
    ) {
        while let Some(event) = rx.recv().await {
            // Check if still recording
            if !*is_recording.read().await {
                break;
            }

            // Store event (ignore errors to prevent blocking)
            let _ = storage.store_mouse_event(session_id.clone(), event).await;
        }
    }

    // ==============================================================================
    // Cleanup
    // ==============================================================================

    pub async fn cleanup_old_events(
        &self,
        retention_days: u32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.storage.cleanup_old_events(retention_days).await
    }
}
