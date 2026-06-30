use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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
}

impl VideoCodec {
    pub fn to_ffmpeg_codec_name(&self, hardware_acceleration: bool, platform: &str) -> String {
        match self {
            VideoCodec::H264 => {
                if hardware_acceleration {
                    match platform {
                        "macos" => "h264_videotoolbox".to_string(),
                        "windows" => "h264_nvenc".to_string(),
                        "linux" => "h264_vaapi".to_string(),
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
    High,
    Medium,
    Low,
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
