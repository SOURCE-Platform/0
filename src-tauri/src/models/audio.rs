// Data models for audio capture, transcription, diarization, and emotion detection

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ==============================================================================
// Audio Recording
// ==============================================================================

/// Audio recording session metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioRecording {
    pub id: String,
    pub session_id: String,
    pub start_timestamp: i64,
    pub end_timestamp: Option<i64>,
    pub sample_rate: u32,          // Hz (e.g., 48000)
    pub channels: u16,             // 1 = mono, 2 = stereo
    pub bit_depth: u16,            // 16 or 24
    pub file_path: PathBuf,
    pub file_size_bytes: Option<u64>,
    pub codec: AudioCodec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioCodec {
    Wav,   // Uncompressed
    Aac,   // Compressed (MP4)
    Mp3,   // Compressed
    Opus,  // Compressed (Ogg)
}

impl AudioCodec {
    pub fn to_string(&self) -> &'static str {
        match self {
            AudioCodec::Wav => "wav",
            AudioCodec::Aac => "aac",
            AudioCodec::Mp3 => "mp3",
            AudioCodec::Opus => "opus",
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            AudioCodec::Wav => "wav",
            AudioCodec::Aac => "m4a",
            AudioCodec::Mp3 => "mp3",
            AudioCodec::Opus => "ogg",
        }
    }
}

// ==============================================================================
// Audio Source Separation
// ==============================================================================

/// Result of audio source separation (Demucs output)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSourceSeparation {
    pub id: String,
    pub recording_id: String,
    pub timestamp: i64,
    pub duration_ms: u64,
    pub sources: SeparatedSources,
    pub classification: SourceClassification,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeparatedSources {
    pub vocals_path: Option<PathBuf>,      // User speech + other vocals
    pub music_path: Option<PathBuf>,       // Music/instrumental
    pub bass_path: Option<PathBuf>,        // Bass frequencies
    pub other_path: Option<PathBuf>,       // Sound effects, ambient
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceClassification {
    pub has_user_speech: bool,
    pub has_system_audio: bool,
    pub system_audio_type: Option<SystemAudioType>,
    pub confidence: f32,
    pub speech_probability: f32,    // 0.0 to 1.0
    pub music_probability: f32,     // 0.0 to 1.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemAudioType {
    Music,
    Video,
    Game,
    Notification,
    VoiceCall,
    Other,
}

impl SystemAudioType {
    pub fn to_string(&self) -> &'static str {
        match self {
            SystemAudioType::Music => "music",
            SystemAudioType::Video => "video",
            SystemAudioType::Game => "game",
            SystemAudioType::Notification => "notification",
            SystemAudioType::VoiceCall => "voice_call",
            SystemAudioType::Other => "other",
        }
    }
}

// ==============================================================================
// Audio Devices
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub device_type: AudioDeviceType,
    pub is_default: bool,
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioDeviceType {
    Microphone,
    SystemLoopback,  // System audio capture
    LineIn,
    Other,
}

impl AudioDeviceType {
    pub fn to_string(&self) -> &'static str {
        match self {
            AudioDeviceType::Microphone => "microphone",
            AudioDeviceType::SystemLoopback => "system_loopback",
            AudioDeviceType::LineIn => "line_in",
            AudioDeviceType::Other => "other",
        }
    }
}

// ==============================================================================
// Speech Transcription (Whisper)
// ==============================================================================

/// Speech transcription result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub id: String,
    pub session_id: String,
    pub recording_id: String,
    pub start_timestamp: i64,      // Session timestamp (ms)
    pub end_timestamp: i64,        // Session timestamp (ms)
    pub text: String,
    pub language: String,          // ISO 639-1 code (e.g., "en", "es")
    pub confidence: f32,           // 0.0 to 1.0
    pub speaker_id: Option<String>, // From diarization
    pub words: Vec<WordTimestamp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordTimestamp {
    pub word: String,
    pub start: i64,    // Offset from segment start (ms)
    pub end: i64,      // Offset from segment start (ms)
    pub confidence: f32,
}

