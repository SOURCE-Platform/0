import { format } from "date-fns";
import { ChannelStatus, ContextSlice, TimelineRail } from "@/types/contextTimeline";

export const MIN_WINDOW_MS = 2 * 60 * 1000;

export function safeFormatDate(
  value: number | Date | null | undefined,
  pattern: string,
  fallback = "Time unavailable",
) {
  if (value == null) return fallback;

  try {
    return format(value, pattern);
  } catch {
    return fallback;
  }
}

export function formatDuration(ms: number) {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const remainder = seconds % 60;

  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m ${remainder}s`;
  return `${remainder}s`;
}

export function formatBytes(bytes: number) {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let index = 0;
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024;
    index += 1;
  }
  return `${value >= 10 || index === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`;
}

export function formatWindowScale(ms: number) {
  const minutes = Math.round(ms / 60_000);
  if (minutes < 60) return `${minutes}m`;
  if (minutes % 60 === 0) return `${minutes / 60}h`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

export function getTimelineTickMs(range: number) {
  if (range <= 15 * 60 * 1000) return 60 * 1000;
  if (range <= 60 * 60 * 1000) return 5 * 60 * 1000;
  return 30 * 60 * 1000;
}

/// Tick timestamps snapped to round clock boundaries (5:10, 5:15, …),
/// never anchored to the arbitrary window start. Both the ruler and the
/// lane grids must use this — anything else drifts them apart.
export function getSnappedTicks(windowStart: number, windowEnd: number, tickMs: number): number[] {
  const first = Math.ceil(windowStart / tickMs) * tickMs;
  const ticks: number[] = [];
  for (let timestamp = first; timestamp <= windowEnd; timestamp += tickMs) {
    ticks.push(timestamp);
  }
  // Always include the window start so the left edge has a mark.
  if (ticks.length === 0 || ticks[0] !== windowStart) {
    ticks.unshift(windowStart);
  }
  return ticks;
}

/// Fraction of the visible window kept clear of round-tick labels at
/// each edge. The ruler pins the live window start/end at the edges;
/// a snapped tick sliding within this band loses its label (its
/// gridline stays) so labels never stack on each other while dragging.
export const RULER_EDGE_PAD_FRACTION = 0.045;

export function getLabeledTicks(windowStart: number, windowEnd: number, tickMs: number): number[] {
  const range = Math.max(1, windowEnd - windowStart);
  const pad = range * RULER_EDGE_PAD_FRACTION;
  return getSnappedTicks(windowStart, windowEnd, tickMs).filter(
    (timestamp) =>
      timestamp !== windowStart &&
      timestamp !== windowEnd &&
      timestamp - windowStart >= pad &&
      windowEnd - timestamp >= pad,
  );
}

export function railTone(railId: string, interactionState?: string | null) {
  if (railId === "system") return "bg-slate-500/85";
  if (railId === "focus") return "bg-blue-500/85";
  if (railId === "visible_windows") return "bg-cyan-500/80";
  if (railId === "ocr") return "bg-amber-500/85";
  if (railId === "attention") return "bg-indigo-500/85";
  if (railId === "vision") return "bg-rose-500/85";
  if (railId === "audio_dictation") return "bg-violet-500/85";
  if (railId === "audio_mobile") return "bg-sky-500/85";
  if (railId === "audio" || railId === "audio_speech" || railId === "audio_ambient_speech") {
    return "bg-emerald-500/85";
  }
  if (railId === "audio_sound_events" || railId === "sound_events") return "bg-teal-500/85";
  if (railId === "audio_emotion_summary" || railId === "audio_emotion_summary_lane") {
    return "bg-violet-500/85";
  }
  if (railId === "audio_emotion_happy") return "bg-yellow-400/90";
  if (railId === "audio_emotion_sad") return "bg-blue-600/85";
  if (railId === "audio_emotion_angry") return "bg-red-500/90";
  if (railId === "audio_emotion_fearful") return "bg-purple-600/85";
  if (railId === "audio_emotion_surprised") return "bg-cyan-400/90";
  if (railId === "audio_emotion_neutral") return "bg-slate-400/80";
  if (railId === "audio_emotion_uncertain") return "bg-zinc-500/70";
  if (railId === "evidence") return "bg-violet-500/85";
  if (interactionState === "active_typing") return "bg-emerald-500/85";
  if (interactionState === "active_pointer") return "bg-sky-500/85";
  if (interactionState === "voice_input_inferred") return "bg-orange-500/85";
  if (interactionState === "mixed") return "bg-fuchsia-500/85";
  return "bg-zinc-500/85";
}

export function getDescendantSliceCount(rail: TimelineRail): number {
  return rail.slices.length + rail.children.reduce((total, child) => total + getDescendantSliceCount(child), 0);
}

export function sliceIsVisible(
  slice: ContextSlice,
  windowStart: number,
  windowEnd: number,
  appFilter: string,
  interactionFilter: string,
  railId: string,
) {
  if (appFilter !== "all" && slice.appName !== appFilter) return false;
  if (interactionFilter !== "all" && railId === "interaction" && slice.interactionState !== interactionFilter) return false;
  return slice.endTimestamp >= windowStart && slice.startTimestamp <= windowEnd;
}

export function getVisibleDescendantSliceCount(
  rail: TimelineRail,
  windowStart: number,
  windowEnd: number,
  appFilter: string,
  interactionFilter: string,
): number {
  const ownCount = rail.slices.filter((slice) =>
    sliceIsVisible(slice, windowStart, windowEnd, appFilter, interactionFilter, rail.id),
  ).length;
  return ownCount + rail.children.reduce(
    (total, child) => total + getVisibleDescendantSliceCount(child, windowStart, windowEnd, appFilter, interactionFilter),
    0,
  );
}

export function walkRails(rails: TimelineRail[], visit: (rail: TimelineRail) => void) {
  rails.forEach((rail) => {
    visit(rail);
    if (rail.children.length > 0) walkRails(rail.children, visit);
  });
}

export function parseBoundingBox(raw: unknown) {
  if (!raw || typeof raw !== "object") return null;
  const candidate = raw as Record<string, unknown>;
  const x = Number(candidate.x);
  const y = Number(candidate.y);
  const width = Number(candidate.width);
  const height = Number(candidate.height);
  if ([x, y, width, height].some((value) => Number.isNaN(value))) return null;
  return { x, y, width, height };
}

export function healthTone(health: ChannelStatus["health"]) {
  if (health === "healthy") return "default";
  if (health === "off") return "secondary";
  if (health === "warming_up") return "outline";
  return "destructive";
}

/// Raw timeline `source` values are storage IDs (e.g. `microphone:0`,
/// `fluid-voice-prompt`). Display a human label instead so Block Detail
/// doesn't read like a device index.
export function formatAudioSource(source: string | null | undefined) {
  if (!source) return "Unknown";
  if (source.startsWith("microphone:")) return "Microphone";
  if (source.startsWith("microphone-name:")) {
    const name = source.slice("microphone-name:".length).trim();
    return name || "Microphone";
  }
  if (source === "fluid-voice-prompt") return "Right Option dictation";
  if (source === "source-mobile") return "Source Mobile";
  if (source === "desktop_output:system" || source === "desktop-output:system") {
    return "Desktop audio";
  }
  if (source === "ambient_audio") return "Microphone";
  return source;
}
