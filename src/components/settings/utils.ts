import { ChannelStatus } from "@/types/contextTimeline";
import {
  OBSERVER_APP_TOAST_EVENT,
  OBSERVER_CONFIG_UPDATED_EVENT,
  ObserverAppToastDetail,
} from "@/lib/app-config-events";
import { Config } from "@/components/settings/types";

export function formatTimestamp(value: number | null) {
  if (!value) return "No samples yet";
  return new Date(value).toLocaleString();
}

export function formatBytes(bytes: number) {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** index;
  return `${value >= 10 || index === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`;
}

export function formatChannelActivity(
  status: ChannelStatus,
  isCaptureActive: boolean,
  isEnabled: boolean,
) {
  if (!isEnabled) {
    return "This channel is turned off.";
  }

  if (status.permissionState === "missing") {
    return "Permission is missing, so this channel cannot record yet.";
  }

  if (status.permissionState === "system_denied") {
    return "macOS Screen Recording permission is off, so this channel cannot see other apps.";
  }

  if (!isCaptureActive) {
    return "Capture is currently stopped. Start capture from the Timeline page to begin collecting data.";
  }

  if (status.sampleCount === 0) {
    return "Capture is running, but this channel has not produced a recent sample yet.";
  }

  const sampleLabel = status.sampleCount === 1 ? "sample" : "samples";
  return `${status.sampleCount} ${sampleLabel} recorded in the last hour (${status.throughputPerMinute.toFixed(2)}/min).`;
}

export function broadcastConfigUpdate(nextConfig: Config) {
  window.dispatchEvent(new CustomEvent(OBSERVER_CONFIG_UPDATED_EVENT, { detail: nextConfig }));
}

export function showSettingsToast(detail: ObserverAppToastDetail) {
  window.dispatchEvent(new CustomEvent(OBSERVER_APP_TOAST_EVENT, { detail }));
}
