use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::core::config_defaults::{default_pii_categories, default_pii_review_threshold};
use crate::core::config_validation::validate_config;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceProfile {
    Minimal,
    Balanced,
    HighFidelity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CaptureChannels {
    pub system: bool,
    pub focus: bool,
    pub visible_windows: bool,
    pub ocr: bool,
    pub keyboard: bool,
    pub mouse: bool,
    pub screen_frames: bool,
    #[serde(default)]
    pub audio_future: bool,
    #[serde(default)]
    pub camera_future: bool,
    #[serde(default)]
    pub sensor_future: bool,
}

impl Default for CaptureChannels {
    fn default() -> Self {
        Self {
            system: true,
            focus: true,
            visible_windows: true,
            ocr: true,
            keyboard: true,
            mouse: true,
            screen_frames: true,
            audio_future: false,
            camera_future: false,
            sensor_future: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PiiDetectionSettings {
    pub detect_only: bool,
    #[serde(default = "crate::core::config_defaults::default_pii_categories")]
    pub enabled_categories: Vec<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "crate::core::config_defaults::default_pii_review_threshold")]
    pub review_confidence_threshold: f32,
}

impl Default for PiiDetectionSettings {
    fn default() -> Self {
        Self {
            detect_only: true,
            enabled_categories: default_pii_categories(),
            enabled: true,
            review_confidence_threshold: default_pii_review_threshold(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewUiDefaults {
    pub default_timeline_view: String,
    pub show_inferred_labels: bool,
}

impl Default for ReviewUiDefaults {
    fn default() -> Self {
        Self {
            default_timeline_view: "today".to_string(),
            show_inferred_labels: true,
        }
    }
}

/// One custom dictionary rule: when `triggers` are heard, write `replacement`.
/// Shared shape with the speech pipeline (`multimodal::speech_provider`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustomDictionaryEntry {
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub replacement: String,
}

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    /// Where recordings are saved
    pub storage_path: PathBuf,
    /// How long to keep different data types (in days)
    pub retention_days: HashMap<String, u32>,
    /// Recording quality: "High", "Medium", or "Low"
    pub recording_quality: String,
    /// Launch on system startup
    pub auto_start: bool,
    /// Motion detection threshold (0.0-1.0, where 0.05 = 5%)
    pub motion_detection_threshold: f32,
    /// Enable OCR processing
    pub ocr_enabled: bool,
    /// OCR languages (e.g., ["eng", "spa"])
    pub ocr_languages: Vec<String>,
    /// OCR confidence threshold (0.0-1.0)
    pub ocr_confidence_threshold: f32,
    /// OCR processing interval in seconds
    pub ocr_interval_seconds: u32,
    /// Default recording frames per second
    pub default_recording_fps: u32,
    /// Video codec to use (e.g., "h264")
    pub video_codec: String,
    /// Video compression quality: "High", "Medium", or "Low"
    pub video_quality: String,
    /// Enable hardware acceleration for video encoding
    pub hardware_acceleration: bool,
    /// Target FPS for video encoding
    pub target_fps: u32,
    /// Websites to exclude from recording
    #[serde(default)]
    pub website_blacklist: Vec<String>,
    /// Application names to exclude from recording
    #[serde(default)]
    pub app_blacklist: Vec<String>,
    /// Preferred AVFoundation audio input for live microphone capture
    #[serde(default)]
    pub selected_audio_input_id: Option<String>,
    /// Capture live microphone input when the audio channel is enabled
    #[serde(default = "crate::core::config_defaults::default_audio_microphone_enabled")]
    pub audio_microphone_enabled: bool,
    /// Capture mixed desktop/app output when the audio channel is enabled
    #[serde(default)]
    pub audio_desktop_enabled: bool,
    /// Run local Parakeet transcription for detected speech.
    #[serde(default = "crate::core::config_defaults::default_audio_transcription_enabled")]
    pub audio_transcription_enabled: bool,
    /// Run speech emotion only after voice activity is detected.
    #[serde(default = "crate::core::config_defaults::default_audio_speech_emotion_enabled")]
    pub audio_speech_emotion_enabled: bool,
    /// Run local sound-event detection for captured audio.
    #[serde(default = "crate::core::config_defaults::default_audio_sound_events_enabled")]
    pub audio_sound_events_enabled: bool,
    /// Gain applied to SOURCE's desktop-audio copy, without changing macOS output volume.
    #[serde(default = "crate::core::config_defaults::default_desktop_audio_gain_db")]
    pub desktop_audio_gain_db: f32,
    /// Custom dictionary: misheard phrases mapped to preferred spellings.
    #[serde(default)]
    pub custom_dictionary: Vec<CustomDictionaryEntry>,
    /// Whether demo/mock data should be shown in the UI
    #[serde(default = "crate::core::config_defaults::default_mock_data_mode")]
    pub mock_data_mode: bool,
    /// Per-channel capture enablement
    #[serde(default)]
    pub capture_channels: CaptureChannels,
    /// Capture fidelity/resource budget
    #[serde(default = "crate::core::config_defaults::default_resource_profile")]
    pub resource_profile: ResourceProfile,
    /// PII detection and review settings
    #[serde(default)]
    pub pii_settings: PiiDetectionSettings,
    /// UI defaults for review surfaces
    #[serde(default)]
    pub review_ui_defaults: ReviewUiDefaults,
}

include!("config_default_impl.rs");

impl Config {
    /// Load configuration from file, creating with defaults if it doesn't exist
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = Self::get_config_path()?;

        if config_path.exists() {
            let contents = std::fs::read_to_string(&config_path)?;
            let config: Config = serde_json::from_str(&contents)?;
            config.validate()?;
            Ok(config)
        } else {
            // Create default config and save it
            let config = Self::default();
            config.save()?;
            Ok(config)
        }
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.validate()?;

        let config_path = Self::get_config_path()?;

        // Create parent directories if they don't exist
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Serialize and write to file with pretty formatting
        let contents = serde_json::to_string_pretty(self)?;
        std::fs::write(&config_path, contents)?;

        Ok(())
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<(), Box<dyn std::error::Error>> {
        validate_config(self)
    }

    /// Reset to default configuration
    pub fn reset() -> Result<Self, Box<dyn std::error::Error>> {
        let config = Self::default();
        config.save()?;
        Ok(config)
    }

    /// Get the configuration file path
    fn get_config_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| "Could not determine home directory")?;

        let mut path = PathBuf::from(home);
        path.push(".observer_data");
        path.push("config");
        path.push("settings.json");

        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn get_test_config_path() -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push("observer_test_config");
        path.push("settings.json");
        path
    }

    fn cleanup_test_config() {
        let path = get_test_config_path();
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.recording_quality, "Medium");
        assert_eq!(config.auto_start, false);
        assert_eq!(config.motion_detection_threshold, 0.05);
        assert_eq!(config.ocr_enabled, true);
        assert_eq!(config.default_recording_fps, 15);
        assert_eq!(config.video_codec, "h264");
        assert_eq!(config.video_quality, "Medium");
        assert_eq!(config.hardware_acceleration, true);
        assert_eq!(config.target_fps, 15);
        assert_eq!(config.retention_days.get("screen"), Some(&30));
        assert_eq!(config.retention_days.get("ocr"), Some(&90));
    }

    #[test]
    fn test_config_validation() {
        let mut config = Config::default();

        // Valid config should pass
        assert!(config.validate().is_ok());

        // Invalid recording quality
        config.recording_quality = "Invalid".to_string();
        assert!(config.validate().is_err());
        config.recording_quality = "Medium".to_string();

        // Invalid motion threshold
        config.motion_detection_threshold = 1.5;
        assert!(config.validate().is_err());
        config.motion_detection_threshold = 0.05;

        // Invalid FPS
        config.default_recording_fps = 0;
        assert!(config.validate().is_err());
        config.default_recording_fps = 100;
        assert!(config.validate().is_err());
        config.default_recording_fps = 15;

        // Invalid retention days
        config.retention_days.insert("test".to_string(), 0);
        assert!(config.validate().is_err());
        config.retention_days.insert("test".to_string(), 5000);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_reset_config() {
        cleanup_test_config();

        let config = Config::reset().unwrap();
        assert_eq!(config, Config::default());

        cleanup_test_config();
    }
}
