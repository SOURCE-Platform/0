#!/usr/bin/env python3
"""
OpenAI Whisper Speech Transcription
Bridge for Rust PyO3 integration
"""

import whisper
import numpy as np
import json
from typing import Dict, List, Optional
import tempfile
import os


class WhisperInference:
    """OpenAI Whisper speech-to-text transcription"""

    def __init__(
        self,
        model_size: str = "base",  # tiny, base, small, medium, large
        device: Optional[str] = None,  # cuda, cpu, or None for auto
        language: Optional[str] = None,  # e.g., "en", or None for auto-detect
    ):
        """
        Initialize Whisper model

        Args:
            model_size: Model size (tiny, base, small, medium, large)
            device: Device to run on (cuda/cpu/None for auto)
            language: Language code (None for auto-detection)
        """
        self.model_size = model_size
        self.language = language

        print(f"Loading Whisper model: {model_size}...")
        self.model = whisper.load_model(model_size, device=device)
        print(f"Whisper model loaded successfully on {self.model.device}")

    def transcribe_audio(
        self,
        audio_path: str,
        word_timestamps: bool = True,
        temperature: float = 0.0,
        best_of: int = 5,
        beam_size: int = 5,
    ) -> Dict:
        """
        Transcribe audio file to text with timestamps

        Args:
            audio_path: Path to audio file (mp3, wav, m4a, etc.)
            word_timestamps: Include word-level timestamps
            temperature: Sampling temperature (0 for greedy)
            best_of: Number of candidates when sampling
            beam_size: Beam size for beam search

        Returns:
            Dictionary with transcription results
        """
        print(f"Transcribing audio: {audio_path}")

        result = self.model.transcribe(
            audio_path,
            language=self.language,
            word_timestamps=word_timestamps,
            temperature=temperature,
            best_of=best_of,
            beam_size=beam_size,
        )

        # Extract segments with timestamps
        segments = []
        for segment in result["segments"]:
            segment_data = {
                "id": segment["id"],
                "start": segment["start"],
                "end": segment["end"],
                "text": segment["text"].strip(),
                "confidence": segment.get("avg_logprob", 0.0),
            }

            # Add word-level timestamps if available
            if word_timestamps and "words" in segment:
                words = []
                for word in segment["words"]:
                    words.append({
                        "word": word["word"].strip(),
                        "start": word["start"],
                        "end": word["end"],
                        "probability": word.get("probability", 1.0),
                    })
                segment_data["words"] = words

            segments.append(segment_data)

        return {
            "text": result["text"].strip(),
            "language": result["language"],
            "segments": segments,
            "duration": result.get("duration", 0.0),
        }

    def transcribe_audio_bytes(
        self,
        audio_bytes: bytes,
        sample_rate: int = 16000,
        word_timestamps: bool = True,
    ) -> Dict:
        """
        Transcribe audio from bytes (for in-memory processing)

        Args:
            audio_bytes: Raw audio bytes (PCM float32)
            sample_rate: Audio sample rate
            word_timestamps: Include word-level timestamps

        Returns:
            Dictionary with transcription results
        """
        # Convert bytes to numpy array
        audio_array = np.frombuffer(audio_bytes, dtype=np.float32)

        # Whisper expects 16kHz mono audio
        # If sample rate is different, we need to resample
        if sample_rate != 16000:
            # Use librosa for resampling
            import librosa
            audio_array = librosa.resample(
                audio_array,
                orig_sr=sample_rate,
                target_sr=16000,
            )

        # Transcribe
        result = self.model.transcribe(
            audio_array,
            language=self.language,
            word_timestamps=word_timestamps,
        )

        # Format results (same as transcribe_audio)
        segments = []
        for segment in result["segments"]:
            segment_data = {
                "id": segment["id"],
                "start": segment["start"],
                "end": segment["end"],
                "text": segment["text"].strip(),
                "confidence": segment.get("avg_logprob", 0.0),
            }

            if word_timestamps and "words" in segment:
                words = []
                for word in segment["words"]:
                    words.append({
                        "word": word["word"].strip(),
                        "start": word["start"],
                        "end": word["end"],
                        "probability": word.get("probability", 1.0),
                    })
                segment_data["words"] = words

            segments.append(segment_data)

        return {
            "text": result["text"].strip(),
            "language": result["language"],
            "segments": segments,
            "duration": result.get("duration", 0.0),
        }


def transcribe_file(
    audio_path: str,
    model_size: str = "base",
    language: Optional[str] = None,
    word_timestamps: bool = True,
) -> str:
    """
    Transcribe audio file and return JSON results
    This is the main entry point for PyO3 bridge

    Args:
        audio_path: Path to audio file
        model_size: Whisper model size
        language: Language code (None for auto-detect)
        word_timestamps: Include word-level timestamps

    Returns:
        JSON string with transcription results
    """
    inference = WhisperInference(model_size=model_size, language=language)
    result = inference.transcribe_audio(audio_path, word_timestamps=word_timestamps)
    return json.dumps(result, indent=2)


def transcribe_bytes(
    audio_bytes: bytes,
    sample_rate: int,
    model_size: str = "base",
    language: Optional[str] = None,
) -> str:
    """
    Transcribe audio bytes and return JSON results
    Entry point for in-memory processing

    Args:
        audio_bytes: Raw audio bytes (PCM float32)
        sample_rate: Audio sample rate
        model_size: Whisper model size
        language: Language code

    Returns:
        JSON string with transcription results
    """
    inference = WhisperInference(model_size=model_size, language=language)
    result = inference.transcribe_audio_bytes(
        audio_bytes,
        sample_rate=sample_rate,
        word_timestamps=True,
    )
    return json.dumps(result, indent=2)


if __name__ == "__main__":
    # Test with a sample audio file
    import sys

    if len(sys.argv) > 1:
        audio_file = sys.argv[1]
        model_size = sys.argv[2] if len(sys.argv) > 2 else "base"

        print(f"Transcribing {audio_file} with Whisper {model_size}...")
        result_json = transcribe_file(audio_file, model_size=model_size)

        result = json.loads(result_json)
        print("\nTranscription Results:")
        print(f"Language: {result['language']}")
        print(f"Duration: {result['duration']:.2f}s")
        print(f"\nFull Text:\n{result['text']}\n")
        print(f"Segments: {len(result['segments'])}")

        for i, segment in enumerate(result['segments'][:3]):
            print(f"\nSegment {i+1}:")
            print(f"  Time: {segment['start']:.2f}s - {segment['end']:.2f}s")
            print(f"  Text: {segment['text']}")
            if "words" in segment:
                print(f"  Words: {len(segment['words'])}")
    else:
        print("Usage: python whisper_inference.py <audio_file> [model_size]")
        print("Example: python whisper_inference.py audio.wav base")
