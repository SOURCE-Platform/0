#!/usr/bin/env python3
"""
MediaPipe Pose, Face, and Hand Inference
Bridge for Rust PyO3 integration
"""

import numpy as np
import mediapipe as mp
from typing import Dict, List, Optional, Tuple
import json


class MediaPipeInference:
    """Unified MediaPipe inference for pose, face, and hands"""

    def __init__(
        self,
        enable_pose: bool = True,
        enable_face: bool = True,
        enable_hands: bool = True,
        model_complexity: int = 2,  # 0=lite, 1=full, 2=heavy
        min_detection_confidence: float = 0.5,
        min_tracking_confidence: float = 0.5,
    ):
        """
        Initialize MediaPipe models

        Args:
            enable_pose: Enable body pose tracking (33 landmarks)
            enable_face: Enable face mesh tracking (468 landmarks + 52 blendshapes)
            enable_hands: Enable hand tracking (21 landmarks per hand)
            model_complexity: Model complexity (0, 1, or 2)
            min_detection_confidence: Minimum confidence for detection
            min_tracking_confidence: Minimum confidence for tracking
        """
        self.enable_pose = enable_pose
        self.enable_face = enable_face
        self.enable_hands = enable_hands

        # Initialize MediaPipe solutions
        if enable_pose:
            self.mp_pose = mp.solutions.pose
            self.pose = self.mp_pose.Pose(
                model_complexity=model_complexity,
                min_detection_confidence=min_detection_confidence,
                min_tracking_confidence=min_tracking_confidence,
            )

        if enable_face:
            self.mp_face_mesh = mp.solutions.face_mesh
            self.face_mesh = self.mp_face_mesh.FaceMesh(
                max_num_faces=1,
                refine_landmarks=True,  # Enables blendshapes
                min_detection_confidence=min_detection_confidence,
                min_tracking_confidence=min_tracking_confidence,
            )

        if enable_hands:
            self.mp_hands = mp.solutions.hands
            self.hands = self.mp_hands.Hands(
                model_complexity=model_complexity,
                max_num_hands=2,
                min_detection_confidence=min_detection_confidence,
                min_tracking_confidence=min_tracking_confidence,
            )

    def process_frame(self, image_rgb: np.ndarray) -> Dict:
        """
        Process a single frame and extract all landmarks

        Args:
            image_rgb: RGB image as numpy array (H, W, 3)

        Returns:
            Dictionary with pose, face, and hand results
        """
        results = {
            "body_pose": None,
            "face_mesh": None,
            "hands": [],
        }

        # Process pose
        if self.enable_pose:
            pose_results = self.pose.process(image_rgb)
            if pose_results.pose_landmarks:
                results["body_pose"] = self._extract_pose_landmarks(pose_results)

        # Process face
        if self.enable_face:
            face_results = self.face_mesh.process(image_rgb)
            if face_results.multi_face_landmarks:
                results["face_mesh"] = self._extract_face_landmarks(face_results)

        # Process hands
        if self.enable_hands:
            hand_results = self.hands.process(image_rgb)
            if hand_results.multi_hand_landmarks:
                results["hands"] = self._extract_hand_landmarks(hand_results)

        return results

    def _extract_pose_landmarks(self, pose_results) -> Dict:
        """Extract 33 body pose landmarks"""
        landmarks = pose_results.pose_landmarks.landmark

        keypoints = []
        for lm in landmarks:
            keypoints.append({
                "x": lm.x,
                "y": lm.y,
                "z": lm.z,
                "visibility": lm.visibility,
            })

        return {
            "keypoints": keypoints,
            "count": len(keypoints),
        }

    def _extract_face_landmarks(self, face_results) -> Dict:
        """Extract 468 face mesh landmarks + 52 blendshapes"""
        # Get first face (we only track one face)
        face_landmarks = face_results.multi_face_landmarks[0]

        # Extract 468 landmarks
        keypoints = []
        for lm in face_landmarks.landmark:
            keypoints.append({
                "x": lm.x,
                "y": lm.y,
                "z": lm.z,
            })

        # Extract blendshapes (ARKit-compatible coefficients)
        blendshapes = {}
        if hasattr(face_results, 'face_blendshapes') and face_results.face_blendshapes:
            for idx, score in enumerate(face_results.face_blendshapes[0].classification):
                blendshapes[score.label] = score.score
        else:
            # Fallback: estimate basic blendshapes from landmarks
            blendshapes = self._estimate_blendshapes_from_landmarks(keypoints)

        return {
            "landmarks": keypoints,
            "blendshapes": blendshapes,
            "landmark_count": len(keypoints),
        }

    def _estimate_blendshapes_from_landmarks(self, landmarks: List[Dict]) -> Dict:
        """
        Estimate basic blendshapes from landmark positions
        This is a simplified approach for when blendshapes are not available
        """
        blendshapes = {}

        # Eye blink (distance between upper and lower eyelids)
        # Left eye: landmarks 159 (upper) and 145 (lower)
        # Right eye: landmarks 386 (upper) and 374 (lower)
        if len(landmarks) > 386:
            left_eye_open = abs(landmarks[159]["y"] - landmarks[145]["y"])
            right_eye_open = abs(landmarks[386]["y"] - landmarks[374]["y"])

            # Normalize to 0-1 range (assume max eye opening is ~0.03)
            blendshapes["eyeBlinkLeft"] = max(0, 1 - (left_eye_open / 0.03))
            blendshapes["eyeBlinkRight"] = max(0, 1 - (right_eye_open / 0.03))

        # Jaw open (distance between upper and lower lip)
        # Upper lip: landmark 13, Lower lip: landmark 14
        if len(landmarks) > 14:
            jaw_open = abs(landmarks[13]["y"] - landmarks[14]["y"])
            blendshapes["jawOpen"] = min(1.0, jaw_open / 0.1)

        # Mouth smile (corner lip positions)
        # Left corner: 61, Right corner: 291
        if len(landmarks) > 291:
            left_smile = landmarks[61]["y"]
            right_smile = landmarks[291]["y"]
            avg_smile = (left_smile + right_smile) / 2
            blendshapes["mouthSmileLeft"] = max(0, min(1, -avg_smile * 10))
            blendshapes["mouthSmileRight"] = blendshapes["mouthSmileLeft"]

        return blendshapes

    def _extract_hand_landmarks(self, hand_results) -> List[Dict]:
        """Extract 21 landmarks per hand"""
        hands = []

        for idx, hand_landmarks in enumerate(hand_results.multi_hand_landmarks):
            handedness = hand_results.multi_handedness[idx].classification[0]

            keypoints = []
            for lm in hand_landmarks.landmark:
                keypoints.append({
                    "x": lm.x,
                    "y": lm.y,
                    "z": lm.z,
                })

            hands.append({
                "hand_type": handedness.label,  # "Left" or "Right"
                "confidence": handedness.score,
                "keypoints": keypoints,
            })

        return hands

    def close(self):
        """Clean up resources"""
        if self.enable_pose:
            self.pose.close()
        if self.enable_face:
            self.face_mesh.close()
        if self.enable_hands:
            self.hands.close()


