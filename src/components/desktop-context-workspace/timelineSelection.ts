import type { ContextSlice, ContextSliceDetail } from "@/types/contextTimeline";

/** Overview rows contain timing, not the text or storage loaded for a selection. */
export function mergeOverviewSlice(current: ContextSlice, overview: ContextSlice): ContextSlice {
  if (current.id !== overview.id || current.rail !== overview.rail) return current;
  if (current.startTimestamp === overview.startTimestamp && current.endTimestamp === overview.endTimestamp && current.confidence === overview.confidence) return current;
  return {
    ...current,
    startTimestamp: overview.startTimestamp,
    endTimestamp: overview.endTimestamp,
    confidence: overview.confidence,
  };
}

export function mergeOverviewDetail(current: ContextSliceDetail | null, overview: ContextSlice): ContextSliceDetail | null {
  if (!current) return current;
  const slice = mergeOverviewSlice(current.slice, overview);
  if (slice === current.slice) return current;
  return { ...current, slice, occurredAt: slice.startTimestamp, durationMs: Math.max(1, slice.endTimestamp - slice.startTimestamp) };
}
