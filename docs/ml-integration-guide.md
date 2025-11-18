# ML Model Integration Guide

**Project:** 0 - Privacy-First Activity Tracker
**Phase:** 2.5 - Pose Estimation & Audio Processing
**Last Updated:** 2025-11-18

---

## Overview

This guide explains how to integrate ML models (MediaPipe, Whisper, pyannote.audio, Demucs, SpeechBrain) into the "0" application. The backend architecture is **100% ready** - all Tauri commands, data models, and database schema are in place. This guide focuses on completing the ML inference pipelines.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    Tauri Frontend (React)                   │
│  invoke('start_pose_tracking', ...)                         │
│  invoke('start_audio_recording', ...)                       │
└─────────────────────┬───────────────────────────────────────┘
                      │ IPC
┌─────────────────────▼───────────────────────────────────────┐
│                   Rust Backend (Tauri)                      │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐        │
│  │PoseDetector │  │AudioRecorder│  │ Transcriber │        │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘        │
│         │                 │                 │               │
└─────────┼─────────────────┼─────────────────┼───────────────┘
          │                 │                 │
┌─────────▼─────────────────▼─────────────────▼───────────────┐
│            Platform-Specific ML Bridges                     │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐        │
│  │  MediaPipe  │  │    cpal     │  │   Whisper   │        │
│  │   Bridge    │  │Audio Capture│  │   Bridge    │        │
│  └─────────────┘  └─────────────┘  └─────────────┘        │
└─────────────────────────────────────────────────────────────┘
          │                 │                 │
┌─────────▼─────────────────▼─────────────────▼───────────────┐
│                ML Models (PyO3 or ONNX)                     │
│  • MediaPipe (468 face + 33 body + 21 hands)                │
│  • Whisper (speech-to-text)                                 │
│  • pyannote.audio (speaker diarization)                     │
│  • Demucs (source separation)                               │
│  • SpeechBrain (emotion detection)                          │
└─────────────────────────────────────────────────────────────┘
```

---

## Integration Options

### Option A: PyO3 (Python ML Stack) ⭐ Recommended for Prototyping

**Pros:**
- Full access to Python ML ecosystem
- Easy to update models
- Rich tooling and documentation

**Cons:**
- Python runtime dependency
- Slightly slower than native
- GIL contention in multi-threaded scenarios

### Option B: ONNX Runtime (Pure Rust)

**Pros:**
- No Python dependency
- Faster inference
- Better for production deployment

**Cons:**
- Not all models available in ONNX format
- More complex conversion process
- Less flexibility for model updates

---

## Setup Instructions

### 1. System Dependencies

#### macOS
```bash
# Install Python and system libraries
brew install python@3.11 ffmpeg portaudio

# Install Xcode Command Line Tools (for Core Audio)
xcode-select --install
```

#### Ubuntu/Debian Linux
```bash
# Install Python and system libraries
sudo apt update
sudo apt install python3.11 python3-pip ffmpeg \
    libportaudio2 portaudio19-dev \
    pulseaudio pulseaudio-utils \
    libx11-dev libxrandr-dev

# For GPU acceleration (optional)
sudo apt install nvidia-cuda-toolkit  # NVIDIA GPUs
```

#### Windows
```powershell
# Install Python
winget install Python.Python.3.11

# Install FFmpeg
choco install ffmpeg

# Install WASAPI dev tools (included in Windows SDK)
```

### 2. Python Environment Setup

```bash
# Create virtual environment
python3.11 -m venv venv
source venv/bin/activate  # On Windows: venv\Scripts\activate

# Install Python dependencies
pip install -r python-requirements.txt

# Download pre-trained models (automated on first use)
# MediaPipe: Auto-downloaded by library
# Whisper: Auto-downloaded by openai-whisper
# pyannote.audio: Requires HuggingFace token (see below)
```

### 3. Hugging Face Authentication (for pyannote.audio)

```bash
# Install huggingface_hub
pip install huggingface_hub

# Login to Hugging Face
huggingface-cli login

# Accept model terms at:
# https://huggingface.co/pyannote/speaker-diarization-3.1
# https://huggingface.co/pyannote/embedding
```

### 4. Rust Dependencies

Add to `src-tauri/Cargo.toml`:

```toml
[dependencies]
# Existing dependencies...

# Audio capture
cpal = "0.15"
hound = "3.5"
rubato = "0.15"

