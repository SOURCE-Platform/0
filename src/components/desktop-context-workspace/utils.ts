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

export function railTone(railId: string, interactionState?: string | null) {
  if (railId === "system") return "from-slate-500/85 to-slate-400/80";
  if (railId === "focus") return "from-blue-500/85 to-sky-400/80";
  if (railId === "visible_windows") return "from-cyan-500/80 to-cyan-300/70";
  if (railId === "ocr") return "from-amber-500/85 to-orange-400/80";
  if (railId === "attention") return "from-indigo-500/85 to-fuchsia-400/80";
  if (railId === "vision") return "from-rose-500/85 to-pink-400/80";
  if (railId === "audio" || railId === "audio_speech") return "from-emerald-500/85 to-lime-400/75";
  if (railId === "audio_sound_events" || railId === "sound_events") return "from-teal-500/85 to-emerald-300/75";
  if (railId === "audio_emotion_summary" || railId === "audio_emotion_summary_lane") {
    return "from-violet-500/85 to-pink-400/75";
  }
  if (railId === "audio_emotion_happy") return "from-yellow-400/90 to-amber-300/80";
  if (railId === "audio_emotion_sad") return "from-blue-600/85 to-blue-400/80";
  if (railId === "audio_emotion_angry") return "from-red-500/90 to-orange-400/80";
  if (railId === "audio_emotion_fearful") return "from-purple-600/85 to-fuchsia-400/75";
  if (railId === "audio_emotion_surprised") return "from-cyan-400/90 to-sky-300/80";
  if (railId === "audio_emotion_neutral") return "from-slate-400/80 to-slate-300/70";
  if (railId === "audio_emotion_uncertain") return "from-zinc-500/70 to-zinc-300/55";
  if (railId === "evidence") return "from-violet-500/85 to-fuchsia-400/75";
  if (interactionState === "active_typing") return "from-emerald-500/85 to-emerald-300/75";
  if (interactionState === "active_pointer") return "from-sky-500/85 to-blue-300/75";
  if (interactionState === "voice_input_inferred") return "from-orange-500/85 to-orange-300/80";
  if (interactionState === "mixed") return "from-fuchsia-500/85 to-pink-300/80";
  return "from-zinc-500/85 to-zinc-300/75";
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
