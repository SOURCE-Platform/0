# Pose Estimation & Audio Processing Modules Architecture

## Overview
This document outlines the architecture for two major feature additions to the "0" application:
1. **Pose Estimation Module**: Body and facial tracking with maximum keypoint capture
2. **Audio Processing Module**: Comprehensive audio capture, separation, transcription, and analysis

Both modules are designed to function independently while integrating seamlessly with the existing session-based recording system.

---

## 1. Pose Estimation Module

### 1.1 Technology Selection

**Primary Framework: MediaPipe (Google)**
- **Rationale**: Highest fidelity keypoint tracking among open-source solutions
- **Components**:
  - **MediaPipe Face Mesh**: 468 3D facial landmarks (eyes, eyebrows, lips, face contour)
  - **MediaPipe Pose**: 33 body keypoints (skeleton tracking)
  - **MediaPipe Hands**: 21 hand keypoints per hand (finger tracking)
  - **MediaPipe Holistic**: Unified tracking of all three components

**Rust Integration**:
- Use `opencv-rust` crate for image processing
- Python bridge via `pyo3` for MediaPipe inference
- Alternative: Pure Rust with ONNX Runtime + MediaPipe ONNX models

### 1.2 Module Structure

```
src-tauri/src/
├── core/
│   ├── pose_detector.rs          # Main pose detection orchestrator
│   ├── face_tracker.rs           # Facial expression analysis
│   └── body_tracker.rs           # Body pose analysis
├── models/
│   └── pose.rs                   # Data structures for pose/face data
└── platform/
    └── pose/
        ├── mediapipe_bridge.rs   # Python/ONNX integration
        └── inference.rs          # Model inference abstraction
```

### 1.3 Data Models

```rust
// Unified pose result containing all tracking data
pub struct PoseFrame {
    pub session_id: String,
    pub timestamp: i64,
    pub frame_id: Option<String>,
    pub body_pose: Option<BodyPose>,
    pub face_mesh: Option<FaceMesh>,
    pub hands: Vec<HandPose>,
    pub processing_time_ms: u64,
}

// Body pose (33 keypoints)
pub struct BodyPose {
    pub keypoints: Vec<Keypoint3D>,  // 33 points
    pub visibility_scores: Vec<f32>,
    pub world_landmarks: Option<Vec<Keypoint3D>>,  // Metric 3D coords
}

// Face mesh (468 landmarks)
pub struct FaceMesh {
    pub landmarks: Vec<Keypoint3D>,  // 468 points
    pub blendshapes: Option<FaceBlendshapes>,  // 52 ARKit-compatible expressions
    pub transformation_matrix: Option<[f32; 16]>,  // Face rotation/translation
}

// Face expression data (ARKit-compatible 52 blendshapes)
pub struct FaceBlendshapes {
    pub eye_blink_left: f32,
    pub eye_blink_right: f32,
    pub jaw_open: f32,
    pub mouth_smile_left: f32,
    pub mouth_smile_right: f32,
    pub brow_down_left: f32,
    pub brow_down_right: f32,
    // ... (52 total blendshapes)
}

// Hand pose (21 keypoints per hand)
pub struct HandPose {
    pub handedness: Handedness,  // Left or Right
    pub landmarks: Vec<Keypoint3D>,  // 21 points
    pub world_landmarks: Option<Vec<Keypoint3D>>,
}

// 3D keypoint with confidence
pub struct Keypoint3D {
    pub x: f32,  // Normalized [0, 1] for image coords
    pub y: f32,
    pub z: f32,  // Depth (relative to hip midpoint for body)
    pub confidence: f32,
}
```

### 1.4 Database Schema

