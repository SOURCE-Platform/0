// OS activity monitoring - tracks application lifecycle and focus

use crate::models::activity::{AppEvent, AppInfo};
use tokio::sync::mpsc;

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos::MacOSMonitor;

#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "windows")]
pub use windows::WindowsMonitor;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
pub use linux::LinuxMonitor;

