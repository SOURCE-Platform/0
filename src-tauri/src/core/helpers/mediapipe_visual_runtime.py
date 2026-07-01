import json
import math
import sys

import cv2
import mediapipe as mp
from onnx_gaze_estimator import estimate_gaze
from mediapipe_visual_math import (
    LEFT_EYE,
    RIGHT_EYE,
    LEFT_IRIS,
    RIGHT_IRIS,
    average_point,
    classify_posture,
    estimate_head_pose,
    face_landmark_to_dict,
    get_face_points,
    min_max_box,
    pose_landmark_to_dict,
)

POSE = mp.solutions.pose.Pose(
    static_image_mode=True,
    model_complexity=1,
    enable_segmentation=False,
)
FACE = mp.solutions.face_mesh.FaceMesh(
    static_image_mode=True,
    refine_landmarks=True,
    max_num_faces=1,
)
def analyze_pose(image_rgb):
    result = POSE.process(image_rgb)
    landmarks = result.pose_landmarks.landmark if result.pose_landmarks else []
    world = result.pose_world_landmarks.landmark if result.pose_world_landmarks else []
    if not landmarks:
        return {
            "personCount": 0,
            "presenceLabel": "no_person_visible",
            "presenceConfidence": 0.9,
            "postureLabel": "unknown",
            "postureConfidence": 0.2,
            "bodyBbox": None,
            "landmarks": {},
            "worldLandmarks": {},
            "notes": ["No pose landmarks detected"],
        }
    pose_names = mp.solutions.pose.PoseLandmark
    mapped = {}
    raw_points = []
    raw_world = []
    for index, landmark in enumerate(landmarks):
        item = pose_landmark_to_dict(landmark)
        raw_points.append(item)
        mapped[pose_names(index).name.lower()] = item
    for landmark in world:
        raw_world.append(pose_landmark_to_dict(landmark))
    posture_label, posture_confidence, notes = classify_posture(mapped)
    confidence = sum(point["visibility"] for point in raw_points) / len(raw_points)
    return {
        "personCount": 1,
        "presenceLabel": "person_visible",
        "presenceConfidence": float(max(0.45, min(0.98, confidence))),
        "postureLabel": posture_label,
        "postureConfidence": float(posture_confidence),
        "bodyBbox": min_max_box(raw_points),
        "landmarks": {"named": mapped, "all": raw_points},
        "worldLandmarks": raw_world,
        "notes": notes,
    }


