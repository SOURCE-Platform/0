# Screen Capture Implementation Guide

**Observer** - Cross-Platform Screen Capture System

**Status:** Phase 2 Complete (Tasks 2.1, 2.2, 2.3)
**Date:** 2025-10-03

---

## Overview

This document describes the cross-platform screen capture infrastructure implemented for Observer. The system provides a unified API across macOS, Windows, and Linux with platform-specific optimizations.

## Architecture

### Unified Interface

All platforms implement the same async API defined in [src-tauri/src/models/capture.rs](../src-tauri/src/models/capture.rs):

```rust
// Common data structures
pub struct Display {
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}

pub struct RawFrame {
    pub timestamp: i64,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,      // BGRA8 or RGBA8 pixel data
    pub format: PixelFormat,
}

pub enum PixelFormat {
    RGBA8,
    BGRA8,
}

// Platform-agnostic interface (via PlatformCapture type alias)
async fn new() -> CaptureResult<Self>
async fn get_displays() -> CaptureResult<Vec<Display>>
async fn capture_frame(display_id: u32) -> CaptureResult<RawFrame>
async fn start_capture(&mut self, display_id: u32) -> CaptureResult<()>
async fn stop_capture(&mut self) -> CaptureResult<()>
fn is_capturing(&self) -> bool
fn current_display_id(&self) -> Option<u32>
```

### Platform Implementations

| File | Platform | Status |
|------|----------|--------|
| [platform/capture/macos.rs](../src-tauri/src/platform/capture/macos.rs) | macOS | ✅ Complete |
| [platform/capture/windows.rs](../src-tauri/src/platform/capture/windows.rs) | Windows | ✅ Complete |
| [platform/capture/linux.rs](../src-tauri/src/platform/capture/linux.rs) | Linux | ✅ X11 Complete, ⚠️ Wayland Partial |

---

## Platform Details

### macOS

**Technology:** Core Graphics + ScreenCaptureKit

**Features:**
- ✅ Multi-monitor detection
- ✅ Retina display support (captures at native pixel resolution)
- ✅ Permission checking
- ✅ Single frame capture
- ✅ BGRA8 pixel format

**Requirements:**
- macOS 10.15+ (for screen recording permission)
- User must grant "Screen Recording" permission in System Settings

**Known Issues:**
- Permission dialog shows on first capture attempt
- No automatic permission request (OS limitation)
- Captured resolution may be higher than logical resolution (Retina displays)

**Example:**
```rust
// On 1920x1080 Retina display, captures 3840x2160 frame
let frame = MacOSScreenCapture::capture_frame(display_id).await?;
// frame.width = 3840, frame.height = 2160
```

---

### Windows

**Technology:** Desktop Duplication API (DirectX 11) + GDI BitBlt fallback

**Features:**
- ✅ Multi-monitor detection via DXGI
- ✅ GPU-accelerated capture (Desktop Duplication)
- ✅ Automatic fallback to GDI (for RDP sessions)
- ✅ Composite display IDs for multi-GPU setups
- ✅ BGRA8 pixel format

**Requirements:**
- Windows 8+ for Desktop Duplication
- DirectX 11 capable GPU
- GDI fallback works on all Windows versions

**Capture Methods:**

**Primary: Desktop Duplication API**
- Fast, GPU-accelerated
- Low CPU usage
- Doesn't work in Remote Desktop

**Fallback: GDI BitBlt**
- Works in all environments
- Slower, higher CPU usage
- Automatically used when Desktop Duplication fails

**Known Issues:**
- Desktop Duplication: Only one app can use it per display
- Desktop Duplication: Doesn't work in RDP sessions (automatic fallback to GDI)
- Desktop Duplication: May require elevation in some scenarios
- DRM-protected content appears black

**Display ID Encoding:**
```rust
// Upper 16 bits: Adapter index
// Lower 16 bits: Output index
let display_id = (adapter_index << 16) | output_index;

// Example: 0x00010002 = GPU 1, Monitor 2
```

---

### Linux

**Technology:** X11 (Xlib + XRandR) + Wayland (Portal/PipeWire - partial)

**Display Server Detection:**
```bash
# Check current display server
echo $XDG_SESSION_TYPE
# Output: "x11" or "wayland"
```

#### X11 Implementation (✅ Complete)

**Technology:** Xlib + XRandR extension

**Features:**
- ✅ Multi-monitor detection
- ✅ Display names from XRandR
- ✅ Single frame capture
- ✅ 24-bit (BGR) and 32-bit (BGRA) support
- ✅ No permission dialogs

**Requirements:**
- X11 display server
- X11 development libraries installed
- `libx11-dev` and `libxrandr-dev` packages

**Known Issues:**
- Currently captures entire root window (not individual monitors)
- TODO: Crop to specific monitor bounds using XRandR coordinates

#### Wayland Implementation (⚠️ Partial)

**Status:** Detection only, capture not implemented

