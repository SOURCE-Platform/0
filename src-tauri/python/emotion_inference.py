#!/usr/bin/env python3
"""
SpeechBrain Emotion Recognition
Bridge for Rust PyO3 integration
"""

from speechbrain.pretrained import EncoderClassifier
import numpy as np
import json
from typing import Dict, List, Optional, Tuple
import torch
import torchaudio


class EmotionInference:
    """Speech emotion recognition using SpeechBrain"""

    # Emotion labels from IEMOCAP dataset
    EMOTION_LABELS = [
        "neutral",
        "happy",
        "sad",
        "angry",
        "fearful",
        "disgusted",
        "surprised",
    ]

    def __init__(
        self,
        model_name: str = "speechbrain/emotion-recognition-wav2vec2-IEMOCAP",
        device: Optional[str] = None,
    ):
        """
        Initialize emotion recognition model

        Args:
            model_name: SpeechBrain model from HuggingFace Hub
            device: Device to run on (cuda/cpu/None for auto)
        """
        self.device = device or ("cuda" if torch.cuda.is_available() else "cpu")

        print(f"Loading emotion recognition model on {self.device}...")

        # Load SpeechBrain emotion classifier
        self.classifier = EncoderClassifier.from_hparams(
            source=model_name,
            savedir=f"pretrained_models/{model_name.split('/')[-1]}",
            run_opts={"device": self.device},
        )

        print("Emotion recognition model loaded successfully")

    def predict_emotion(
        self,
        audio_path: str,
        return_probabilities: bool = True,
    ) -> Dict:
        """
        Predict emotion from audio file

        Args:
            audio_path: Path to audio file
            return_probabilities: Return probability distribution over all emotions

        Returns:
            Dictionary with emotion prediction and confidence
        """
        print(f"Predicting emotion for: {audio_path}")

        # Load and preprocess audio
        waveform, sample_rate = torchaudio.load(audio_path)

        # Convert to mono if stereo
        if waveform.shape[0] > 1:
            waveform = torch.mean(waveform, dim=0, keepdim=True)

        # Resample to 16kHz if needed (SpeechBrain models expect 16kHz)
        if sample_rate != 16000:
            resampler = torchaudio.transforms.Resample(sample_rate, 16000)
            waveform = resampler(waveform)

        # Run inference
        with torch.no_grad():
            out_prob, score, index, text_lab = self.classifier.classify_batch(waveform)

        # Get predicted emotion
        predicted_emotion = text_lab[0]
        confidence = score[0].item()

        # Calculate valence and arousal from emotion
        valence, arousal = self._emotion_to_valence_arousal(predicted_emotion)

        result = {
            "emotion": predicted_emotion,
            "confidence": confidence,
            "valence": valence,  # -1.0 (negative) to 1.0 (positive)
            "arousal": arousal,  # 0.0 (calm) to 1.0 (excited)
        }

        # Add probability distribution if requested
        if return_probabilities:
            probabilities = out_prob[0].cpu().numpy()

            # Map to emotion labels (if available)
            # Note: Some models may not have explicit labels
            if len(probabilities) == len(self.EMOTION_LABELS):
                emotion_probs = {
                    emotion: float(prob)
                    for emotion, prob in zip(self.EMOTION_LABELS, probabilities)
                }
                result["probabilities"] = emotion_probs

        return result

    def predict_emotion_from_bytes(
        self,
        audio_bytes: bytes,
        sample_rate: int = 16000,
    ) -> Dict:
        """
        Predict emotion from audio bytes

        Args:
            audio_bytes: Raw audio bytes (PCM float32)
            sample_rate: Audio sample rate

        Returns:
            Dictionary with emotion prediction
        """
        # Convert bytes to tensor
        audio_array = np.frombuffer(audio_bytes, dtype=np.float32)
        waveform = torch.from_numpy(audio_array).unsqueeze(0)

        # Resample if needed
        if sample_rate != 16000:
            resampler = torchaudio.transforms.Resample(sample_rate, 16000)
            waveform = resampler(waveform)

        # Run inference
        with torch.no_grad():
            out_prob, score, index, text_lab = self.classifier.classify_batch(waveform)

        predicted_emotion = text_lab[0]
        confidence = score[0].item()

        valence, arousal = self._emotion_to_valence_arousal(predicted_emotion)

        result = {
            "emotion": predicted_emotion,
            "confidence": confidence,
            "valence": valence,
            "arousal": arousal,
        }

        # Add probabilities
        probabilities = out_prob[0].cpu().numpy()
        if len(probabilities) == len(self.EMOTION_LABELS):
            emotion_probs = {
                emotion: float(prob)
                for emotion, prob in zip(self.EMOTION_LABELS, probabilities)
            }
            result["probabilities"] = emotion_probs

        return result

    def analyze_audio_segments(
        self,
        audio_path: str,
        segments: List[Tuple[float, float]],
    ) -> List[Dict]:
        """
        Analyze emotion for multiple segments of audio

        Args:
            audio_path: Path to audio file
            segments: List of (start_time, end_time) tuples in seconds

        Returns:
            List of emotion predictions for each segment
        """
        # Load audio once
        waveform, sample_rate = torchaudio.load(audio_path)

        if waveform.shape[0] > 1:
            waveform = torch.mean(waveform, dim=0, keepdim=True)

        if sample_rate != 16000:
            resampler = torchaudio.transforms.Resample(sample_rate, 16000)
            waveform = resampler(waveform)
            sample_rate = 16000

        results = []

        for start_time, end_time in segments:
            # Extract segment
            start_sample = int(start_time * sample_rate)
            end_sample = int(end_time * sample_rate)
            segment = waveform[:, start_sample:end_sample]

            # Skip very short segments
            if segment.shape[1] < 1600:  # Less than 0.1s at 16kHz
                results.append({
                    "emotion": "neutral",
                    "confidence": 0.0,
                    "valence": 0.0,
                    "arousal": 0.0,
                    "start": start_time,
                    "end": end_time,
                })
                continue

            # Run inference
            with torch.no_grad():
                out_prob, score, index, text_lab = self.classifier.classify_batch(
                    segment
                )

            predicted_emotion = text_lab[0]
            confidence = score[0].item()
            valence, arousal = self._emotion_to_valence_arousal(predicted_emotion)

            results.append({
                "emotion": predicted_emotion,
                "confidence": confidence,
                "valence": valence,
                "arousal": arousal,
                "start": start_time,
                "end": end_time,
            })

        return results

    def _emotion_to_valence_arousal(self, emotion: str) -> Tuple[float, float]:
        """
        Convert emotion label to valence and arousal values

        Based on circumplex model of affect:
        - Valence: negative (-1) to positive (+1)
        - Arousal: calm (0) to excited (1)

        Args:
            emotion: Emotion label

        Returns:
            Tuple of (valence, arousal)
        """
        emotion_map = {
            "neutral": (0.0, 0.3),
            "happy": (0.8, 0.7),
            "sad": (-0.7, 0.2),
            "angry": (-0.6, 0.9),
            "fearful": (-0.8, 0.8),
            "disgusted": (-0.7, 0.5),
            "surprised": (0.3, 0.8),
        }

        return emotion_map.get(emotion.lower(), (0.0, 0.5))


