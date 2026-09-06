import type { Dispatch, SetStateAction } from "react";
import {
  enableAllAudioContext,
  updateAudioSourceConfig,
} from "@/components/settings/audioConfig";
import { Config } from "@/components/settings/types";

type AudioAnalysisKey =
  | "audio_transcription_enabled"
  | "audio_speech_emotion_enabled"
  | "audio_sound_events_enabled";

export function createAudioSettingsActions(
  config: Config | null,
  setConfig: Dispatch<SetStateAction<Config | null>>,
) {
  return {
    selectAudioInput(value: string) {
      if (!config) return;
      setConfig({
        ...config,
        selected_audio_input_id: value === "__auto__" ? null : value,
      });
    },

    updateAudioMicrophoneEnabled(enabled: boolean) {
      if (!config) return;
      setConfig(updateAudioSourceConfig(config, { audio_microphone_enabled: enabled }));
    },

    updateAudioDesktopEnabled(enabled: boolean) {
      if (!config) return;
      setConfig(updateAudioSourceConfig(config, { audio_desktop_enabled: enabled }));
    },

    updateDesktopAudioGainDb(gainDb: number) {
      if (!config) return;
      setConfig({
        ...config,
        desktop_audio_gain_db: Math.min(24, Math.max(0, gainDb)),
      });
    },

    updateAudioAnalysis(key: AudioAnalysisKey, enabled: boolean) {
      if (!config) return;
      setConfig({ ...config, [key]: enabled });
    },

    enableAllAudio() {
      if (!config) return;
      setConfig(enableAllAudioContext(config));
    },

    addDictionaryEntry(heardAs: string, writeAs: string) {
      if (!config) return;
      const triggers = heardAs
        .split(",")
        .map((part) => part.trim())
        .filter((part) => part.length > 0);
      const replacement = writeAs.trim();
      if (triggers.length === 0 || replacement.length === 0) return;
      setConfig({
        ...config,
        custom_dictionary: [
          ...(config.custom_dictionary ?? []),
          { triggers, replacement },
        ],
      });
    },

    removeDictionaryEntry(index: number) {
      if (!config) return;
      setConfig({
        ...config,
        custom_dictionary: (config.custom_dictionary ?? []).filter(
          (_, entryIndex) => entryIndex !== index,
        ),
      });
    },
  };
}