# PyO3 for Python integration (Option A)
pyo3 = { version = "0.21", features = ["auto-initialize"], optional = true }

# ONNX Runtime for native inference (Option B)
ort = { version = "2.0", features = ["download-binaries"], optional = true }

# HTTP client for model downloads
reqwest = { version = "0.12", features = ["blocking"] }

[features]
default = ["pyo3"]  # Use Python bridge by default
pyo3 = ["dep:pyo3"]
onnx = ["dep:ort"]
```

---

## Implementation Steps

### Step 1: Complete Audio Capture (Platform-Specific)

The platform-specific audio modules are stubbed out in:
- `src-tauri/src/platform/audio/macos.rs`
- `src-tauri/src/platform/audio/windows.rs`
- `src-tauri/src/platform/audio/linux.rs`

**Complete `cpal` integration:**

```rust
// Example for macOS (src-tauri/src/platform/audio/macos.rs)
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub fn enumerate_devices() -> AudioResult<Vec<AudioDevice>> {
    let host = cpal::default_host();
    let mut devices = Vec::new();

    // Input devices (microphones)
    for device in host.input_devices()? {
        let name = device.name()?;
        let config = device.default_input_config()?;

        devices.push(AudioDevice {
            id: name.clone(),
            name,
            device_type: AudioDeviceType::Microphone,
            is_default: false,  // Check with host.default_input_device()
            sample_rate: config.sample_rate().0,
            channels: config.channels(),
        });
    }

    // System loopback (requires custom aggregate device on macOS)
    // TODO: Implement loopback device setup

    Ok(devices)
}
```

**Testing:**
```bash
cd src-tauri
cargo test --features pyo3 platform::audio
```

### Step 2: Integrate MediaPipe (Pose Detection)

**Option A: PyO3 Integration**

Complete `src-tauri/src/platform/pose/mediapipe_bridge.rs` → `pyo3_backend`:

```rust
impl MediaPipeBridge for PyO3MediaPipe {
    fn new(config: &PoseConfig) -> PoseResult<Self> {
        Python::with_gil(|py| {
            let mp = py.import("mediapipe")?;
            let solutions = mp.getattr("solutions")?;

            let pose_detector = if config.enable_body_tracking {
                let pose = solutions.getattr("pose")?;
                let detector = pose.call_method(
                    "Pose",
                    (),
                    Some(PyDict::from_sequence(py, &[
                        ("min_detection_confidence", config.min_detection_confidence),
                        ("min_tracking_confidence", config.min_tracking_confidence),
                        ("model_complexity", config.model_complexity as i32),
                    ])?)
                )?;
                Some(detector.into())
            } else {
                None
            };

            // Similar for face_mesh and hands_detector

            Ok(Self {
                pose_detector,
                face_mesh: None,
                hands_detector: None,
                config: config.clone(),
            })
        })
        .map_err(|e: PyErr| PoseError::ModelLoadFailed(e.to_string()))
    }

    fn process_frame(&self, frame_data: &[u8], width: u32, height: u32) -> PoseResult<MediaPipeResult> {
        Python::with_gil(|py| {
            // Convert frame to numpy array
            let np = py.import("numpy")?;
            let cv2 = py.import("cv2")?;

            let frame_bytes = PyBytes::new(py, frame_data);
            let image = np.call_method1("frombuffer", (frame_bytes, "uint8"))?
                .call_method1("reshape", ((height, width, 4)))?;

            // Convert RGBA to RGB
            let rgb = cv2.call_method1("cvtColor", (image, 4))?; // COLOR_RGBA2RGB = 4

            // Run pose detection
            if let Some(ref detector) = self.pose_detector {
                let results = detector.call_method1(py, "process", (rgb,))?;
                let landmarks = results.getattr(py, "pose_landmarks")?;

                // Extract keypoints
                let keypoints = extract_pose_landmarks(py, landmarks)?;

                return Ok(MediaPipeResult {
                    body_pose: Some(BodyPose {
                        keypoints,
                        visibility_scores: vec![],  // Extract from results
                        world_landmarks: None,
                        pose_classification: None,
                    }),
                    face_mesh: None,
                    hands: vec![],
                    processing_time_ms: 0,
                });
            }

            Ok(MediaPipeResult::default())
        })
        .map_err(|e: PyErr| PoseError::InferenceFailed(e.to_string()))
    }
}
```

**Update `PoseDetector::run_inference()`:**

```rust
// In src-tauri/src/core/pose_detector.rs
use crate::platform::pose::{DefaultMediaPipe, MediaPipeBridge};