def predict_file(
    audio_path: str,
    model_name: str = "speechbrain/emotion-recognition-wav2vec2-IEMOCAP",
) -> str:
    """
    Predict emotion from audio file and return JSON results
    This is the main entry point for PyO3 bridge

    Args:
        audio_path: Path to audio file
        model_name: SpeechBrain model name

    Returns:
        JSON string with emotion prediction
    """
    inference = EmotionInference(model_name=model_name)
    result = inference.predict_emotion(audio_path)
    return json.dumps(result, indent=2)


def predict_segments(
    audio_path: str,
    segments_json: str,
    model_name: str = "speechbrain/emotion-recognition-wav2vec2-IEMOCAP",
) -> str:
    """
    Predict emotions for multiple segments

    Args:
        audio_path: Path to audio file
        segments_json: JSON array of [start, end] time pairs
        model_name: SpeechBrain model name

    Returns:
        JSON string with emotion predictions for each segment
    """
    segments = json.loads(segments_json)
    inference = EmotionInference(model_name=model_name)
    results = inference.analyze_audio_segments(audio_path, segments)
    return json.dumps(results, indent=2)


if __name__ == "__main__":
    # Test with a sample audio file
    import sys

    if len(sys.argv) > 1:
        audio_file = sys.argv[1]

        print(f"Analyzing emotion in {audio_file}...")
        result_json = predict_file(audio_file)

        result = json.loads(result_json)
        print("\nEmotion Recognition Results:")
        print(f"Emotion: {result['emotion']}")
        print(f"Confidence: {result['confidence']:.2%}")
        print(f"Valence: {result['valence']:.2f} (negative to positive)")
        print(f"Arousal: {result['arousal']:.2f} (calm to excited)")

        if "probabilities" in result:
            print("\nProbability Distribution:")
            for emotion, prob in sorted(
                result["probabilities"].items(), key=lambda x: x[1], reverse=True
            ):
                print(f"  {emotion:12s}: {prob:.2%}")
    else:
        print("Usage: python emotion_inference.py <audio_file>")
        print("Example: python emotion_inference.py speech.wav")
