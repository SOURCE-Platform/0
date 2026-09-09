export interface DesktopCaptureStatus {
  isActive: boolean;
  sessionId: string | null;
  startedAt: number | null;
  displayId: number | null;
  displayName: string | null;
  audioSourceName: string | null;
  channelsEnabled: string[];
  warnings: string[];
  missingPermissions: string[];
  resourceProfile: string;
}

export interface ChannelStatus {
  channel: string;
  enabled: boolean;
  health: string;
  permissionState: string;
  lastEventTime: number | null;
  sampleCount: number;
  throughputPerMinute: number;
  lastError: string | null;
  supportsSoloTest: boolean;
  details: string;
}

export interface WindowSnapshot {
  appName: string;
  bundleId: string;
  processId: number;
  isFrontmost: boolean;
  confidence: number;
}

export interface ContextSlice {
  id: string;
  rail: string;
  sliceKind: "event" | "span";
  startTimestamp: number;
  endTimestamp: number;
  title: string;
  subtitle: string | null;
  source: string;
  confidence: number;
  sessionId: string | null;
  appName: string | null;
  windowTitle: string | null;
  interactionState: string | null;
  reasons: string[];
  visibleWindows: WindowSnapshot[];
  ocrPreview: string | null;
  piiCount: number;
  evidenceFramePath: string | null;
  storageBytes: number;
  storageExact: boolean;
  rowCount: number;
  fileCount: number;
  hasDetailView: boolean;
  tags: string[];
}

export interface AsrSegment {
  asrSegmentId: string;
  sessionId: string;
  sourceId: string;
  startTimestamp: number;
  endTimestamp: number;
  language: string | null;
  transcript: string;
  confidence: number | null;
  modelName: string;
  modelVersion: string;
  audioChunkIds: string[];
  isFinal: boolean;
}

export interface TimelineRail {
  id: string;
  kind: "group" | "lane";
  label: string;
  description: string;
  confidenceNote: string;
  defaultExpanded: boolean;
  waveform?: TimelineWaveform | null;
  slices: ContextSlice[];
  children: TimelineRail[];
}

export interface TimelineWaveformSample {
  timestamp: number;
  level: number;
}

export interface TimelineWaveform {
  sourceId: string;
  sourceLabel: string;
  samples: TimelineWaveformSample[];
}

export interface TimelineSummary {
  ocrBlockCount: number;
  evidenceFrameCount: number;
  interaction: {
    activeTypingMs: number;
    activePointerMs: number;
    passiveViewingMs: number;
    voiceInputInferredMs: number;
    mixedMs: number;
  };
  appMetrics: {
    focusedAppCount: number;
    visibleAppCount: number;
    totalFocusTimeMs: number;
    totalVisibleTimeMs: number;
    totalInteractionTimeMs: number;
  };
}

export interface ContextTimelineData {
  startTimestamp: number;
  endTimestamp: number;
  nowTimestamp: number;
  defaultVisibleWindowMs: number;
  summary: TimelineSummary;
  rails: TimelineRail[];
}

export interface RawPayload {
  label: string;
  rawJson: string;
}

export interface OcrReconstructionBlock {
  id: string;
  text: string;
  confidence: number;
  boundingBox: unknown;
  piiEntities: PiiEntity[];
}

export interface OcrReconstruction {
  width: number;
  height: number;
  framePath: string | null;
  backdropAvailable: boolean;
  blocks: OcrReconstructionBlock[];
}

export interface ContextSliceDetail {
  slice: ContextSlice;
  railLabel: string;
  occurredAt: number;
  durationMs: number;
  storageBytes: number;
  storageExact: boolean;
  rowCount: number;
  fileCount: number;
  linkedFilePaths: string[];
  focusedApp: string | null;
  focusedBundleId: string | null;
  visibleWindows: WindowSnapshot[];
  interactionReasons: string[];
  piiEntities: PiiEntity[];
  rawPayloads: RawPayload[];
  ocrReconstruction: OcrReconstruction | null;
  nearbySystemEvents: ContextSlice[];
}

export interface ContextInspector {
  timestamp: number;
  focusedApp: string | null;
  focusedBundleId: string | null;
  visibleWindows: WindowSnapshot[];
  interactionState: string | null;
  interactionReasons: string[];
  recentInputState: string;
  ocrText: string[];
  piiEntities: PiiEntity[];
  evidenceFramePath: string | null;
  nearbySystemEvents: ContextSlice[];
}

export interface AppUsageOverviewItem {
  appName: string;
  bundleId: string;
  focusedTimeMs: number;
  visibleTimeMs: number;
  interactionTimeMs: number;
  ocrHitCount: number;
  recentSegmentCount: number;
}

export interface AppUsageOverview {
  items: AppUsageOverviewItem[];
}

export interface PiiEntity {
  id: string;
  timestamp: number;
  appName: string | null;
  windowTitle: string | null;
  entityType: string;
  redactedPreview: string;
  confidence: number;
  contextText: string;
  boundingBox: unknown;
  framePath: string | null;
}

export interface OcrReviewItem {
  id: string;
  timestamp: number;
  appName: string | null;
  windowTitle: string | null;
  text: string;
  confidence: number;
  framePath: string | null;
  piiEntities: PiiEntity[];
}
