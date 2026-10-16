import type { ContextSlice, TimelineRail } from "@/types/contextTimeline";

export const TIMELINE_GRID_CLASS_NAME = "grid gap-0 md:grid-cols-[8rem_minmax(0,1fr)]";
export const TRACK_HEIGHT_PX = 24;

interface SliceLayout {
  topPx: number;
  heightPx: number;
  opacity: number;
  showInlineTitle: boolean;
}

/** All vertical geometry fits the track's 22px inner area. */
export function getSliceLayout(rail: TimelineRail, slice: ContextSlice): SliceLayout {
  const confidence = Math.max(0.2, Math.min(1, slice.confidence));
  const base = { opacity: Math.max(0.55, confidence), showInlineTitle: false };
  if (slice.tags.includes("activity_bucket")) {
    const heightPx = Math.min(16, 4 + Math.log2(Math.max(1, slice.rowCount)) * 1.75);
    return { ...base, topPx: 19 - heightPx, heightPx, opacity: 0.85 };
  }
  if (rail.id === "audio_speech" && slice.tags.includes("asr")) {
    return { ...base, topPx: 4, heightPx: 14, opacity: Math.max(0.65, confidence), showInlineTitle: true };
  }
  if (rail.id === "audio_emotion_summary_lane") {
    const heightPx = Math.max(3, confidence * 7);
    if (slice.tags.includes("positive")) {
      return { ...base, topPx: 11 - heightPx, heightPx, opacity: Math.max(0.6, confidence) };
    }
    if (slice.tags.includes("negative")) {
      return { ...base, topPx: 11, heightPx, opacity: Math.max(0.6, confidence) };
    }
    if (slice.tags.includes("uncertain")) {
      return { ...base, topPx: 9, heightPx: 4, opacity: 0.45 };
    }
    const neutralHeight = Math.max(3, confidence * 5);
    return { ...base, topPx: 11 - neutralHeight / 2, heightPx: neutralHeight };
  }
  if (rail.id.startsWith("audio_emotion_")) {
    const heightPx = Math.max(3, confidence * 14);
    return { ...base, topPx: 19 - heightPx, heightPx };
  }
  return { ...base, topPx: 3, heightPx: 16, showInlineTitle: true };
}
