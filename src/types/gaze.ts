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
