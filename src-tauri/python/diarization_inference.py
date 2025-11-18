#!/usr/bin/env python3
"""
pyannote.audio Speaker Diarization
Bridge for Rust PyO3 integration
"""

from pyannote.audio import Pipeline
from pyannote.audio.pipelines.speaker_verification import PretrainedSpeakerEmbedding
import numpy as np
import json
from typing import Dict, List, Optional
import torch


class SpeakerDiarization:
    """Speaker diarization using pyannote.audio"""

    def __init__(
        self,
        auth_token: Optional[str] = None,
        device: Optional[str] = None,
    ):
        """
        Initialize speaker diarization pipeline

        Args:
            auth_token: Hugging Face auth token (required for pyannote models)
            device: Device to run on (cuda/cpu/None for auto)
        """
        self.device = device or ("cuda" if torch.cuda.is_available() else "cpu")

        print(f"Loading speaker diarization pipeline on {self.device}...")

        # Load speaker diarization pipeline
        # Requires Hugging Face token: https://hf.co/pyannote/speaker-diarization-3.1
        self.pipeline = Pipeline.from_pretrained(
            "pyannote/speaker-diarization-3.1",
            use_auth_token=auth_token,
        )
        self.pipeline.to(torch.device(self.device))

        # Load speaker embedding model for voice fingerprints
        self.embedding_model = PretrainedSpeakerEmbedding(
            "pyannote/embedding",
            device=torch.device(self.device),
            use_auth_token=auth_token,
        )

        print("Speaker diarization pipeline loaded successfully")

    def diarize_audio(
        self,
        audio_path: str,
        num_speakers: Optional[int] = None,
        min_speakers: Optional[int] = None,
        max_speakers: Optional[int] = None,
    ) -> Dict:
        """
        Perform speaker diarization on audio file

        Args:
            audio_path: Path to audio file
            num_speakers: Exact number of speakers (if known)
            min_speakers: Minimum number of speakers
            max_speakers: Maximum number of speakers

        Returns:
            Dictionary with speaker segments and embeddings
        """
        print(f"Diarizing audio: {audio_path}")

        # Run diarization pipeline
        diarization = self.pipeline(
            audio_path,
            num_speakers=num_speakers,
            min_speakers=min_speakers,
            max_speakers=max_speakers,
        )

        # Extract speaker segments
        segments = []
        speaker_embeddings = {}

        for turn, _, speaker in diarization.itertracks(yield_label=True):
            segment = {
                "speaker_id": speaker,
                "start": turn.start,
                "end": turn.end,
                "duration": turn.end - turn.start,
            }
            segments.append(segment)

            # Extract speaker embedding if not already done
            if speaker not in speaker_embeddings:
                # Get audio for this speaker's first segment
                embedding = self._extract_speaker_embedding(
                    audio_path, turn.start, turn.end
                )
                if embedding is not None:
                    speaker_embeddings[speaker] = embedding.tolist()

        # Count speakers and calculate statistics
        unique_speakers = list(set([s["speaker_id"] for s in segments]))
        speaker_stats = {}

        for speaker in unique_speakers:
            speaker_segments = [s for s in segments if s["speaker_id"] == speaker]
            total_time = sum(s["duration"] for s in speaker_segments)

            speaker_stats[speaker] = {
                "total_duration": total_time,
                "num_segments": len(speaker_segments),
                "embedding": speaker_embeddings.get(speaker, []),
            }

        return {
            "segments": segments,
            "speakers": unique_speakers,
            "num_speakers": len(unique_speakers),
            "speaker_stats": speaker_stats,
        }

    def _extract_speaker_embedding(
        self,
        audio_path: str,
        start_time: float,
        end_time: float,
    ) -> Optional[np.ndarray]:
        """
        Extract speaker embedding from audio segment

        Args:
            audio_path: Path to audio file
            start_time: Segment start time (seconds)
            end_time: Segment end time (seconds)

        Returns:
            512-dimensional speaker embedding vector
        """
        try:
            # Load audio segment
            import torchaudio

            waveform, sample_rate = torchaudio.load(audio_path)

            # Extract segment
            start_sample = int(start_time * sample_rate)
            end_sample = int(end_time * sample_rate)
            segment = waveform[:, start_sample:end_sample]

            # Convert to mono if stereo
            if segment.shape[0] > 1:
                segment = torch.mean(segment, dim=0, keepdim=True)

            # Extract embedding
            with torch.no_grad():
                embedding = self.embedding_model(segment)

            return embedding.cpu().numpy()

        except Exception as e:
            print(f"Error extracting embedding: {e}")
            return None

    def compare_speakers(
        self,
        embedding1: np.ndarray,
        embedding2: np.ndarray,
    ) -> float:
        """
        Compare two speaker embeddings

        Args:
            embedding1: First speaker embedding
            embedding2: Second speaker embedding

        Returns:
            Similarity score (0-1, higher is more similar)
        """
        # Cosine similarity
        dot_product = np.dot(embedding1, embedding2)
        norm1 = np.linalg.norm(embedding1)
        norm2 = np.linalg.norm(embedding2)

        similarity = dot_product / (norm1 * norm2)

        # Convert to 0-1 range
        return (similarity + 1) / 2


