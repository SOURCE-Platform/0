use crate::models::capture::{CaptureError, CaptureResult, Display, RawFrame};

pub(super) async fn get_displays_wayland() -> CaptureResult<Vec<Display>> {
    Ok(vec![Display {
        id: 0,
        name: "Primary Display (Wayland)".to_string(),
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
        is_primary: true,
    }])
}

pub(super) async fn capture_frame_wayland(_display_id: u32) -> CaptureResult<RawFrame> {
    Err(CaptureError::CaptureFailed(
        "Wayland screen capture requires XDG Desktop Portal integration. \
        This is not yet fully implemented. Please use X11 for now, or use \
        a tool like OBS Studio which has full Wayland PipeWire support."
            .to_string(),
    ))
}