```sql
-- Body pose tracking
CREATE TABLE pose_frames (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    frame_id TEXT,
    body_keypoints_json TEXT,      -- 33 body points
    body_visibility_json TEXT,     -- Visibility scores
    face_landmarks_json TEXT,      -- 468 facial points
    face_blendshapes_json TEXT,    -- 52 expression values
    left_hand_json TEXT,           -- 21 left hand points
    right_hand_json TEXT,          -- 21 right hand points
    processing_time_ms INTEGER,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX idx_pose_session ON pose_frames(session_id);
CREATE INDEX idx_pose_timestamp ON pose_frames(timestamp);

-- Facial expression events (aggregated)
CREATE TABLE facial_expressions (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    expression_type TEXT NOT NULL,  -- smile, frown, surprise, neutral, etc.
    intensity REAL NOT NULL,        -- 0.0 to 1.0
    duration_ms INTEGER,
    blendshapes_json TEXT,          -- Raw blendshape values
    created_at INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX idx_expressions_session ON facial_expressions(session_id);
CREATE INDEX idx_expressions_type ON facial_expressions(expression_type);
```

### 1.5 API Commands

```rust
// Start pose tracking for a session
#[tauri::command]
async fn start_pose_tracking(session_id: String, config: PoseConfig) -> Result<(), String>;

// Stop pose tracking
#[tauri::command]
async fn stop_pose_tracking() -> Result<(), String>;

// Get pose data for time range
#[tauri::command]
async fn get_pose_frames(
    session_id: String,
    start: i64,
    end: i64
) -> Result<Vec<PoseFrameDto>, String>;

// Get facial expression events
#[tauri::command]
async fn get_facial_expressions(
    session_id: String,
    expression_type: Option<String>,
    start: i64,
    end: i64
) -> Result<Vec<FacialExpressionDto>, String>;

// Get body pose statistics
#[tauri::command]
async fn get_pose_statistics(session_id: String) -> Result<PoseStatistics, String>;
```

---

## 2. Audio Processing Module

### 2.1 Technology Selection

**Audio Capture**:
- **Rust**: `cpal` crate for cross-platform audio capture
- **Platform-specific APIs**:
  - macOS: Core Audio + Loopback Audio
  - Windows: WASAPI with loopback
  - Linux: PulseAudio/ALSA with monitor sources

**Audio Source Separation**:
- **Demucs v4** (Meta AI): State-of-the-art source separation
  - Separates: vocals, drums, bass, other (music/effects)
  - Can differentiate system audio from microphone input
- **Spleeter** (Deezer): Alternative, faster but less accurate

**Speech Transcription**:
- **OpenAI Whisper**: Open-source speech-to-text
  - Models: tiny, base, small, medium, large-v3
  - Support for 99 languages
  - Timestamp-level transcription

**Speaker Diarization**:
- **pyannote.audio 3.x**: Speaker segmentation and identification
  - Detects "who spoke when"
  - Voice embedding clustering

**Emotion Detection**:
- **speechbrain**: Speech emotion recognition
  - Models: wav2vec2-large-emotion
  - Emotions: neutral, happy, sad, angry, fearful, surprised, disgusted
- **Hugging Face Transformers**: emotion-english-distilroberta-base

### 2.2 Module Structure

```
src-tauri/src/
├── core/
│   ├── audio_capture.rs          # Audio device capture
│   ├── audio_separator.rs        # Source separation (Demucs)
│   ├── speech_transcriber.rs     # Whisper integration
│   ├── speaker_diarizer.rs       # pyannote.audio integration
│   └── emotion_detector.rs       # Emotion recognition
├── models/
│   └── audio.rs                  # Audio data structures
└── platform/
    └── audio/
        ├── macos.rs              # Core Audio + loopback
        ├── windows.rs            # WASAPI loopback
        └── linux.rs              # PulseAudio monitor
```

### 2.3 Audio Data Flow

