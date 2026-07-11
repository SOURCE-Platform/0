use super::config::ResourceProfile;

pub(crate) fn default_mock_data_mode() -> bool {
    true
}

pub(crate) fn default_resource_profile() -> ResourceProfile {
    ResourceProfile::Balanced
}

pub(crate) fn default_audio_microphone_enabled() -> bool {
    true
}

pub(crate) fn default_desktop_audio_gain_db() -> f32 {
    0.0
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
