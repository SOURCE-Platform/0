impl Default for Config {
    fn default() -> Self {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        let storage_path = PathBuf::from(home).join(".observer_data").join("recordings");
        let retention_days = HashMap::from([
            ("screen".to_string(), 30),
            ("ocr".to_string(), 90),
            ("keyboard".to_string(), 30),
            ("mouse".to_string(), 7),
        ]);

        Self {
            storage_path,
            retention_days,
            recording_quality: "Medium".to_string(),
            auto_start: false,
            motion_detection_threshold: 0.05,
            ocr_enabled: false,
            ocr_languages: vec!["eng".to_string()],
            ocr_confidence_threshold: 0.7,
            ocr_interval_seconds: 60,
            default_recording_fps: 15,
            video_codec: "h264".to_string(),
            video_quality: "Medium".to_string(),
            hardware_acceleration: true,
            target_fps: 15,
            website_blacklist: Vec::new(),
            app_blacklist: Vec::new(),
            selected_audio_input_id: None,
            audio_microphone_enabled: true,
            audio_desktop_enabled: false,
            audio_transcription_enabled: crate::core::config_defaults::default_audio_transcription_enabled(),
            audio_speech_emotion_enabled: crate::core::config_defaults::default_audio_speech_emotion_enabled(),
            audio_sound_events_enabled: crate::core::config_defaults::default_audio_sound_events_enabled(),
            desktop_audio_gain_db: crate::core::config_defaults::default_desktop_audio_gain_db(),
            custom_dictionary: Vec::new(),
            capture_channels: CaptureChannels::default(),
            resource_profile: ResourceProfile::Balanced,
            pii_settings: PiiDetectionSettings::default(),
            review_ui_defaults: ReviewUiDefaults::default(),
            mobile_enabled: crate::core::config_defaults::default_mobile_enabled(),
            mobile_port: crate::core::config_defaults::default_mobile_port(),
            mobile_clip_retention_days:
                crate::core::config_defaults::default_mobile_clip_retention_days(),
            mobile_agent_prompts_enabled: false,
        }
    }
}
