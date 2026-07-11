use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioMeterReadingDto {
    pub source_id: String,
    pub source_name: String,
    pub level: f32,
    pub status: String,
    pub message: Option<String>,
    pub sampled_at: i64,
}

impl AudioMeterReadingDto {
    pub(crate) fn active(source_id: String, source_name: String, level: f32) -> Self {
        Self {
            source_id,
            source_name,
            level,
            status: "active".to_string(),
            message: None,
            sampled_at: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub(crate) fn unavailable(
        source_id: impl Into<String>,
        source_name: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            source_name: source_name.into(),
            level: 0.0,
            status: "unavailable".to_string(),
            message: Some(message.into()),
            sampled_at: chrono::Utc::now().timestamp_millis(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioMetersDto {
    pub microphone: AudioMeterReadingDto,
    pub desktop: AudioMeterReadingDto,
}