def analyze_face(image_rgb, width, height):
    result = FACE.process(image_rgb)
    faces = result.multi_face_landmarks or []
    if not faces:
        return {
            "faceBbox": None,
            "faceCenterX": 0.5,
            "faceCenterY": 0.5,
            "faceWidth": 0.0,
            "faceHeight": 0.0,
            "leftEyeX": None,
            "leftEyeY": None,
            "rightEyeX": None,
            "rightEyeY": None,
            "eyeMidX": None,
            "eyeMidY": None,
            "interEyeDistance": None,
            "yaw": None,
            "pitch": None,
            "roll": None,
            "confidence": 0.0,
            "notes": ["No face landmarks detected"],
            "faceLandmarks": [],
            "leftIrisLandmarks": [],
            "rightIrisLandmarks": [],
            "headPose": None,
        }
    landmarks = faces[0].landmark
    face_landmarks = [face_landmark_to_dict(landmark) for landmark in landmarks]
    face_bbox = min_max_box(face_landmarks)
    left_iris = get_face_points(landmarks, LEFT_IRIS)
    right_iris = get_face_points(landmarks, RIGHT_IRIS)
    left_eye_outer = face_landmarks[LEFT_EYE["outer"]]
    left_eye_inner = face_landmarks[LEFT_EYE["inner"]]
    right_eye_inner = face_landmarks[RIGHT_EYE["inner"]]
    right_eye_outer = face_landmarks[RIGHT_EYE["outer"]]
    left_eye_upper = face_landmarks[LEFT_EYE["upper"]]
    left_eye_lower = face_landmarks[LEFT_EYE["lower"]]
    right_eye_upper = face_landmarks[RIGHT_EYE["upper"]]
    right_eye_lower = face_landmarks[RIGHT_EYE["lower"]]
    left_center = average_point(left_iris) if left_iris else None
    right_center = average_point(right_iris) if right_iris else None
    eye_mid = average_point([point for point in [left_center, right_center] if point]) if (left_center or right_center) else None
    distance = None
    if left_center and right_center:
        dx = left_center["x"] - right_center["x"]
        dy = left_center["y"] - right_center["y"]
        distance = math.sqrt(dx * dx + dy * dy)
    head_pose = estimate_head_pose(landmarks, width, height)
    notes = []
    if not left_iris:
        notes.append("Left iris unavailable")
    if not right_iris:
        notes.append("Right iris unavailable")
    yaw = head_pose["yaw"] if head_pose else None
    pitch = head_pose["pitch"] if head_pose else None
    roll = head_pose["roll"] if head_pose else None
    return {
        "faceBbox": face_bbox,
        "faceCenterX": float(face_bbox["x"] + face_bbox["width"] / 2.0) if face_bbox else 0.5,
        "faceCenterY": float(face_bbox["y"] + face_bbox["height"] / 2.0) if face_bbox else 0.5,
        "faceWidth": float(face_bbox["width"]) if face_bbox else 0.0,
        "faceHeight": float(face_bbox["height"]) if face_bbox else 0.0,
        "leftEyeX": None if not left_center else left_center["x"],
        "leftEyeY": None if not left_center else left_center["y"],
        "rightEyeX": None if not right_center else right_center["x"],
        "rightEyeY": None if not right_center else right_center["y"],
        "eyeMidX": None if not eye_mid else eye_mid["x"],
        "eyeMidY": None if not eye_mid else eye_mid["y"],
        "interEyeDistance": None if distance is None else float(distance),
        "yaw": None if yaw is None else float(yaw),
        "pitch": None if pitch is None else float(pitch),
        "roll": None if roll is None else float(roll),
        "confidence": 0.92,
        "notes": notes,
        "faceLandmarks": face_landmarks,
        "leftIrisLandmarks": left_iris,
        "rightIrisLandmarks": right_iris,
        "headPose": head_pose,
        "leftEyeOuter": left_eye_outer,
        "leftEyeInner": left_eye_inner,
        "leftEyeUpper": left_eye_upper,
        "leftEyeLower": left_eye_lower,
        "rightEyeOuter": right_eye_outer,
        "rightEyeInner": right_eye_inner,
        "rightEyeUpper": right_eye_upper,
        "rightEyeLower": right_eye_lower,
    }


def run(mode, image_path):
    image_bgr = cv2.imread(image_path)
    if image_bgr is None:
        raise RuntimeError("Could not load image")
    image_rgb = cv2.cvtColor(image_bgr, cv2.COLOR_BGR2RGB)
    height, width = image_rgb.shape[:2]
    face = analyze_face(image_rgb, width, height)
    gaze = estimate_gaze(image_bgr, face["faceBbox"])
    face["gazeYawDegrees"] = gaze["yawDegrees"]
    face["gazePitchDegrees"] = gaze["pitchDegrees"]
    face["gazeModelName"] = gaze["modelName"]
    face["gazeModelVersion"] = gaze["modelVersion"]
    face["gazeVector"] = gaze["vector"]
    if mode == "face_iris":
        return face
    pose = analyze_pose(image_rgb)
    return {"pose": pose, "faceIris": face, "gaze": gaze}


if __name__ == "__main__":
    if len(sys.argv) < 3:
        raise SystemExit("Usage: mediapipe_visual_runtime.py <scene|face_iris> <image_path>")
    print(json.dumps(run(sys.argv[1], sys.argv[2])))
