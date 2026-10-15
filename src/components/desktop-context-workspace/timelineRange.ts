import type { ContextTimelineData } from "@/types/contextTimeline";

export interface TimelineRange { start: number; end: number }

/** One nearby overview buffer, capped at 30 minutes on each side. No detail text. */
export function bufferedTimelineRange(view: TimelineRange, bounds: TimelineRange): TimelineRange {
  const padding = Math.min(Math.max(0, view.end - view.start) / 2, 30 * 60_000);
  return {
    start: Math.max(bounds.start, view.start - padding),
    end: Math.min(bounds.end, view.end + padding),
  };
}

export function timelineCoversRange(
  timeline: Pick<ContextTimelineData, "startTimestamp" | "endTimestamp"> | null,
  view: TimelineRange,
): boolean {
  return !!timeline && timeline.startTimestamp <= view.start && timeline.endTimestamp >= view.end;
}
