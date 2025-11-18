# Python ML Inference Scripts

This directory contains Python scripts for ML inference, designed to be called from Rust via PyO3.

## Installation

```bash
# Create a virtual environment (recommended)
python -m venv venv
source venv/bin/activate  # On Windows: venv\Scripts\activate

# Install dependencies
pip install -r requirements.txt
```

## Scripts

### 1. MediaPipe Inference (`mediapipe_inference.py`)

Performs pose estimation, face mesh tracking, and hand tracking using Google MediaPipe.

**Features:**
- 33 body pose keypoints
- 468 facial landmarks + 52 ARKit-compatible blendshapes
- 21 hand keypoints per hand (supports 2 hands)

**Usage:**
```python
from mediapipe_inference import process_image_bytes

# Process an image
result_json = process_image_bytes(
    image_bytes=rgb_bytes,
    width=640,
    height=480,
    enable_pose=True,
    enable_face=True,
    enable_hands=True,
    model_complexity=2,  # 0=lite, 1=full, 2=heavy
)
```

**Output:**
```json
{
  "body_pose": {
    "keypoints": [...],
    "count": 33
  },
  "face_mesh": {
    "landmarks": [...],
    "blendshapes": {...},
    "landmark_count": 468
  },
  "hands": [
    {
      "hand_type": "Left",
      "confidence": 0.95,
      "keypoints": [...]
    }
  ]
}
```

### 2. Whisper Transcription (`whisper_inference.py`)

Speech-to-text transcription using OpenAI Whisper.

**Features:**
- Multilingual transcription (99 languages)
- Word-level timestamps
- Automatic language detection
- Multiple model sizes (tiny, base, small, medium, large)

**Usage:**
```python
from whisper_inference import transcribe_file

# Transcribe an audio file
result_json = transcribe_file(
    audio_path="audio.wav",
    model_size="base",  # tiny, base, small, medium, large
    language=None,  # Auto-detect
    word_timestamps=True,
)
```

**Output:**
```json
{
  "text": "Full transcription text...",
  "language": "en",
  "segments": [
    {
      "id": 0,
      "start": 0.0,
      "end": 3.5,
      "text": "Hello world",
      "words": [
        {"word": "Hello", "start": 0.0, "end": 0.5},
        {"word": "world", "start": 0.6, "end": 1.0}
      ]
    }
  ]
}
```

### 3. Speaker Diarization (`diarization_inference.py`)

Speaker identification and segmentation using pyannote.audio.

**Features:**
- Automatic speaker detection
- Speaker embeddings (512-dim vectors)
- Timeline of who spoke when
- Speaker statistics

**Requirements:**
- Hugging Face account and auth token
- Accept terms at: https://hf.co/pyannote/speaker-diarization-3.1

**Usage:**
```python
from diarization_inference import diarize_file

# Diarize an audio file
result_json = diarize_file(
    audio_path="conversation.wav",
    auth_token="hf_...",
    num_speakers=None,  # Auto-detect
    min_speakers=2,
    max_speakers=10,
)
```

**Output:**
```json
{
  "segments": [
    {
      "speaker_id": "SPEAKER_00",
      "start": 0.0,
      "end": 3.5,
      "duration": 3.5
    }
  ],
  "speakers": ["SPEAKER_00", "SPEAKER_01"],
  "num_speakers": 2,
  "speaker_stats": {
    "SPEAKER_00": {
      "total_duration": 45.2,
      "num_segments": 12,
      "embedding": [...]
    }
  }
}
```

### 4. Emotion Recognition (`emotion_inference.py`)

Speech emotion detection using SpeechBrain.

**Features:**
- 7 emotion categories (neutral, happy, sad, angry, fearful, disgusted, surprised)
- Valence and arousal scores
- Probability distribution over emotions
- Segment-level analysis

**Usage:**
```python
from emotion_inference import predict_file

# Predict emotion from audio
result_json = predict_file(
    audio_path="speech.wav",
    model_name="speechbrain/emotion-recognition-wav2vec2-IEMOCAP",
)
```

**Output:**
```json
{
  "emotion": "happy",
  "confidence": 0.89,
  "valence": 0.8,
  "arousal": 0.7,
  "probabilities": {
    "happy": 0.89,
    "neutral": 0.05,
    "sad": 0.02,
    ...
  }
}
```

## Testing

Each script can be run standalone for testing:

```bash
# Test MediaPipe
python mediapipe_inference.py

# Test Whisper
python whisper_inference.py audio.wav base

# Test Diarization
HF_TOKEN=your_token python diarization_inference.py audio.wav

# Test Emotion
python emotion_inference.py speech.wav
```

## Integration with Rust

These scripts are designed to be called from Rust using PyO3. See the ML Integration Guide in `docs/ml-integration-guide.md` for details on how to integrate with the Rust codebase.

## Performance

- **MediaPipe**: ~30-60 FPS on CPU, ~120+ FPS on GPU
- **Whisper (base)**: ~0.5x realtime on CPU, ~5x realtime on GPU
- **Diarization**: ~1x realtime on CPU, ~3x realtime on GPU
- **Emotion**: ~10x realtime on CPU, ~50x realtime on GPU

## Model Sizes

- **Whisper**:
  - tiny: 39 MB, fastest
  - base: 74 MB, good balance
  - small: 244 MB, more accurate
  - medium: 769 MB, high accuracy
  - large: 1550 MB, best accuracy

- **MediaPipe**: ~10-13 MB per model
- **pyannote**: ~17 MB
- **SpeechBrain**: ~378 MB

## License

These scripts integrate multiple open-source projects:
- MediaPipe: Apache 2.0
- Whisper: MIT
- pyannote.audio: MIT
- SpeechBrain: Apache 2.0

See individual project licenses for details.
