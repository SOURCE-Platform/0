/// Test video encoding with FFmpeg
///
/// This example creates a few test frames and encodes them to an MP4 file
/// to verify the FFmpeg integration works correctly.
///
/// Run with: cargo run --example test_video_encoding

use std::path::PathBuf;
use zero_lib::core::video_encoder::{VideoEncoder, VideoCodec, CompressionQuality};
use zero_lib::models::capture::{RawFrame, PixelFormat};

fn create_colored_frame(width: u32, height: u32, r: u8, g: u8, b: u8, timestamp: i64) -> RawFrame {
    let mut data = Vec::with_capacity((width * height * 4) as usize);

    for _ in 0..(width * height) {
        data.push(r); // R
        data.push(g); // G
        data.push(b); // B
        data.push(255); // A (fully opaque)
    }

    RawFrame {
        data,
        width,
        height,
        timestamp,
        format: PixelFormat::RGBA8,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Video Encoding Test ===\n");

    // Create test frames (60 frames = 2 seconds at 30fps)
    println!("Creating 60 test frames...");
    let mut frames = Vec::new();
    let width = 640;
    let height = 480;
    let fps = 30;

    for i in 0..60 {
        let timestamp = i * (1000 / fps as i64); // milliseconds

        // Cycle through colors
        let (r, g, b) = match i % 6 {
            0 => (255, 0, 0),     // Red
            1 => (255, 128, 0),   // Orange
            2 => (255, 255, 0),   // Yellow
            3 => (0, 255, 0),     // Green
            4 => (0, 0, 255),     // Blue
            5 => (128, 0, 255),   // Purple
            _ => (128, 128, 128), // Gray
        };

        frames.push(create_colored_frame(width, height, r, g, b, timestamp));
    }

    println!("✓ Created {} frames ({}x{})", frames.len(), width, height);

    // Create output path
    let output_dir = std::env::temp_dir().join("observer_test");
    std::fs::create_dir_all(&output_dir)?;
    let output_path = output_dir.join("test_video.mp4");

    println!("\nOutput path: {:?}", output_path);

    // Test 1: Hardware acceleration (VideoToolbox on macOS)
    println!("\n--- Test 1: Hardware Acceleration ---");
    let encoder = VideoEncoder::new(
        VideoCodec::H264,
        CompressionQuality::Medium,
        true, // hardware acceleration
    )?;

    match encoder.encode_frames(frames.clone(), output_path.clone(), fps).await {
        Ok(segment) => {
            println!("✓ Hardware encoding successful!");
            println!("  - Frames: {}", segment.frame_count);
            println!("  - Duration: {} ms", segment.duration_ms);
            println!("  - File size: {} KB", segment.file_size_bytes / 1024);
            println!("  - Compression ratio: {:.1}:1",
                     (frames.len() * width as usize * height as usize * 4) as f64 / segment.file_size_bytes as f64);
        }
        Err(e) => {
            println!("✗ Hardware encoding failed: {}", e);
            println!("  (This is expected if VideoToolbox is not available)");
        }
    }

    // Test 2: Software encoding (libx264)
    println!("\n--- Test 2: Software Encoding (Fallback) ---");
    let output_path_software = output_dir.join("test_video_software.mp4");

    let encoder_software = VideoEncoder::new(
        VideoCodec::H264,
        CompressionQuality::Medium,
        false, // no hardware acceleration
    )?;

    match encoder_software.encode_frames(frames.clone(), output_path_software.clone(), fps).await {
        Ok(segment) => {
            println!("✓ Software encoding successful!");
            println!("  - Frames: {}", segment.frame_count);
            println!("  - Duration: {} ms", segment.duration_ms);
            println!("  - File size: {} KB", segment.file_size_bytes / 1024);
            println!("  - Path: {:?}", segment.path);
        }
        Err(e) => {
            println!("✗ Software encoding failed: {}", e);
            return Err(e.into());
        }
    }

    // Verify the file exists and is playable
    println!("\n--- Verification ---");
    if output_path_software.exists() {
        let metadata = std::fs::metadata(&output_path_software)?;
        println!("✓ Output file exists");
        println!("  Size: {} bytes", metadata.len());

        if metadata.len() > 1000 {
            println!("✓ File size is reasonable (>1KB)");
        } else {
            println!("✗ File size seems too small");
        }

        // Try to probe with ffmpeg
        println!("\nProbing video with ffprobe...");
        let output = std::process::Command::new("ffprobe")
            .args(&[
                "-v", "quiet",
                "-print_format", "json",
                "-show_streams",
                output_path_software.to_str().unwrap(),
            ])
            .output();

        match output {
            Ok(result) if result.status.success() => {
                println!("✓ Video is valid and can be read by FFmpeg");
                let json_str = String::from_utf8_lossy(&result.stdout);
                if json_str.contains("\"codec_name\": \"h264\"") {
                    println!("✓ Codec is H.264");
                }
                if json_str.contains("\"width\": 640") {
                    println!("✓ Width is correct (640)");
                }
                if json_str.contains("\"height\": 480") {
                    println!("✓ Height is correct (480)");
                }
            }
            _ => {
                println!("⚠ Could not probe video with ffprobe (is ffprobe installed?)");
            }
        }
    } else {
        println!("✗ Output file does not exist!");
        return Err("Output file not created".into());
    }

    println!("\n=== Test Complete ===");
    println!("Video files saved to: {:?}", output_dir);
    println!("\nYou can play the videos with:");
    println!("  ffplay {:?}", output_path_software);
    println!("  or open them in any video player");

    Ok(())
}