fn run_inference(...) -> PoseResult<PoseFrame> {
    // Initialize MediaPipe bridge (cache this!)
    let bridge = DefaultMediaPipe::new(&config)?;

    // Run inference
    let result = bridge.process_frame(frame_data, width, height)?;

    Ok(PoseFrame {
        session_id,
        timestamp,
        frame_id: None,
        body_pose: result.body_pose,
        face_mesh: result.face_mesh,
        hands: result.hands,
        processing_time_ms: result.processing_time_ms,
    })
}
```

**Testing:**
```bash
# Test MediaPipe integration
cargo test --features pyo3 pose_detector

# Run full app
npm run tauri dev
```

### Step 3: Integrate Whisper (Speech Transcription)

**Update `SpeechTranscriber::transcribe_audio()`:**

```rust
use pyo3::prelude::*;
use pyo3::types::PyDict;

pub async fn transcribe_audio(
    &self,
    session_id: &str,
    recording_id: &str,
    audio_file_path: &str,
) -> AudioResult<Vec<TranscriptSegment>> {
    // Run Whisper inference in blocking thread pool
    let audio_path = audio_file_path.to_string();
    let model_size = self.model_size.read().await.clone();

    tokio::task::spawn_blocking(move || {
        Python::with_gil(|py| {
            let whisper = py.import("whisper")?;

            // Load model (cache this!)
            let model = whisper.call_method1("load_model", (model_size.to_string(),))?;

            // Transcribe
            let result = model.call_method(
                "transcribe",
                (audio_path,),
                Some(PyDict::from_sequence(py, &[
                    ("word_timestamps", true),
                    ("language", language.unwrap_or("en")),
                ])?)
            )?;

            // Extract segments
            let segments = result.get_item("segments")?;
            // Parse segments into Vec<TranscriptSegment>

            Ok(vec![])
        })
        .map_err(|e: PyErr| AudioError::TranscriptionFailed(e.to_string()))
    })
    .await
    .map_err(|e| AudioError::TranscriptionFailed(e.to_string()))?
}
```

### Step 4: Integrate pyannote.audio (Speaker Diarization)

```rust
// src-tauri/src/core/speaker_diarizer.rs
use pyo3::prelude::*;

pub async fn diarize_audio(&self, recording_id: &str, audio_file_path: &str) -> AudioResult<Vec<SpeakerSegment>> {
    let audio_path = audio_file_path.to_string();

    tokio::task::spawn_blocking(move || {
        Python::with_gil(|py| {
            let pyannote = py.import("pyannote.audio")?;

            // Load pipeline (requires HuggingFace token in environment)
            let pipeline = pyannote.call_method1(
                "Pipeline.from_pretrained",
                ("pyannote/speaker-diarization-3.1",)
            )?;

            // Run diarization
            let diarization = pipeline.call1((audio_path,))?;

            // Extract speaker segments
            let segments = vec![];  // Parse diarization output

            Ok(segments)
        })
        .map_err(|e: PyErr| AudioError::DiarizationFailed(e.to_string()))
    })
    .await
    .map_err(|e| AudioError::DiarizationFailed(e.to_string()))?
}
```

### Step 5: Integrate Demucs (Source Separation)

```python
# Create a Python helper script: scripts/audio_separation.py
import demucs.separate
import numpy as np

def separate_sources(audio_path, output_dir):
    """Separate audio into vocals, drums, bass, other"""
    model = demucs.pretrained.get_model('htdemucs')
    demucs.separate.separate_audio_file(
        model,
        audio_path,
        output_dir=output_dir
    )
    return {
        'vocals': f'{output_dir}/vocals.wav',
        'drums': f'{output_dir}/drums.wav',
        'bass': f'{output_dir}/bass.wav',
        'other': f'{output_dir}/other.wav'
    }
