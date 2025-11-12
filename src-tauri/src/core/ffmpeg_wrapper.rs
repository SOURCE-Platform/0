/// FFmpeg wrapper providing safe Rust interfaces around unsafe FFmpeg C bindings
///
/// This module encapsulates all unsafe FFmpeg operations and provides a safe API
/// for video encoding operations.

use crate::models::capture::{RawFrame, PixelFormat};
use std::ffi::CString;
use std::path::Path;
use std::ptr;
use thiserror::Error;

// Import FFmpeg C bindings
use ffmpeg_sys_next::*;

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

/// Safe wrapper around FFmpeg encoder
pub struct FFmpegEncoder {
    codec_context: *mut AVCodecContext,
    format_context: *mut AVFormatContext,
    video_stream: *mut AVStream,
    frame: *mut AVFrame,
    packet: *mut AVPacket,
    sws_context: *mut SwsContext,
    frame_count: i64,
}

unsafe impl Send for FFmpegEncoder {}

impl FFmpegEncoder {
    /// Create a new encoder
    ///
    /// # Arguments
    /// * `output_path` - Path to output MP4 file
    /// * `width` - Video width in pixels
    /// * `height` - Video height in pixels
    /// * `fps` - Frames per second
    /// * `codec_name` - FFmpeg codec name (e.g., "h264_videotoolbox", "libx264")
    /// * `crf` - Constant Rate Factor for quality (lower = better quality)
    pub fn new(
        output_path: &Path,
        width: u32,
        height: u32,
        fps: u32,
        codec_name: &str,
        crf: u32,
    ) -> Result<Self> {
        unsafe {
            // Convert output path to C string
            let output_path_c = CString::new(output_path.to_str().unwrap())
                .map_err(|_| FFmpegError::FormatContextCreation)?;

            // Find codec
            let codec_name_c = CString::new(codec_name)
                .map_err(|_| FFmpegError::CodecNotFound(codec_name.to_string()))?;
            let codec = avcodec_find_encoder_by_name(codec_name_c.as_ptr());

            if codec.is_null() {
                return Err(FFmpegError::CodecNotFound(codec_name.to_string()));
            }

            // Allocate codec context
            let codec_context = avcodec_alloc_context3(codec);
            if codec_context.is_null() {
                return Err(FFmpegError::CodecContextAllocation);
            }

            // Configure codec context
            (*codec_context).width = width as i32;
            (*codec_context).height = height as i32;
            (*codec_context).time_base = AVRational {
                num: 1,
                den: fps as i32,
            };
            (*codec_context).framerate = AVRational {
                num: fps as i32,
                den: 1,
            };
            (*codec_context).pix_fmt = AVPixelFormat::AV_PIX_FMT_YUV420P;
            (*codec_context).gop_size = fps as i32 * 2; // Keyframe every 2 seconds
            (*codec_context).max_b_frames = 2;

            // Set CRF for quality control (H.264 specific)
            let crf_str = CString::new(crf.to_string()).unwrap();
            let crf_key = CString::new("crf").unwrap();
            av_opt_set(
                (*codec_context).priv_data,
                crf_key.as_ptr(),
                crf_str.as_ptr(),
                0,
            );

            // Set preset to "medium" for balance between speed and compression
            let preset_key = CString::new("preset").unwrap();
            let preset_value = CString::new("medium").unwrap();
            av_opt_set(
                (*codec_context).priv_data,
                preset_key.as_ptr(),
                preset_value.as_ptr(),
                0,
            );

            // Open codec
            let ret = avcodec_open2(codec_context, codec, ptr::null_mut());
            if ret < 0 {
                avcodec_free_context(&mut (codec_context as *mut _));
                return Err(FFmpegError::CodecOpenFailed(format!("Error code: {}", ret)));
            }

            // Create output format context
            let mut format_context: *mut AVFormatContext = ptr::null_mut();
            let ret = avformat_alloc_output_context2(
                &mut format_context,
                ptr::null_mut(),
                ptr::null(),
                output_path_c.as_ptr(),
            );
            if ret < 0 || format_context.is_null() {
                avcodec_free_context(&mut (codec_context as *mut _));
                return Err(FFmpegError::FormatContextCreation);
            }

            // Create video stream
            let video_stream = avformat_new_stream(format_context, ptr::null());
            if video_stream.is_null() {
                avformat_free_context(format_context);
                avcodec_free_context(&mut (codec_context as *mut _));
                return Err(FFmpegError::StreamCreation);
            }

            (*video_stream).time_base = (*codec_context).time_base;

            // Copy codec parameters to stream
            let ret = avcodec_parameters_from_context((*video_stream).codecpar, codec_context);
            if ret < 0 {
                avformat_free_context(format_context);
                avcodec_free_context(&mut (codec_context as *mut _));
                return Err(FFmpegError::StreamCreation);
            }

            // Open output file
            if ((*format_context).oformat).is_null() {
                avformat_free_context(format_context);
                avcodec_free_context(&mut (codec_context as *mut _));
                return Err(FFmpegError::FormatContextCreation);
            }

            if (*(*format_context).oformat).flags & AVFMT_NOFILE == 0 {
                let ret = avio_open(
                    &mut (*format_context).pb,
                    output_path_c.as_ptr(),
                    AVIO_FLAG_WRITE,
                );
                if ret < 0 {
                    avformat_free_context(format_context);
                    avcodec_free_context(&mut (codec_context as *mut _));
                    return Err(FFmpegError::FormatContextCreation);
                }
            }

            // Write file header
            let ret = avformat_write_header(format_context, ptr::null_mut());
            if ret < 0 {
                avio_closep(&mut (*format_context).pb);
                avformat_free_context(format_context);
                avcodec_free_context(&mut (codec_context as *mut _));
                return Err(FFmpegError::WriteHeaderFailed);
            }

            // Allocate frame
            let frame = av_frame_alloc();
            if frame.is_null() {
                avio_closep(&mut (*format_context).pb);
                avformat_free_context(format_context);
                avcodec_free_context(&mut (codec_context as *mut _));
                return Err(FFmpegError::FrameAllocation);
            }

            (*frame).format = AVPixelFormat::AV_PIX_FMT_YUV420P as i32;
            (*frame).width = width as i32;
            (*frame).height = height as i32;

            let ret = av_frame_get_buffer(frame, 0);
            if ret < 0 {
                av_frame_free(&mut (frame as *mut _));
                avio_closep(&mut (*format_context).pb);
                avformat_free_context(format_context);
                avcodec_free_context(&mut (codec_context as *mut _));
                return Err(FFmpegError::FrameAllocation);
            }

            // Allocate packet
            let packet = av_packet_alloc();
            if packet.is_null() {
                av_frame_free(&mut (frame as *mut _));
                avio_closep(&mut (*format_context).pb);
                avformat_free_context(format_context);
                avcodec_free_context(&mut (codec_context as *mut _));
                return Err(FFmpegError::PacketAllocation);
            }

            // Initialize swscale context for RGBA -> YUV420P conversion
            let sws_context = sws_getContext(
                width as i32,
                height as i32,
                AVPixelFormat::AV_PIX_FMT_RGBA,
                width as i32,
                height as i32,
                AVPixelFormat::AV_PIX_FMT_YUV420P,
                1, // SWS_BILINEAR flag
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            );

            if sws_context.is_null() {
                av_packet_free(&mut (packet as *mut _));
                av_frame_free(&mut (frame as *mut _));
                avio_closep(&mut (*format_context).pb);
                avformat_free_context(format_context);
                avcodec_free_context(&mut (codec_context as *mut _));
                return Err(FFmpegError::SwscaleInitFailed);
            }

            Ok(Self {
                codec_context,
                format_context,
                video_stream,
                frame,
                packet,
                sws_context,
                frame_count: 0,
            })
        }
    }