**Why Partial?**
- Requires XDG Desktop Portal + PipeWire integration
- User must approve screen share dialog for EVERY session
- Adds ~3 major dependencies
- Behavior varies by desktop environment
- Better served by dedicated tools (OBS, FFmpeg)

**Current Behavior:**
```rust
Err(CaptureError::CaptureFailed(
    "Wayland screen capture requires XDG Desktop Portal integration. \
    This is not yet fully implemented. Please use X11 for now..."
))
```

**Workarounds for Wayland Users:**
1. Use XWayland compatibility: `DISPLAY=:0 ./observer`
2. Use system tools: OBS Studio, FFmpeg
3. Switch to X11 session (if available)

**Future Implementation Plan:**
- Integrate `pipewire` crate
- Implement Portal session management
- Handle user permission dialogs
- Stream frame capture
- Estimated effort: 1-2 weeks

---

## Comparison Matrix

| Feature | macOS | Windows | Linux (X11) | Linux (Wayland) |
|---------|-------|---------|-------------|-----------------|
| **Multi-Monitor** | ✅ | ✅ | ✅ | ⚠️ Portal only |
| **Permission Required** | ✅ System | ❌ | ❌ | ✅ Per-session |
| **GPU Accelerated** | ✅ | ✅ Desktop Dup | ❌ | N/A |
| **Remote Desktop** | ✅ | ✅ GDI fallback | ✅ | ⚠️ |
| **Retina/HiDPI** | ✅ Native res | ✅ | ✅ | ⚠️ |
| **Implementation** | Complete | Complete | Complete | Partial |
| **Production Ready** | ✅ | ✅ | ✅ | ❌ |

---

## Usage Example

```rust
use zero_lib::platform::capture::PlatformCapture;
use zero_lib::models::capture::PixelFormat;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Get available displays
    let displays = PlatformCapture::get_displays().await?;
    println!("Found {} display(s)", displays.len());

    for display in &displays {
        println!("Display {}: {} ({}x{})",
            display.id,
            display.name,
            display.width,
            display.height
        );
    }

    // 2. Find primary display
    let primary = displays.iter()
        .find(|d| d.is_primary)
        .ok_or("No primary display found")?;

    // 3. Capture a frame
    let frame = PlatformCapture::capture_frame(primary.id).await?;
    println!("Captured: {}x{}, {} bytes, format: {:?}",
        frame.width,
        frame.height,
        frame.data.len(),
        frame.format
    );

    // 4. Save as PNG
    save_frame_as_png(&frame, "screenshot.png")?;

    Ok(())
}

fn save_frame_as_png(
    frame: &RawFrame,
    filename: &str
) -> Result<(), Box<dyn std::error::Error>> {
    use image::{ImageBuffer, Rgba};

    // Convert BGRA to RGBA if needed
    let rgba_data = match frame.format {
        PixelFormat::BGRA8 => {
            let mut rgba = Vec::with_capacity(frame.data.len());
            for chunk in frame.data.chunks_exact(4) {
                rgba.push(chunk[2]); // R
                rgba.push(chunk[1]); // G
                rgba.push(chunk[0]); // B
                rgba.push(chunk[3]); // A
            }
            rgba
        }
        PixelFormat::RGBA8 => frame.data.clone(),
    };

    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_raw(frame.width, frame.height, rgba_data)
            .ok_or("Failed to create image buffer")?;

    img.save(filename)?;
    Ok(())
}
```

**Run Example:**
```bash
cargo run --example test_capture
```

---

## Testing

### Run All Tests

```bash
# macOS
cargo test --lib platform::capture::macos::tests -- --nocapture

# Windows (on Windows machine)
cargo test --lib platform::capture::windows::tests -- --nocapture

# Linux (on Linux machine)
cargo test --lib platform::capture::linux::tests -- --nocapture
```

### Test Coverage

- ✅ Display enumeration
- ✅ Single frame capture
- ✅ Capture lifecycle (start/stop)
- ✅ Error handling
- ✅ Multi-monitor support
- ✅ Permission checking (macOS)
- ✅ Display server detection (Linux)

---

## Error Handling

### Common Errors

```rust
pub enum CaptureError {
    PermissionDenied(String),      // macOS screen recording permission
    DisplayNotFound(u32),           // Invalid display ID
    CaptureFailed(String),          // Capture operation failed
    NotSupported,                   // Platform/feature not supported
    AlreadyCapturing,               // Already in capturing state
    NotCapturing,                   // Not currently capturing
}
```

### Example Error Handling

```rust
match PlatformCapture::capture_frame(display_id).await {
    Ok(frame) => {
        // Process frame
    }
    Err(CaptureError::PermissionDenied(msg)) => {
        eprintln!("Permission denied: {}", msg);
        eprintln!("Please enable screen recording permission in System Settings");
    }
    Err(CaptureError::DisplayNotFound(id)) => {
        eprintln!("Display {} not found", id);
    }
    Err(e) => {
        eprintln!("Capture failed: {}", e);
    }
}
```

---

## What's NOT Implemented Yet