def diarize_file(
    audio_path: str,
    auth_token: Optional[str] = None,
    num_speakers: Optional[int] = None,
    min_speakers: Optional[int] = None,
    max_speakers: Optional[int] = None,
) -> str:
    """
    Perform speaker diarization and return JSON results
    This is the main entry point for PyO3 bridge

    Args:
        audio_path: Path to audio file
        auth_token: Hugging Face auth token
        num_speakers: Exact number of speakers
        min_speakers: Minimum number of speakers
        max_speakers: Maximum number of speakers

    Returns:
        JSON string with diarization results
    """
    diarizer = SpeakerDiarization(auth_token=auth_token)
    result = diarizer.diarize_audio(
        audio_path,
        num_speakers=num_speakers,
        min_speakers=min_speakers,
        max_speakers=max_speakers,
    )
    return json.dumps(result, indent=2)


if __name__ == "__main__":
    # Test with a sample audio file
    import sys
    import os

    if len(sys.argv) > 1:
        audio_file = sys.argv[1]
        auth_token = os.getenv("HF_TOKEN")  # Get from environment variable

        if not auth_token:
            print("Warning: HF_TOKEN environment variable not set")
            print("You need a Hugging Face token to use pyannote models")
            print("Get one at: https://huggingface.co/settings/tokens")
            print("Then accept terms at: https://hf.co/pyannote/speaker-diarization-3.1")
            sys.exit(1)

        print(f"Diarizing {audio_file}...")
        result_json = diarize_file(audio_file, auth_token=auth_token)

        result = json.loads(result_json)
        print("\nDiarization Results:")
        print(f"Number of speakers: {result['num_speakers']}")
        print(f"Speakers: {', '.join(result['speakers'])}")
        print(f"\nTotal segments: {len(result['segments'])}")

        print("\nSpeaker Statistics:")
        for speaker, stats in result['speaker_stats'].items():
            print(f"\n{speaker}:")
            print(f"  Total duration: {stats['total_duration']:.2f}s")
            print(f"  Number of segments: {stats['num_segments']}")
            print(f"  Embedding dimension: {len(stats['embedding'])}")

        print(f"\nFirst 5 segments:")
        for i, segment in enumerate(result['segments'][:5]):
            print(
                f"{i+1}. {segment['speaker_id']}: "
                f"{segment['start']:.2f}s - {segment['end']:.2f}s "
                f"({segment['duration']:.2f}s)"
            )
    else:
        print("Usage: python diarization_inference.py <audio_file>")
        print("Note: Requires HF_TOKEN environment variable")
        print("Example: HF_TOKEN=your_token python diarization_inference.py audio.wav")
