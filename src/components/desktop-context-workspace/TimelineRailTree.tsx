import { ChevronDown, ChevronRight, CircleHelp } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { RailLane } from "@/components/desktop-context-workspace/RailLane";
import { getVisibleDescendantSliceCount } from "@/components/desktop-context-workspace/utils";
import { ContextSlice, TimelineRail } from "@/types/contextTimeline";

interface TimelineRailTreeProps {
  rail: TimelineRail;
  depth: number;
  expandedRails: Record<string, boolean>;
  onToggleRail: (railId: string, nextExpanded: boolean) => void;
  windowStart: number;
  windowEnd: number;
  selectedSliceId: string | null;
  selectedRailId: string | null;
  onSelect: (slice: ContextSlice) => void;
  onDeselect: () => void;
  appFilter: string;
  interactionFilter: string;
}

export function TimelineRailTree({
  rail,
  depth,
  expandedRails,
  onToggleRail,
  windowStart,
  windowEnd,
  selectedSliceId,
  selectedRailId,
  onSelect,
  onDeselect,
  appFilter,
  interactionFilter,
}: TimelineRailTreeProps) {
  if (rail.kind === "lane") {
    return (
      <RailLane
        rail={rail}
        depth={depth}
        windowStart={windowStart}
        windowEnd={windowEnd}
        selectedSliceId={selectedSliceId}
        selectedRailId={selectedRailId}
        onSelect={onSelect}
        onDeselect={onDeselect}
        appFilter={appFilter}
        interactionFilter={interactionFilter}
      />
    );
  }

  const isExpanded = expandedRails[rail.id] ?? rail.defaultExpanded;
  const railPadding = depth * 16;

  return (
    <div className="space-y-2">
      <div className="grid gap-2 md:grid-cols-[10.5rem_minmax(0,1fr)]">
        <div
          className="sticky left-0 z-10 flex items-center gap-2 bg-muted/10 py-1 backdrop-blur-sm"
          style={{ paddingLeft: railPadding }}
        >
          <button
            type="button"
            onClick={() => onToggleRail(rail.id, !isExpanded)}
            className="inline-flex items-center gap-1.5 text-sm font-semibold text-foreground transition hover:text-white"
          >
            {isExpanded ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
            <span>{rail.label}</span>
          </button>
          <Badge variant="outline" className="text-[10px] uppercase tracking-wide">
            {getVisibleDescendantSliceCount(
              rail,
              windowStart,
              windowEnd,
              appFilter,
              interactionFilter,
            )}
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

        <div className="flex h-8 items-center rounded-xl border border-dashed border-border/60 px-3 text-xs text-muted-foreground">
          {isExpanded ? "Expanded grouped lanes stay aligned to the same time ruler." : "Expand to inspect nested audio lanes."}
        </div>
      </div>

      {isExpanded ? (
        <div className="space-y-2">
          {rail.children.map((child) => (
            <TimelineRailTree
              key={child.id}
              rail={child}
              depth={depth + 1}
              expandedRails={expandedRails}
              onToggleRail={onToggleRail}
              windowStart={windowStart}
              windowEnd={windowEnd}
              selectedSliceId={selectedSliceId}
              selectedRailId={selectedRailId}
              onSelect={onSelect}
              onDeselect={onDeselect}
              appFilter={appFilter}
              interactionFilter={interactionFilter}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}
