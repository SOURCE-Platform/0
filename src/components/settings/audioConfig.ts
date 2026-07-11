import { CaptureChannels, Config } from "@/components/settings/types";

export function normalizeAudioConfig(config: Config): Config {
  const audioSourceEnabled = config.audio_microphone_enabled || config.audio_desktop_enabled;
  if (!audioSourceEnabled || config.capture_channels.audio_future) return config;

  return {
    ...config,
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
