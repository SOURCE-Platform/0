import { useState } from "react";
import { mockActivities, ActivitySegment } from "@/data/mockTimeline";
import { Breadcrumb } from "@/components/vertical-timeline/Breadcrumb";
import {
  CHUNK_MODES,
  ChunkMode,
  LEGEND_DOT,
  TYPE_DISPLAY,
} from "@/components/vertical-timeline/constants";
import { DetailView } from "@/components/vertical-timeline/DetailView";
import { hasChildren } from "@/components/vertical-timeline/helpers";
import { RecordingView } from "@/components/vertical-timeline/RecordingView";
import { WeekView } from "@/components/vertical-timeline/WeekView";
import { cn } from "@/lib/utils";

export default function VerticalTimeline() {
  const [mode, setMode] = useState<ChunkMode>("calendar");
  const [drillStack, setDrillStack] = useState<ActivitySegment[]>([]);

  function drillDown(segment: ActivitySegment) {
    setDrillStack((previous) => [...previous, segment]);
  }

  function navigateTo(level: number) {
    setDrillStack((previous) => previous.slice(0, level));
  }

  const inDrill = drillStack.length > 0;
  const current = drillStack[drillStack.length - 1];
  const isLeaf = inDrill && !hasChildren(current);

  return (
    <div className="w-full">
      <div className="sticky top-0 z-20 flex items-center justify-between gap-4 bg-background py-3">
        {!inDrill ? (
          <div className="flex items-center gap-1">
            {CHUNK_MODES.map(({ value, label }) => (
              <button
                key={value}
                onClick={() => setMode(value)}
                className={cn(
                  "cursor-pointer rounded-full px-3 py-1 text-sm transition-colors",
                  mode === value
                    ? "bg-foreground font-medium text-background"
                    : "text-muted-foreground hover:bg-muted hover:text-foreground",
                )}
              >
                {label}
              </button>
            ))}
          </div>
        ) : (
          <Breadcrumb stack={drillStack} onNavigate={navigateTo} />
        )}

        <div className="flex shrink-0 items-center gap-4 text-xs text-muted-foreground">
          {Object.keys(TYPE_DISPLAY).map((type) => (
            <div key={type} className="flex items-center gap-1.5">
              <div className={cn("h-2.5 w-2.5 rounded-sm", LEGEND_DOT[type as keyof typeof LEGEND_DOT])} />
              <span>{TYPE_DISPLAY[type as keyof typeof TYPE_DISPLAY]}</span>
            </div>
          ))}
        </div>
      </div>

      {!inDrill && mockActivities.length === 0 ? (
        <div className="mx-auto w-full max-w-2xl rounded-lg border bg-card p-8 text-center">
          <h2 className="text-xl font-medium tracking-tight">No activity yet</h2>
          <p className="mx-auto mt-2 max-w-[52ch] text-sm leading-6 text-muted-foreground">
            Recorded days will appear here once capture is running. Nothing
            demo or placeholder lives in this view.
          </p>
        </div>
      ) : null}

      {!inDrill && mockActivities.length > 0 ? <WeekView mode={mode} onDrillDown={drillDown} activities={mockActivities} /> : null}
      {inDrill && isLeaf ? <RecordingView segment={current} /> : null}
      {inDrill && !isLeaf ? <DetailView segment={current} onDrillDown={drillDown} /> : null}
    </div>
  );
}
