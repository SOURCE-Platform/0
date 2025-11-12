// Playback data structures

export interface PlaybackInfo {
  sessionId: string;
  startTimestamp: number;
  endTimestamp: number;
  baseLayerPath: string;
  segments: VideoSegmentInfo[];
  totalDurationMs: number;
  frameCount: number;
}

export interface VideoSegmentInfo {
  path: string;
  startTimestamp: number;
  endTimestamp: number;
  durationMs: number;
}

export interface SeekInfo {
  videoPath: string;
  offsetMs: number;
  segmentStart: number;
}
