# SOURCE Gaze Model Selection

## Current state

The active path is:

- MediaPipe face mesh + iris tracking
- head-pose estimation
- `MobileOne-S0` ONNX gaze estimation
- calibration-based screen projection

## Why this was chosen

- The machine currently has limited free disk space.
- SOURCE must run beside Chrome, Codex, and other tools.
- Real-time local reliability matters more than benchmark prestige.
- A small ONNX model is a better first fit than a very large
  research model such as `UniGaze-H`.

## What comes next

1. Keep MediaPipe as the face / iris tracking layer.
2. Use the ONNX estimator for live yaw / pitch / vector output.
3. Keep calibration, but upgrade it to a deterministic projection
   fit that maps model output onto the screen.
4. Validate gaze-driven attention blocks live on the timeline.
