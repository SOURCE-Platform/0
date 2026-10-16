import type { ContextSlice } from "@/types/contextTimeline";

type TimedSlice = Pick<ContextSlice, "startTimestamp" | "endTimestamp">;

/** Time-scaled bounds, with a one-pixel marker only where neighboring time allows. */
export function layoutTimelineSlices<T extends TimedSlice>(
  slices: readonly T[],
  windowStart: number,
  windowEnd: number,
  trackWidth: number,
) {
  const range = Math.max(1, windowEnd - windowStart);
  const markerWidth = trackWidth > 0 ? 100 / trackWidth : 0;
  const ordered = [...slices].sort((a, b) => a.startTimestamp - b.startTimestamp);
  const nextStarts = new Map<number, number>();
  for (let index = 1; index < ordered.length; index++) {
    const previous = ordered[index - 1].startTimestamp;
    const current = ordered[index].startTimestamp;
    if (current > previous) nextStarts.set(previous, current);
  }

  return ordered.flatMap((slice) => {
    const end = Math.max(slice.startTimestamp, slice.endTimestamp);
    if (slice.startTimestamp >= windowEnd || end < windowStart) return [];
    if (end === windowStart && slice.startTimestamp < windowStart) return [];

    const start = Math.max(slice.startTimestamp, windowStart);
    const left = ((start - windowStart) / range) * 100;
    const rawWidth = ((Math.min(end, windowEnd) - start) / range) * 100;
    const nextStart = nextStarts.get(slice.startTimestamp);
    let limit = 100 - left;
    if (nextStart !== undefined) {
      // Never invent a collision. Preserve real overlaps and leave at least
      // half of a positive gap clear when widening a subpixel marker.
      const gap = nextStart - end;
      limit = gap < 0
        ? rawWidth
        : Math.min(limit, ((nextStart - start - gap / 2) / range) * 100);
    }
    const width = Math.min(Math.max(rawWidth, markerWidth), limit);
    return width > 0 ? [{ slice, left, width }] : [];
  });
}
