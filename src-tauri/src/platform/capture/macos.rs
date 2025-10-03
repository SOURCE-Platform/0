// macOS screen capture implementation using ScreenCaptureKit and Core Graphics

use crate::models::capture::{CaptureError, CaptureResult, Display, PixelFormat, RawFrame};
use core_graphics::display::CGDisplay;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// macOS screen capture implementation
pub struct MacOSScreenCapture {
    is_capturing: Arc<AtomicBool>,
    current_display_id: Option<u32>,
}

impl MacOSScreenCapture {
    /// Create a new macOS screen capture instance
    pub async fn new() -> CaptureResult<Self> {
        // Check for screen recording permission
        if !Self::check_screen_recording_permission() {
            return Err(CaptureError::PermissionDenied(
                "Screen recording permission not granted. Please enable it in System Settings > Privacy & Security > Screen Recording".to_string()
            ));
        }

        Ok(Self {
            is_capturing: Arc::new(AtomicBool::new(false)),
            current_display_id: None,
        })
    }

    /// Check if screen recording permission is granted
    fn check_screen_recording_permission() -> bool {
        // On macOS 10.15+, we need screen recording permission
        // We can check this by attempting to get display info
        // If permission is denied, the API will return limited information

        // Try to create an image from the main display
        // If this fails with permission error, we know permission is not granted
        let display = CGDisplay::main();
        match display.image() {
            Some(_) => true,
            None => {
                // Could be permission issue or other error
                // For now, we'll try to continue and let specific operations fail
                eprintln!("Warning: Unable to capture screen. This may indicate missing screen recording permission.");
                false
            }
        }
    }

    /// Get list of all available displays
    pub async fn get_displays() -> CaptureResult<Vec<Display>> {
        unsafe {
            // Get all active displays
            let max_displays = 32;
            let mut display_ids: Vec<u32> = vec![0; max_displays];
            let mut display_count = 0u32;

            let result = core_graphics::display::CGGetActiveDisplayList(
                max_displays as u32,
                display_ids.as_mut_ptr(),
                &mut display_count,
            );

            if result != 0 {
                return Err(CaptureError::CaptureFailed(format!(
                    "Failed to get display list, error code: {}",
                    result
                )));
            }

            // Truncate to actual count
            display_ids.truncate(display_count as usize);

            let main_display_id = CGDisplay::main().id;

            let displays: Vec<Display> = display_ids
                .iter()
                .filter_map(|&id| {
                    let cg_display = CGDisplay::new(id);
                    let bounds = cg_display.bounds();

                    Some(Display {
                        id,
                        name: format!("Display {}", id),
                        width: bounds.size.width as u32,
                        height: bounds.size.height as u32,
                        is_primary: id == main_display_id,
                    })
                })
                .collect();

            if displays.is_empty() {
                return Err(CaptureError::CaptureFailed(
                    "No displays found".to_string(),
                ));
            }

            Ok(displays)
        }
    }

    /// Capture a single frame from the specified display
    pub async fn capture_frame(display_id: u32) -> CaptureResult<RawFrame> {
        let timestamp = chrono::Utc::now().timestamp_millis();

        unsafe {
            let display = CGDisplay::new(display_id);

            // Try to capture the display
            let cg_image = display
                .image()
                .ok_or_else(|| {
                    CaptureError::CaptureFailed(
                        "Failed to capture display. Check screen recording permissions.".to_string()
                    )
                })?;

            let width = cg_image.width() as u32;
            let height = cg_image.height() as u32;
            let bytes_per_row = cg_image.bytes_per_row();
            let bits_per_pixel = cg_image.bits_per_pixel();

            // Determine pixel format
            // CGImage typically provides BGRA on macOS
            let format = if bits_per_pixel == 32 {
                PixelFormat::BGRA8
            } else {
                return Err(CaptureError::CaptureFailed(format!(
                    "Unsupported pixel format: {} bits per pixel",
                    bits_per_pixel
                )));
            };

            // Get the raw pixel data using data() method
            let data = cg_image.data();
            let data_len = data.len() as usize;
            let bytes: &[u8] = std::slice::from_raw_parts(data.as_ptr(), data_len);

            // Copy the data
            // Note: If bytes_per_row has padding, we need to handle it
            let expected_bytes = (width * height * 4) as usize;
            let mut pixel_data = Vec::with_capacity(expected_bytes);

            if bytes_per_row == width as usize * 4 {
                // No padding, direct copy
                pixel_data.extend_from_slice(&bytes[0..expected_bytes.min(bytes.len())]);
            } else {
                // Has padding, copy row by row
                for y in 0..height {
                    let row_start = (y as usize) * bytes_per_row;
                    let row_end = row_start + (width as usize * 4);
                    if row_end <= bytes.len() {
                        pixel_data.extend_from_slice(&bytes[row_start..row_end]);
                    }
                }
            }

            Ok(RawFrame {
                timestamp,
                width,
                height,
                data: pixel_data,
                format,
            })
        }
    }

