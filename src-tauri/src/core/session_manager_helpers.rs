use std::time::Duration;
use tokio::sync::mpsc;

use super::session_manager_types::{AppUsageInfo, PowerEvent, SessionType};

pub struct IdleDetector;

impl IdleDetector {
    pub async fn get_idle_time() -> Result<Duration, Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(target_os = "macos")]
        {
            Self::get_idle_time_macos()
        }

        #[cfg(target_os = "windows")]
        {
            Self::get_idle_time_windows()
        }

        #[cfg(target_os = "linux")]
        {
            Self::get_idle_time_linux()
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            Ok(Duration::from_secs(0))
        }
    }

    #[cfg(target_os = "macos")]
    fn get_idle_time_macos() -> Result<Duration, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Duration::from_secs(0))
    }

    #[cfg(target_os = "windows")]
    fn get_idle_time_windows() -> Result<Duration, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Duration::from_secs(0))
    }

    #[cfg(target_os = "linux")]
    fn get_idle_time_linux() -> Result<Duration, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Duration::from_secs(0))
    }
}

pub struct PowerEventMonitor {
    pub event_tx: mpsc::Sender<PowerEvent>,
}

impl PowerEventMonitor {
    pub fn new() -> (Self, mpsc::Receiver<PowerEvent>) {
        let (tx, rx) = mpsc::channel(10);
        (Self { event_tx: tx }, rx)
    }

    pub async fn start_monitoring(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
}

pub fn categorize_app(app_name: &str) -> SessionType {
    let name_lower = app_name.to_lowercase();

    if name_lower.contains("code")
        || name_lower.contains("studio")
        || name_lower.contains("vim")
        || name_lower.contains("xcode")
        || name_lower.contains("intellij")
    {
        return SessionType::Development;
    }
    if name_lower.contains("slack")
        || name_lower.contains("teams")
        || name_lower.contains("zoom")
        || name_lower.contains("discord")
        || name_lower.contains("mail")
        || name_lower.contains("outlook")
    {
        return SessionType::Communication;
    }
    if name_lower.contains("safari")
        || name_lower.contains("chrome")
        || name_lower.contains("firefox")
        || name_lower.contains("browser")
    {
        return SessionType::Research;
    }
    if name_lower.contains("spotify")
        || name_lower.contains("netflix")
        || name_lower.contains("youtube")
        || name_lower.contains("music")
        || name_lower.contains("games")
    {
        return SessionType::Entertainment;
    }
    if name_lower.contains("word")
        || name_lower.contains("excel")
        || name_lower.contains("powerpoint")
        || name_lower.contains("keynote")
        || name_lower.contains("pages")
    {
        return SessionType::Work;
    }

    SessionType::Unknown
}

pub fn calculate_productivity_score(apps: &[AppUsageInfo]) -> f32 {
    if apps.is_empty() {
        return 0.0;
    }

    let total_focus: i64 = apps.iter().map(|a| a.focus_duration_ms).sum();
    if total_focus == 0 {
        return 0.0;
    }

    let avg_focus_per_app = total_focus as f32 / apps.len() as f32;
    let switch_penalty = 1.0 / (1.0 + apps.len() as f32 * 0.1);
    let focus_score = (avg_focus_per_app / 60000.0).min(1.0);
    focus_score * switch_penalty
}
