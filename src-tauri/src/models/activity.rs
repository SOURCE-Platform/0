// Data structures for OS activity tracking

use serde::{Deserialize, Serialize};

/// Application event representing lifecycle and focus changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppEvent {
    pub timestamp: i64,
    pub event_type: AppEventType,
    pub app_info: AppInfo,
}

/// Types of application events
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppEventType {
    Launch,
    Terminate,
    FocusGain,
    FocusLoss,
}

impl AppEventType {
    pub fn to_string(&self) -> &'static str {
        match self {
            AppEventType::Launch => "launch",
            AppEventType::Terminate => "terminate",
            AppEventType::FocusGain => "focus_gain",
            AppEventType::FocusLoss => "focus_loss",
        }
    }
}

/// Information about an application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub bundle_id: String,
    pub process_id: u32,
    pub version: Option<String>,
    pub executable_path: Option<String>,
}

impl AppInfo {
    /// Create a new AppInfo with required fields
    pub fn new(name: String, bundle_id: String, process_id: u32) -> Self {
        Self {
            name,
            bundle_id,
            process_id,
            version: None,
            executable_path: None,
        }
    }

    /// Create a full AppInfo with all optional fields
    pub fn with_details(
        name: String,
        bundle_id: String,
        process_id: u32,
        version: Option<String>,
        executable_path: Option<String>,
    ) -> Self {
        Self {
            name,
            bundle_id,
            process_id,
            version,
            executable_path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_event_type_to_string() {
        assert_eq!(AppEventType::Launch.to_string(), "launch");
        assert_eq!(AppEventType::Terminate.to_string(), "terminate");
        assert_eq!(AppEventType::FocusGain.to_string(), "focus_gain");
        assert_eq!(AppEventType::FocusLoss.to_string(), "focus_loss");
    }

    #[test]
    fn test_app_info_new() {
        let app_info = AppInfo::new(
            "Safari".to_string(),
            "com.apple.Safari".to_string(),
            12345,
        );

        assert_eq!(app_info.name, "Safari");
        assert_eq!(app_info.bundle_id, "com.apple.Safari");
        assert_eq!(app_info.process_id, 12345);
        assert!(app_info.version.is_none());
        assert!(app_info.executable_path.is_none());
    }

    #[test]
    fn test_app_info_with_details() {
        let app_info = AppInfo::with_details(
            "Safari".to_string(),
            "com.apple.Safari".to_string(),
            12345,
            Some("16.0".to_string()),
            Some("/Applications/Safari.app/Contents/MacOS/Safari".to_string()),
        );

        assert_eq!(app_info.name, "Safari");
        assert_eq!(app_info.version, Some("16.0".to_string()));
        assert!(app_info.executable_path.is_some());
    }

    #[test]
    fn test_app_event_serialization() {
        let app_info = AppInfo::new(
            "Safari".to_string(),
            "com.apple.Safari".to_string(),
            12345,
        );

        let event = AppEvent {
            timestamp: 1234567890,
            event_type: AppEventType::Launch,
            app_info,
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("launch"));
        assert!(json.contains("Safari"));
        assert!(json.contains("com.apple.Safari"));
    }
}