    /// Start continuous capture from the specified display
    pub async fn start_capture(&mut self, display_id: u32) -> CaptureResult<()> {
        if self.is_capturing.load(Ordering::SeqCst) {
            return Err(CaptureError::AlreadyCapturing);
        }

        // Verify display exists
        let displays = Self::get_displays().await?;
        if !displays.iter().any(|d| d.id == display_id) {
            return Err(CaptureError::DisplayNotFound(display_id));
        }

        self.current_display_id = Some(display_id);
        self.is_capturing.store(true, Ordering::SeqCst);

        Ok(())
    }

    /// Stop continuous capture
    pub async fn stop_capture(&mut self) -> CaptureResult<()> {
        if !self.is_capturing.load(Ordering::SeqCst) {
            return Err(CaptureError::NotCapturing);
        }

        self.is_capturing.store(false, Ordering::SeqCst);
        self.current_display_id = None;

        Ok(())
    }

    /// Check if currently capturing
    pub fn is_capturing(&self) -> bool {
        self.is_capturing.load(Ordering::SeqCst)
    }

    /// Get the current display being captured
    pub fn current_display_id(&self) -> Option<u32> {
        self.current_display_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_displays() {
        let displays = MacOSScreenCapture::get_displays().await;
        match displays {
            Ok(displays) => {
                assert!(!displays.is_empty(), "Should have at least one display");
                println!("Found {} display(s)", displays.len());
                for display in displays {
                    println!(
                        "  Display {}: {}x{} (primary: {})",
                        display.id, display.width, display.height, display.is_primary
                    );
                }
            }
            Err(e) => {
                eprintln!("Failed to get displays: {}", e);
                panic!("Display enumeration failed");
            }
        }
    }

    #[tokio::test]
    async fn test_capture_frame() {
        // Get displays first
        let displays = MacOSScreenCapture::get_displays().await.expect("Failed to get displays");
        assert!(!displays.is_empty());

        let primary_display = displays.iter().find(|d| d.is_primary).unwrap();

        // Capture a frame
        let frame = MacOSScreenCapture::capture_frame(primary_display.id).await;
        match frame {
            Ok(frame) => {
                println!(
                    "Captured frame: {}x{}, {} bytes, format: {:?}",
                    frame.width,
                    frame.height,
                    frame.data.len(),
                    frame.format
                );
                // Note: On Retina displays, the captured frame resolution can be higher than the logical display resolution
                // For example, a 1920x1080 logical display might capture as 3840x2160 (2x scaling)
                // We'll just verify the frame has reasonable dimensions and correct data size
                assert!(frame.width > 0, "Frame width should be positive");
                assert!(frame.height > 0, "Frame height should be positive");
                assert_eq!(frame.data.len(), (frame.width * frame.height * 4) as usize, "Data size should match dimensions");
            }
            Err(e) => {
                eprintln!("Failed to capture frame: {}", e);
                if let CaptureError::PermissionDenied(_) = e {
                    println!("Permission denied - this is expected if screen recording permission is not granted");
                } else {
                    panic!("Frame capture failed");
                }
            }
        }
    }

    #[tokio::test]
    async fn test_capture_lifecycle() {
        let displays = MacOSScreenCapture::get_displays().await.expect("Failed to get displays");
        let primary_display = displays.iter().find(|d| d.is_primary).unwrap();

        let mut capture = MacOSScreenCapture::new().await.expect("Failed to create capture");

        assert!(!capture.is_capturing());

        // Start capture
        capture.start_capture(primary_display.id).await.expect("Failed to start capture");
        assert!(capture.is_capturing());
        assert_eq!(capture.current_display_id(), Some(primary_display.id));

        // Try to start again (should fail)
        let result = capture.start_capture(primary_display.id).await;
        assert!(matches!(result, Err(CaptureError::AlreadyCapturing)));

        // Stop capture
        capture.stop_capture().await.expect("Failed to stop capture");
        assert!(!capture.is_capturing());
        assert_eq!(capture.current_display_id(), None);

        // Try to stop again (should fail)
        let result = capture.stop_capture().await;
        assert!(matches!(result, Err(CaptureError::NotCapturing)));
    }
}
