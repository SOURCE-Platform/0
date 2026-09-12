import { CaptureChannels, Config } from "@/components/settings/types";

export const OCR_INTERVAL_PRESETS = [15, 30, 60, 120, 300];

export const OCR_LANGUAGES: Array<{ code: string; label: string }> = [
  { code: "eng", label: "English" },
  { code: "spa", label: "Spanish" },
  { code: "fra", label: "French" },
  { code: "deu", label: "German" },
  { code: "ita", label: "Italian" },
  { code: "por", label: "Portuguese" },
  { code: "rus", label: "Russian" },
  { code: "chi_sim", label: "Chinese (Simplified)" },
  { code: "chi_tra", label: "Chinese (Traditional)" },
  { code: "jpn", label: "Japanese" },
  { code: "kor", label: "Korean" },
  { code: "ara", label: "Arabic" },
  { code: "hin", label: "Hindi" },
];

export function clampOcrInterval(seconds: number) {
  if (!Number.isFinite(seconds)) return 60;
  return Math.min(3600, Math.max(1, Math.round(seconds)));
}

/// OCR reads screenshots, so it can only run under Screen keyframes.
/// Repair legacy states: a lone channel flag without the master switch is
/// treated as on (visible intent wins); anything OCR without keyframes is
/// switched off (it could never have produced data).
export function normalizeOcrConfig(config: Config): Config {
  const normalized = {
    ...config,
    ocr_languages:
      config.ocr_languages && config.ocr_languages.length > 0
        ? config.ocr_languages
        : ["eng"],
    ocr_interval_seconds: clampOcrInterval(config.ocr_interval_seconds ?? 60),
  };
  if (normalized.capture_channels.ocr && !normalized.ocr_enabled) {
    normalized.ocr_enabled = true;
  }
  if (!normalized.capture_channels.screen_frames && normalized.capture_channels.ocr) {
    normalized.capture_channels = {
      ...normalized.capture_channels,
      ocr: false,
    };
    normalized.ocr_enabled = false;
  }
  return normalized;
}

export function updateOcrChannelConfig(
  config: Config,
  channel: keyof CaptureChannels,
  enabled: boolean,
): Config {
  if (channel === "ocr") {
    return {
      ...config,
      ocr_enabled: enabled,
      capture_channels: { ...config.capture_channels, ocr: enabled },
    };
  }
  if (channel === "screen_frames" && !enabled) {
    return {
      ...config,
      ocr_enabled: false,
      capture_channels: {
        ...config.capture_channels,
        screen_frames: false,
        ocr: false,
      },
    };
  }
  return config;
}