```

```rust
// Call from Rust via PyO3
// src-tauri/src/core/audio_recorder.rs (add new method)
pub async fn separate_sources(&self, audio_path: &str) -> AudioResult<SeparatedSources> {
    Python::with_gil(|py| {
        let script = include_str!("../../../scripts/audio_separation.py");
        let module = PyModule::from_code(py, script, "audio_separation.py", "audio_separation")?;

        let result = module.call_method1("separate_sources", (audio_path, "/tmp/separated"))?;

        // Parse result into SeparatedSources

        Ok(SeparatedSources {
            vocals_path: Some(PathBuf::from("/tmp/separated/vocals.wav")),
            music_path: Some(PathBuf::from("/tmp/separated/other.wav")),
            bass_path: Some(PathBuf::from("/tmp/separated/bass.wav")),
            other_path: Some(PathBuf::from("/tmp/separated/drums.wav")),
        })
    })
    .map_err(|e: PyErr| AudioError::SeparationFailed(e.to_string()))
}
```

### Step 6: Integrate SpeechBrain (Emotion Detection)

```rust
// src-tauri/src/core/emotion_detector.rs
pub async fn detect_emotions(&self, session_id: &str, recording_id: &str, audio_file_path: &str) -> AudioResult<Vec<EmotionResult>> {
    let audio_path = audio_file_path.to_string();

    tokio::task::spawn_blocking(move || {
        Python::with_gil(|py| {
            let sb = py.import("speechbrain.pretrained")?;

            // Load emotion recognition model
            let classifier = sb.call_method1(
                "EncoderClassifier.from_hparams",
                (),
                Some(PyDict::from_sequence(py, &[
                    ("source", "speechbrain/emotion-recognition-wav2vec2-IEMOCAP"),
                    ("savedir", "models/emotion"),
                ])?)
            )?;

            // Classify emotion
            let result = classifier.call_method1("classify_file", (audio_path,))?;

            // Extract emotion predictions
            // Parse into Vec<EmotionResult>

            Ok(vec![])
        })
        .map_err(|e: PyErr| AudioError::EmotionDetectionFailed(e.to_string()))
    })
    .await
    .map_err(|e| AudioError::EmotionDetectionFailed(e.to_string()))?
}
```

---

## Testing

### Unit Tests

```bash
# Test individual modules
cargo test --features pyo3 pose_detector
cargo test --features pyo3 audio_recorder
cargo test --features pyo3 speech_transcriber

# Test all
cargo test --features pyo3
```

### Integration Tests

```rust
// tests/integration_test.rs
#[tokio::test]
async fn test_full_recording_pipeline() {
    // 1. Start session
    let session_id = "test-session";

    // 2. Start pose tracking
    let pose_detector = PoseDetector::new(...).await.unwrap();
    pose_detector.start_tracking(session_id.to_string(), PoseConfig::default()).await.unwrap();

    // 3. Start audio recording
    let audio_recorder = AudioRecorder::new(...).await.unwrap();
    let recording_id = audio_recorder.start_recording(session_id.to_string(), AudioConfig::default()).await.unwrap();

    // 4. Simulate some activity (wait 5 seconds)
    tokio::time::sleep(Duration::from_secs(5)).await;

    // 5. Stop everything
    pose_detector.stop_tracking().await.unwrap();
    audio_recorder.stop_recording().await.unwrap();

    // 6. Query results
    let pose_frames = pose_detector.get_pose_frames(session_id, 0, i64::MAX).await.unwrap();
    assert!(!pose_frames.is_empty());

    // 7. Run transcription
    let transcriber = SpeechTranscriber::new(...).await.unwrap();
    let transcripts = transcriber.transcribe_audio(session_id, &recording_id, "audio.wav").await.unwrap();

    assert!(!transcripts.is_empty());
}
```

### Manual Testing

```typescript
// From browser console or React component
import { invoke } from '@tauri-apps/api/core';

// Test pose tracking
await invoke('start_pose_tracking', {
    sessionId: 'test-123',
    config: {
        enableBodyTracking: true,
        enableFaceTracking: true,
        enableHandTracking: true,
        targetFps: 15,
        minDetectionConfidence: 0.5,
        minTrackingConfidence: 0.5,
        modelComplexity: 'full'
    }
});

// Wait a bit, then get results
const poses = await invoke('get_pose_frames', {
    sessionId: 'test-123',
    start: 0,
    end: Date.now()
});
console.log('Pose frames:', poses);

// Test audio
const recordingId = await invoke('start_audio_recording', {
    sessionId: 'test-123',
    config: {
        enableMicrophone: true,
        enableSystemAudio: false,
        enableTranscription: true,
        sampleRate: 48000,
        channels: 2,
        codec: 'aac',
        whisperModelSize: 'base'
    }
});

// Speak something...

await invoke('stop_audio_recording');

