// Input device monitoring - keyboard and mouse events

#[cfg(target_os = "macos")]
pub mod keyboard_macos;
#[cfg(target_os = "macos")]
pub use keyboard_macos::MacOSKeyboardListener;

#[cfg(target_os = "windows")]
pub mod keyboard_windows;
#[cfg(target_os = "windows")]
pub use keyboard_windows::WindowsKeyboardListener;

#[cfg(target_os = "linux")]
pub mod keyboard_linux;
#[cfg(target_os = "linux")]
pub use keyboard_linux::LinuxKeyboardListener;