```
┌─────────────────────────────────────────────────────────────┐
│                      Audio Capture                          │
│  ┌───────────────┐              ┌────────────────┐         │
│  │  Microphone   │              │  System Audio  │         │
│  │   Input       │              │   (Loopback)   │         │
│  └───────┬───────┘              └────────┬───────┘         │
│          │                                │                 │
│          └────────────┬───────────────────┘                 │
└───────────────────────┼─────────────────────────────────────┘
                        ↓
┌───────────────────────────────────────────────────────────────┐
│                  Source Separation (Demucs)                   │
│  ┌──────────┐  ┌───────────┐  ┌──────┐  ┌────────────────┐  │
│  │  Vocals  │  │  Music    │  │ Bass │  │ Other (SFX)    │  │
│  └────┬─────┘  └─────┬─────┘  └──┬───┘  └────┬───────────┘  │
└───────┼──────────────┼───────────┼───────────┼───────────────┘
        ↓              ↓           ↓           ↓
┌───────────────┐  ┌────────────────────────────────┐
│   Vocals      │  │      Music/SFX Metadata        │
│   Stream      │  │  - Volume levels               │
└───────┬───────┘  │  - Frequency analysis          │
        ↓          │  - Source classification       │
┌──────────────────────────────────┐  └────────────────────────────────┘
│  Speaker Diarization (pyannote)  │
│  ┌──────────┐  ┌───────────┐    │
│  │ Speaker1 │  │ Speaker2  │    │
│  └────┬─────┘  └─────┬─────┘    │
└───────┼──────────────┼───────────┘
        ↓              ↓
┌────────────────────────────────────────────┐
│    Speech Transcription (Whisper)         │
│  ┌────────────────────────────────────┐   │
│  │ "Hello, how are you?" (Speaker1)   │   │
│  │ [00:01.20 - 00:02.50]              │   │
│  └────────────────────────────────────┘   │
└────────────────┬───────────────────────────┘
                 ↓
┌────────────────────────────────────────────┐
│    Emotion Detection (speechbrain)        │
│  Speaker1: Happy (0.85 confidence)        │
│  Speaker2: Neutral (0.72 confidence)      │
└────────────────────────────────────────────┘
```

### 2.4 Data Models

```rust
// Audio recording session
pub struct AudioRecording {
    pub id: String,
    pub session_id: String,
    pub start_timestamp: i64,
    pub end_timestamp: Option<i64>,
    pub sample_rate: u32,
    pub channels: u16,
    pub file_path: PathBuf,
}

// Audio chunk with source separation
pub struct AudioChunk {
    pub timestamp: i64,
    pub duration_ms: u64,
    pub sources: AudioSources,
}

pub struct AudioSources {
    pub microphone_vocals: Option<Vec<f32>>,  // User speech
    pub system_audio: Option<Vec<f32>>,       // OS/app audio
    pub music: Option<Vec<f32>>,
    pub other: Option<Vec<f32>>,
    pub classification: SourceClassification,
}

pub struct SourceClassification {
    pub is_user_speaking: bool,
    pub has_system_audio: bool,
    pub system_audio_type: Option<SystemAudioType>,  // Music, Video, Game, etc.
    pub confidence: f32,
}

pub enum SystemAudioType {
    Music,
    Video,
    Game,
    Notification,
    Other,
}

// Speech transcription result
pub struct TranscriptSegment {
    pub id: String,
    pub session_id: String,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub text: String,
    pub language: String,
    pub confidence: f32,
    pub speaker_id: Option<String>,  // From diarization
    pub words: Vec<WordTimestamp>,
}

pub struct WordTimestamp {
    pub word: String,
    pub start: i64,
    pub end: i64,
    pub confidence: f32,
}

// Speaker diarization result
pub struct SpeakerSegment {
    pub speaker_id: String,  // "SPEAKER_00", "SPEAKER_01", etc.
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub confidence: f32,
    pub embedding: Option<Vec<f32>>,  // Voice embedding for identification
}

// Emotion detection result
pub struct EmotionResult {
    pub timestamp: i64,
    pub speaker_id: Option<String>,
    pub emotion: Emotion,
    pub confidence: f32,
    pub valence: f32,   // Positive/negative sentiment (-1 to 1)
    pub arousal: f32,   // Intensity/energy (0 to 1)
}

pub enum Emotion {
    Neutral,
    Happy,
    Sad,
    Angry,
    Fearful,
    Surprised,
    Disgusted,
}
```

