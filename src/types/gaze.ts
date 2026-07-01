export interface GazeCalibrationPoint {
  pointId: string;
  phase: string;
  targetX: number;
  targetY: number;
  timestamp: number;
}

export interface GazeCalibration {
  calibrationId: string;
  sessionId: string | null;
  createdAt: number;
  displayId: number | null;
  displayName: string | null;
  displayX: number;
  displayY: number;
  screenWidth: number;
  screenHeight: number;
  cameraId: string;
  modelName: string;
  modelVersion: string;
  calibrationPoints: GazeCalibrationPoint[];
  validationErrorPx: number | null;
  validationQuality: string | null;
  headPoseRange: unknown;
  active: boolean;
}

export interface GazeCameraSource {
  cameraId: string;
  name: string;
  index: number;
}
