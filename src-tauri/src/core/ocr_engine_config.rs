use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrConfig {
    pub languages: Vec<String>,
    pub psm: u32,
    pub oem: u32,
    pub dpi: u32,
    pub confidence_threshold: f32,
    pub preprocess_enabled: bool,
    pub contrast_factor: f32,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            languages: vec!["eng".to_string()],
            psm: 3,
            oem: 3,
            dpi: 300,
            confidence_threshold: 0.6,
            preprocess_enabled: true,
            contrast_factor: 1.5,
        }
    }
}

impl OcrConfig {
    pub fn with_languages(languages: Vec<String>) -> Self {
        Self {
            languages,
            ..Default::default()
        }
    }

    pub fn for_screenshots() -> Self {
        Self {
            languages: vec!["eng".to_string()],
            psm: 3,
            oem: 3,
            dpi: 144,
            confidence_threshold: 0.7,
            preprocess_enabled: true,
            contrast_factor: 1.3,
        }
    }

    pub fn for_documents() -> Self {
        Self {
            languages: vec!["eng".to_string()],
            psm: 1,
            oem: 3,
            dpi: 300,
            confidence_threshold: 0.8,
            preprocess_enabled: true,
            contrast_factor: 1.5,
        }
    }
}
