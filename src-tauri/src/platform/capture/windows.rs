// Windows screen capture implementation using Desktop Duplication API and GDI fallback

use crate::models::capture::{CaptureError, CaptureResult, Display, PixelFormat, RawFrame};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::*;

/// Windows screen capture implementation
pub struct WindowsScreenCapture {
    is_capturing: Arc<AtomicBool>,
    current_display_id: Option<u32>,
    d3d_device: Option<ID3D11Device>,
    d3d_context: Option<ID3D11DeviceContext>,
}

impl WindowsScreenCapture {
    /// Create a new Windows screen capture instance
    pub async fn new() -> CaptureResult<Self> {
        // Try to create D3D11 device for Desktop Duplication API
        let (device, context) = match Self::create_d3d_device() {
            Ok((dev, ctx)) => (Some(dev), Some(ctx)),
            Err(e) => {
                eprintln!("Warning: Failed to create D3D11 device: {}. Will use GDI fallback.", e);
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

    /// Create D3D11 device for Desktop Duplication API
    fn create_d3d_device() -> Result<(ID3D11Device, ID3D11DeviceContext)> {
        unsafe {
            let mut device: Option<ID3D11Device> = None;
            let mut context: Option<ID3D11DeviceContext> = None;

            let feature_levels = [
                D3D_FEATURE_LEVEL_11_0,
                D3D_FEATURE_LEVEL_10_1,
                D3D_FEATURE_LEVEL_10_0,
            ];

            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_FLAG(0),
                Some(&feature_levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;

            Ok((device.unwrap(), context.unwrap()))
        }
    }

    /// Get list of all available displays
    pub async fn get_displays() -> CaptureResult<Vec<Display>> {
        unsafe {
            let mut displays = Vec::new();
            let mut adapter_index = 0;

            // Create DXGI factory
            let factory: IDXGIFactory1 = CreateDXGIFactory1()
                .map_err(|e| CaptureError::CaptureFailed(format!("Failed to create DXGI factory: {}", e)))?;

            // Enumerate adapters
            loop {
                let adapter: IDXGIAdapter1 = match factory.EnumAdapters1(adapter_index) {
                    Ok(a) => a,
                    Err(_) => break, // No more adapters
                };

                let mut output_index = 0;

                // Enumerate outputs for this adapter
                loop {
                    let output: IDXGIOutput = match adapter.EnumOutputs(output_index) {
                        Ok(o) => o,
                        Err(_) => break, // No more outputs
                    };

                    let desc = output.GetDesc()
                        .map_err(|e| CaptureError::CaptureFailed(format!("Failed to get output desc: {}", e)))?;

                    if desc.AttachedToDesktop.as_bool() {
                        let width = (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left) as u32;
                        let height = (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top) as u32;

                        // Use a combination of adapter and output index as display ID
                        let display_id = (adapter_index << 16) | output_index;

                        // Check if this is the primary monitor
                        let is_primary = desc.DesktopCoordinates.left == 0 && desc.DesktopCoordinates.top == 0;

                        let device_name = String::from_utf16_lossy(desc.DeviceName.as_wide());

                        displays.push(Display {
                            id: display_id,
                            name: format!("{} ({}x{})", device_name.trim_end_matches('\0'), width, height),
                            width,
                            height,
                            is_primary,
                        });
                    }

                    output_index += 1;
                }

                adapter_index += 1;
            }

            if displays.is_empty() {
                return Err(CaptureError::CaptureFailed("No displays found".to_string()));
            }

            Ok(displays)
        }
    }

    /// Capture a single frame from the specified display using Desktop Duplication API
    pub async fn capture_frame(display_id: u32) -> CaptureResult<RawFrame> {
        // Try Desktop Duplication API first
        match Self::capture_frame_desktop_duplication(display_id).await {
            Ok(frame) => Ok(frame),
            Err(e) => {
                // Fall back to GDI
                eprintln!("Desktop Duplication failed: {}. Falling back to GDI.", e);
                Self::capture_frame_gdi(display_id).await
            }
        }
    }

    /// Capture frame using Desktop Duplication API
    async fn capture_frame_desktop_duplication(display_id: u32) -> CaptureResult<RawFrame> {
        let timestamp = chrono::Utc::now().timestamp_millis();

        unsafe {
            // Create temporary D3D device for this capture
            let (device, context) = Self::create_d3d_device()
                .map_err(|e| CaptureError::CaptureFailed(format!("Failed to create D3D device: {}", e)))?;

            let adapter_index = (display_id >> 16) as u32;
            let output_index = (display_id & 0xFFFF) as u32;

            // Get DXGI adapter and output
            let factory: IDXGIFactory1 = CreateDXGIFactory1()
                .map_err(|e| CaptureError::CaptureFailed(format!("Failed to create DXGI factory: {}", e)))?;

            let adapter: IDXGIAdapter1 = factory.EnumAdapters1(adapter_index)
                .map_err(|_| CaptureError::DisplayNotFound(display_id))?;

            let output: IDXGIOutput = adapter.EnumOutputs(output_index)
                .map_err(|_| CaptureError::DisplayNotFound(display_id))?;

            let output1: IDXGIOutput1 = output.cast()
                .map_err(|e| CaptureError::CaptureFailed(format!("Failed to cast to IDXGIOutput1: {}", e)))?;

            // Get output description
            let desc = output.GetDesc()
                .map_err(|e| CaptureError::CaptureFailed(format!("Failed to get output desc: {}", e)))?;

            let width = (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left) as u32;
            let height = (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top) as u32;

            // Create duplication
            let duplication: IDXGIOutputDuplication = output1.DuplicateOutput(&device)
                .map_err(|e| {
                    if e.code() == E_ACCESSDENIED {
                        CaptureError::CaptureFailed(
                            "Access denied. Desktop Duplication may already be in use or requires elevation.".to_string()
                        )
                    } else {
                        CaptureError::CaptureFailed(format!("Failed to duplicate output: {}", e))
                    }
                })?;

            // Acquire next frame
            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut desktop_resource: Option<IDXGIResource> = None;

            duplication.AcquireNextFrame(1000, &mut frame_info, &mut desktop_resource)
                .map_err(|e| {
                    if e.code() == DXGI_ERROR_WAIT_TIMEOUT {
                        CaptureError::CaptureFailed("Timeout waiting for frame".to_string())
                    } else {
                        CaptureError::CaptureFailed(format!("Failed to acquire frame: {}", e))
                    }
                })?;

            let desktop_resource = desktop_resource.unwrap();

            // Get texture from resource
            let texture: ID3D11Texture2D = desktop_resource.cast()
                .map_err(|e| CaptureError::CaptureFailed(format!("Failed to cast to texture: {}", e)))?;

            // Create staging texture to read pixel data
            let mut texture_desc = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut texture_desc);

            texture_desc.Usage = D3D11_USAGE_STAGING;
            texture_desc.BindFlags = D3D11_BIND_FLAG(0);
            texture_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
            texture_desc.MiscFlags = D3D11_RESOURCE_MISC_FLAG(0);

            let staging_texture = device.CreateTexture2D(&texture_desc, None)
                .map_err(|e| CaptureError::CaptureFailed(format!("Failed to create staging texture: {}", e)))?;

            // Copy texture to staging
            context.CopyResource(&staging_texture, &texture);

            // Map staging texture to read pixels
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context.Map(&staging_texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|e| CaptureError::CaptureFailed(format!("Failed to map texture: {}", e)))?;

            // Copy pixel data
            let bytes_per_pixel = 4;
            let expected_bytes = (width * height * bytes_per_pixel) as usize;
            let mut pixel_data = Vec::with_capacity(expected_bytes);

            let row_pitch = mapped.RowPitch as usize;
            let src_ptr = mapped.pData as *const u8;

            for y in 0..height {
                let row_start = (y as usize) * row_pitch;
                let src_row = std::slice::from_raw_parts(src_ptr.add(row_start), width as usize * bytes_per_pixel as usize);
                pixel_data.extend_from_slice(src_row);
            }

            // Unmap texture
            context.Unmap(&staging_texture, 0);

            // Release frame
            let _ = duplication.ReleaseFrame();

            Ok(RawFrame {
                timestamp,
                width,
                height,
                data: pixel_data,
                format: PixelFormat::BGRA8,
            })
        }
    }

    /// Fallback capture using GDI BitBlt (for Remote Desktop, etc.)
    async fn capture_frame_gdi(display_id: u32) -> CaptureResult<RawFrame> {
        let timestamp = chrono::Utc::now().timestamp_millis();

        unsafe {
            // Get display info
            let displays = Self::get_displays().await?;
            let display = displays.iter().find(|d| d.id == display_id)
                .ok_or(CaptureError::DisplayNotFound(display_id))?;

            let width = display.width;
            let height = display.height;

            // Get desktop DC
            let desktop_dc = GetDC(None);
            if desktop_dc.is_invalid() {
                return Err(CaptureError::CaptureFailed("Failed to get desktop DC".to_string()));
            }

            // Create compatible DC and bitmap
            let mem_dc = CreateCompatibleDC(desktop_dc);
            if mem_dc.is_invalid() {
                let _ = ReleaseDC(None, desktop_dc);
                return Err(CaptureError::CaptureFailed("Failed to create compatible DC".to_string()));
            }

            let bitmap = CreateCompatibleBitmap(desktop_dc, width as i32, height as i32);
            if bitmap.is_invalid() {
                let _ = DeleteDC(mem_dc);
                let _ = ReleaseDC(None, desktop_dc);
                return Err(CaptureError::CaptureFailed("Failed to create bitmap".to_string()));
            }

            let old_bitmap = SelectObject(mem_dc, bitmap);

            // Copy screen to bitmap
            if !BitBlt(mem_dc, 0, 0, width as i32, height as i32, desktop_dc, 0, 0, SRCCOPY).as_bool() {
                let _ = SelectObject(mem_dc, old_bitmap);
                let _ = DeleteObject(bitmap);
                let _ = DeleteDC(mem_dc);
                let _ = ReleaseDC(None, desktop_dc);
                return Err(CaptureError::CaptureFailed("BitBlt failed".to_string()));
            }

            // Get bitmap data
            let mut bitmap_info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width as i32,
                    biHeight: -(height as i32), // Negative for top-down bitmap
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0 as u32,
                    ..Default::default()
                },
                ..Default::default()
            };

            let bytes_per_pixel = 4;
            let expected_bytes = (width * height * bytes_per_pixel) as usize;
            let mut pixel_data = vec![0u8; expected_bytes];

            let result = GetDIBits(
                mem_dc,
                bitmap,
                0,
                height,
                Some(pixel_data.as_mut_ptr() as *mut _),
                &mut bitmap_info,
                DIB_RGB_COLORS,
            );

            // Cleanup
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
        let displays = WindowsScreenCapture::get_displays().await;
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
        let displays = WindowsScreenCapture::get_displays().await.expect("Failed to get displays");
        assert!(!displays.is_empty());

        let primary_display = displays.iter().find(|d| d.is_primary).unwrap();

        let frame = WindowsScreenCapture::capture_frame(primary_display.id).await;
        match frame {
            Ok(frame) => {
                println!(
                    "Captured frame: {}x{}, {} bytes, format: {:?}",
                    frame.width,
                    frame.height,
                    frame.data.len(),
                    frame.format
                );
                assert!(frame.width > 0, "Frame width should be positive");
                assert!(frame.height > 0, "Frame height should be positive");
                assert_eq!(frame.data.len(), (frame.width * frame.height * 4) as usize, "Data size should match dimensions");
            }
            Err(e) => {
                eprintln!("Failed to capture frame: {}", e);
                // Don't panic - capture might fail in some environments (VM, RDP, etc.)
                println!("Note: Capture may fail in virtual machines or Remote Desktop sessions");
            }
        }
    }

    #[tokio::test]
    async fn test_capture_lifecycle() {
        let displays = WindowsScreenCapture::get_displays().await.expect("Failed to get displays");
        let primary_display = displays.iter().find(|d| d.is_primary).unwrap();

        let mut capture = WindowsScreenCapture::new().await.expect("Failed to create capture");

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
