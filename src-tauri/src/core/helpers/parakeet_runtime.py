"""Small local JSON wrapper around MLX Parakeet for SOURCE audio chunks."""

import json
import sys

from mlx_audio.stt.utils import load

MODEL_ID = "mlx-community/parakeet-tdt-0.6b-v3"


def get_text(result):
    text = getattr(result, "text", "")
    if text:
        return str(text).strip()
    sentences = getattr(result, "sentences", []) or []
    return " ".join(str(getattr(item, "text", "")).strip() for item in sentences).strip()


def main():
    if len(sys.argv) != 2:
        raise SystemExit("Usage: parakeet_runtime.py <audio-path>")
    model = load(MODEL_ID)
    if sys.argv[1] == "--serve":
        for line in sys.stdin:
            try:
                payload = json.loads(line)
                print_result(model, payload["audioPath"])
            except Exception as error:
                print(json.dumps({"error": str(error)}), flush=True)
        return
    print_result(model, sys.argv[1])


def print_result(model, audio_path):
    result = model.generate(audio_path)
    payload = {
        "text": get_text(result),
        "language": getattr(result, "language", None),
        "confidence": getattr(result, "confidence", None),
        "segments": get_segments(result),
    }
    print(json.dumps(payload, ensure_ascii=False), flush=True)


def get_segments(result):
    segments = []
    for sentence in getattr(result, "sentences", []) or []:
        text = str(getattr(sentence, "text", "")).strip()
        if not text:
            continue
        words = []
        for token in getattr(sentence, "tokens", []) or []:
            value = str(getattr(token, "text", "")).strip()
            if value:
                words.append({
                    "word": value,
                    "start": float(getattr(token, "start", 0.0)),
                    "end": float(getattr(token, "end", 0.0)),
                })
        segments.append({
            "text": text,
            "start": float(getattr(sentence, "start", 0.0)),
            "end": float(getattr(sentence, "end", 0.0)),
            "words": words,
        })
    return segments


if __name__ == "__main__":
    main()
