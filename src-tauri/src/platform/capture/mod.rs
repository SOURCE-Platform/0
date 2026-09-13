// Platform-specific screen capture implementations
// Each platform module provides the same interface defined in models/capture.rs

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub mod macos_display_names;

#[cfg(target_os = "macos")]
pub use macos::MacOSScreenCapture as PlatformCapture;
#[cfg(target_os = "macos")]
pub use macos::{request_screen_capture_permission, screen_capture_permission_granted};

#[cfg(not(target_os = "macos"))]
pub fn screen_capture_permission_granted() -> bool {
    true
}

#[cfg(not(target_os = "macos"))]
pub fn request_screen_capture_permission() -> bool {
    true
}

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "windows")]
pub use windows::WindowsScreenCapture as PlatformCapture;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
pub use linux::LinuxScreenCapture as PlatformCapture;
