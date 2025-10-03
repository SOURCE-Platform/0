# Video Encoding Implementation Notes

## Status: Architecture Complete, FFmpeg Integration Pending

### Completed Components

1. **Video Encoder Module** ([src-tauri/src/core/video_encoder.rs](../src-tauri/src/core/video_encoder.rs))
   - `VideoEncoder` struct with quality and codec configuration
   - `VideoCodec` enum (H264, with room for H265/VP9)
   - `CompressionQuality` enum (High/Medium/Low mapped to CRF values)
   - Platform-specific hardware acceleration support:
     - macOS: VideoToolbox (`h264_videotoolbox`)
     - Windows: NVENC (`h264_nvenc`)
     - Linux: VAAPI (`h264_vaapi`)
   - Software fallback: `libx264`
   - Async encoding methods:
     - `encode_frames()` - batch encoding
     - `encode_frame_stream()` - stream-based encoding
   - Full test coverage (7 tests, all passing)

2. **Database Schema** ([src-tauri/migrations/20251003000003_create_video_segments_table.sql](../src-tauri/migrations/20251003000003_create_video_segments_table.sql))
   - `video_segments` table with:
     - Segment metadata (timestamps, frame count, duration, file size)
     - Foreign key to sessions
     - Indexes for efficient queries

3. **Configuration** ([src-tauri/src/core/config.rs](../src-tauri/src/core/config.rs))
   - New video encoding settings:
     - `video_codec`: String (default "h264")
     - `video_quality`: String (default "Medium")
     - `hardware_acceleration`: bool (default true)
     - `target_fps`: u32 (default 15)
   - Full validation for all video settings
   - Updated tests

4. **Data Structures**
   - `RawFrame`: Holds frame data with timestamp and format info
   - `FrameFormat`: RGBA/RGB/BGRA/BGR with FFmpeg format mapping
   - `VideoSegment`: Complete metadata for encoded video segments

### FFmpeg Integration Issue

**Problem**: The `ffmpeg-next 6.0` crate is incompatible with FFmpeg 8.0+.

**Root Cause**: FFmpeg 8.0 removed the deprecated `avfft.h` header (FFT functionality), but `ffmpeg-sys-next 6.1.0` (used by `ffmpeg-next 6.0`) still tries to include it during binding generation.

**Error**:
```
fatal error: '/usr/include/libavcodec/avfft.h' file not found
```

**Current Workaround**:
The `ffmpeg-next` dependency is commented out in [Cargo.toml](../src-tauri/Cargo.toml:33-35). The encoder implementation uses a placeholder that:
- Logs encoding parameters
- Creates marker files instead of actual video
- Allows the rest of the system to be tested

**Production Solutions** (choose one when ready):

1. **Wait for ffmpeg-next 7.x** (Recommended)
   - Monitor: https://github.com/zmwangx/rust-ffmpeg/issues
   - Should support FFmpeg 8.0+ when released

2. **Downgrade FFmpeg** (Quick fix, not recommended)
   ```bash
   brew uninstall ffmpeg
   brew install ffmpeg@7
   ```
   - Loses FFmpeg 8.0 improvements
   - May cause conflicts with other tools

3. **Use Alternative Crates**
   - `ffmpeg-sidecar`: Calls ffmpeg CLI instead of C bindings
   - `opencv`: Has video encoding but heavy dependency
   - `gstreamer-rs`: Full media framework, more complex

4. **Direct FFmpeg CLI** (Pragmatic approach)
   - Replace `encode_frames_sync()` implementation
   - Call `ffmpeg` command directly with proper arguments
   - Example:
     ```bash
     ffmpeg -f rawvideo -pix_fmt rgba -s 1920x1080 -r 15 \
            -i - -c:v h264_videotoolbox -crf 25 -preset medium \
            -pix_fmt yuv420p output.mp4
     ```
   - Pros: Works immediately, no binding issues
   - Cons: Requires ffmpeg in PATH, IPC overhead

5. **Custom FFmpeg Bindings**
   - Use `bindgen` directly with corrected bindings
   - Most work, but most control

### Recommended Next Steps

1. **Short-term** (before Task 2.8):
   - Implement Option 4 (Direct FFmpeg CLI) in `encode_frames_sync()`
   - Allows testing of the full recording pipeline
   - Can be swapped out later when ffmpeg-next is fixed

2. **Medium-term** (after Task 2.8):
   - Monitor `ffmpeg-next` repository for 7.x release
   - Switch back to native bindings when available
   - Better performance than CLI approach

### Implementation Architecture

The encoder is designed to be swapped out easily:

```rust
// Current placeholder
fn encode_frames_sync(...) -> Result<()> {
    // TODO: Replace with real encoding
    println!("Would encode frames...");
    std::fs::write(output_path, marker_content)?;
    Ok(())
}
```

Can be replaced with any of:
1. FFmpeg CLI call
2. FFmpeg-next bindings (when compatible)
3. Alternative crate

The public API (`encode_frames()`, `encode_frame_stream()`) remains unchanged.

### Storage Structure

Videos will be stored as:
```
~/.observer_data/
  recordings/
    {session-uuid}/
      segments/
        1234567890.mp4  (first segment)
        1234567950.mp4  (second segment)
      base_layer.png    (static background from motion detection)
```

### Performance Targets

When fully implemented:
- Encoding should not block capture (already async)
- Should encode faster than real-time (target: 1s video in <0.5s)
- Hardware acceleration should be auto-detected and used when available
- 60 frames at 15fps = 4 seconds of video per segment

### Testing

All 7 tests pass:
- ✓ Codec name generation for each platform
- ✓ CRF quality mapping
- ✓ Frame format conversion
- ✓ Single frame encoding
- ✓ Multiple frame encoding
- ✓ Stream-based encoding
- ✓ Error handling for empty frames

Run tests:
```bash
cargo test --lib core::video_encoder::tests
```

### Next Task

**Task 2.8**: Integrate everything into a dynamic recording system that:
1. Captures frames from screen
2. Detects motion
3. Buffers frames with motion
4. Encodes buffer to video segment
5. Saves to database
6. Clears buffer and continues

The video encoder is ready for this integration once the encoding implementation is chosen.
