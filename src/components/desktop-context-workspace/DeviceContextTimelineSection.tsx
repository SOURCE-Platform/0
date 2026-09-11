import { useEffect, useRef } from "react";
import { addDays, startOfDay } from "date-fns";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { TimelineRailTree } from "@/components/desktop-context-workspace/TimelineRailTree";
import { TimelineMicrophoneSelect } from "@/components/desktop-context-workspace/TimelineMicrophoneSelect";
import { TooltipProvider } from "@/components/ui/tooltip";
import { TimelineRuler } from "@/components/desktop-context-workspace/TimelineRuler";
import { useDesktopContextWorkspace } from "@/components/desktop-context-workspace/useDesktopContextWorkspace";
import {
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
  const selectedRailId = controller.selectedSlice?.rail ?? null;
  const autoStartedRef = useRef(false);

  // Always-on audio: capture starts with the workspace. Screen recording and
  // OCR stay disabled during the audio-timeline phase.
  useEffect(() => {
    if (autoStartedRef.current) return;
    if (controller.loading || !controller.canStartCapture) return;
    if (controller.status?.isActive) {
      autoStartedRef.current = true;
      return;
    }
    autoStartedRef.current = true;
    void controller.handleStartCapture();
  }, [controller.loading, controller.canStartCapture, controller.status?.isActive]);

  return (
    <section className="space-y-5">
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
            className={`bg-transparent px-1 py-4 ${
              controller.isPanning ? "cursor-grabbing select-none" : "cursor-default"
            }`}
          >
            <div className="flex flex-wrap items-center justify-between gap-3 pb-3">
              <div className="text-sm text-muted-foreground">
                {safeFormatDate(controller.dayStart, "EEEE, MMMM d", "Selected day unavailable")}
                {" · "}
                {safeFormatDate(controller.effectiveWindowStart, "p")} to{" "}
                {safeFormatDate(controller.effectiveWindowEnd, "p")}
              </div>
              <TimelineMicrophoneSelect
                currentSourceName={controller.status?.audioSourceName ?? null}
              />
            </div>
            <div className="pt-2">
              <TimelineRuler
                startTimestamp={controller.effectiveWindowStart}
                endTimestamp={controller.effectiveWindowEnd}
              />
              <div className="space-y-0">
                {controller.timeline.rails
                  .filter((rail) => isAudioRail(rail.id))
                  .map((rail) => (
                  <TimelineRailTree
                    key={rail.id}
                    rail={rail}
                    depth={0}
                    expandedRails={{}}
                    onToggleRail={() => {}}
                    windowStart={controller.effectiveWindowStart}
                    windowEnd={controller.effectiveWindowEnd}
                    selectedSliceId={controller.selectedSlice?.id ?? null}
                    selectedRailId={selectedRailId}
                    onSelect={controller.handleSelectSlice}
                    onDeselect={controller.clearSelectedSlice}
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
