// Input device monitoring - keyboard and mouse events

// Keyboard listeners
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

// Mouse listeners
#[cfg(target_os = "macos")]
pub mod mouse_macos;
#[cfg(target_os = "macos")]
pub use mouse_macos::MacOSMouseListener;

#[cfg(target_os = "windows")]
pub mod mouse_windows;
#[cfg(target_os = "windows")]
pub use mouse_windows::WindowsMouseListener;

#[cfg(target_os = "linux")]
pub mod mouse_linux;
#[cfg(target_os = "linux")]
pub use mouse_linux::LinuxMouseListener;