// ==============================================================================
// Speaker Diarization (pyannote.audio)
// ==============================================================================

/// Speaker segment from diarization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerSegment {
    pub id: String,
    pub recording_id: String,
    pub speaker_id: String,        // "SPEAKER_00", "SPEAKER_01", etc.
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub confidence: f32,
    pub embedding: Option<Vec<f32>>, // Voice embedding vector (512-dim)
}

/// Speaker information (aggregated)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerInfo {
    pub speaker_id: String,
    pub session_id: String,
    pub total_speaking_time_ms: u64,
    pub segment_count: u32,
    pub average_confidence: f32,
    pub is_primary_user: bool,     // Heuristic: most speaking time
}

// ==============================================================================
// Emotion Detection
// ==============================================================================

/// Speech emotion detection result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmotionResult {
    pub id: String,
    pub session_id: String,
    pub recording_id: String,
    pub timestamp: i64,
    pub speaker_id: Option<String>,
    pub emotion: Emotion,
    pub confidence: f32,
    pub valence: f32,              // Positive/negative sentiment (-1.0 to 1.0)
    pub arousal: f32,              // Intensity/energy (0.0 to 1.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Emotion {
    Neutral,
    Happy,
    Sad,
    Angry,
    Fearful,
    Surprised,
    Disgusted,
}

impl Emotion {
    pub fn to_string(&self) -> &'static str {
        match self {
            Emotion::Neutral => "neutral",
            Emotion::Happy => "happy",
            Emotion::Sad => "sad",
            Emotion::Angry => "angry",
            Emotion::Fearful => "fearful",
            Emotion::Surprised => "surprised",
            Emotion::Disgusted => "disgusted",
        }
    }

    pub fn from_string(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "neutral" => Some(Emotion::Neutral),
            "happy" => Some(Emotion::Happy),
            "sad" => Some(Emotion::Sad),
            "angry" => Some(Emotion::Angry),
            "fearful" | "fear" => Some(Emotion::Fearful),
            "surprised" | "surprise" => Some(Emotion::Surprised),
            "disgusted" | "disgust" => Some(Emotion::Disgusted),
            _ => None,
        }
    }
}

/// Aggregated emotion statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmotionStatistics {
    pub session_id: String,
    pub speaker_id: Option<String>,
    pub total_detections: u64,
    pub emotion_distribution: Vec<(String, u32)>, // (emotion, count)
    pub average_valence: f32,
    pub average_arousal: f32,
    pub dominant_emotion: Option<String>,
}