### 2.5 Database Schema

```sql
-- Audio recordings
CREATE TABLE audio_recordings (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    start_timestamp INTEGER NOT NULL,
    end_timestamp INTEGER,
    sample_rate INTEGER NOT NULL,
    channels INTEGER NOT NULL,
    file_path TEXT NOT NULL,
    file_size_bytes INTEGER,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

-- Separated audio sources metadata
CREATE TABLE audio_sources (
    id TEXT PRIMARY KEY,
    recording_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    has_user_speech BOOLEAN NOT NULL,
    has_system_audio BOOLEAN NOT NULL,
    system_audio_type TEXT,
    confidence REAL NOT NULL,
    vocals_file_path TEXT,
    system_file_path TEXT,
    music_file_path TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (recording_id) REFERENCES audio_recordings(id) ON DELETE CASCADE
);

-- Speech transcriptions
CREATE TABLE transcripts (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    recording_id TEXT NOT NULL,
    start_timestamp INTEGER NOT NULL,
    end_timestamp INTEGER NOT NULL,
    text TEXT NOT NULL,
    language TEXT NOT NULL,
    confidence REAL NOT NULL,
    speaker_id TEXT,
    words_json TEXT,  -- Array of word timestamps
    created_at INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE,
    FOREIGN KEY (recording_id) REFERENCES audio_recordings(id) ON DELETE CASCADE
);

CREATE VIRTUAL TABLE transcripts_fts USING fts5(
    text,
    content=transcripts,
    content_rowid=rowid
);

-- Speaker diarization
CREATE TABLE speaker_segments (
    id TEXT PRIMARY KEY,
    recording_id TEXT NOT NULL,
    speaker_id TEXT NOT NULL,
    start_timestamp INTEGER NOT NULL,
    end_timestamp INTEGER NOT NULL,
    confidence REAL NOT NULL,
    embedding_json TEXT,  -- Voice embedding vector
    created_at INTEGER NOT NULL,
    FOREIGN KEY (recording_id) REFERENCES audio_recordings(id) ON DELETE CASCADE
);

CREATE INDEX idx_speakers_recording ON speaker_segments(recording_id);
CREATE INDEX idx_speakers_id ON speaker_segments(speaker_id);

-- Emotion detection
CREATE TABLE emotion_detections (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    recording_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    speaker_id TEXT,
    emotion TEXT NOT NULL,
    confidence REAL NOT NULL,
    valence REAL NOT NULL,
    arousal REAL NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE,
    FOREIGN KEY (recording_id) REFERENCES audio_recordings(id) ON DELETE CASCADE
);

CREATE INDEX idx_emotions_session ON emotion_detections(session_id);
CREATE INDEX idx_emotions_speaker ON emotion_detections(speaker_id);
CREATE INDEX idx_emotions_type ON emotion_detections(emotion);
```

### 2.6 API Commands

