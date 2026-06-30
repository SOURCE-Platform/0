use crate::models::capture::{CaptureError, CaptureResult, Display, PixelFormat, RawFrame};
use windows::core::{Error as WinError, Result};
use windows::Win32::Foundation::{E_ACCESSDENIED, E_FAIL};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;

pub(super) fn create_d3d_device() -> Result<(ID3D11Device, ID3D11DeviceContext)> {
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

pub(super) async fn get_displays() -> CaptureResult<Vec<Display>> {
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().map_err(capture_failed)?;
        let mut displays = Vec::new();
        let mut adapter_index = 0;

        loop {
            let adapter: IDXGIAdapter1 = match factory.EnumAdapters1(adapter_index) {
                Ok(adapter) => adapter,
                Err(_) => break,
            };
            let mut output_index = 0;

            loop {
                let output: IDXGIOutput = match adapter.EnumOutputs(output_index) {
                    Ok(output) => output,
                    Err(_) => break,
                };
                let desc = output.GetDesc().map_err(capture_failed)?;
                if desc.AttachedToDesktop.as_bool() {
                    let width =
                        (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left) as u32;
                    let height =
                        (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top) as u32;
                    let display_id = (adapter_index << 16) | output_index;
                    let device_name = String::from_utf16_lossy(desc.DeviceName.as_wide())
                        .trim_end_matches('\0')
                        .to_string();

                    displays.push(Display {
                        id: display_id,
                        name: format!("{} ({}x{})", device_name, width, height),
                        width,
                        height,
                        is_primary: desc.DesktopCoordinates.left == 0
                            && desc.DesktopCoordinates.top == 0,
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

pub(super) async fn capture_frame_desktop_duplication(display_id: u32) -> CaptureResult<RawFrame> {
    let timestamp = chrono::Utc::now().timestamp_millis();
    unsafe {
        let (device, context) = create_d3d_device().map_err(|error| {
            CaptureError::CaptureFailed(format!("Failed to create D3D device: {}", error))
        })?;
        let adapter_index = display_id >> 16;
        let output_index = display_id & 0xFFFF;

        let factory: IDXGIFactory1 = CreateDXGIFactory1().map_err(capture_failed)?;
        let adapter: IDXGIAdapter1 = factory
            .EnumAdapters1(adapter_index)
            .map_err(|_| CaptureError::DisplayNotFound(display_id))?;
        let output: IDXGIOutput = adapter
            .EnumOutputs(output_index)
            .map_err(|_| CaptureError::DisplayNotFound(display_id))?;
        let output1: IDXGIOutput1 = output.cast().map_err(capture_failed)?;
        let desc = output.GetDesc().map_err(capture_failed)?;
        let width = (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left) as u32;
        let height = (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top) as u32;

        let duplication: IDXGIOutputDuplication = output1.DuplicateOutput(&device).map_err(|e| {
            if e.code() == E_ACCESSDENIED {
                CaptureError::CaptureFailed(
                    "Access denied. Desktop Duplication may already be in use or requires elevation."
                        .to_string(),
                )
            } else {
                capture_failed(e)
            }
        })?;

        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut desktop_resource: Option<IDXGIResource> = None;
        duplication
            .AcquireNextFrame(1000, &mut frame_info, &mut desktop_resource)
            .map_err(|e| {
                if e.code() == DXGI_ERROR_WAIT_TIMEOUT {
                    CaptureError::CaptureFailed("Timeout waiting for frame".to_string())
                } else {
                    capture_failed(e)
                }
            })?;

        let texture: ID3D11Texture2D = desktop_resource.unwrap().cast().map_err(capture_failed)?;
        let mut texture_desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut texture_desc);
        texture_desc.Usage = D3D11_USAGE_STAGING;
        texture_desc.BindFlags = D3D11_BIND_FLAG(0);
        texture_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
        texture_desc.MiscFlags = D3D11_RESOURCE_MISC_FLAG(0);

        let staging_texture = device
            .CreateTexture2D(&texture_desc, None)
            .map_err(capture_failed)?;
        context.CopyResource(&staging_texture, &texture);

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context
            .Map(&staging_texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .map_err(capture_failed)?;

        let row_pitch = mapped.RowPitch as usize;
        let src_ptr = mapped.pData as *const u8;
        let mut pixel_data = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            let row_start = y as usize * row_pitch;
            let src_row = std::slice::from_raw_parts(src_ptr.add(row_start), width as usize * 4);
            pixel_data.extend_from_slice(src_row);
        }

        context.Unmap(&staging_texture, 0);
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

fn capture_failed(error: WinError) -> CaptureError {
    if error.code() == E_FAIL {
        CaptureError::CaptureFailed("Windows capture API returned E_FAIL".to_string())
    } else {
        CaptureError::CaptureFailed(format!("{}", error))
    }
}
