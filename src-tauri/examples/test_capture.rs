// Test cross-platform screen capture functionality

use zero_lib::models::capture::{PixelFormat};
use zero_lib::platform::capture::PlatformCapture;

#[tokio::main]
async fn main() {
    #[cfg(target_os = "macos")]
    println!("=== macOS Screen Capture Test ===\n");

    #[cfg(target_os = "windows")]
    println!("=== Windows Screen Capture Test ===\n");

    #[cfg(target_os = "linux")]
    println!("=== Linux Screen Capture Test ===\n");

    // Test 1: Get displays
    println!("Test 1: Enumerating displays...");
    match PlatformCapture::get_displays().await {
        Ok(displays) => {
            println!("✓ Found {} display(s):\n", displays.len());
            for display in &displays {
                println!(
                    "  Display {} - {}",
                    display.id,
                    if display.is_primary { "[PRIMARY]" } else { "" }
                );
                println!("    Name: {}", display.name);
                println!("    Resolution: {}x{}", display.width, display.height);
                println!();
            }

            // Test 2: Capture frame from primary display
            if let Some(primary) = displays.iter().find(|d| d.is_primary) {
                println!("\nTest 2: Capturing frame from primary display (ID {})...", primary.id);
                match PlatformCapture::capture_frame(primary.id).await {
                    Ok(frame) => {
                        println!("✓ Successfully captured frame:");
                        println!("    Timestamp: {}", frame.timestamp);
                        println!("    Resolution: {}x{}", frame.width, frame.height);
                        println!("    Data size: {} bytes", frame.data.len());
                        println!("    Format: {:?}", frame.format);

                        // Test 3: Save frame as PNG
                        println!("\nTest 3: Saving frame as PNG...");
                        if let Err(e) = save_frame_as_png(&frame, "test_screenshot.png") {
                            println!("✗ Failed to save PNG: {}", e);
                        } else {
                            println!("✓ Saved to test_screenshot.png");
                        }
                    }
                    Err(e) => {
                        println!("✗ Failed to capture frame: {}", e);
                    }
                }
            }

            // Test 4: Capture lifecycle
            println!("\nTest 4: Testing capture lifecycle...");
            match PlatformCapture::new().await {
                Ok(mut capture) => {
                    println!("✓ Created capture instance");

                    if let Some(primary) = displays.iter().find(|d| d.is_primary) {
                        // Start capture
                        match capture.start_capture(primary.id).await {
                            Ok(_) => {
                                println!("✓ Started capture");
                                println!("  Is capturing: {}", capture.is_capturing());
                                println!("  Display ID: {:?}", capture.current_display_id());

                                // Stop capture
                                match capture.stop_capture().await {
                                    Ok(_) => {
                                        println!("✓ Stopped capture");
                                        println!("  Is capturing: {}", capture.is_capturing());
                                    }
                                    Err(e) => println!("✗ Failed to stop capture: {}", e),
                                }
                            }
                            Err(e) => println!("✗ Failed to start capture: {}", e),
                        }
                    }
                }
                Err(e) => {
                    println!("✗ Failed to create capture instance: {}", e);
                    #[cfg(target_os = "macos")]
                    println!("  This likely means screen recording permission is not granted.");
                    #[cfg(target_os = "macos")]
                    println!("  Please grant permission in System Settings > Privacy & Security > Screen Recording");
                    #[cfg(target_os = "windows")]
                    println!("  Desktop Duplication may be unavailable (RDP session, permissions, or already in use)");
                    #[cfg(target_os = "linux")]
                    println!("  Linux screen capture is not yet implemented");
                }
            }
        }
        Err(e) => {
            println!("✗ Failed to get displays: {}", e);
        }
    }

    println!("\n=== Test Complete ===");
}

fn save_frame_as_png(
    frame: &zero_lib::models::capture::RawFrame,
    filename: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use image::{ImageBuffer, Rgba};

    // Convert BGRA to RGBA if needed
    let rgba_data = match frame.format {
        PixelFormat::BGRA8 => {
            // Convert BGRA to RGBA
            let mut rgba = Vec::with_capacity(frame.data.len());
            for chunk in frame.data.chunks_exact(4) {
                rgba.push(chunk[2]); // R (from B)
                rgba.push(chunk[1]); // G
                rgba.push(chunk[0]); // B (from R)
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
