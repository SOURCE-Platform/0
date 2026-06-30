use thiserror::Error;

#[derive(Error, Debug)]
pub enum FFmpegError {
    #[error("Failed to allocate codec context")]
    CodecContextAllocation,
    #[error("Codec not found: {0}")]
    CodecNotFound(String),
    #[error("Failed to open codec: {0}")]
    CodecOpenFailed(String),
    #[error("Failed to allocate frame")]
    FrameAllocation,
    #[error("Failed to allocate packet")]
    PacketAllocation,
    #[error("Failed to create output format context")]
    FormatContextCreation,
    #[error("Failed to create video stream")]
    StreamCreation,
    #[error("Failed to write header")]
    WriteHeaderFailed,
    #[error("Encoding error: {0}")]
    EncodingError(String),
    #[error("Failed to initialize swscale context")]
    SwscaleInitFailed,
    #[error("Color conversion failed")]
    ColorConversionFailed,
}

pub type Result<T> = std::result::Result<T, FFmpegError>;