def process_image_bytes(
    image_bytes: bytes,
    width: int,
    height: int,
    enable_pose: bool = True,
    enable_face: bool = True,
    enable_hands: bool = True,
    model_complexity: int = 2,
) -> str:
    """
    Process image bytes and return JSON results
    This is the main entry point for PyO3 bridge

    Args:
        image_bytes: Raw RGB image bytes
        width: Image width
        height: Image height
        enable_pose: Enable pose tracking
        enable_face: Enable face tracking
        enable_hands: Enable hand tracking
        model_complexity: Model complexity (0-2)

    Returns:
        JSON string with results
    """
    # Convert bytes to numpy array
    image_array = np.frombuffer(image_bytes, dtype=np.uint8)
    image_rgb = image_array.reshape((height, width, 3))

    # Create inference instance
    inference = MediaPipeInference(
        enable_pose=enable_pose,
        enable_face=enable_face,
        enable_hands=enable_hands,
        model_complexity=model_complexity,
    )

    # Process frame
    results = inference.process_frame(image_rgb)

    # Clean up
    inference.close()

    # Return JSON
    return json.dumps(results)


if __name__ == "__main__":
    # Test with a dummy image
    import cv2

    # Create a test image (solid color)
    test_image = np.zeros((480, 640, 3), dtype=np.uint8)
    test_image[:, :] = (255, 200, 150)  # Light skin tone

    inference = MediaPipeInference()
    results = inference.process_frame(test_image)

    print("MediaPipe Inference Test Results:")
    print(f"Body Pose: {results['body_pose'] is not None}")
    print(f"Face Mesh: {results['face_mesh'] is not None}")
    print(f"Hands: {len(results['hands'])} hands detected")

    if results['body_pose']:
        print(f"  - Body keypoints: {results['body_pose']['count']}")

    if results['face_mesh']:
        print(f"  - Face landmarks: {results['face_mesh']['landmark_count']}")
        print(f"  - Blendshapes: {len(results['face_mesh']['blendshapes'])}")

    inference.close()