    /// Encode a single frame
    pub fn encode_frame(&mut self, raw_frame: &RawFrame) -> Result<()> {
        unsafe {
            // Make frame writable
            let ret = av_frame_make_writable(self.frame);
            if ret < 0 {
                return Err(FFmpegError::EncodingError("Failed to make frame writable".to_string()));
            }

            // Convert RGBA to YUV420P
            let src_data = [
                raw_frame.data.as_ptr() as *const u8,
                ptr::null(),
                ptr::null(),
                ptr::null(),
            ];
            let src_linesize = [
                (raw_frame.width * 4) as i32, // RGBA has 4 bytes per pixel
                0,
                0,
                0,
            ];

            let ret = sws_scale(
                self.sws_context,
                src_data.as_ptr(),
                src_linesize.as_ptr(),
                0,
                raw_frame.height as i32,
                (*self.frame).data.as_ptr() as *const *mut u8,
                (*self.frame).linesize.as_ptr(),
            );

            if ret < 0 {
                return Err(FFmpegError::ColorConversionFailed);
            }

            // Set frame PTS (presentation timestamp)
            (*self.frame).pts = self.frame_count;
            self.frame_count += 1;

            // Send frame to encoder
            let ret = avcodec_send_frame(self.codec_context, self.frame);
            if ret < 0 {
                return Err(FFmpegError::EncodingError(format!("Send frame failed: {}", ret)));
            }

            // Receive encoded packets
            self.receive_packets()?;

            Ok(())
        }
    }