// Search transcripts
const results = await invoke('search_transcripts', {
    query: 'hello',
    sessionId: 'test-123'
});
console.log('Transcripts:', results);
```

---

## Performance Optimization

### 1. Model Caching

Cache loaded models in-memory to avoid reload overhead:

```rust
lazy_static! {
    static ref WHISPER_MODEL: Mutex<Option<PyObject>> = Mutex::new(None);
    static ref MEDIAPIPE_POSE: Mutex<Option<PyObject>> = Mutex::new(None);
}
```

### 2. Batch Processing

Process audio in chunks for better throughput:

```rust
// Process 30 seconds at a time
const CHUNK_DURATION_SEC: u64 = 30;

for chunk in audio_chunks(CHUNK_DURATION_SEC) {
    transcriber.transcribe_chunk(chunk).await?;
}
```

### 3. GPU Acceleration

Enable GPU for models:

```python
# In Python bridge
import torch
device = "cuda" if torch.cuda.is_available() else "cpu"
model = whisper.load_model("base").to(device)
```

### 4. Thread Pool Tuning

Adjust tokio runtime for CPU-intensive tasks:

```rust
#[tokio::main(worker_threads = 8)]
async fn main() {
    // ...
}
```

---

## Troubleshooting

### Common Issues

**1. PyO3 Import Error**
```
Error: ModuleNotFoundError: No module named 'mediapipe'
```
**Solution:** Ensure Python packages are installed in the same environment Rust is using:
```bash
which python3  # Should match PyO3's Python
pip install mediapipe openai-whisper
```

**2. HuggingFace Authentication**
```
Error: You need to authenticate to access this model
```
**Solution:** Login and accept terms:
```bash
huggingface-cli login
# Visit https://huggingface.co/pyannote/speaker-diarization-3.1 and accept
```

**3. CUDA Not Found**
```
Error: CUDA not available
```
**Solution:** Install CUDA toolkit or use CPU:
```python
device = "cpu"  # Force CPU mode
```

**4. Audio Device Not Found**
```
Error: Failed to get audio devices
```
**Solution:** Check system audio permissions (especially on macOS):
```bash
# macOS: Grant microphone access in System Preferences
# Linux: Check PulseAudio is running
pulseaudio --check
```

---

## Production Deployment

### 1. Bundle Python Dependencies

Use PyInstaller or similar to bundle Python runtime:

```bash
pyinstaller --onefile \
    --hidden-import=mediapipe \
    --hidden-import=whisper \
    --hidden-import=pyannote.audio \
    scripts/ml_bridge.py
```

### 2. Model Download on First Run

Implement lazy model download:

```rust
// src-tauri/src/core/ml_models.rs
impl ModelManager {
    pub async fn ensure_model(&self, model: &ModelInfo) -> Result<PathBuf> {
        if !self.is_cached(model) {
            self.download_model(model).await?;
        }
        Ok(self.get_model_path(&model.name))
    }
}
```

### 3. Build Configuration

```toml
# Cargo.toml
[profile.release]
lto = true           # Link-time optimization
codegen-units = 1    # Better optimization
strip = true         # Remove debug symbols
opt-level = 3        # Maximum optimization
```

---

## Next Steps

1. **Complete Audio Capture** - Implement `cpal` integration for each platform
2. **Integrate MediaPipe** - Complete PyO3 bridge for pose/face/hand tracking
3. **Integrate Whisper** - Complete transcription pipeline
4. **Integrate pyannote** - Complete speaker diarization
5. **Integrate Demucs** - Complete source separation
6. **Integrate SpeechBrain** - Complete emotion detection
7. **Create Frontend UI** - Build React components for visualization
8. **Performance Testing** - Benchmark and optimize
9. **Documentation** - API docs and user guide

---

## Resources

- **MediaPipe:** https://developers.google.com/mediapipe
- **Whisper:** https://github.com/openai/whisper
- **pyannote.audio:** https://github.com/pyannote/pyannote-audio
- **Demucs:** https://github.com/facebookresearch/demucs
- **SpeechBrain:** https://speechbrain.github.io/
- **PyO3:** https://pyo3.rs/
- **ONNX Runtime:** https://onnxruntime.ai/
- **cpal:** https://github.com/RustAudio/cpal

---

**Author:** Claude (Anthropic AI)
**Status:** Integration guide for completing ML model implementations
**Last Updated:** 2025-11-18
