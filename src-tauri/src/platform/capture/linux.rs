// Linux screen capture implementation supporting both X11 and Wayland

use crate::models::capture::{CaptureError, CaptureResult, Display, PixelFormat, RawFrame};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Display server type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayServer {
    X11,
    Wayland,
    Unknown,
}

impl DisplayServer {
    /// Detect which display server is currently running
    pub fn detect() -> Self {
        // Check for Wayland first (as X11 compatibility layer may set DISPLAY on Wayland)
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            DisplayServer::Wayland
        } else if std::env::var("DISPLAY").is_ok() {
            DisplayServer::X11
        } else {
            DisplayServer::Unknown
        }
    }
}

/// Linux screen capture implementation
/// Supports both X11 and Wayland display servers
pub struct LinuxScreenCapture {
    is_capturing: Arc<AtomicBool>,
    current_display_id: Option<u32>,
    display_server: DisplayServer,
}

impl LinuxScreenCapture {
    /// Create a new Linux screen capture instance
    pub async fn new() -> CaptureResult<Self> {
        let display_server = DisplayServer::detect();

        match display_server {
            DisplayServer::X11 => {
                // X11 is available, proceed
                Ok(Self {
                    is_capturing: Arc::new(AtomicBool::new(false)),
                    current_display_id: None,
                    display_server,
                })
            }
            DisplayServer::Wayland => {
                // Wayland requires portal setup
                // For now, we create the instance but capture will require user permission
                Ok(Self {
                    is_capturing: Arc::new(AtomicBool::new(false)),
                    current_display_id: None,
                    display_server,
                })
            }
            DisplayServer::Unknown => {
                Err(CaptureError::CaptureFailed(
                    "No display server detected. Neither DISPLAY nor WAYLAND_DISPLAY is set.".to_string()
                ))
            }
        }
    }

    /// Get list of all available displays
    pub async fn get_displays() -> CaptureResult<Vec<Display>> {
        let display_server = DisplayServer::detect();

        match display_server {
            DisplayServer::X11 => Self::get_displays_x11().await,
            DisplayServer::Wayland => Self::get_displays_wayland().await,
            DisplayServer::Unknown => {
                Err(CaptureError::CaptureFailed(
                    "No display server detected".to_string()
                ))
            }
        }
    }

    /// Get displays on X11
    async fn get_displays_x11() -> CaptureResult<Vec<Display>> {
        unsafe {
            // Open X11 display
            let display = x11::xlib::XOpenDisplay(std::ptr::null());
            if display.is_null() {
                return Err(CaptureError::CaptureFailed(
                    "Failed to open X11 display".to_string()
                ));
            }

            let screen = x11::xlib::XDefaultScreen(display);
            let root = x11::xlib::XRootWindow(display, screen);

            // Get screen dimensions
            let mut root_return = 0;
            let mut x = 0;
            let mut y = 0;
            let mut width = 0u32;
            let mut height = 0u32;
            let mut border = 0u32;
            let mut depth = 0u32;

            x11::xlib::XGetGeometry(
                display,
                root,
                &mut root_return,
                &mut x,
                &mut y,
                &mut width,
                &mut height,
                &mut border,
                &mut depth,
            );

            // Try to get RandR extension info for multi-monitor
            let mut displays = Vec::new();

            // Check if XRandR extension is available
            let mut event_base = 0;
            let mut error_base = 0;
            let has_randr = x11::xrandr::XRRQueryExtension(display, &mut event_base, &mut error_base) != 0;

            if has_randr {
                // Use XRandR to get screen resources
                let screen_resources = x11::xrandr::XRRGetScreenResources(display, root);
                if !screen_resources.is_null() {
                    let noutput = (*screen_resources).noutput;

                    for i in 0..noutput {
                        let output = *(*screen_resources).outputs.add(i as usize);
                        let output_info = x11::xrandr::XRRGetOutputInfo(display, screen_resources, output);

                        if !output_info.is_null() && (*output_info).connection == x11::xrandr::RR_Connected as u16 {
                            if (*output_info).crtc != 0 {
                                let crtc_info = x11::xrandr::XRRGetCrtcInfo(display, screen_resources, (*output_info).crtc);

                                if !crtc_info.is_null() {
                                    let name = if !(*output_info).name.is_null() {
                                        let name_slice = std::slice::from_raw_parts(
                                            (*output_info).name as *const u8,
                                            (*output_info).nameLen as usize,
                                        );
                                        String::from_utf8_lossy(name_slice).to_string()
                                    } else {
                                        format!("Display {}", i)
                                    };

                                    let is_primary = (*crtc_info).x == 0 && (*crtc_info).y == 0;

                                    displays.push(Display {
                                        id: i as u32,
                                        name: format!("{} ({}x{})", name, (*crtc_info).width, (*crtc_info).height),
                                        width: (*crtc_info).width as u32,
                                        height: (*crtc_info).height as u32,
                                        is_primary,
                                    });

                                    x11::xrandr::XRRFreeCrtcInfo(crtc_info);
                                }
                            }

                            x11::xrandr::XRRFreeOutputInfo(output_info);
                        }
                    }

                    x11::xrandr::XRRFreeScreenResources(screen_resources);
                }
            }

            // If no displays found via RandR, fall back to default screen
            if displays.is_empty() {
                displays.push(Display {
                    id: 0,
                    name: format!("Default Display ({}x{})", width, height),
                    width,
                    height,
                    is_primary: true,
                });
            }

            x11::xlib::XCloseDisplay(display);

            Ok(displays)
        }
    }

