import type { Dispatch, SetStateAction } from "react";
import { clampOcrInterval } from "@/components/settings/ocrConfig";
import { Config } from "@/components/settings/types";

export function createOcrSettingsActions(
  config: Config | null,
  setConfig: Dispatch<SetStateAction<Config | null>>,
) {
  return {
    updateOcrEnabled(enabled: boolean) {
      if (!config) return;
      setConfig({
        ...config,
        ocr_enabled: enabled,
        capture_channels: { ...config.capture_channels, ocr: enabled },
      });
    },

    updateOcrInterval(seconds: number) {
      if (!config) return;
      setConfig({ ...config, ocr_interval_seconds: clampOcrInterval(seconds) });
    },

    toggleOcrLanguage(code: string) {
      if (!config) return;
      const current = config.ocr_languages ?? ["eng"];
      const next = current.includes(code)
        ? current.filter((language) => language !== code)
        : [...current, code];
      if (next.length === 0) return;
      setConfig({ ...config, ocr_languages: next });
    },
  };
}
