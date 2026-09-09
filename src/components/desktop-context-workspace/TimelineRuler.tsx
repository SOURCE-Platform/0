import { getSnappedTicks, getTimelineTickMs, safeFormatDate } from "@/components/desktop-context-workspace/utils";

interface TimelineRulerProps {
  startTimestamp: number;
  endTimestamp: number;
}

export function TimelineRuler({ startTimestamp, endTimestamp }: TimelineRulerProps) {
  const range = Math.max(1, endTimestamp - startTimestamp);
  const tickMs = getTimelineTickMs(range);
  const ticks = getSnappedTicks(startTimestamp, endTimestamp, tickMs);

  return (
    <div className="relative h-10">
      {/* Same grid as the lanes below (label column + track) so ticks
          line up exactly with lane subdivisions. */}
      <div className="grid h-full gap-2 md:grid-cols-[8rem_minmax(0,1fr)]">
        <div className="hidden md:block" />
        <div className="relative h-full w-full bg-background/60">
          {ticks.map((timestamp) => {
            const left = ((timestamp - startTimestamp) / range) * 100;
            const isLastTick = timestamp + tickMs > endTimestamp;
            return (
              <div
                key={timestamp}
                className="absolute bottom-0 top-0 border-l border-white/10"
                style={{ left: `${left}%` }}
              >
                <div
                  className="absolute top-1 text-[10px] uppercase tracking-wide text-muted-foreground"
                  style={isLastTick ? { right: "0.35rem" } : { left: "0.5rem" }}
                >
                  {safeFormatDate(timestamp, "p", "--")}
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
