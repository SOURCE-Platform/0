use super::config::Config;

pub(crate) fn validate_config(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let valid_qualities = ["High", "Medium", "Low"];
    if !valid_qualities.contains(&config.recording_quality.as_str()) {
        return Err(format!(
            "Invalid recording quality: {}. Must be one of: High, Medium, Low",
            config.recording_quality
        )
        .into());
    }

    if !valid_qualities.contains(&config.video_quality.as_str()) {
        return Err(format!(
            "Invalid video quality: {}. Must be one of: High, Medium, Low",
            config.video_quality
        )
        .into());
    }

    let valid_codecs = ["h264", "H264"];
    if !valid_codecs.contains(&config.video_codec.as_str()) {
        return Err(format!(
            "Invalid video codec: {}. Must be one of: h264",
            config.video_codec
        )
        .into());
    }

    if !(0.0..=1.0).contains(&config.motion_detection_threshold) {
        return Err(format!(
            "Invalid motion detection threshold: {}. Must be between 0.0 and 1.0",
            config.motion_detection_threshold
        )
        .into());
    }

    if config.default_recording_fps == 0 || config.default_recording_fps > 60 {
        return Err(format!(
            "Invalid FPS: {}. Must be between 1 and 60",
            config.default_recording_fps
        )
        .into());
    }

    if config.target_fps == 0 || config.target_fps > 60 {
        return Err(format!(
            "Invalid target FPS: {}. Must be between 1 and 60",
            config.target_fps
        )
        .into());
    }

    for (data_type, days) in &config.retention_days {
        if *days == 0 || *days > 3650 {
            return Err(format!(
                "Invalid retention days for {}: {}. Must be between 1 and 3650",
                data_type, days
            )
            .into());
        }
    }

    if !(0.0..=1.0).contains(&config.ocr_confidence_threshold) {
        return Err(format!(
            "Invalid OCR confidence threshold: {}. Must be between 0.0 and 1.0",
            config.ocr_confidence_threshold
        )
        .into());
    }

    if config.ocr_interval_seconds == 0 || config.ocr_interval_seconds > 3600 {
        return Err(format!(
            "Invalid OCR interval: {}. Must be between 1 and 3600 seconds",
            config.ocr_interval_seconds
        )
        .into());
    }

    if config.ocr_languages.is_empty() {
        return Err("OCR languages cannot be empty".into());
    }

    if !(0.0..=24.0).contains(&config.desktop_audio_gain_db) {
        return Err(format!(
            "Invalid desktop audio gain: {}. Must be between 0 and 24 dB",
            config.desktop_audio_gain_db
        )
        .into());
    }

    if !(0.0..=1.0).contains(&config.pii_settings.review_confidence_threshold) {
        return Err(format!(
            "Invalid PII review confidence threshold: {}. Must be between 0.0 and 1.0",
            config.pii_settings.review_confidence_threshold
        )
        .into());
    }

    Ok(())
}
