import { CaptureChannels, Config } from "@/components/settings/types";

export function normalizeAudioConfig(config: Config): Config {
  const audioSourceEnabled = config.audio_microphone_enabled || config.audio_desktop_enabled;
  const normalized = {
    ...config,
    audio_transcription_enabled: config.audio_transcription_enabled ?? true,
    audio_speech_emotion_enabled: config.audio_speech_emotion_enabled ?? true,
    audio_sound_events_enabled: config.audio_sound_events_enabled ?? true,
    custom_dictionary: config.custom_dictionary ?? [],
  };
  if (!audioSourceEnabled || normalized.capture_channels.audio_future) return normalized;

  return {
    ...normalized,
    capture_channels: {
      ...config.capture_channels,
      audio_future: true,
    },
  };
}

export function updateAudioSourceConfig(
  config: Config,
  updates: Pick<Partial<Config>, "audio_microphone_enabled" | "audio_desktop_enabled">,
): Config {
  return normalizeAudioConfig({ ...config, ...updates });
}

export function enableAllAudioContext(config: Config): Config {
  return normalizeAudioConfig({
    ...config,
    audio_microphone_enabled: true,
    audio_desktop_enabled: true,
    audio_transcription_enabled: true,
    audio_speech_emotion_enabled: true,
    audio_sound_events_enabled: true,
  });
}

export function updateChannelConfig(
  config: Config,
  channel: keyof CaptureChannels,
  enabled: boolean,
): Config {
  const nextConfig = {
    ...config,
    capture_channels: {
      ...config.capture_channels,
      [channel]: enabled,
    },
  };

  if (channel !== "audio_future" || enabled) return nextConfig;
  return {
    ...nextConfig,
    audio_microphone_enabled: false,
    audio_desktop_enabled: false,
  };
}
