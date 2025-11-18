use crate::core::consent::ConsentManager;
use crate::core::database::Database;
use crate::models::audio::{
    AudioRecording, AudioConfig, AudioCodec, AudioError, AudioResult,
    AudioDevice, AudioDeviceType, TranscriptSegment, SpeakerSegment,
    EmotionResult, AudioSourceSeparation, WordTimestamp,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

// ==============================================================================
// Database Models
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AudioRecordingRecord {
    pub id: String,
    pub session_id: String,
    pub start_timestamp: i64,
    pub end_timestamp: Option<i64>,
    pub sample_rate: i64,
    pub channels: i64,
    pub bit_depth: i64,
    pub file_path: String,
    pub file_size_bytes: Option<i64>,
    pub codec: String,
    pub created_at: i64,
}

// ==============================================================================
// Audio Recorder
// ==============================================================================

pub struct AudioRecorder {
    db: Arc<Database>,
    consent_manager: Arc<ConsentManager>,
    config: Arc<RwLock<AudioConfig>>,
    current_session_id: Arc<RwLock<Option<String>>>,
    current_recording_id: Arc<RwLock<Option<String>>>,
    is_recording: Arc<RwLock<bool>>,
    storage_path: PathBuf,
}

impl AudioRecorder {
    pub async fn new(
        consent_manager: Arc<ConsentManager>,
        db: Arc<Database>,
        storage_path: PathBuf,
    ) -> AudioResult<Self> {
        std::fs::create_dir_all(&storage_path)
            .map_err(|e| AudioError::IoError(e.to_string()))?;

        Ok(Self {
            db,
            consent_manager,
            config: Arc::new(RwLock::new(AudioConfig::default())),
            current_session_id: Arc::new(RwLock::new(None)),
            current_recording_id: Arc::new(RwLock::new(None)),
            is_recording: Arc::new(RwLock::new(false)),
            storage_path,
        })
    }

    /// Start audio recording for a session
    pub async fn start_recording(&self, session_id: String, config: AudioConfig) -> AudioResult<String> {
        // Check if already recording
        let mut is_recording = self.is_recording.write().await;
        if *is_recording {
            return Err(AudioError::AlreadyRunning);
        }

        // Check consent
        // TODO: Add ConsentFeature::AudioRecording to consent enum

        // Store session ID and config
        *self.current_session_id.write().await = Some(session_id.clone());
        *self.config.write().await = config.clone();

        // Create recording record
        let recording_id = Uuid::new_v4().to_string();
        let start_timestamp = chrono::Utc::now().timestamp_millis();

        // Create file path
        let filename = format!("{}.{}", recording_id, config.codec.extension());
        let file_path = self.storage_path.join(&session_id).join(&filename);

        // Create session directory
        std::fs::create_dir_all(file_path.parent().unwrap())
            .map_err(|e| AudioError::IoError(e.to_string()))?;

        // Store in database
        let recording = AudioRecording {
            id: recording_id.clone(),
            session_id: session_id.clone(),
            start_timestamp,
            end_timestamp: None,
            sample_rate: config.sample_rate,
            channels: config.channels,
            bit_depth: 16, // Default
            file_path: file_path.clone(),
            file_size_bytes: None,
            codec: config.codec,
        };

        Self::store_recording(&self.db, &recording).await?;

        *self.current_recording_id.write().await = Some(recording_id.clone());
        *is_recording = true;

        // TODO: Start actual audio capture
        // For now, this is a placeholder
        println!("Started audio recording {} for session {}", recording_id, session_id);

        Ok(recording_id)
    }

    /// Stop audio recording
    pub async fn stop_recording(&self) -> AudioResult<()> {
        let mut is_recording = self.is_recording.write().await;
        if !*is_recording {
            return Ok(());
        }

        let recording_id = self.current_recording_id.read().await.clone();
        if let Some(recording_id) = recording_id {
            let end_timestamp = chrono::Utc::now().timestamp_millis();

            // Update recording end timestamp
            Self::update_recording_end_time(&self.db, &recording_id, end_timestamp).await?;

            // TODO: Stop actual audio capture
            println!("Stopped audio recording {}", recording_id);
        }

        *is_recording = false;
        *self.current_session_id.write().await = None;
        *self.current_recording_id.write().await = None;

        Ok(())
    }

    /// Get available audio devices
    pub async fn get_devices() -> AudioResult<Vec<AudioDevice>> {
        // Use platform-specific audio capture to enumerate devices
        #[cfg(target_os = "macos")]
        {
            use crate::platform::audio::MacOSAudioCapture;
            MacOSAudioCapture::enumerate_devices()
        }

        #[cfg(target_os = "windows")]
        {
            use crate::platform::audio::WindowsAudioCapture;
            WindowsAudioCapture::enumerate_devices()
        }

        #[cfg(target_os = "linux")]
        {
            use crate::platform::audio::LinuxAudioCapture;
            LinuxAudioCapture::enumerate_devices()
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            Err(AudioError::NotSupported)
        }
    }

    /// Store audio recording in database
    async fn store_recording(db: &Arc<Database>, recording: &AudioRecording) -> AudioResult<()> {
        let pool = db.pool();
        let created_at = chrono::Utc::now().timestamp_millis();

        sqlx::query(
            "INSERT INTO audio_recordings (
                id, session_id, start_timestamp, end_timestamp,
                sample_rate, channels, bit_depth, file_path, file_size_bytes, codec, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&recording.id)
        .bind(&recording.session_id)
        .bind(recording.start_timestamp)
        .bind(recording.end_timestamp)
        .bind(recording.sample_rate as i64)
        .bind(recording.channels as i64)
        .bind(recording.bit_depth as i64)
        .bind(recording.file_path.to_str().unwrap())
        .bind(recording.file_size_bytes.map(|s| s as i64))
        .bind(recording.codec.to_string())
        .bind(created_at)
        .execute(pool)
        .await
        .map_err(|e| AudioError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    /// Update recording end time
    async fn update_recording_end_time(db: &Arc<Database>, recording_id: &str, end_timestamp: i64) -> AudioResult<()> {
        let pool = db.pool();

        sqlx::query(
            "UPDATE audio_recordings SET end_timestamp = ? WHERE id = ?"
        )
        .bind(end_timestamp)
        .bind(recording_id)
        .execute(pool)
        .await
        .map_err(|e| AudioError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    /// Get audio recording by ID
    pub async fn get_recording(&self, recording_id: &str) -> AudioResult<Option<AudioRecording>> {
        let pool = self.db.pool();

        let record: Option<AudioRecordingRecord> = sqlx::query_as(
            "SELECT * FROM audio_recordings WHERE id = ?"
        )
        .bind(recording_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AudioError::DatabaseError(e.to_string()))?;

        Ok(record.map(|r| AudioRecording {
            id: r.id,
            session_id: r.session_id,
            start_timestamp: r.start_timestamp,
            end_timestamp: r.end_timestamp,
            sample_rate: r.sample_rate as u32,
            channels: r.channels as u16,
            bit_depth: r.bit_depth as u16,
            file_path: PathBuf::from(r.file_path),
            file_size_bytes: r.file_size_bytes.map(|s| s as u64),
            codec: match r.codec.as_str() {
                "wav" => AudioCodec::Wav,
                "aac" => AudioCodec::Aac,
                "mp3" => AudioCodec::Mp3,
                "opus" => AudioCodec::Opus,
                _ => AudioCodec::Aac,
            },
        }))
    }

    /// Get recordings for a session
    pub async fn get_session_recordings(&self, session_id: &str) -> AudioResult<Vec<AudioRecording>> {
        let pool = self.db.pool();

        let records: Vec<AudioRecordingRecord> = sqlx::query_as(
            "SELECT * FROM audio_recordings WHERE session_id = ? ORDER BY start_timestamp ASC"
        )
        .bind(session_id)
        .fetch_all(pool)
        .await
        .map_err(|e| AudioError::DatabaseError(e.to_string()))?;

        Ok(records.into_iter().map(|r| AudioRecording {
            id: r.id,
            session_id: r.session_id,
            start_timestamp: r.start_timestamp,
            end_timestamp: r.end_timestamp,
            sample_rate: r.sample_rate as u32,
            channels: r.channels as u16,
            bit_depth: r.bit_depth as u16,
            file_path: PathBuf::from(r.file_path),
            file_size_bytes: r.file_size_bytes.map(|s| s as u64),
            codec: match r.codec.as_str() {
                "wav" => AudioCodec::Wav,
                "aac" => AudioCodec::Aac,
                "mp3" => AudioCodec::Mp3,
                "opus" => AudioCodec::Opus,
                _ => AudioCodec::Aac,
            },
        }).collect())
    }
}
