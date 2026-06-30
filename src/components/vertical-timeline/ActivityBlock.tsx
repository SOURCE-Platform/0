import { ChevronRight } from "lucide-react";
import { ActivitySegment } from "@/data/mockTimeline";
import {
  BG_STYLES,
  FG_STYLES,
  LABEL_COLOR,
  TYPE_DISPLAY,
} from "@/components/vertical-timeline/constants";
import { hasChildren, isBackground } from "@/components/vertical-timeline/helpers";
import { cn } from "@/lib/utils";

interface ActivityBlockProps {
  segment: ActivitySegment;
  top: number;
  height: number;
  left: number;
  width: number;
  onClick?: () => void;
  className?: string;
}

export function ActivityBlock({
  segment,
  top,
  height,
  left,
  width,
  onClick,
  className,
}: ActivityBlockProps) {
  const isSensor = segment.track === "sensor";
  const background = isBackground(segment.type) && !isSensor;
  const childPresent = hasChildren(segment);

  const style = background
    ? BG_STYLES[segment.type]
    : isSensor && segment.type === "sleep"
      ? "bg-indigo-400/70"
      : FG_STYLES[segment.type];

  const labelColor = isSensor && segment.type === "sleep" ? "text-white" : LABEL_COLOR[segment.type];

  return (
    <div
      className={cn(
        "absolute cursor-pointer overflow-hidden transition-opacity hover:opacity-80",
        style,
        className,
      )}
      style={{ top, height, left, width }}
      onClick={onClick}
      title={`${TYPE_DISPLAY[segment.type]}${segment.label ? ` — ${segment.label}` : ""}`}
    >
      {height >= 24 ? (
        <div className={cn("flex items-center justify-between px-3 py-2", labelColor)}>
          <p className="truncate text-[11px] leading-tight">
            <span className="font-medium">{segment.label ?? TYPE_DISPLAY[segment.type]}</span>
          </p>
          {childPresent && height >= 20 ? (
            <ChevronRight className="h-3 w-3 shrink-0 opacity-60" />
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
