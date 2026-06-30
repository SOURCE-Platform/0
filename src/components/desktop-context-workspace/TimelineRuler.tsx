import { getTimelineTickMs, safeFormatDate } from "@/components/desktop-context-workspace/utils";

interface TimelineRulerProps {
  startTimestamp: number;
  endTimestamp: number;
}

export function TimelineRuler({ startTimestamp, endTimestamp }: TimelineRulerProps) {
  const range = Math.max(1, endTimestamp - startTimestamp);
  const tickMs = getTimelineTickMs(range);
  const tickCount = Math.max(1, Math.ceil(range / tickMs));

  return (
    <div className="relative h-10 border-b border-border/70 bg-background/60">
      <div className="relative h-full w-full">
        {Array.from({ length: tickCount + 1 }).map((_, index) => {
          const timestamp = startTimestamp + index * tickMs;
          if (timestamp > endTimestamp) return null;
          const left = ((timestamp - startTimestamp) / range) * 100;
          const isLastTick = index === tickCount || timestamp + tickMs > endTimestamp;
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
  );
}
