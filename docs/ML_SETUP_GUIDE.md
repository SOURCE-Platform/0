# ML Feature Setup Guide

This guide explains how to set up and use the pose estimation and audio processing ML features.

## Quick Start

### 1. Install Python Dependencies

```bash
cd src-tauri/python
python -m venv venv
source venv/bin/activate  # On Windows: venv\Scripts\activate
pip install -r requirements.txt
```

### 2. Build with ML Features

```bash
# Build with PyO3 support (Python ML backend)
cargo build --features ml-pyo3

# Or run in development mode
cargo run --features ml-pyo3
```

### 3. Run the Application

```bash
# From project root
npm run tauri dev -- --features ml-pyo3
```

## Feature Flags

The ML features are controlled by Cargo feature flags:

- **`ml-pyo3`**: Enable Python-based ML inference (MediaPipe, Whisper, etc.)
- **`ml-onnx`**: Enable ONNX-based ML inference (pure Rust, not yet implemented)
- **Default (no features)**: Dummy implementations (no actual ML inference)

## Python ML Models

### MediaPipe (Pose, Face, Hands)

**Installation:**
```bash
pip install mediapipe>=0.10.0
```

**Usage:**
- Automatically loaded when pose tracking is started
- Models are downloaded on first use (~10-13 MB each)
- Cached in `~/.mediapipe/`

**Performance:**
- CPU: ~30-60 FPS
- GPU: ~120+ FPS

**Configuration:**
```rust
let config = PoseConfig {
    enable_body_tracking: true,    // 33 keypoints
    enable_face_tracking: true,    // 468 landmarks + 52 blendshapes
    enable_hand_tracking: true,    // 21 keypoints per hand
    min_detection_confidence: 0.5,
    min_tracking_confidence: 0.5,
    ..Default::default()
};
```

### Whisper (Speech Transcription)

**Installation:**
```bash
pip install openai-whisper>=20230314
```

**Models:**
- `tiny`: 39 MB, fastest
- `base`: 74 MB, good balance **(default)**
- `small`: 244 MB, more accurate
- `medium`: 769 MB, high accuracy
- `large`: 1550 MB, best accuracy

**Performance:**
- Tiny: ~10x realtime on CPU
- Base: ~5x realtime on CPU, ~20x on GPU
- Large: ~1x realtime on CPU, ~10x on GPU

**First Run:**
- Models are auto-downloaded to `~/.cache/whisper/`
- First transcription may take longer

### pyannote.audio (Speaker Diarization)

**Installation:**
```bash
pip install pyannote.audio>=3.1.0
```

**Setup:**
1. Create Hugging Face account: https://huggingface.co/join
2. Get auth token: https://huggingface.co/settings/tokens
3. Accept model terms: https://hf.co/pyannote/speaker-diarization-3.1
4. Set environment variable:
```bash
export HF_TOKEN=your_token_here
```

**Models:**
- Speaker diarization: ~17 MB
- Speaker embedding: ~17 MB
- Downloaded on first use to `~/.cache/torch/`

**Performance:**
- ~1x realtime on CPU
- ~3x realtime on GPU

### SpeechBrain (Emotion Recognition)

**Installation:**
```bash
pip install speechbrain>=0.5.16
```

**Models:**
- Emotion classifier: ~378 MB
- Auto-downloaded on first use to `pretrained_models/`

**Emotions Detected:**
- neutral, happy, sad, angry, fearful, disgusted, surprised

**Performance:**
- ~10x realtime on CPU
- ~50x realtime on GPU

## GPU Acceleration (Optional)

### Install PyTorch with CUDA

For NVIDIA GPUs:

```bash
# CUDA 11.8
pip install torch torchvision torchaudio --index-url https://download.pytorch.org/whl/cu118

# CUDA 12.1
pip install torch torchvision torchaudio --index-url https://download.pytorch.org/whl/cu121
```

**Verify GPU:**
```python
import torch
print(torch.cuda.is_available())  # Should print True
print(torch.cuda.get_device_name(0))  # Your GPU name
```

## Development Workflow

### Testing MediaPipe Directly

```bash
cd src-tauri/python
python mediapipe_inference.py
```

### Testing Whisper Directly

```bash
cd src-tauri/python
python whisper_inference.py test_audio.wav base
```

### Testing from Rust

```rust
// Enable pose tracking
let result = invoke('start_pose_tracking', {
    sessionId: 'test',
    config: {
        enable_body_tracking: true,
        enable_face_tracking: true,
        enable_hand_tracking: true,
    }
}).await;

// Get pose data
let frames = invoke('get_pose_frames', {
    sessionId: 'test',
    startTimestamp: 0,
    endTimestamp: Date.now(),
}).await;
```

## Troubleshooting

### "Failed to import mediapipe_inference"

**Solution:**
```bash
cd src-tauri/python
pip install -r requirements.txt
```

### "Python.h: No such file or directory" (Linux)

