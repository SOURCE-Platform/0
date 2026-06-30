use crate::models::capture::{CaptureError, CaptureResult, Display, RawFrame};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext};

mod d3d;
mod gdi;
mod tests;

pub struct WindowsScreenCapture {
    is_capturing: Arc<AtomicBool>,
    current_display_id: Option<u32>,
    d3d_device: Option<ID3D11Device>,
    d3d_context: Option<ID3D11DeviceContext>,
}

impl WindowsScreenCapture {
    pub async fn new() -> CaptureResult<Self> {
        let (device, context) = match d3d::create_d3d_device() {
            Ok((device, context)) => (Some(device), Some(context)),
            Err(error) => {
                eprintln!(
                    "Warning: Failed to create D3D11 device: {}. Will use GDI fallback.",
                    error
                );
                (None, None)
            }
        };

        Ok(Self {
            is_capturing: Arc::new(AtomicBool::new(false)),
            current_display_id: None,
            d3d_device: device,
            d3d_context: context,
        })
    }

    pub async fn get_displays() -> CaptureResult<Vec<Display>> {
        d3d::get_displays().await
    }

    pub async fn capture_frame(display_id: u32) -> CaptureResult<RawFrame> {
        match d3d::capture_frame_desktop_duplication(display_id).await {
            Ok(frame) => Ok(frame),
            Err(error) => {
                eprintln!(
                    "Desktop Duplication failed: {}. Falling back to GDI.",
                    error
                );
                gdi::capture_frame_gdi(display_id).await
            }
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

    pub fn has_d3d_device(&self) -> bool {
        self.d3d_device.is_some() && self.d3d_context.is_some()
    }
}
