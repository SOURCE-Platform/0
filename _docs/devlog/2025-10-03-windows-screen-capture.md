# Windows Screen Capture Implementation

**Date:** 2025-10-03
**Task:** 2.2 - Windows Screen Capture
**Status:** ✅ Complete

## Overview

Implemented cross-platform screen capture for Windows using Desktop Duplication API with GDI fallback.

## Implementation Details

### Primary Method: Desktop Duplication API

**Location:** [src-tauri/src/platform/capture/windows.rs](../../src-tauri/src/platform/capture/windows.rs)

**Technology:**
- **DirectX 11 / DXGI** - Modern Windows screen capture API
- Requires Windows 8 or later
- GPU-accelerated, efficient frame capture
- Direct access to desktop framebuffer

**Process:**
1. Create D3D11 device and context
2. Enumerate DXGI adapters and outputs
3. Call `DuplicateOutput()` to get `IDXGIOutputDuplication`
4. `AcquireNextFrame()` to capture frames
5. Copy texture to staging buffer
6. Map CPU memory and read pixel data

**Advantages:**
- ✅ Very fast (GPU-accelerated)
- ✅ Low CPU usage
- ✅ High quality capture
- ✅ Supports multiple monitors

**Limitations:**
- ❌ Doesn't work in Remote Desktop (RDP) sessions
- ❌ Only one duplication per output allowed
- ❌ May require elevated privileges in some scenarios
- ❌ Doesn't capture DRM-protected content

### Fallback Method: GDI BitBlt

**When Used:**
- Desktop Duplication fails
- RDP/Remote Desktop sessions
- Virtual machines
- Legacy systems

**Technology:**
- Classic Windows GDI (Graphics Device Interface)
- Works everywhere but slower
- CPU-based rendering

**Process:**
1. Get desktop device context (DC)
2. Create compatible DC and bitmap
3. Use `BitBlt()` to copy screen pixels
4. Read bitmap data with `GetDIBits()`

**Advantages:**
- ✅ Works in RDP sessions
- ✅ Works in all environments
- ✅ No special permissions needed

**Disadvantages:**
- ❌ Slower than Desktop Duplication
- ❌ Higher CPU usage
- ❌ May capture cursor artifacts

## Dependencies Added

```toml
[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.58", features = [
    "Win32_Graphics_Dxgi",
    "Win32_Graphics_Dxgi_Common",
    "Win32_Graphics_Direct3D11",
    "Win32_Graphics_Gdi",
    "Win32_Foundation",
    "Win32_System_Threading",
]}
```

## API Interface

Same interface as macOS implementation for consistency:

```rust
impl WindowsScreenCapture {
    async fn new() -> CaptureResult<Self>
    async fn get_displays() -> CaptureResult<Vec<Display>>
    async fn capture_frame(display_id: u32) -> CaptureResult<RawFrame>
    async fn start_capture(&mut self, display_id: u32) -> CaptureResult<()>
    async fn stop_capture(&mut self) -> CaptureResult<()>
    fn is_capturing(&self) -> bool
    fn current_display_id(&self) -> Option<u32>
}
```

## Display ID Encoding

Windows uses a composite display ID:
- **Upper 16 bits:** Adapter index
- **Lower 16 bits:** Output index
- Example: `0x00010002` = Adapter 1, Output 2

This allows uniquely identifying displays across multiple GPUs.

## Error Handling

The implementation automatically falls back to GDI when Desktop Duplication fails:

```rust
match Self::capture_frame_desktop_duplication(display_id).await {
    Ok(frame) => Ok(frame),
    Err(e) => {
        eprintln!("Desktop Duplication failed: {}. Falling back to GDI.", e);
        Self::capture_frame_gdi(display_id).await
    }
}
```

Common error scenarios:
- `E_ACCESSDENIED` - Desktop Duplication already in use or needs elevation
- `DXGI_ERROR_WAIT_TIMEOUT` - No new frame available
- Device creation fails - Falls back to GDI

## Testing

**Unit Tests:**
- ✅ Display enumeration
- ✅ Single frame capture
- ✅ Capture lifecycle (start/stop)

**Test Example:**
- [examples/test_capture.rs](../../src-tauri/examples/test_capture.rs)
- Now cross-platform (macOS, Windows, Linux stub)

**Testing Notes:**
- Tests are designed to not panic in restricted environments
- Graceful degradation if capture unavailable
- Clear error messages for debugging

## Platform Support Status

| Platform | Status | Method | Notes |
|----------|--------|--------|-------|
| **macOS** | ✅ Complete | Core Graphics / ScreenCaptureKit | Requires screen recording permission |
| **Windows** | ✅ Complete | Desktop Duplication + GDI fallback | Works in all environments |
| **Linux** | ⚠️ Stub | Not implemented | Returns `NotSupported` error |

## Known Issues & Limitations

### Windows-Specific:
1. **Remote Desktop:** Desktop Duplication fails in RDP sessions (automatically uses GDI fallback)
2. **Single Duplication:** Only one app can use Desktop Duplication per display at a time
3. **DRM Content:** Protected content (Netflix, etc.) appears black
4. **Virtual Machines:** May have limited support depending on VM graphics drivers

### General:
- No compression/encoding yet (raw BGRA pixel data)
- No motion detection
- No automatic recording loop
- No Tauri commands exposed to frontend

## Next Steps

To make this feature user-accessible:

1. **Recording Service** - Continuous capture loop with FPS control
2. **Storage System** - Save frames to configured directory
3. **UI Controls** - Start/Stop Recording button in header
4. **Compression** - Encode to H.264 or compress PNG sequence
5. **Motion Detection** - Skip frames with minimal change
6. **Session Management** - Track recordings in database

## References

- [MSDN: Desktop Duplication API](https://docs.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api)
- [windows-rs crate documentation](https://microsoft.github.io/windows-docs-rs/)
- Project architecture: [CLAUDE.md](../../CLAUDE.md)