```rust
// Audio Capture
#[tauri::command]
async fn start_audio_recording(
    session_id: String,
    config: AudioConfig
) -> Result<String, String>;

#[tauri::command]
async fn stop_audio_recording(recording_id: String) -> Result<(), String>;

#[tauri::command]
async fn get_audio_devices() -> Result<Vec<AudioDeviceInfo>, String>;

// Transcription
#[tauri::command]
async fn get_transcripts(
    session_id: String,
    start: i64,
    end: i64,
    speaker_id: Option<String>
) -> Result<Vec<TranscriptSegmentDto>, String>;

#[tauri::command]
async fn search_transcripts(
    query: String,
    session_id: Option<String>,
    start: Option<i64>,
    end: Option<i64>
) -> Result<Vec<TranscriptSearchResult>, String>;

// Speaker Diarization
#[tauri::command]
async fn get_speakers(
    session_id: String
) -> Result<Vec<SpeakerInfo>, String>;

#[tauri::command]
async fn get_speaker_segments(
    recording_id: String,
    speaker_id: Option<String>
) -> Result<Vec<SpeakerSegmentDto>, String>;

// Emotion Detection
#[tauri::command]
async fn get_emotions(
    session_id: String,
    start: i64,
    end: i64,
    speaker_id: Option<String>,
    emotion_type: Option<String>
) -> Result<Vec<EmotionResultDto>, String>;

#[tauri::command]
async fn get_emotion_statistics(
    session_id: String,
    speaker_id: Option<String>
) -> Result<EmotionStatistics, String>;

// Source Separation
#[tauri::command]
async fn get_audio_sources(
    recording_id: String,
    start: i64,
    end: i64
) -> Result<Vec<AudioSourceDto>, String>;
```

---

## 3. Integration with Existing System

### 3.1 Session Integration

Both modules integrate with the existing session system:

```rust
// In SessionManager
pub struct Session {
    pub id: String,
    pub start_timestamp: i64,
    pub end_timestamp: Option<i64>,
    // ... existing fields
    pub pose_tracking_enabled: bool,      // NEW
    pub audio_recording_enabled: bool,    // NEW
}

// Start unified recording session
#[tauri::command]
async fn start_unified_recording(
    config: UnifiedRecordingConfig
) -> Result<String, String> {
    let session_id = session_manager.create_session().await?;

    if config.enable_screen_recording {
        screen_recorder.start(session_id.clone()).await?;
    }
    if config.enable_pose_tracking {
        pose_detector.start(session_id.clone()).await?;
    }
    if config.enable_audio_recording {
        audio_recorder.start(session_id.clone()).await?;
    }

    Ok(session_id)
}
```

### 3.2 Real-time Feedback Architecture

```rust
// Event streaming for real-time updates
pub enum RecordingEvent {
    PoseDetected(PoseFrameDto),
    FacialExpression(FacialExpressionDto),
    TranscriptSegment(TranscriptSegmentDto),
    EmotionDetected(EmotionResultDto),
    SpeakerChanged(SpeakerChangeEvent),
}

// Tauri event emitter
impl PoseDetector {
    async fn emit_pose_event(&self, app_handle: &AppHandle, pose: PoseFrame) {
        app_handle.emit_all("pose-detected", pose).ok();
    }
}

// Frontend listener (TypeScript)
import { listen } from '@tauri-apps/api/event';

listen<PoseFrameDto>('pose-detected', (event) => {
    console.log('New pose detected:', event.payload);
});
```

### 3.3 Consent Management

```rust
// Add new consent types
pub enum ConsentFeature {
    // ... existing
    PoseTracking,
    AudioRecording,
    SpeechTranscription,
    EmotionDetection,
}

// Check before starting modules
impl PoseDetector {
    pub async fn start(&self, session_id: String) -> Result<()> {
        if !self.consent_manager.has_consent(ConsentFeature::PoseTracking).await? {
            return Err("Pose tracking consent not granted".into());
        }
        // ... proceed
    }
}
```

---

## 4. Performance Considerations

### 4.1 Pose Estimation Performance

- **Target FPS**: 15-30 fps (balance between accuracy and CPU usage)
- **Processing**: Async pipeline with frame buffering
- **Optimization**:
  - Skip frames when no motion detected
  - Use lower resolution input (640x480 sufficient for MediaPipe)
  - GPU acceleration via ONNX Runtime CUDA/CoreML

### 4.2 Audio Processing Performance