**Solution:**
```bash
# Ubuntu/Debian
sudo apt install python3-dev

# Fedora
sudo dnf install python3-devel
```

### "CUDA out of memory"

**Solutions:**
1. Use smaller models (Whisper tiny/base instead of large)
2. Reduce batch size
3. Use CPU instead of GPU
4. Close other GPU-intensive applications

### Slow Performance on macOS

**Solutions:**
1. Install on Apple Silicon: MediaPipe has ARM64 support
2. For Whisper, consider using CoreML backend:
```bash
pip install whisper-coreml
```

### "HF_TOKEN not set" for Diarization

**Solution:**
```bash
export HF_TOKEN=your_huggingface_token
# Add to ~/.bashrc or ~/.zshrc for persistence
```

## Environment Variables

```bash
# Required for speaker diarization
export HF_TOKEN=your_huggingface_token

# Optional: Force CPU even if GPU available
export CUDA_VISIBLE_DEVICES=-1

# Optional: Set PyTorch device
export TORCH_DEVICE=cuda  # or cpu

# Optional: Whisper model cache directory
export WHISPER_CACHE_DIR=/path/to/cache
```

## File Locations

### Python Scripts
- Location: `src-tauri/python/`
- Entry points:
  - `mediapipe_inference.py`
  - `whisper_inference.py`
  - `diarization_inference.py`
  - `emotion_inference.py`

### Model Cache Locations

| Model | Default Cache Location |
|-------|----------------------|
| MediaPipe | `~/.mediapipe/` |
| Whisper | `~/.cache/whisper/` |
| pyannote.audio | `~/.cache/torch/` |
| SpeechBrain | `./pretrained_models/` |

### Disk Space Requirements

| Model | Size |
|-------|------|
| MediaPipe (all 3 models) | ~30 MB |
| Whisper tiny | 39 MB |
| Whisper base | 74 MB |
| Whisper small | 244 MB |
| Whisper medium | 769 MB |
| Whisper large | 1550 MB |
| pyannote diarization | ~34 MB |
| SpeechBrain emotion | ~378 MB |

**Total (with Whisper base):** ~516 MB

## Production Deployment

### Docker

```dockerfile
FROM rust:latest

# Install Python and dependencies
RUN apt-get update && apt-get install -y \
    python3 \
    python3-pip \
    python3-venv \
    && rm -rf /var/lib/apt/lists/*

# Copy Python requirements
COPY src-tauri/python/requirements.txt /app/python/
RUN pip3 install -r /app/python/requirements.txt

# Pre-download models
RUN python3 -c "import whisper; whisper.load_model('base')"
RUN python3 -c "import mediapipe as mp; mp.solutions.pose.Pose()"

# Build application with ML features
COPY . /app
WORKDIR /app
RUN cargo build --release --features ml-pyo3
```

### Security Considerations

1. **HF_TOKEN**: Never commit tokens to version control
2. **Model Downloads**: Verify checksums for security
3. **Input Validation**: Sanitize audio/video file paths
4. **Resource Limits**: Set memory/CPU limits for inference

## Performance Optimization

### Reduce Latency

1. **Use smaller models** (Whisper tiny/base)
2. **Batch processing** for multiple frames
3. **GPU acceleration** (see GPU section above)
4. **Model preloading** at startup
5. **Async processing** (already implemented)

### Reduce Memory Usage

1. **Unload models** when not in use
2. **Stream processing** for long audio
3. **Lower resolution** for pose tracking (if acceptable)
4. **Chunked transcription** for large files

## API Reference

### Tauri Commands (Frontend → Rust)

**Pose Tracking:**
```typescript
// Start tracking
await invoke('start_pose_tracking', {
  sessionId: string,
  config: PoseConfig
})

// Stop tracking
await invoke('stop_pose_tracking', {
  sessionId: string
})

// Get pose frames
await invoke('get_pose_frames', {
  sessionId: string,
  startTimestamp: number,
  endTimestamp: number
})
```

**Audio Recording & Transcription:**
```typescript
// Start recording
await invoke('start_audio_recording', {
  sessionId: string,
  config: AudioConfig
})

// Stop recording
await invoke('stop_audio_recording')

// Get transcripts
await invoke('get_transcripts', {
  sessionId: string,
  recordingId: string
})

// Get speakers
await invoke('get_speakers', {
  sessionId: string
})

// Get emotions
await invoke('get_emotions', {
  sessionId: string,
  recordingId: string
})
```

### Python API (Direct Usage)

See `src-tauri/python/README.md` for Python API documentation.

## Support

- **GitHub Issues**: Report bugs and feature requests
- **Documentation**: `docs/` directory
- **Examples**: `examples/` directory (TODO)

## License

See individual Python package licenses:
- MediaPipe: Apache 2.0
- Whisper: MIT
- pyannote.audio: MIT
- SpeechBrain: Apache 2.0
