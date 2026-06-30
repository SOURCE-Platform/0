import { CircleHelp } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { ContextSlice, TimelineRail } from "@/types/contextTimeline";
import {
  formatBytes,
  getTimelineTickMs,
  railTone,
  safeFormatDate,
} from "@/components/desktop-context-workspace/utils";

interface RailLaneProps {
  rail: TimelineRail;
  windowStart: number;
  windowEnd: number;
  selectedSliceId: string | null;
  onSelect: (slice: ContextSlice) => void;
  appFilter: string;
  interactionFilter: string;
}

export function RailLane({
  rail,
  windowStart,
  windowEnd,
  selectedSliceId,
  onSelect,
  appFilter,
  interactionFilter,
}: RailLaneProps) {
  const range = Math.max(1, windowEnd - windowStart);
  const tickMs = getTimelineTickMs(range);
  const tickCount = Math.max(1, Math.ceil(range / tickMs));
  const slices = rail.slices.filter((slice) => {
    if (appFilter !== "all" && slice.appName !== appFilter) return false;
    if (interactionFilter !== "all" && rail.id === "interaction" && slice.interactionState !== interactionFilter) return false;
    if (slice.endTimestamp < windowStart || slice.startTimestamp > windowEnd) return false;
    return true;
  });

  return (
    <div className="grid gap-2 md:grid-cols-[10.5rem_minmax(0,1fr)]">
      <div className="sticky left-0 z-10 flex items-center gap-2 bg-muted/10 py-1 backdrop-blur-sm">
        <div className="min-w-0">
          <span className="text-sm font-semibold text-foreground">{rail.label}</span>
        </div>
        <Badge variant="outline" className="text-[10px] uppercase tracking-wide">
          {rail.slices.length}
        </Badge>
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              className="inline-flex h-6 w-6 items-center justify-center rounded-full text-muted-foreground transition hover:text-foreground"
            >
              <CircleHelp className="h-4 w-4" />
            </button>
          </TooltipTrigger>
          <TooltipContent side="right" className="max-w-[32ch] text-left leading-6">
            <div className="space-y-2">
              <p>{rail.description}</p>
              <p>{rail.confidenceNote}</p>
            </div>
          </TooltipContent>
        </Tooltip>
      </div>

      <div className="relative h-16 rounded-2xl border border-border/70 bg-background/65">
        <div className="pointer-events-none absolute inset-y-0 right-0 z-20 w-px bg-blue-400/90 shadow-[0_0_18px_rgba(59,130,246,0.45)]" />
        <div className="relative h-full w-full">
          {Array.from({ length: tickCount + 1 }).map((_, index) => {
            const timestamp = windowStart + index * tickMs;
            if (timestamp > windowEnd) return null;
            const left = ((timestamp - windowStart) / range) * 100;
            return (
              <div
                key={`${rail.id}-${timestamp}`}
                className="pointer-events-none absolute inset-y-0 z-0 border-l border-white/10"
                style={{ left: `${left}%` }}
              />
            );
          })}

          {slices.length === 0 ? (
            <div className="flex h-full items-center justify-center text-xs text-muted-foreground">
              No slices for the current filters.
            </div>
          ) : (
            slices.map((slice) => {
              const clippedStart = Math.max(slice.startTimestamp, windowStart);
              const clippedEnd = Math.min(Math.max(slice.endTimestamp, slice.startTimestamp + 1), windowEnd);
              const left = ((clippedStart - windowStart) / range) * 100;
              const rawWidth = ((Math.max(clippedEnd, clippedStart + 1) - clippedStart) / range) * 100;
              const width = slice.sliceKind === "event" ? Math.max(1.25, rawWidth) : Math.max(2.5, rawWidth);
              const anchor = Math.min(92, Math.max(8, left + width / 2));
              const storageLabel = `${formatBytes(slice.storageBytes)} ${slice.storageExact ? "Exact" : "Estimated"}`;

              return (
                <div key={slice.id}>
                  <button
                    type="button"
                    onClick={() => onSelect(slice)}
                    className={`group absolute top-2 h-12 overflow-hidden rounded-xl border bg-gradient-to-r text-left shadow-sm transition ${
                      selectedSliceId === slice.id
                        ? "border-white/70 ring-1 ring-white/30"
                        : "border-white/10 hover:border-white/35"
                    } ${railTone(rail.id, slice.interactionState)}`}
                    style={{ left: `${left}%`, width: `${width}%`, opacity: Math.max(0.55, slice.confidence) }}
                    title={`${slice.title} • ${safeFormatDate(slice.startTimestamp, "p")} • ${storageLabel}`}
                  >
                    <div className="px-2 py-1.5 text-[11px] font-semibold text-white">
                      {width > 12 ? <div className="truncate">{slice.title}</div> : null}
                      {width > 18 && slice.subtitle ? (
                        <div className="truncate pt-0.5 text-[10px] text-white/85">{slice.subtitle}</div>
                      ) : null}
                    </div>
                  </button>

                  {selectedSliceId === slice.id ? (
                    <div
                      className="pointer-events-none absolute bottom-[calc(100%+0.4rem)] z-30 w-max max-w-[16rem] -translate-x-1/2 rounded-xl border border-white/15 bg-black/85 px-3 py-2 text-left shadow-2xl backdrop-blur"
                      style={{ left: `${anchor}%` }}
                    >
                      <div className="truncate text-[11px] font-semibold text-white">{slice.title}</div>
                      <div className="pt-0.5 text-[10px] uppercase tracking-wide text-white/75">
                        {safeFormatDate(slice.startTimestamp, "p")} • {storageLabel}
                      </div>
                      {slice.subtitle ? (
                        <div className="pt-1 text-[10px] leading-4 text-white/85">{slice.subtitle}</div>
                      ) : null}
                    </div>
                  ) : null}
                </div>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
}
