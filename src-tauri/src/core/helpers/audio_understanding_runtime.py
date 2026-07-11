import csv
import json
import os
import sys
import urllib.request
import wave

import numpy as np
import onnxruntime as ort


SPEECH_EMOTION_MODEL_NAME = "speech_emotion_classification_onnx"
SPEECH_EMOTION_MODEL_VERSION = "v1"
SPEECH_EMOTION_MODEL_URL = (
    "https://huggingface.co/onnx-community/"
    "Speech-Emotion-Classification-ONNX/resolve/main/onnx/model.onnx?download=true"
)
SPEECH_EMOTION_CONFIG_URL = (
    "https://huggingface.co/onnx-community/"
    "Speech-Emotion-Classification-ONNX/resolve/main/config.json?download=true"
)

YAMNET_MODEL_NAME = "yamnet_onnx"
YAMNET_MODEL_VERSION = "v1"
YAMNET_MODEL_URL = (
    "https://raw.githubusercontent.com/quic/ai-hub-models/main/models/yamnet/export/YamNet.onnx"
)
YAMNET_LABELS_URL = (
    "https://raw.githubusercontent.com/tensorflow/models/master/"
    "research/audioset/yamnet/yamnet_class_map.csv"
)

_SPEECH_SESSION = None
_YAMNET_SESSION = None
_SPEECH_LABELS = None
_YAMNET_LABELS = None


def run(mode, audio_path):
    waveform = _load_waveform(audio_path)
    if mode == "speech_emotion":
        return _run_speech_emotion(waveform)
    if mode == "sound_events":
        return _run_sound_events(waveform)
    raise RuntimeError(f"Unknown mode: {mode}")


def _run_speech_emotion(waveform):
    try:
        session = _get_speech_session()
        labels = _get_speech_labels()
        inputs = _prepare_waveform_inputs(session, waveform)
        outputs = session.run(None, inputs)
        logits = np.array(outputs[0])[0]
        probs = _softmax(logits)
        best_index = int(np.argmax(probs))
        label = labels.get(str(best_index), labels.get(best_index, "unknown"))
        top_candidates = _top_candidates(probs, labels, limit=4)
        return {
            "available": True,
            "label": label,
            "canonicalLabel": _canonical_emotion_label(label),
            "confidence": float(probs[best_index]),
            "modelName": SPEECH_EMOTION_MODEL_NAME,
            "modelVersion": SPEECH_EMOTION_MODEL_VERSION,
            "topCandidates": top_candidates,
            "rawJson": {"probabilities": top_candidates},
            "notes": ["Local ONNX speech emotion classifier"],
        }
    except Exception as error:
        return {
            "available": False,
            "label": None,
            "canonicalLabel": None,
            "confidence": 0.0,
            "modelName": SPEECH_EMOTION_MODEL_NAME,
            "modelVersion": SPEECH_EMOTION_MODEL_VERSION,
            "topCandidates": [],
            "rawJson": {"error": str(error)},
            "notes": [str(error)],
        }


def _run_sound_events(waveform):
    try:
        session = _get_yamnet_session()
        labels = _get_yamnet_labels()
        inputs = _prepare_waveform_inputs(session, waveform)
        outputs = session.run(None, inputs)
        scores = np.array(outputs[0])
        if scores.ndim == 1:
            class_scores = scores
        else:
            class_scores = np.mean(scores, axis=0)
        top_candidates = _top_candidates(class_scores, labels, limit=5)
        events = []
        for candidate in top_candidates:
            canonical = _canonical_sound_label(candidate["label"])
            events.append(
                {
                    "label": candidate["label"],
                    "canonicalLabel": canonical,
                    "confidence": candidate["confidence"],
                }
            )
        return {
            "available": True,
            "modelName": YAMNET_MODEL_NAME,
            "modelVersion": YAMNET_MODEL_VERSION,
            "events": events,
            "rawJson": {"topCandidates": top_candidates},
            "notes": ["Local YAMNet ONNX sound event classifier"],
        }
    except Exception as error:
        return {
            "available": False,
            "modelName": YAMNET_MODEL_NAME,
            "modelVersion": YAMNET_MODEL_VERSION,
            "events": [],
            "rawJson": {"error": str(error)},
            "notes": [str(error)],
        }


def _load_waveform(path):
    with wave.open(path, "rb") as wav:
        sample_rate = wav.getframerate()
        channels = wav.getnchannels()
        sample_width = wav.getsampwidth()
        frames = wav.readframes(wav.getnframes())
    if sample_width != 2:
        raise RuntimeError("Expected 16-bit PCM WAV input")
    samples = np.frombuffer(frames, dtype=np.int16).astype(np.float32) / 32768.0
    if channels > 1:
        samples = samples.reshape(-1, channels).mean(axis=1)
    if sample_rate != 16000:
        raise RuntimeError(f"Expected 16kHz WAV input, got {sample_rate}Hz")
    return samples


