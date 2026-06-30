use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub idle_timeout_minutes: u32,
    pub minimum_session_duration_minutes: u32,
    pub auto_end_on_sleep: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            idle_timeout_minutes: 30,
            minimum_session_duration_minutes: 5,
            auto_end_on_sleep: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionType {
    Work,
    Communication,
    Research,
    Entertainment,
    Development,
    Unknown,
}

impl SessionType {
    pub fn to_string(&self) -> &'static str {
        match self {
            SessionType::Work => "work",
            SessionType::Communication => "communication",
            SessionType::Research => "research",
            SessionType::Entertainment => "entertainment",
            SessionType::Development => "development",
            SessionType::Unknown => "unknown",
        }
    }

    pub fn from_string(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "work" => SessionType::Work,
            "communication" => SessionType::Communication,
            "research" => SessionType::Research,
            "entertainment" => SessionType::Entertainment,
            "development" => SessionType::Development,
            _ => SessionType::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub start_timestamp: i64,
    pub end_timestamp: Option<i64>,
    pub session_type: Option<String>,
    pub device_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetrics {
    pub total_duration_ms: u64,
    pub active_duration_ms: u64,
    pub idle_duration_ms: u64,
    pub app_switches: u32,
    pub unique_apps: u32,
    pub most_used_app: String,
    pub productivity_score: f32,
}

#[derive(Debug, Clone)]
pub struct AppUsageInfo {
    pub app_name: String,
    pub focus_duration_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerEvent {
    WillSleep,
    DidWake,
    BatteryLow,
}
