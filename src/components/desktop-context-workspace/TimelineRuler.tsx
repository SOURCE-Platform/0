import {
  getLabeledTicks,
  getSnappedTicks,
  getTimelineTickMs,
  safeFormatDate,
} from "@/components/desktop-context-workspace/utils";

interface TimelineRulerProps {
  startTimestamp: number;
  endTimestamp: number;
}

export function TimelineRuler({ startTimestamp, endTimestamp }: TimelineRulerProps) {
  const range = Math.max(1, endTimestamp - startTimestamp);
  const tickMs = getTimelineTickMs(range);
  // Gridlines stay shared with the lane grids below; labels are a
  // filtered subset plus the two pinned viewport edges.
  const gridTicks = getSnappedTicks(startTimestamp, endTimestamp, tickMs);
  const labeledTicks = getLabeledTicks(startTimestamp, endTimestamp, tickMs);

  return (
    <div className="relative h-10">
      {/* Same grid as the lanes below (label column + track) so ticks
          line up exactly with lane subdivisions. */}
      <div className="grid h-full gap-2 md:grid-cols-[8rem_minmax(0,1fr)]">
        <div className="hidden md:block" />
        <div className="relative h-full w-full border border-b-0 border-border/70 bg-background/60">
          {gridTicks.map((timestamp) => {
            const left = ((timestamp - startTimestamp) / range) * 100;
            return (
              <div
                key={timestamp}
                className="absolute bottom-0 top-0 border-l border-white/10"
                style={{ left: `${left}%` }}
              />
            );
          })}
          {labeledTicks.map((timestamp) => {
            const left = ((timestamp - startTimestamp) / range) * 100;
            return (
              <div
                key={`label-${timestamp}`}
                className="absolute top-1 whitespace-nowrap text-[10px] uppercase tabular-nums tracking-wide text-muted-foreground"
                style={{ left: `${left}%`, transform: "translateX(0.5rem)" }}
              >
                {safeFormatDate(timestamp, "p", "--")}
              </div>
            );
          })}
          {/* Pinned viewport edges: fixed position, live text. The start
              mirrors the old behavior; the end is new so the right edge
              always shows a stable, updating time while dragging. */}
          <div
            key="viewport-start"
            className="absolute left-2 top-1 whitespace-nowrap text-[10px] uppercase tabular-nums tracking-wide text-muted-foreground"
          >
            {safeFormatDate(startTimestamp, "p", "--")}
          </div>
          <div
            key="viewport-end"
            className="absolute right-2 top-1 whitespace-nowrap text-[10px] uppercase tabular-nums tracking-wide text-muted-foreground"
          >
            {safeFormatDate(endTimestamp, "p", "--")}
          </div>
        </div>
      </div>
    </div>
  );
}
