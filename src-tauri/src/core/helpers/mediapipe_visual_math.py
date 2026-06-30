import math

import cv2
import numpy as np


LEFT_IRIS = [468, 469, 470, 471, 472]
RIGHT_IRIS = [473, 474, 475, 476, 477]
LEFT_EYE = {"outer": 33, "inner": 133, "upper": 159, "lower": 145}
RIGHT_EYE = {"inner": 362, "outer": 263, "upper": 386, "lower": 374}
HEAD_POSE_POINTS = {
    "nose": 1,
    "chin": 152,
    "left_eye_outer": 33,
    "right_eye_outer": 263,
    "mouth_left": 61,
    "mouth_right": 291,
}
MODEL_3D_POINTS = np.array(
    [
        (0.0, 0.0, 0.0),
        (0.0, -330.0, -65.0),
        (-225.0, 170.0, -135.0),
        (225.0, 170.0, -135.0),
        (-150.0, -150.0, -125.0),
        (150.0, -150.0, -125.0),
    ],
    dtype=np.float64,
)


def pose_landmark_to_dict(landmark):
    return {
        "x": float(landmark.x),
        "y": float(landmark.y),
        "z": float(landmark.z),
        "visibility": float(getattr(landmark, "visibility", 0.0)),
        "presence": float(getattr(landmark, "presence", 0.0)),
    }


def face_landmark_to_dict(landmark):
    return {"x": float(landmark.x), "y": float(landmark.y), "z": float(landmark.z)}


def average_point(points):
    xs = [point["x"] for point in points]
    ys = [point["y"] for point in points]
    zs = [point.get("z", 0.0) for point in points]
    return {"x": float(sum(xs) / len(xs)), "y": float(sum(ys) / len(ys)), "z": float(sum(zs) / len(zs))}


def min_max_box(points):
    if not points:
        return None
    xs = [point["x"] for point in points]
    ys = [point["y"] for point in points]
    min_x, max_x = min(xs), max(xs)
    min_y, max_y = min(ys), max(ys)
    return {"x": float(min_x), "y": float(min_y), "width": float(max_x - min_x), "height": float(max_y - min_y)}


def get_face_points(landmarks, indices):
    return [face_landmark_to_dict(landmarks[index]) for index in indices if index < len(landmarks)]


def angle_degrees(a, b, c):
    ab = (a[0] - b[0], a[1] - b[1])
    cb = (c[0] - b[0], c[1] - b[1])
    dot = ab[0] * cb[0] + ab[1] * cb[1]
    mag_ab = math.sqrt(ab[0] ** 2 + ab[1] ** 2)
    mag_cb = math.sqrt(cb[0] ** 2 + cb[1] ** 2)
    if mag_ab <= 0 or mag_cb <= 0:
        return 0.0
    cosine = max(-1.0, min(1.0, dot / (mag_ab * mag_cb)))
    return math.degrees(math.acos(cosine))


def classify_posture(landmarks):
    left_shoulder = landmarks.get("left_shoulder")
    right_shoulder = landmarks.get("right_shoulder")
    left_hip = landmarks.get("left_hip")
    right_hip = landmarks.get("right_hip")
    left_knee = landmarks.get("left_knee")
    right_knee = landmarks.get("right_knee")
    left_ankle = landmarks.get("left_ankle")
    right_ankle = landmarks.get("right_ankle")
    if not (left_shoulder and right_shoulder and left_hip and right_hip):
        return "unknown", 0.3, ["Missing stable shoulder/hip landmarks"]

    shoulder_center = (
        (left_shoulder["x"] + right_shoulder["x"]) / 2.0,
        (left_shoulder["y"] + right_shoulder["y"]) / 2.0,
    )
    hip_center = ((left_hip["x"] + right_hip["x"]) / 2.0, (left_hip["y"] + right_hip["y"]) / 2.0)
    torso_angle = abs(
        math.degrees(math.atan2(shoulder_center[0] - hip_center[0], shoulder_center[1] - hip_center[1]))
    )
    notes = [f"torso_angle={torso_angle:.1f}"]

    knee_angles = []
    if left_hip and left_knee and left_ankle:
        knee_angles.append(
            angle_degrees(
                (left_hip["x"], left_hip["y"]),
                (left_knee["x"], left_knee["y"]),
                (left_ankle["x"], left_ankle["y"]),
            )
        )
    if right_hip and right_knee and right_ankle:
        knee_angles.append(
            angle_degrees(
                (right_hip["x"], right_hip["y"]),
                (right_knee["x"], right_knee["y"]),
                (right_ankle["x"], right_ankle["y"]),
            )
        )
    avg_knee_angle = sum(knee_angles) / len(knee_angles) if knee_angles else 0.0
    if avg_knee_angle > 0:
        notes.append(f"avg_knee_angle={avg_knee_angle:.1f}")

    if torso_angle > 58:
        return "low_or_lying", 0.74, notes + ["Torso is close to horizontal"]
    if torso_angle < 24 and avg_knee_angle > 145:
        return "standing", 0.82, notes + ["Torso vertical and knees extended"]
    if torso_angle < 35 and 65 < avg_knee_angle < 140:
        return "sitting", 0.79, notes + ["Torso vertical with bent knees"]
    return "transition", 0.58, notes + ["Pose is changing or ambiguous"]


def estimate_head_pose(face_landmarks, width, height):
    points_2d = []
    for index in HEAD_POSE_POINTS.values():
        if index >= len(face_landmarks):
            return None
        landmark = face_landmarks[index]
        points_2d.append((landmark.x * width, landmark.y * height))
    points_2d = np.array(points_2d, dtype=np.float64)
    camera_matrix = np.array([[width, 0, width / 2.0], [0, width, height / 2.0], [0, 0, 1]], dtype=np.float64)
    dist_coeffs = np.zeros((4, 1))
    success, rotation_vector, translation_vector = cv2.solvePnP(
        MODEL_3D_POINTS, points_2d, camera_matrix, dist_coeffs, flags=cv2.SOLVEPNP_ITERATIVE
    )
    if not success:
        return None
    rotation_matrix, _ = cv2.Rodrigues(rotation_vector)
    sy = math.sqrt(rotation_matrix[0, 0] ** 2 + rotation_matrix[1, 0] ** 2)
    singular = sy < 1e-6
    if not singular:
        pitch = math.degrees(math.atan2(rotation_matrix[2, 1], rotation_matrix[2, 2]))
        yaw = math.degrees(math.atan2(-rotation_matrix[2, 0], sy))
        roll = math.degrees(math.atan2(rotation_matrix[1, 0], rotation_matrix[0, 0]))
    else:
        pitch = math.degrees(math.atan2(-rotation_matrix[1, 2], rotation_matrix[1, 1]))
        yaw = math.degrees(math.atan2(-rotation_matrix[2, 0], sy))
        roll = 0.0
    return {
        "yaw": float(yaw),
        "pitch": float(pitch),
        "roll": float(roll),
        "translation": translation_vector.reshape(-1).tolist(),
    }