// ==============================================================================
// DTOs (Data Transfer Objects for Tauri)
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDeviceDto {
    pub id: String,
    pub name: String,
    pub device_type: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegmentDto {
    pub timestamp: i64,
    pub end_timestamp: i64,
    pub text: String,
    pub language: String,
    pub speaker_id: Option<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerSegmentDto {
    pub speaker_id: String,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmotionResultDto {
    pub timestamp: i64,
    pub speaker_id: Option<String>,
    pub emotion: String,
    pub confidence: f32,
    pub valence: f32,
    pub arousal: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSourceDto {
    pub timestamp: i64,
    pub duration_ms: u64,
    pub has_user_speech: bool,
    pub has_system_audio: bool,
    pub system_audio_type: Option<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSearchResult {
    pub segment_id: String,
    pub timestamp: i64,
    pub text: String,
    pub speaker_id: Option<String>,
    pub relevance_score: f32,
}

// ==============================================================================
// Configuration
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub enable_microphone: bool,
    pub enable_system_audio: bool,
    pub enable_source_separation: bool,
    pub enable_transcription: bool,
    pub enable_diarization: bool,
    pub enable_emotion_detection: bool,

    pub sample_rate: u32,              // Default: 48000 Hz
    pub channels: u16,                 // Default: 2 (stereo)
    pub codec: AudioCodec,             // Default: AAC

    pub microphone_device_id: Option<String>,
    pub system_device_id: Option<String>,

    pub whisper_model_size: WhisperModelSize,
    pub transcription_language: Option<String>, // ISO 639-1 code, None = auto-detect

    pub min_speech_duration_ms: u64,   // Minimum segment duration (default: 500ms)
    pub min_silence_duration_ms: u64,  // Silence threshold for segmentation (default: 300ms)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhisperModelSize {
    Tiny,      // Fastest, least accurate (~39M params)
    Base,      // Fast, decent accuracy (~74M params)
    Small,     // Balanced (~244M params)
    Medium,    // Slower, more accurate (~769M params)
    Large,     // Slowest, most accurate (~1550M params)
    LargeV3,   // Latest large model
}

impl WhisperModelSize {
    pub fn to_string(&self) -> &'static str {
        match self {
            WhisperModelSize::Tiny => "tiny",
            WhisperModelSize::Base => "base",
            WhisperModelSize::Small => "small",
            WhisperModelSize::Medium => "medium",
            WhisperModelSize::Large => "large",
            WhisperModelSize::LargeV3 => "large-v3",
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            enable_microphone: true,
            enable_system_audio: true,
            enable_source_separation: true,
            enable_transcription: true,
            enable_diarization: true,
            enable_emotion_detection: true,
            sample_rate: 48000,
            channels: 2,
            codec: AudioCodec::Aac,
            microphone_device_id: None,
            system_device_id: None,
            whisper_model_size: WhisperModelSize::Base,
            transcription_language: None,
            min_speech_duration_ms: 500,
            min_silence_duration_ms: 300,
        }
    }
}

// ==============================================================================
// Error Types
// ==============================================================================

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("Audio capture not initialized")]
    NotInitialized,

    #[error("Audio capture already running")]
    AlreadyRunning,

    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("Invalid audio format: {0}")]
    InvalidFormat(String),

    #[error("Capture failed: {0}")]
    CaptureFailed(String),

    #[error("Model loading failed: {0}")]
    ModelLoadFailed(String),

    #[error("Transcription failed: {0}")]
    TranscriptionFailed(String),

    #[error("Diarization failed: {0}")]
    DiarizationFailed(String),

    #[error("Source separation failed: {0}")]
    SeparationFailed(String),

    #[error("Emotion detection failed: {0}")]
    EmotionDetectionFailed(String),

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("File I/O error: {0}")]
    IoError(String),

    #[error("Not supported on this platform")]
    NotSupported,
}

pub type AudioResult<T> = Result<T, AudioError>;

// ==============================================================================
// Real-time Events
// ==============================================================================

/// Events emitted during audio processing for real-time updates
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AudioEvent {
    RecordingStarted {
        recording_id: String,
    },
    RecordingStopped {
        recording_id: String,
    },
    TranscriptSegment {
        segment: TranscriptSegmentDto,
    },
    SpeakerDetected {
        speaker_id: String,
        timestamp: i64,
    },
    EmotionDetected {
        emotion: EmotionResultDto,
    },
    AudioSourceClassified {
        source: AudioSourceDto,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_codec_extension() {
        assert_eq!(AudioCodec::Wav.extension(), "wav");
        assert_eq!(AudioCodec::Aac.extension(), "m4a");
        assert_eq!(AudioCodec::Mp3.extension(), "mp3");
        assert_eq!(AudioCodec::Opus.extension(), "ogg");
    }

    #[test]
    fn test_emotion_from_string() {
        assert_eq!(Emotion::from_string("happy"), Some(Emotion::Happy));
        assert_eq!(Emotion::from_string("ANGRY"), Some(Emotion::Angry));
        assert_eq!(Emotion::from_string("fear"), Some(Emotion::Fearful));
        assert_eq!(Emotion::from_string("invalid"), None);
    }

    #[test]
    fn test_audio_config_default() {
        let config = AudioConfig::default();
        assert_eq!(config.sample_rate, 48000);
        assert_eq!(config.channels, 2);
        assert!(config.enable_transcription);
        assert!(config.enable_diarization);
        assert!(config.enable_emotion_detection);
    }

    #[test]
    fn test_whisper_model_size() {
        assert_eq!(WhisperModelSize::Tiny.to_string(), "tiny");
        assert_eq!(WhisperModelSize::LargeV3.to_string(), "large-v3");
    }
}
