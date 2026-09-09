use super::config::ResourceProfile;

pub(crate) fn default_resource_profile() -> ResourceProfile {
    ResourceProfile::Balanced
}

pub(crate) fn default_audio_microphone_enabled() -> bool {
    true
}

pub(crate) fn default_desktop_audio_gain_db() -> f32 {
    0.0
}

pub(crate) fn default_audio_transcription_enabled() -> bool {
    true
}

pub(crate) fn default_audio_speech_emotion_enabled() -> bool {
    true
}

pub(crate) fn default_audio_sound_events_enabled() -> bool {
    true
}

pub(crate) fn default_pii_categories() -> Vec<String> {
    vec![
        "email".to_string(),
        "phone".to_string(),
        "government_id".to_string(),
        "credit_card".to_string(),
        "ip_address".to_string(),
    ]
}

pub(crate) fn default_pii_review_threshold() -> f32 {
    0.6
}

pub(crate) fn default_mobile_enabled() -> bool {
    true
}

pub(crate) fn default_mobile_port() -> u16 {
    8787
}

pub(crate) fn default_mobile_clip_retention_days() -> u32 {
    90
}