    /// Get displays on Wayland
    async fn get_displays_wayland() -> CaptureResult<Vec<Display>> {
        // On Wayland, we can't enumerate displays without user permission
        // We'll return a generic "Primary Display" entry
        // The actual dimensions will be determined when capturing

        // Note: Wayland's security model doesn't allow querying display info
        // without user permission via the portal
        Ok(vec![Display {
            id: 0,
            name: "Primary Display (Wayland)".to_string(),
            width: 1920, // Placeholder, will be updated on capture
            height: 1080, // Placeholder, will be updated on capture
            is_primary: true,
        }])
    }

    /// Capture a single frame from the specified display
    pub async fn capture_frame(display_id: u32) -> CaptureResult<RawFrame> {
        let display_server = DisplayServer::detect();

        match display_server {
            DisplayServer::X11 => Self::capture_frame_x11(display_id).await,
            DisplayServer::Wayland => Self::capture_frame_wayland(display_id).await,
            DisplayServer::Unknown => {
                Err(CaptureError::CaptureFailed(
                    "No display server detected".to_string()
                ))
            }
        }
    }

    /// Capture frame using X11
    async fn capture_frame_x11(display_id: u32) -> CaptureResult<RawFrame> {
        let timestamp = chrono::Utc::now().timestamp_millis();

        unsafe {
            // Open X11 display
            let display = x11::xlib::XOpenDisplay(std::ptr::null());
            if display.is_null() {
                return Err(CaptureError::CaptureFailed(
                    "Failed to open X11 display".to_string()
                ));
            }

            let screen = x11::xlib::XDefaultScreen(display);
            let root = x11::xlib::XRootWindow(display, screen);

            // Get screen dimensions
            let mut root_return = 0;
            let mut x = 0;
            let mut y = 0;
            let mut width = 0u32;
            let mut height = 0u32;
            let mut border = 0u32;
            let mut depth = 0u32;

            x11::xlib::XGetGeometry(
                display,
                root,
                &mut root_return,
                &mut x,
                &mut y,
                &mut width,
                &mut height,
                &mut border,
                &mut depth,
            );

            // For multi-monitor, we should get the specific monitor's geometry
            // For now, we capture the entire root window
            // TODO: Support capturing specific monitors via RandR

            // Capture the screen
            let image = x11::xlib::XGetImage(
                display,
                root,
                0,
                0,
                width,
                height,
                x11::xlib::XAllPlanes(),
                x11::xlib::ZPixmap,
            );

            if image.is_null() {
                x11::xlib::XCloseDisplay(display);
                return Err(CaptureError::CaptureFailed(
                    "Failed to capture X11 image".to_string()
                ));
            }

            // Get image data
            let bytes_per_pixel = ((*image).bits_per_pixel / 8) as usize;
            let bytes_per_line = (*image).bytes_per_line as usize;
            let image_data = (*image).data;

            // Convert to RGBA format
            let pixel_count = (width * height) as usize;
            let mut pixel_data = Vec::with_capacity(pixel_count * 4);

            for row in 0..height {
                for col in 0..width {
                    let offset = (row as usize * bytes_per_line) + (col as usize * bytes_per_pixel);

                    if bytes_per_pixel == 4 {
                        // BGRA or RGBA format
                        let b = *image_data.add(offset);
                        let g = *image_data.add(offset + 1);
                        let r = *image_data.add(offset + 2);
                        let a = *image_data.add(offset + 3);

                        // X11 typically uses BGRA
                        pixel_data.push(b);
                        pixel_data.push(g);
                        pixel_data.push(r);
                        pixel_data.push(a);
                    } else if bytes_per_pixel == 3 {
                        // BGR format
                        let b = *image_data.add(offset);
                        let g = *image_data.add(offset + 1);
                        let r = *image_data.add(offset + 2);

                        pixel_data.push(b);
                        pixel_data.push(g);
                        pixel_data.push(r);
                        pixel_data.push(255); // Alpha
                    } else {
                        // Unsupported format
                        x11::xlib::XDestroyImage(image);
                        x11::xlib::XCloseDisplay(display);
                        return Err(CaptureError::CaptureFailed(
                            format!("Unsupported pixel format: {} bytes per pixel", bytes_per_pixel)
                        ));
                    }
                }
            }

            x11::xlib::XDestroyImage(image);
            x11::xlib::XCloseDisplay(display);

            Ok(RawFrame {
                timestamp,
                width,
                height,
                data: pixel_data,
                format: PixelFormat::BGRA8,
            })
        }
    }

