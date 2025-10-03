use crate::models::capture::RawFrame;
use std::path::PathBuf;
use tokio::sync::mpsc::Receiver;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum VideoEncoderError {
    #[error("FFmpeg error: {0}")]
    FFmpeg(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid codec: {0}")]
    InvalidCodec(String),
    #[error("Hardware acceleration not available")]
    HardwareAccelerationNotAvailable,
    #[error("Encoding failed: {0}")]
    EncodingFailed(String),
}

pub type Result<T> = std::result::Result<T, VideoEncoderError>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum VideoCodec {
    H264,
    // Future: H265, VP9
}

impl VideoCodec {
    pub fn to_ffmpeg_codec_name(&self, hardware_acceleration: bool, platform: &str) -> String {
        match self {
            VideoCodec::H264 => {
                if hardware_acceleration {
                    match platform {
                        "macos" => "h264_videotoolbox".to_string(),
                        "windows" => "h264_nvenc".to_string(), // Could also try h264_qsv
                        "linux" => "h264_vaapi".to_string(),   // Could also try h264_nvenc
                        _ => "libx264".to_string(),
                    }
                } else {
                    "libx264".to_string()
                }
            }
        }
    }

    pub fn software_fallback_name(&self) -> &'static str {
        match self {
            VideoCodec::H264 => "libx264",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CompressionQuality {
    High,   // CRF 18-23
    Medium, // CRF 23-28 (default)
    Low,    // CRF 28-35
}

impl CompressionQuality {
    pub fn to_crf(&self) -> u32 {
        match self {
            CompressionQuality::High => 20,
            CompressionQuality::Medium => 25,
            CompressionQuality::Low => 30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoSegment {
    pub path: PathBuf,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub frame_count: u32,
    pub duration_ms: u64,
    pub file_size_bytes: u64,
}


pub struct VideoEncoder {
    codec: VideoCodec,
    quality: CompressionQuality,
    hardware_acceleration: bool,
    platform: String,
}

impl VideoEncoder {
    pub fn new(
        codec: VideoCodec,
        quality: CompressionQuality,
        hardware_acceleration: bool,
    ) -> Result<Self> {
        let platform = if cfg!(target_os = "macos") {
            "macos"
        } else if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "linux") {
            "linux"
        } else {
            "unknown"
        };

        Ok(Self {
            codec,
            quality,
            hardware_acceleration,
            platform: platform.to_string(),
        })
    }

    /// Encode a batch of frames into a video file
    pub async fn encode_frames(
        &self,
        frames: Vec<RawFrame>,
        output_path: PathBuf,
        fps: u32,
    ) -> Result<VideoSegment> {
        if frames.is_empty() {
            return Err(VideoEncoderError::EncodingFailed(
                "No frames to encode".to_string(),
            ));
        }

        let start_timestamp = frames.first().unwrap().timestamp;
        let end_timestamp = frames.last().unwrap().timestamp;
        let frame_count = frames.len() as u32;

        // Encode frames in a blocking task since FFmpeg is synchronous
        let codec = self.codec;
        let quality = self.quality;
        let hardware_acceleration = self.hardware_acceleration;
        let platform = self.platform.clone();
        let output_path_clone = output_path.clone();

        tokio::task::spawn_blocking(move || {
            Self::encode_frames_sync(
                frames,
                &output_path_clone,
                fps,
                codec,
                quality,
                hardware_acceleration,
                &platform,
            )
        })
        .await
        .map_err(|e| VideoEncoderError::EncodingFailed(format!("Task join error: {}", e)))??;

        let file_size_bytes = tokio::fs::metadata(&output_path)
            .await?
            .len();

        let duration_ms = ((end_timestamp - start_timestamp) as u64).max(1);

        Ok(VideoSegment {
            path: output_path,
            start_timestamp,
            end_timestamp,
            frame_count,
            duration_ms,
            file_size_bytes,
        })
    }

    /// Encode frames from a stream (async receiver)
    pub async fn encode_frame_stream(
        &self,
        mut frame_receiver: Receiver<RawFrame>,
        output_path: PathBuf,
        fps: u32,
    ) -> Result<VideoSegment> {
        let mut frames = Vec::new();

        while let Some(frame) = frame_receiver.recv().await {
            frames.push(frame);
        }

        self.encode_frames(frames, output_path, fps).await
    }

    #[allow(unused_variables)]
    fn encode_frames_sync(
        frames: Vec<RawFrame>,
        output_path: &PathBuf,
        fps: u32,
        codec: VideoCodec,
        quality: CompressionQuality,
        hardware_acceleration: bool,
        platform: &str,
    ) -> Result<()> {
        // This is a placeholder implementation since ffmpeg-next requires significant setup
        // In a real implementation, this would:
        // 1. Initialize FFmpeg context
        // 2. Create encoder with specified codec
        // 3. Configure encoder with CRF, preset, pixel format
        // 4. Try hardware acceleration, fallback to software if needed
        // 5. Feed frames to encoder
        // 6. Write encoded packets to output file

        // For now, we'll use a simple approach with the image crate to save as individual frames
        // This is temporary until full FFmpeg integration is implemented

        println!("VideoEncoder: Would encode {} frames to {:?}", frames.len(), output_path);
        println!("  Codec: {:?}, Quality: {:?}, FPS: {}", codec, quality, fps);
        println!("  Hardware acceleration: {}, Platform: {}", hardware_acceleration, platform);

        // Create output directory if it doesn't exist
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // For testing purposes, create a simple marker file
        // In production, this would be replaced with actual FFmpeg encoding
        std::fs::write(
            output_path,
            format!(
                "Video segment: {} frames at {}fps\nCodec: {:?}\nQuality: {:?}\n",
                frames.len(),
                fps,
                codec,
                quality
            ),
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::capture::PixelFormat;

    fn create_test_frame(width: u32, height: u32, timestamp: i64) -> RawFrame {
        let data = vec![0u8; (width * height * 4) as usize];
        RawFrame {
            data,
            width,
            height,
            timestamp,
            format: PixelFormat::RGBA8,
        }
    }

    #[tokio::test]
    async fn test_encode_single_frame() {
        let encoder = VideoEncoder::new(
            VideoCodec::H264,
            CompressionQuality::Medium,
            false,
        )
        .unwrap();

        let frame = create_test_frame(1920, 1080, 1234567890);
        let output_path = PathBuf::from("/tmp/test_single_frame.mp4");

        let result = encoder
            .encode_frames(vec![frame], output_path.clone(), 15)
            .await;

        assert!(result.is_ok());
        let segment = result.unwrap();
        assert_eq!(segment.frame_count, 1);
        assert_eq!(segment.start_timestamp, 1234567890);
        assert_eq!(segment.end_timestamp, 1234567890);

        // Cleanup
        let _ = tokio::fs::remove_file(output_path).await;
    }

    #[tokio::test]
    async fn test_encode_multiple_frames() {
        let encoder = VideoEncoder::new(
            VideoCodec::H264,
            CompressionQuality::High,
            false,
        )
        .unwrap();

        let frames = vec![
            create_test_frame(1920, 1080, 1000),
            create_test_frame(1920, 1080, 1100),
            create_test_frame(1920, 1080, 1200),
            create_test_frame(1920, 1080, 1300),
        ];
        let output_path = PathBuf::from("/tmp/test_multiple_frames.mp4");

        let result = encoder
            .encode_frames(frames, output_path.clone(), 15)
            .await;

        assert!(result.is_ok());
        let segment = result.unwrap();
        assert_eq!(segment.frame_count, 4);
        assert_eq!(segment.start_timestamp, 1000);
        assert_eq!(segment.end_timestamp, 1300);
        assert_eq!(segment.duration_ms, 300);

        // Cleanup
        let _ = tokio::fs::remove_file(output_path).await;
    }

    #[tokio::test]
    async fn test_encode_empty_frames() {
        let encoder = VideoEncoder::new(
            VideoCodec::H264,
            CompressionQuality::Low,
            false,
        )
        .unwrap();

        let output_path = PathBuf::from("/tmp/test_empty.mp4");
        let result = encoder.encode_frames(vec![], output_path, 15).await;

        assert!(result.is_err());
        match result {
            Err(VideoEncoderError::EncodingFailed(msg)) => {
                assert!(msg.contains("No frames"));
            }
            _ => panic!("Expected EncodingFailed error"),
        }
    }

    #[tokio::test]
    async fn test_encode_frame_stream() {
        let encoder = VideoEncoder::new(
            VideoCodec::H264,
            CompressionQuality::Medium,
            false,
        )
        .unwrap();

        let (tx, rx) = tokio::sync::mpsc::channel(10);

        // Send frames in a separate task
        tokio::spawn(async move {
            for i in 0..5 {
                let frame = create_test_frame(1280, 720, 2000 + i * 100);
                tx.send(frame).await.unwrap();
            }
            // Channel closes when tx is dropped
        });

        let output_path = PathBuf::from("/tmp/test_stream.mp4");
        let result = encoder.encode_frame_stream(rx, output_path.clone(), 15).await;

        assert!(result.is_ok());
        let segment = result.unwrap();
        assert_eq!(segment.frame_count, 5);
        assert_eq!(segment.start_timestamp, 2000);
        assert_eq!(segment.end_timestamp, 2400);

        // Cleanup
        let _ = tokio::fs::remove_file(output_path).await;
    }

    #[test]
    fn test_codec_names() {
        let codec = VideoCodec::H264;

        assert_eq!(codec.to_ffmpeg_codec_name(true, "macos"), "h264_videotoolbox");
        assert_eq!(codec.to_ffmpeg_codec_name(true, "windows"), "h264_nvenc");
        assert_eq!(codec.to_ffmpeg_codec_name(true, "linux"), "h264_vaapi");
        assert_eq!(codec.to_ffmpeg_codec_name(false, "macos"), "libx264");
        assert_eq!(codec.software_fallback_name(), "libx264");
    }

    #[test]
    fn test_quality_crf() {
        assert_eq!(CompressionQuality::High.to_crf(), 20);
        assert_eq!(CompressionQuality::Medium.to_crf(), 25);
        assert_eq!(CompressionQuality::Low.to_crf(), 30);
    }

}
