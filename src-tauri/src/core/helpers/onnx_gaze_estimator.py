import math
import os
import urllib.request

import cv2
import numpy as np
import onnxruntime as ort


ACTIVE_GAZE_MODEL_NAME = "mobileone_s0_onnx"
ACTIVE_GAZE_MODEL_VERSION = "v1"
MODEL_URL = (
    "https://github.com/yakhyo/gaze-estimation/releases/download/weights/"
    "mobileone_s0_gaze.onnx"
)

_SESSION = None


def estimate_gaze(image_bgr, face_bbox):
    if image_bgr is None or face_bbox is None:
        return _unavailable("Face crop unavailable")

    try:
        crop = _crop_face(image_bgr, face_bbox)
        if crop is None or crop.size == 0:
            return _unavailable("Face crop unavailable")

        session = _get_session()
        input_tensor = _preprocess(crop, session)
        yaw_logits, pitch_logits = session.run(None, {"input": input_tensor})
        yaw_deg, pitch_deg = _decode(yaw_logits, pitch_logits)
        yaw_rad = math.radians(yaw_deg)
        pitch_rad = math.radians(pitch_deg)
        return {
            "available": True,
            "confidence": 0.86,
            "vector": {
                "x": float(math.sin(yaw_rad) * math.cos(pitch_rad)),
                "y": float(-math.sin(pitch_rad)),
                "z": float(math.cos(yaw_rad) * math.cos(pitch_rad)),
            },
            "yawDegrees": float(yaw_deg),
            "pitchDegrees": float(pitch_deg),
            "modelName": ACTIVE_GAZE_MODEL_NAME,
            "modelVersion": ACTIVE_GAZE_MODEL_VERSION,
            "notes": ["ONNX MobileOne-S0 gaze estimator"],
        }
    except Exception as error:
        return _unavailable(str(error))


def _unavailable(reason):
    return {
        "available": False,
        "confidence": 0.0,
        "vector": None,
        "yawDegrees": None,
        "pitchDegrees": None,
        "modelName": ACTIVE_GAZE_MODEL_NAME,
        "modelVersion": ACTIVE_GAZE_MODEL_VERSION,
        "notes": [reason],
    }


def _get_session():
    global _SESSION
    if _SESSION is None:
        model_path = _ensure_model()
        _SESSION = ort.InferenceSession(model_path, providers=["CPUExecutionProvider"])
    return _SESSION


def _ensure_model():
    home = os.path.expanduser("~")
    models_dir = os.path.join(home, ".observer_data", "models", "gaze")
    os.makedirs(models_dir, exist_ok=True)
    model_path = os.path.join(models_dir, "mobileone_s0_gaze.onnx")
    if not os.path.exists(model_path):
        urllib.request.urlretrieve(MODEL_URL, model_path)
    return model_path


def _crop_face(image_bgr, face_bbox):
    height, width = image_bgr.shape[:2]
    x = max(0, int(face_bbox["x"] * width))
    y = max(0, int(face_bbox["y"] * height))
    w = max(1, int(face_bbox["width"] * width))
    h = max(1, int(face_bbox["height"] * height))
    pad_x = max(4, int(w * 0.08))
    pad_y = max(4, int(h * 0.08))
    x0 = max(0, x - pad_x)
    y0 = max(0, y - pad_y)
    x1 = min(width, x + w + pad_x)
    y1 = min(height, y + h + pad_y)
    return image_bgr[y0:y1, x0:x1]


def _preprocess(face_bgr, session):
    input_cfg = session.get_inputs()[0]
    input_h = int(input_cfg.shape[2])
    input_w = int(input_cfg.shape[3])
    image = cv2.cvtColor(face_bgr, cv2.COLOR_BGR2RGB)
    image = cv2.resize(image, (input_w, input_h))
    image = image.astype(np.float32) / 255.0
    mean = np.array([0.485, 0.456, 0.406], dtype=np.float32)
    std = np.array([0.229, 0.224, 0.225], dtype=np.float32)
    image = (image - mean) / std
    image = np.transpose(image, (2, 0, 1))
    return np.expand_dims(image, axis=0).astype(np.float32)


def _decode(yaw_logits, pitch_logits):
    bins = 90
    bin_width = 4.0
    angle_offset = 180.0
    idx = np.arange(bins, dtype=np.float32)
    yaw_probs = _softmax(yaw_logits)
    pitch_probs = _softmax(pitch_logits)
    yaw = np.sum(yaw_probs * idx, axis=1) * bin_width - angle_offset
    pitch = np.sum(pitch_probs * idx, axis=1) * bin_width - angle_offset
    return float(yaw[0]), float(pitch[0])


def _softmax(logits):
    values = logits - np.max(logits, axis=1, keepdims=True)
    exp = np.exp(values)
    return exp / exp.sum(axis=1, keepdims=True)
