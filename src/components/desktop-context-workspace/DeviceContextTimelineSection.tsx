import { useState } from "react";
import { addDays, startOfDay } from "date-fns";
import { ChevronLeft, ChevronRight, Circle, StopCircle } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { TimelineRailTree } from "@/components/desktop-context-workspace/TimelineRailTree";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { TooltipProvider } from "@/components/ui/tooltip";
import { TimelineRuler } from "@/components/desktop-context-workspace/TimelineRuler";
import { useDesktopContextWorkspace } from "@/components/desktop-context-workspace/useDesktopContextWorkspace";
import {
  formatWindowScale,
  safeFormatDate,
} from "@/components/desktop-context-workspace/utils";

// Audio-only focus: microphone + desktop audio and dictation are the active
// data sources. Every other rail stays in the backend and comes back as its
// source is onboarded — this filter is the only thing hiding them.
function isAudioRail(railId: string): boolean {
  return railId === "audio" || railId.startsWith("audio_");
}

export function DeviceContextTimelineSection({
  controller,
}: {
  controller: ReturnType<typeof useDesktopContextWorkspace>;
}) {
  const [expandedRails, setExpandedRails] = useState<Record<string, boolean>>({});
  const selectedRailId = controller.selectedSlice?.rail ?? null;

  function handleToggleRail(railId: string, nextExpanded: boolean) {
    setExpandedRails((current) => ({
      ...current,
      [railId]: nextExpanded,
    }));
  }

  return (
    <section className="space-y-5">
      <div className="flex flex-wrap items-start justify-between gap-4 px-1">
        <div className="space-y-2">
          <h2 className="text-2xl font-semibold tracking-tight text-foreground">Device Context Timeline</h2>
          <p className="max-w-[58ch] text-base leading-8 text-muted-foreground">
            The live edge is pinned to the far right. New capture blocks appear there and drift left as your device context accumulates over time.
          </p>
        </div>

        <div className="flex items-center gap-3">
          <Badge variant={controller.status?.isActive ? "destructive" : "outline"} className="gap-2 px-3 py-1.5">
            <Circle className={`h-3 w-3 ${controller.status?.isActive ? "fill-current animate-pulse" : ""}`} />
            {controller.status?.isActive
              ? controller.status.displayName
                ? `Capturing on ${controller.status.displayName}`
                : "Capture running"
              : "Capture stopped"}
          </Badge>
          {!controller.status?.isActive ? (
            <Button onClick={controller.handleStartCapture} className="gap-2" disabled={!controller.canStartCapture}>
              <Circle className="h-3.5 w-3.5 fill-current" />
              Start Capture
            </Button>
          ) : (
            <Button onClick={controller.handleStopCapture} variant="destructive" className="gap-2">
              <StopCircle className="h-3.5 w-3.5" />
              Stop Capture
            </Button>
          )}
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-3 px-1">
        <Button variant="outline" size="sm" onClick={() => controller.setDayStart(addDays(controller.dayStart, -1).getTime())}>
          Previous day
        </Button>
        <Button variant="outline" size="sm" onClick={() => controller.setDayStart(startOfDay(new Date()).getTime())}>
          Today
        </Button>
        <Button variant="outline" size="sm" onClick={() => controller.setDayStart(addDays(controller.dayStart, 1).getTime())}>
          Next day
        </Button>
        <Badge variant="outline" className="text-xs">
          {safeFormatDate(controller.dayStart, "EEEE, MMMM d", "Selected day unavailable")}
        </Badge>

        <div className="ml-auto flex flex-wrap items-center gap-2">
          <Button variant="outline" size="sm" onClick={() => controller.shiftWindow(-1)} className="gap-2">
            <ChevronLeft className="h-3.5 w-3.5" />
            Back
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={() => controller.shiftWindow(1)}
            className="gap-2"
            disabled={controller.effectiveWindowEnd >= controller.dateRange.end}
          >
            Forward
            <ChevronRight className="h-3.5 w-3.5" />
          </Button>
          {!controller.isLiveFollowing && controller.isToday ? (
            <Button variant="secondary" size="sm" onClick={controller.handleJumpToNow}>
              Jump to Now
            </Button>
          ) : null}
          <Badge variant="outline" className="px-3 py-1.5 text-xs uppercase tracking-wide">
            Zoom {formatWindowScale(controller.windowDurationMs)}
          </Badge>
          <Select value={controller.appFilter} onValueChange={controller.setAppFilter}>
            <SelectTrigger className="w-[210px]">
              <SelectValue placeholder="All apps" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">All apps</SelectItem>
              {controller.appOptions.map((app) => (
                <SelectItem key={app} value={app}>
                  {app}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Select value={controller.interactionFilter} onValueChange={controller.setInteractionFilter}>
            <SelectTrigger className="w-[210px]">
              <SelectValue placeholder="All interaction states" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">All interaction states</SelectItem>
              <SelectItem value="active_typing">Typing</SelectItem>
              <SelectItem value="active_pointer">Mouse / pointer</SelectItem>
              <SelectItem value="passive_viewing">Passive viewing</SelectItem>
              <SelectItem value="voice_input_inferred">Inferred voice input</SelectItem>
              <SelectItem value="mixed">Mixed</SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>

      {controller.loading || !controller.timeline ? (
        <div className="rounded-2xl border border-border/70 bg-muted/25 py-20 text-center text-sm text-muted-foreground">
          Loading live device timeline...
        </div>
      ) : (
        <TooltipProvider>
          <div
            ref={controller.timelineSurfaceRef}
            className={`border-y border-border/70 bg-transparent px-1 py-4 ${
              controller.isPanning ? "cursor-grabbing select-none" : "cursor-default"
            }`}
          >
            <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border/70 pb-3">
              <div className="text-sm text-muted-foreground">
                Showing {safeFormatDate(controller.effectiveWindowStart, "p")} to{" "}
                {safeFormatDate(controller.effectiveWindowEnd, "p")}
              </div>
              <div className="text-sm text-muted-foreground">
                {controller.status?.isActive
                  ? "Hold Command and scroll to zoom all tracks."
                  : "Capture is stopped. You are reviewing previously recorded data for this day."}
              </div>
            </div>
            <div className="pt-3">
              <TimelineRuler
                startTimestamp={controller.effectiveWindowStart}
                endTimestamp={controller.effectiveWindowEnd}
              />
              <div className="space-y-2 pt-2">
                {controller.timeline.rails
                  .filter((rail) => isAudioRail(rail.id))
                  .map((rail) => (
                  <TimelineRailTree
                    key={rail.id}
                    rail={rail}
                    depth={0}
                    expandedRails={expandedRails}
                    onToggleRail={handleToggleRail}
                    windowStart={controller.effectiveWindowStart}
                    windowEnd={controller.effectiveWindowEnd}
                    selectedSliceId={controller.selectedSlice?.id ?? null}
                    selectedRailId={selectedRailId}
                    onSelect={controller.handleSelectSlice}
                    appFilter={controller.appFilter}
                    interactionFilter={controller.interactionFilter}
                  />
                ))}
              </div>
            </div>
          </div>
        </TooltipProvider>
      )}
    </section>
  );
}
