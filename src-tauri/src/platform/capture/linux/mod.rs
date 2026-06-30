use crate::models::capture::{CaptureError, CaptureResult, Display, RawFrame};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

mod tests;
mod wayland;
mod x11;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayServer {
    X11,
    Wayland,
    Unknown,
}

impl DisplayServer {
    pub fn detect() -> Self {
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            Self::Wayland
        } else if std::env::var("DISPLAY").is_ok() {
            Self::X11
        } else {
            Self::Unknown
        }
    }
}

pub struct LinuxScreenCapture {
    is_capturing: Arc<AtomicBool>,
    current_display_id: Option<u32>,
    display_server: DisplayServer,
}

impl LinuxScreenCapture {
    pub async fn new() -> CaptureResult<Self> {
        let display_server = DisplayServer::detect();
        match display_server {
            DisplayServer::X11 | DisplayServer::Wayland => Ok(Self {
                is_capturing: Arc::new(AtomicBool::new(false)),
                current_display_id: None,
                display_server,
            }),
            DisplayServer::Unknown => Err(CaptureError::CaptureFailed(
                "No display server detected. Neither DISPLAY nor WAYLAND_DISPLAY is set."
                    .to_string(),
            )),
        }
    }

    pub async fn get_displays() -> CaptureResult<Vec<Display>> {
        match DisplayServer::detect() {
            DisplayServer::X11 => x11::get_displays_x11().await,
            DisplayServer::Wayland => wayland::get_displays_wayland().await,
            DisplayServer::Unknown => Err(CaptureError::CaptureFailed(
                "No display server detected".to_string(),
            )),
        }
    }

    pub async fn capture_frame(display_id: u32) -> CaptureResult<RawFrame> {
        match DisplayServer::detect() {
            DisplayServer::X11 => x11::capture_frame_x11(display_id).await,
            DisplayServer::Wayland => wayland::capture_frame_wayland(display_id).await,
            DisplayServer::Unknown => Err(CaptureError::CaptureFailed(
                "No display server detected".to_string(),
            )),
        }
    }

    pub async fn start_capture(&mut self, display_id: u32) -> CaptureResult<()> {
        if self.is_capturing.load(Ordering::SeqCst) {
            return Err(CaptureError::AlreadyCapturing);
        }

        let displays = Self::get_displays().await?;
        if !displays.iter().any(|d| d.id == display_id) {
            return Err(CaptureError::DisplayNotFound(display_id));
        }

        self.current_display_id = Some(display_id);
        self.is_capturing.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub async fn stop_capture(&mut self) -> CaptureResult<()> {
        if !self.is_capturing.load(Ordering::SeqCst) {
            return Err(CaptureError::NotCapturing);
        }

        self.is_capturing.store(false, Ordering::SeqCst);
        self.current_display_id = None;
        Ok(())
    }

    pub fn is_capturing(&self) -> bool {
        self.is_capturing.load(Ordering::SeqCst)
    }

    pub fn current_display_id(&self) -> Option<u32> {
        self.current_display_id
    }

    pub fn display_server(&self) -> DisplayServer {
        self.display_server
    }
}