- **Real-time Requirements**:
  - Source separation: ~100ms latency with Demucs (GPU required for real-time)
  - Transcription: Batch processing every 5-10 seconds
  - Diarization: Post-processing (non-real-time)
  - Emotion: Real-time with lightweight models

- **Optimization**:
  - Use Whisper tiny/base models for real-time transcription
  - Demucs hybrid transformer (HT) model on GPU
  - Batch audio chunks for efficiency
  - Async processing pipeline with thread pools

### 4.3 Storage Estimates

**Pose Data** (per hour):
- 30 fps × 3600s = 108,000 frames
- ~2KB per frame (JSON) = ~216 MB/hour

**Audio Data** (per hour):
- Raw audio (48kHz, stereo): ~660 MB/hour (WAV)
- Compressed (AAC): ~60 MB/hour
- Separated sources (4 tracks): ~240 MB/hour
- Transcripts: ~1-5 MB/hour (text)

---

## 5. Dependencies

### 5.1 Rust Crates

```toml
[dependencies]
# Audio
cpal = "0.15"                      # Cross-platform audio
hound = "3.5"                      # WAV file I/O
rubato = "0.15"                    # Resampling

# Computer Vision
opencv = "0.92"                    # Image processing
ort = "2.0"                        # ONNX Runtime

# Python Interop (for ML models)
pyo3 = { version = "0.21", features = ["auto-initialize"] }

# Existing dependencies used
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sqlx = { version = "0.7", features = ["sqlite", "runtime-tokio-native-tls"] }
```

### 5.2 Python Dependencies (via PyO3)

```python
# requirements.txt
mediapipe==0.10.11               # Pose/face tracking
openai-whisper==20231117         # Speech transcription
pyannote.audio==3.1.1            # Speaker diarization
demucs==4.0.1                    # Source separation
speechbrain==1.0.0               # Emotion detection
torch==2.2.0                     # PyTorch backend
onnxruntime-gpu==1.17.0          # ONNX inference (GPU)
```

### 5.3 System Requirements

- **GPU**: NVIDIA GPU (CUDA 11.8+) or Apple Silicon (Metal) for real-time processing
- **RAM**: 8GB minimum, 16GB recommended
- **Disk**: SSD recommended for real-time I/O
- **FFmpeg**: 8.0+ (already required)

---

## 6. Module Independence & Composability

Each module can be:
1. **Enabled/Disabled Independently**: Via config flags
2. **Run Standalone**: Modules don't depend on each other
3. **Data Sharing**: Via session_id foreign keys in database
4. **Event Streaming**: Modules emit events that others can subscribe to

Example configurations:
- Pose tracking only (no audio)
- Audio transcription only (no pose)
- Full suite (pose + audio + emotion + diarization)
- Audio separation + transcription (no emotion detection)

---

## 7. Next Steps: Implementation Plan

1. **Phase 1: Pose Estimation**
   - Set up MediaPipe integration
   - Implement pose detection pipeline
   - Create database schema and models
   - Add Tauri commands

2. **Phase 2: Audio Capture**
   - Platform-specific audio capture
   - Loopback audio integration
   - Audio file storage

3. **Phase 3: Source Separation**
   - Demucs integration
   - Real-time separation pipeline
   - Source classification

4. **Phase 4: Speech Transcription**
   - Whisper integration
   - Batch processing pipeline
   - Full-text search integration

5. **Phase 5: Speaker Diarization**
   - pyannote.audio integration
   - Speaker clustering
   - Transcript attribution

6. **Phase 6: Emotion Detection**
   - Model integration (speechbrain)
   - Real-time emotion pipeline
   - Statistics aggregation

7. **Phase 7: Integration & Testing**
   - Unified recording command
   - Real-time event streaming
   - Performance optimization
   - End-to-end testing

---

This architecture provides a comprehensive, modular approach to pose estimation and audio processing that integrates seamlessly with the existing "0" application while maintaining independence and composability.