    /// Receive and write encoded packets
    fn receive_packets(&mut self) -> Result<()> {
        unsafe {
            loop {
                let ret = avcodec_receive_packet(self.codec_context, self.packet);

                if ret == AVERROR(EAGAIN) || ret == AVERROR_EOF {
                    break; // Need more frames or encoding is done
                }

                if ret < 0 {
                    return Err(FFmpegError::EncodingError(format!("Receive packet failed: {}", ret)));
                }

                // Rescale packet timestamps
                av_packet_rescale_ts(
                    self.packet,
                    (*self.codec_context).time_base,
                    (*self.video_stream).time_base,
                );
                (*self.packet).stream_index = (*self.video_stream).index;

                // Write packet
                let ret = av_interleaved_write_frame(self.format_context, self.packet);

                av_packet_unref(self.packet);

                if ret < 0 {
                    return Err(FFmpegError::EncodingError(format!("Write frame failed: {}", ret)));
                }
            }
            Ok(())
        }
    }

    /// Flush encoder and write trailer
    pub fn finish(&mut self) -> Result<()> {
        unsafe {
            // Flush encoder
            let ret = avcodec_send_frame(self.codec_context, ptr::null());
            if ret < 0 {
                return Err(FFmpegError::EncodingError("Failed to flush encoder".to_string()));
            }

            // Receive remaining packets
            self.receive_packets()?;

            // Write trailer
            let ret = av_write_trailer(self.format_context);
            if ret < 0 {
                return Err(FFmpegError::EncodingError("Failed to write trailer".to_string()));
            }

            Ok(())
        }
    }
}

impl Drop for FFmpegEncoder {
    fn drop(&mut self) {
        unsafe {
            // Clean up resources in reverse order
            if !self.sws_context.is_null() {
                sws_freeContext(self.sws_context);
            }

            if !self.packet.is_null() {
                av_packet_free(&mut (self.packet as *mut _));
            }

            if !self.frame.is_null() {
                av_frame_free(&mut (self.frame as *mut _));
            }

            if !self.format_context.is_null() {
                if (*self.format_context).pb as usize != 0 {
                    avio_closep(&mut (*self.format_context).pb);
                }
                avformat_free_context(self.format_context);
            }

            if !self.codec_context.is_null() {
                avcodec_free_context(&mut (self.codec_context as *mut _));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::capture::PixelFormat;

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

        let result = FFmpegEncoder::new(
            &output_path,
            640,
            480,
            30,
            "libx264",
            23,
        );

        assert!(result.is_ok());
    }
}