    /// Capture frame using Wayland (via XDG Desktop Portal)
    async fn capture_frame_wayland(_display_id: u32) -> CaptureResult<RawFrame> {
        // Wayland screen capture requires using the XDG Desktop Portal
        // This is a complex async operation that requires:
        // 1. Creating a portal session
        // 2. Requesting screen share permission (shows dialog to user)
        // 3. Setting up PipeWire stream
        // 4. Capturing frames from the stream

        // For this implementation, we'll return an error with instructions
        // A full implementation would use ashpd to interact with the portal

        Err(CaptureError::CaptureFailed(
            "Wayland screen capture requires XDG Desktop Portal integration. \
            This is not yet fully implemented. Please use X11 for now, or use \
            a tool like OBS Studio which has full Wayland PipeWire support.".to_string()
        ))

        // TODO: Full Wayland implementation would look like:
        // 1. Use ashpd::desktop::screenshot::Screenshot or screencast
        // 2. Request permission from user
        // 3. Get PipeWire stream FD
        // 4. Use pipewire crate to capture frames
        // This requires significant additional dependencies and complexity
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

    /// Get the detected display server type
    pub fn display_server(&self) -> DisplayServer {
        self.display_server
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_server_detection() {
        let server = DisplayServer::detect();
        println!("Detected display server: {:?}", server);

        // Just verify detection doesn't panic
        // The actual result depends on the environment
        match server {
            DisplayServer::X11 => println!("Running on X11"),
            DisplayServer::Wayland => println!("Running on Wayland"),
            DisplayServer::Unknown => println!("No display server detected (headless?)"),
        }
    }

    #[tokio::test]
    async fn test_get_displays() {
        let displays = LinuxScreenCapture::get_displays().await;
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
                // May fail in headless or non-Linux environments
                eprintln!("Failed to get displays: {}", e);
                println!("Note: This is expected if not running on Linux with a display server");
            }
        }
    }

    #[tokio::test]
    async fn test_capture_frame() {
        let displays = match LinuxScreenCapture::get_displays().await {
            Ok(d) => d,
            Err(_) => {
                println!("Skipping frame capture test - no display server available");
                return;
            }
        };

        if displays.is_empty() {
            println!("Skipping frame capture test - no displays found");
            return;
        }

        let primary_display = displays.iter().find(|d| d.is_primary).unwrap();

        let frame = LinuxScreenCapture::capture_frame(primary_display.id).await;
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
                println!("Note: Capture may fail on Wayland or in headless environments");
            }
        }
    }

    #[tokio::test]
    async fn test_capture_lifecycle() {
        let displays = match LinuxScreenCapture::get_displays().await {
            Ok(d) => d,
            Err(_) => {
                println!("Skipping lifecycle test - no display server available");
                return;
            }
        };

        if displays.is_empty() {
            return;
        }

        let primary_display = displays.iter().find(|d| d.is_primary).unwrap();

        let mut capture = match LinuxScreenCapture::new().await {
            Ok(c) => c,
            Err(_) => {
                println!("Skipping lifecycle test - failed to create capture");
                return;
            }
        };

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
