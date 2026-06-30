import { Play, Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ActivitySegment } from "@/data/mockTimeline";
import { FG_STYLES, TYPE_DISPLAY } from "@/components/vertical-timeline/constants";
import { durationLabel, formatTime } from "@/components/vertical-timeline/helpers";
import { cn } from "@/lib/utils";

export function RecordingView({ segment }: { segment: ActivitySegment }) {
  return (
    <div className="mx-auto max-w-md space-y-6 py-12">
      <div className="space-y-1">
        <div
          className={cn(
            "mb-3 inline-flex items-center gap-2 rounded-full px-3 py-1.5 text-sm font-medium text-white",
            FG_STYLES[segment.type],
          )}
        >
          {TYPE_DISPLAY[segment.type]}
        </div>
        <h2 className="text-2xl font-semibold">{segment.label}</h2>
        <p className="text-sm text-muted-foreground">
          {formatTime(segment.start)} – {formatTime(segment.end)} · {durationLabel(segment)}
        </p>
      </div>

      <div className="flex aspect-video w-full items-center justify-center rounded-lg border border-border/50 bg-muted">
        <div className="space-y-2 text-center">
          <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-full bg-foreground/10">
            <Play className="ml-0.5 h-5 w-5 text-foreground/60" />
          </div>
          <p className="text-xs text-muted-foreground">Recording available</p>
        </div>
      </div>

      <div className="flex gap-3">
        <Button className="flex-1 gap-2">
          <Play className="h-4 w-4" />
          Play Recording
        </Button>
        <Button variant="outline" className="flex-1 gap-2">
          <Sparkles className="h-4 w-4" />
          AI Overview
        </Button>
      </div>
    </div>
  );
}
