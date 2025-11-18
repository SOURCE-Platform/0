// Platform-specific audio capture module
// Re-exports the appropriate platform implementation

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

// Cross-platform audio capture trait
use crate::models::audio::{AudioDevice, AudioResult};

pub trait AudioCapture {
    fn new() -> AudioResult<Self>
    where
        Self: Sized;
    fn enumerate_devices() -> AudioResult<Vec<AudioDevice>>;
    fn start_capture(&mut self, device_id: &str) -> AudioResult<()>;
    fn stop_capture(&mut self) -> AudioResult<()>;
    fn read_samples(&mut self) -> AudioResult<Vec<f32>>;
}

// Platform-specific type alias
#[cfg(target_os = "macos")]
pub type PlatformAudioCapture = macos::MacOSAudioCapture;

#[cfg(target_os = "windows")]
pub type PlatformAudioCapture = windows::WindowsAudioCapture;

#[cfg(target_os = "linux")]
pub type PlatformAudioCapture = linux::LinuxAudioCapture;
