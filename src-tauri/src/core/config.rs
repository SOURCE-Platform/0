use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

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
}

impl Default for Config {
    fn default() -> Self {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());

        let mut storage_path = PathBuf::from(home.clone());
        storage_path.push(".observer_data");
        storage_path.push("recordings");

        let mut retention_days = HashMap::new();
        retention_days.insert("screen".to_string(), 30);
        retention_days.insert("ocr".to_string(), 90);
        retention_days.insert("keyboard".to_string(), 30);
        retention_days.insert("mouse".to_string(), 7);

        Self {
            storage_path,
            retention_days,
            recording_quality: "Medium".to_string(),
            auto_start: false,
            motion_detection_threshold: 0.05,
            ocr_enabled: true,
            ocr_languages: vec!["eng".to_string()],
            ocr_confidence_threshold: 0.7,
            ocr_interval_seconds: 60, // Run OCR every 60 seconds
            default_recording_fps: 15,
            video_codec: "h264".to_string(),
            video_quality: "Medium".to_string(),
            hardware_acceleration: true,
            target_fps: 15,
        }
    }
}

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
        // Validate recording quality
        let valid_qualities = ["High", "Medium", "Low"];
        if !valid_qualities.contains(&self.recording_quality.as_str()) {
            return Err(format!(
                "Invalid recording quality: {}. Must be one of: High, Medium, Low",
                self.recording_quality
            )
            .into());
        }

        // Validate video quality
        if !valid_qualities.contains(&self.video_quality.as_str()) {
            return Err(format!(
                "Invalid video quality: {}. Must be one of: High, Medium, Low",
                self.video_quality
            )
            .into());
        }

        // Validate video codec
        let valid_codecs = ["h264", "H264"];
        if !valid_codecs.contains(&self.video_codec.as_str()) {
            return Err(format!(
                "Invalid video codec: {}. Must be one of: h264",
                self.video_codec
            )
            .into());
        }

        // Validate motion detection threshold
        if !(0.0..=1.0).contains(&self.motion_detection_threshold) {
            return Err(format!(
                "Invalid motion detection threshold: {}. Must be between 0.0 and 1.0",
                self.motion_detection_threshold
            )
            .into());
        }

        // Validate FPS
        if self.default_recording_fps == 0 || self.default_recording_fps > 60 {
            return Err(format!(
                "Invalid FPS: {}. Must be between 1 and 60",
                self.default_recording_fps
            )
            .into());
        }

        // Validate target FPS
        if self.target_fps == 0 || self.target_fps > 60 {
            return Err(format!(
                "Invalid target FPS: {}. Must be between 1 and 60",
                self.target_fps
            )
            .into());
        }

        // Validate retention days
        for (data_type, days) in &self.retention_days {
            if *days == 0 || *days > 3650 {
                return Err(format!(
                    "Invalid retention days for {}: {}. Must be between 1 and 3650",
                    data_type, days
                )
                .into());
            }
        }

        // Validate OCR confidence threshold
        if !(0.0..=1.0).contains(&self.ocr_confidence_threshold) {
            return Err(format!(
                "Invalid OCR confidence threshold: {}. Must be between 0.0 and 1.0",
                self.ocr_confidence_threshold
            )
            .into());
        }

        // Validate OCR interval
        if self.ocr_interval_seconds == 0 || self.ocr_interval_seconds > 3600 {
            return Err(format!(
                "Invalid OCR interval: {}. Must be between 1 and 3600 seconds",
                self.ocr_interval_seconds
            )
            .into());
        }

        // Validate OCR languages
        if self.ocr_languages.is_empty() {
            return Err("OCR languages cannot be empty".into());
        }

        Ok(())
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
