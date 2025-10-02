/// Example program to test platform detection and abstraction
/// Run with: cargo run --example test_platform

use zero_lib::platform::get_platform;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Platform Detection Test ===\n");

    let platform = get_platform();

    // Test OS Name
    println!("Operating System:");
    let os_name = platform.get_os_name();
    println!("  Name: {}", os_name);

    // Test OS Version
    let os_version = platform.get_os_version();
    println!("  Version: {}", os_version);
    println!();

    // Test Device ID
    println!("Device Information:");
    match platform.get_device_id() {
        Ok(device_id) => {
            println!("  Device ID: {}", device_id);
            println!("  ID Length: {} characters", device_id.len());
        }
        Err(e) => {
            println!("  Error getting device ID: {}", e);
        }
    }
    println!();

    // Test Screen Recording Support
    println!("Capabilities:");
    let supports_recording = platform.supports_screen_recording();
    println!("  Screen Recording: {}", if supports_recording { "✓ Supported" } else { "✗ Not Supported" });
    println!();

    // Test Data Directory
    println!("Data Storage:");
    match platform.get_data_directory() {
        Ok(data_dir) => {
            println!("  Data Directory: {}", data_dir.display());
            println!("  Is Absolute: {}", data_dir.is_absolute());

            // Check if directory exists or can be created
            if !data_dir.exists() {
                println!("  Status: Does not exist (will be created when needed)");
            } else {
                println!("  Status: ✓ Exists");
                if let Ok(metadata) = std::fs::metadata(&data_dir) {
                    println!("  Is Directory: {}", metadata.is_dir());
                }
            }
        }
        Err(e) => {
            println!("  Error getting data directory: {}", e);
        }
    }
    println!();

    // Test platform consistency
    println!("=== Consistency Test ===\n");
    println!("Testing device ID stability across multiple calls...");

    let id1 = platform.get_device_id()?;
    let id2 = platform.get_device_id()?;
    let id3 = platform.get_device_id()?;

    if id1 == id2 && id2 == id3 {
        println!("✓ Device ID is stable across calls");
    } else {
        println!("✗ Warning: Device ID is not consistent!");
        println!("  Call 1: {}", id1);
        println!("  Call 2: {}", id2);
        println!("  Call 3: {}", id3);
    }
    println!();

    // Platform-specific information
    println!("=== Platform-Specific Details ===\n");

    #[cfg(target_os = "macos")]
    {
        println!("Running on macOS");
        println!("  Using Hardware UUID for device identification");
        println!("  Screen recording via AVFoundation/ScreenCaptureKit");
        println!("  Data location: ~/. observer_data");
    }

    #[cfg(target_os = "windows")]
    {
        println!("Running on Windows");
        println!("  Using MachineGuid for device identification");
        println!("  Screen recording via Windows.Graphics.Capture API");
        println!("  Data location: %APPDATA%\\.observer_data");
    }

    #[cfg(target_os = "linux")]
    {
        println!("Running on Linux");
        println!("  Using /etc/machine-id for device identification");
        println!("  Screen recording via X11/Wayland (PipeWire)");
        println!("  Data location: ~/.observer_data (XDG compatible)");
    }

    println!("\n✓ Platform detection test complete!");

    Ok(())
}
