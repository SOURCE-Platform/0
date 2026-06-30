use super::d3d;
use crate::models::capture::{CaptureError, CaptureResult, PixelFormat, RawFrame};
use windows::Win32::Graphics::Gdi::*;

pub(super) async fn capture_frame_gdi(display_id: u32) -> CaptureResult<RawFrame> {
    let timestamp = chrono::Utc::now().timestamp_millis();
    unsafe {
        let displays = d3d::get_displays().await?;
        let display = displays
            .iter()
            .find(|display| display.id == display_id)
            .ok_or(CaptureError::DisplayNotFound(display_id))?;

        let desktop_dc = GetDC(None);
        if desktop_dc.is_invalid() {
            return Err(CaptureError::CaptureFailed(
                "Failed to get desktop DC".to_string(),
            ));
        }

        let width = display.width;
        let height = display.height;
        let mem_dc = CreateCompatibleDC(desktop_dc);
        if mem_dc.is_invalid() {
            let _ = ReleaseDC(None, desktop_dc);
            return Err(CaptureError::CaptureFailed(
                "Failed to create compatible DC".to_string(),
            ));
        }

        let bitmap = CreateCompatibleBitmap(desktop_dc, width as i32, height as i32);
        if bitmap.is_invalid() {
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(None, desktop_dc);
            return Err(CaptureError::CaptureFailed(
                "Failed to create bitmap".to_string(),
            ));
        }

        let old_bitmap = SelectObject(mem_dc, bitmap);
        if !BitBlt(
            mem_dc,
            0,
            0,
            width as i32,
            height as i32,
            desktop_dc,
            0,
            0,
            SRCCOPY,
        )
        .as_bool()
        {
            let _ = SelectObject(mem_dc, old_bitmap);
            let _ = DeleteObject(bitmap);
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(None, desktop_dc);
            return Err(CaptureError::CaptureFailed("BitBlt failed".to_string()));
        }

        let mut bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0 as u32,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut pixel_data = vec![0u8; (width * height * 4) as usize];
        let result = GetDIBits(
            mem_dc,
            bitmap,
            0,
            height,
            Some(pixel_data.as_mut_ptr() as *mut _),
            &mut bitmap_info,
            DIB_RGB_COLORS,
        );

        let _ = SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(bitmap);
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, desktop_dc);

        if result == 0 {
            return Err(CaptureError::CaptureFailed("GetDIBits failed".to_string()));
        }

        Ok(RawFrame {
            timestamp,
            width,
            height,
            data: pixel_data,
            format: PixelFormat::BGRA8,
        })
    }
}
