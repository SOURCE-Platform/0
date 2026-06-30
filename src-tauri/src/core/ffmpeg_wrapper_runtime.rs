use ffmpeg_sys_next::*;

use super::ffmpeg_wrapper_types::{FFmpegError, Result};

pub(super) unsafe fn receive_packets(
    codec_context: *mut AVCodecContext,
    packet: *mut AVPacket,
    format_context: *mut AVFormatContext,
    video_stream: *mut AVStream,
) -> Result<()> {
    loop {
        let ret = avcodec_receive_packet(codec_context, packet);
        if ret == AVERROR(EAGAIN) || ret == AVERROR_EOF {
            break;
        }

        if ret < 0 {
            return Err(FFmpegError::EncodingError(format!(
                "Receive packet failed: {}",
                ret
            )));
        }

        av_packet_rescale_ts(
            packet,
            (*codec_context).time_base,
            (*video_stream).time_base,
        );
        (*packet).stream_index = (*video_stream).index;

        let ret = av_interleaved_write_frame(format_context, packet);
        av_packet_unref(packet);

        if ret < 0 {
            return Err(FFmpegError::EncodingError(format!(
                "Write frame failed: {}",
                ret
            )));
        }
    }

    Ok(())
}

pub(super) unsafe fn cleanup_encoder(
    sws_context: *mut SwsContext,
    packet: *mut AVPacket,
    frame: *mut AVFrame,
    format_context: *mut AVFormatContext,
    codec_context: *mut AVCodecContext,
) {
    if !sws_context.is_null() {
        sws_freeContext(sws_context);
    }

    if !packet.is_null() {
        av_packet_free(&mut (packet as *mut _));
    }

    if !frame.is_null() {
        av_frame_free(&mut (frame as *mut _));
    }

    if !format_context.is_null() {
        if (*format_context).pb as usize != 0 {
            avio_closep(&mut (*format_context).pb);
        }
        avformat_free_context(format_context);
    }

    if !codec_context.is_null() {
        avcodec_free_context(&mut (codec_context as *mut _));
    }
}
