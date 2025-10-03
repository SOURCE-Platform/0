// Data structures for screen capture

use serde::{Deserialize, Serialize};

/// Represents a display/monitor that can be captured
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Display {
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}

/// A captured frame from the screen
#[derive(Debug, Clone)]
pub struct RawFrame {
    pub timestamp: i64,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    pub format: PixelFormat,
}

/// Pixel format of captured frames
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    RGBA8,
    BGRA8,
}

/// Error types for screen capture operations
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Display not found: {0}")]
    DisplayNotFound(u32),

    #[error("Capture failed: {0}")]
    CaptureFailed(String),

    #[error("Not supported on this platform")]
    NotSupported,

    #[error("Already capturing")]
    AlreadyCapturing,

    #[error("Not currently capturing")]
    NotCapturing,
}

pub type CaptureResult<T> = Result<T, CaptureError>;
