// Platform-specific screen capture implementations
// Each platform module provides the same interface defined in models/capture.rs

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "macos")]
pub use macos::MacOSScreenCapture as PlatformCapture;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "windows")]
pub use windows::WindowsScreenCapture as PlatformCapture;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
pub use linux::LinuxScreenCapture as PlatformCapture;
