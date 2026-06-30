#[cfg(test)]
mod tests {
    use crate::core::ffmpeg_wrapper::FFmpegEncoder;
    use crate::models::capture::{PixelFormat, RawFrame};

    fn create_test_frame(width: u32, height: u32) -> RawFrame {
        RawFrame {
            data: vec![128u8; (width * height * 4) as usize],
            width,
            height,
            timestamp: 0,
            format: PixelFormat::RGBA8,
        }
    }

    #[test]
    fn test_encoder_creation() {
        let temp_dir = std::env::temp_dir();
        let output_path = temp_dir.join("test_video.mp4");
        let result = FFmpegEncoder::new(&output_path, 640, 480, 30, "libx264", 23);
        assert!(result.is_ok());
        let _ = create_test_frame(2, 2);
    }
}