This is **low-level capture infrastructure only**. The following are NOT yet implemented:

❌ **No continuous recording loop** - Doesn't automatically capture frames
❌ **No storage system** - Frames aren't saved automatically
❌ **No UI controls** - Can't start/stop from app interface
❌ **No Tauri commands** - Frontend can't call these functions
❌ **No compression** - Raw BGRA pixels only (huge files)
❌ **No motion detection** - Captures every frame
❌ **No session management** - Not tracked in database

### Next Steps (Phase 3)

To make this feature user-accessible:

1. **Recording Service** (`src-tauri/src/core/recorder.rs`)
   - Async capture loop at configured FPS
   - Motion detection (skip similar frames)
   - Compression/encoding (H.264 or image sequence)
   - Session management

2. **Tauri Commands** (expose to frontend)
   ```rust
   #[tauri::command]
   async fn start_recording(display_id: u32) -> Result<()>

   #[tauri::command]
   async fn stop_recording() -> Result<()>

   #[tauri::command]
   async fn get_recording_status() -> Result<RecordingStatus>
   ```

3. **UI Components**
   - "Start Recording" button in header (prominent, explicit)
   - Button changes to "Stop Recording" when active
   - Display selector (if multiple monitors)
   - Recording status indicator
   - "Open Storage Folder" button

4. **Storage System**
   - Save to `Config.storage_path`
   - Organize by session/date
   - Track in database
   - Auto-delete per retention policy

---

## Dependencies

### Common
```toml
[dependencies]
image = "0.25"
thiserror = "2.0"
chrono = "0.4"
tokio = { version = "1", features = ["full"] }
```

### Platform-Specific

**macOS:**
```toml
[target.'cfg(target_os = "macos")'.dependencies]
screencapturekit = "0.2"
core-graphics = "0.23"
```

**Windows:**
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

**Linux:**
```toml
[target.'cfg(target_os = "linux")'.dependencies]
x11 = { version = "2.21", features = ["xlib", "xrandr"] }
ashpd = "0.9"  # For future Wayland support
```

---

## File Structure

```
src-tauri/src/
├── models/
│   ├── mod.rs
│   └── capture.rs              # Common data structures
├── platform/
│   └── capture/
│       ├── mod.rs              # Platform abstraction
│       ├── macos.rs            # macOS implementation
│       ├── windows.rs          # Windows implementation
│       └── linux.rs            # Linux implementation
└── examples/
    └── test_capture.rs         # Cross-platform test example
```

---

## Documentation

- [macOS Implementation Details](./_docs/devlog/2025-10-03-theme-switching-implementation.md)
- [Windows Implementation Details](./_docs/devlog/2025-10-03-windows-screen-capture.md)
- [Linux Implementation Details](./_docs/devlog/2025-10-03-linux-screen-capture.md)
- [Project Architecture](../CLAUDE.md)

---

## Important UX Requirements (Future Implementation)

When building the recording service and UI:

⚠️ **NO AUTOMATIC RECORDING**
- App MUST NOT start recording automatically
- Requires explicit user action

✅ **UI Requirements:**
1. **Prominent "Start Recording" button** in header
2. Button changes to **"Stop Recording"** when active
3. **"Open Storage Folder"** button for quick access
4. Clear recording status indicator

✅ **Quality Settings:**
- Frame dimensions/resolution control
- Compression level
- Encoding type (PNG sequence vs H.264 video)
- FPS control (from config)

---

## Troubleshooting

### macOS

**Problem:** "Permission denied" error
**Solution:** Grant screen recording permission in System Settings > Privacy & Security > Screen Recording

**Problem:** Captured resolution doesn't match display resolution
**Solution:** This is expected on Retina displays. Logical 1920x1080 captures as 3840x2160 (2x scaling).

### Windows

**Problem:** Desktop Duplication fails in RDP
**Solution:** Automatic fallback to GDI BitBlt (expected behavior)

**Problem:** "Access denied" error
**Solution:** Another app may be using Desktop Duplication, or elevation required

### Linux

**Problem:** "Failed to open X11 display"
**Solution:** Ensure `DISPLAY` environment variable is set, X11 server is running

**Problem:** "Wayland not supported" error
**Solution:** Switch to X11 session or use XWayland compatibility mode

**Problem:** Missing dependencies on build
**Solution:** Install X11 development libraries:
```bash
# Ubuntu/Debian
sudo apt-get install libx11-dev libxrandr-dev

# Fedora
sudo dnf install libX11-devel libXrandr-devel
```

---

## License & Attribution

Part of the Observer project - privacy-first screen recording and activity tracking.

**Third-party dependencies:**
- Core Graphics (macOS) - Apple Inc.
- DirectX/DXGI (Windows) - Microsoft Corporation
- Xlib/XRandR (Linux) - X.Org Foundation
- Image processing - `image` crate contributors

---

**Last Updated:** 2025-10-03
**Phase:** 2 Complete (Screen Capture Infrastructure)
**Next Phase:** 3 - Recording Service & UI Integration
