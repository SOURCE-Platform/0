export interface Display {
  id: number;
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  is_primary: boolean;
}

export interface CaptureChannels {
  system: boolean;
  focus: boolean;
  visible_windows: boolean;
  ocr: boolean;
  keyboard: boolean;
  mouse: boolean;
  screen_frames: boolean;
  audio_future: boolean;
  camera_future: boolean;
  sensor_future: boolean;
}

export interface DictionaryEntry {
  triggers: string[];
  replacement: string;
}

export interface Config {
  storage_path: string;
  retention_days: Record<string, number>;
  recording_quality: "High" | "Medium" | "Low";
  auto_start: boolean;
  motion_detection_threshold: number;
  ocr_enabled: boolean;
  default_recording_fps: number;
  website_blacklist: string[];
  app_blacklist: string[];
  selected_audio_input_id?: string | null;
  audio_microphone_enabled: boolean;
  audio_desktop_enabled: boolean;
  audio_transcription_enabled: boolean;
  audio_speech_emotion_enabled: boolean;
  audio_sound_events_enabled: boolean;
  desktop_audio_gain_db: number;
  custom_dictionary: DictionaryEntry[];
  capture_channels: CaptureChannels;
  resource_profile: "minimal" | "balanced" | "high_fidelity";
  mobile_enabled: boolean;
  mobile_port: number;
  mobile_clip_retention_days: number;
  pii_settings: {
    detect_only: boolean;
    enabled: boolean;
    enabled_categories: string[];
    review_confidence_threshold: number;
  };
}

export interface CaptureDataChannelUsage {
  channel: string;
  label: string;
  storageKind: string;
  rowCount: number;
  diskBytes: number;
  lastEventTime: number | null;
}

export interface CaptureDataOverview {
  databasePath: string;
  configuredStoragePath: string;
  actualRecordingsPath: string;
  databaseSizeBytes: number;
  recordingsSizeBytes: number;
  totalSizeBytes: number;
  diskTotalBytes: number;
  diskFreeBytes: number;
  diskUsedBytes: number;
  sourcePercentOfDisk: number;
  sourcePercentOfFreeSpace: number;
  diskHealth: string;
  diskWarning: string | null;
  channels: CaptureDataChannelUsage[];
  notes: string[];
}

export interface CapturePreviewRow {
  timestamp: number | null;
  summary: string;
  rawJson: string;
}

export interface CapturePreview {
  channel: string;
  label: string;
  rows: CapturePreviewRow[];
}

export interface AudioInputSource {
  sourceId: string;
  name: string;
  index: number;
  isSystemDefault: boolean;
}

export interface AudioMeterReading {
  sourceId: string;
  sourceName: string;
  level: number;
  status: "active" | "unavailable" | "degraded";
  message: string | null;
  sampledAt: number;
}

export interface AudioSourceMeters {
  microphone: AudioMeterReading;
  desktop: AudioMeterReading;
}

export type SettingsTab = "general" | "capture" | "privacy" | "storage" | "hardware" | "dictation" | "mobile";