def _prepare_waveform_inputs(session, waveform):
    prepared = waveform.astype(np.float32)
    inputs = {}
    session_inputs = session.get_inputs()
    for input_meta in session_inputs:
        name = input_meta.name
        shape = list(input_meta.shape)
        if name == "attention_mask":
            mask = np.ones((1, prepared.shape[0]), dtype=np.int64)
            inputs[name] = mask
            continue
        values = prepared
        if len(shape) == 2:
            values = np.expand_dims(values, axis=0)
            fixed_len = shape[1] if isinstance(shape[1], int) and shape[1] > 0 else None
        else:
            fixed_len = shape[0] if isinstance(shape[0], int) and shape[0] > 0 else None
        if fixed_len is not None:
            values = _pad_or_trim(values, fixed_len)
        inputs[name] = values.astype(np.float32)
    return inputs


def _pad_or_trim(values, length):
    if values.ndim == 2:
        current = values.shape[1]
        if current > length:
            return values[:, :length]
        if current < length:
            return np.pad(values, ((0, 0), (0, length - current)))
        return values
    current = values.shape[0]
    if current > length:
        return values[:length]
    if current < length:
        return np.pad(values, (0, length - current))
    return values


def _top_candidates(scores, labels, limit):
    indices = np.argsort(scores)[::-1][:limit]
    return [
        {
            "index": int(index),
            "label": labels.get(str(int(index)), labels.get(int(index), f"class_{index}")),
            "confidence": float(scores[index]),
        }
        for index in indices
    ]


def _softmax(logits):
    values = logits - np.max(logits)
    exp = np.exp(values)
    return exp / np.sum(exp)


def _get_speech_session():
    global _SPEECH_SESSION
    if _SPEECH_SESSION is None:
        _SPEECH_SESSION = ort.InferenceSession(
            _ensure_speech_model(), providers=["CPUExecutionProvider"]
        )
    return _SPEECH_SESSION


def _get_yamnet_session():
    global _YAMNET_SESSION
    if _YAMNET_SESSION is None:
        _YAMNET_SESSION = ort.InferenceSession(
            _ensure_yamnet_model(), providers=["CPUExecutionProvider"]
        )
    return _YAMNET_SESSION


def _get_speech_labels():
    global _SPEECH_LABELS
    if _SPEECH_LABELS is None:
        with open(_ensure_speech_config(), "r", encoding="utf-8") as handle:
            config = json.load(handle)
        _SPEECH_LABELS = config.get("id2label", {})
    return _SPEECH_LABELS


def _get_yamnet_labels():
    global _YAMNET_LABELS
    if _YAMNET_LABELS is None:
        labels_path = _ensure_yamnet_labels()
        labels = {}
        with open(labels_path, "r", encoding="utf-8") as handle:
            for row in csv.DictReader(handle):
                labels[int(row["index"])] = row["display_name"]
        _YAMNET_LABELS = labels
    return _YAMNET_LABELS


def _ensure_speech_model():
    return _download_if_missing(
        "audio/speech_emotion/model.onnx", SPEECH_EMOTION_MODEL_URL
    )


def _ensure_speech_config():
    return _download_if_missing("audio/speech_emotion/config.json", SPEECH_EMOTION_CONFIG_URL)


def _ensure_yamnet_model():
    return _download_if_missing("audio/yamnet/YamNet.onnx", YAMNET_MODEL_URL)


def _ensure_yamnet_labels():
    return _download_if_missing(
        "audio/yamnet/yamnet_class_map.csv", YAMNET_LABELS_URL
    )


def _download_if_missing(relative_path, url):
    home = os.path.expanduser("~")
    path = os.path.join(home, ".observer_data", "models", *relative_path.split("/"))
    os.makedirs(os.path.dirname(path), exist_ok=True)
    if not os.path.exists(path):
        urllib.request.urlretrieve(url, path)
    return path


def _canonical_emotion_label(label):
    value = str(label).strip().lower().replace("-", "_").replace(" ", "_")
    if value in {"happy", "joy", "excited"}:
        return "happy"
    if value in {"sad", "sadness"}:
        return "sad"
    if value in {"angry", "anger"}:
        return "angry"
    if value in {"fear", "fearful"}:
        return "fearful"
    if value in {"surprise", "surprised"}:
        return "surprised"
    if value in {"neutral", "calm"}:
        return value
    return "uncertain"


def _canonical_sound_label(label):
    value = str(label).strip().lower()
    if "speech" in value or "conversation" in value:
        return "speech"
    if "music" in value:
        return "music"
    if "typing" in value or "keyboard" in value:
        return "keyboard"
    if "footstep" in value or "walk" in value:
        return "footsteps"
    if "door" in value:
        return "door"
    if "slam" in value or "bang" in value or "thud" in value or "impact" in value:
        return "impact"
    if "car" in value or "engine" in value or "vehicle" in value:
        return "vehicle"
    if "dog" in value or "bark" in value:
        return "dog"
    if "water" in value or "rain" in value:
        return "water"
    if "alarm" in value or "siren" in value:
        return "alarm"
    if "appliance" in value or "microwave" in value or "blender" in value:
        return "appliance"
    if "crowd" in value or "applause" in value or "cheer" in value:
        return "crowd"
    return "unknown_sound"


if __name__ == "__main__":
    if len(sys.argv) < 3:
        raise SystemExit(
            "Usage: audio_understanding_runtime.py <speech_emotion|sound_events> <audio_path>"
        )

    print(json.dumps(run(sys.argv[1], sys.argv[2])))
