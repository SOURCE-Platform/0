import { ActivitySegment, mockActivities } from "@/data/mockTimeline";
import {
  AXIS_X,
  DIGITAL_X,
  SENSOR_X,
  TRACK_W,
  TYPE_DISPLAY,
} from "@/components/vertical-timeline/constants";
import { ActivityBlock } from "@/components/vertical-timeline/ActivityBlock";
import {
  formatTime,
  getDetailPxPerMin,
  getDetailTicks,
  toY,
} from "@/components/vertical-timeline/helpers";
import { cn } from "@/lib/utils";

interface DetailViewProps {
  segment: ActivitySegment;
  onDrillDown: (segment: ActivitySegment) => void;
}

export function DetailView({ segment, onDrillDown }: DetailViewProps) {
  const pxPerMin = getDetailPxPerMin(segment);
  const totalHeight = Math.max(600, ((segment.end - segment.start) / 60000) * pxPerMin);
  const ticks = getDetailTicks(segment);
  const children = segment.children ?? [];
  const isPhysical = segment.track === "sensor" || segment.type === "physical";

  const sensorInRange = isPhysical
    ? []
    : mockActivities
        .filter((activity) => activity.track === "sensor" && activity.start < segment.end && activity.end > segment.start)
        .map((activity) => ({
          ...activity,
          start: Math.max(activity.start, segment.start),
          end: Math.min(activity.end, segment.end),
        }));

  const parentBlockStyle =
    segment.track === "sensor" && segment.type === "sleep"
      ? "bg-indigo-500/40"
      : segment.type === "physical"
        ? "bg-amber-500/30"
        : "bg-orange-400/30";

  return (
    <div className="w-full pb-16">
      <div className="relative mb-1 h-5">
        {isPhysical ? (
          <span className="absolute text-[11px] font-medium text-muted-foreground/70" style={{ left: DIGITAL_X }}>
            Physical space
          </span>
        ) : (
          <>
            <span
              className="absolute text-[11px] font-medium text-muted-foreground/70"
              style={{ left: DIGITAL_X + TRACK_W / 2, transform: "translateX(-50%)" }}
            >
              Digital
            </span>
            <span
              className="absolute text-[11px] font-medium text-muted-foreground/70"
              style={{ left: SENSOR_X + TRACK_W / 2, transform: "translateX(-50%)" }}
            >
              Physical space
            </span>
          </>
        )}
      </div>

      <div className="relative" style={{ height: totalHeight }}>
        {ticks.map((tick) => {
          const y = toY(tick, segment.start, pxPerMin);
          return (
            <div key={tick} className="absolute inset-x-0" style={{ top: y }}>
              <span
                className="absolute select-none text-[11px] leading-none text-muted-foreground/60"
                style={{ right: `calc(100% - ${AXIS_X - 6}px)`, top: -1 }}
              >
                {formatTime(tick)}
              </span>
              <div className="absolute h-px bg-border/40" style={{ left: AXIS_X, right: 0 }} />
            </div>
          );
        })}

        <div className="absolute bottom-0 top-0 w-px bg-border/50" style={{ left: AXIS_X }} />

        {isPhysical ? (
          <>
            <div
              className={cn(
                "absolute overflow-hidden rounded-bl-lg rounded-tl-lg border-r border-white/40",
                parentBlockStyle,
              )}
              style={{ top: 0, height: totalHeight, left: DIGITAL_X, width: TRACK_W }}
            >
              <p className="select-none px-3 py-2 text-[11px] font-medium text-white/80">
                {segment.label ?? TYPE_DISPLAY[segment.type]}
              </p>
            </div>

            {children.map((child, index) => {
              const top = toY(child.start, segment.start, pxPerMin);
              const height = Math.max(toY(child.end, segment.start, pxPerMin) - top, 3);
              const childX = DIGITAL_X + TRACK_W;
              const isFirst = index === 0;
              const isLast = index === children.length - 1;

              return (
                <div key={child.id}>
                  {!isFirst ? (
                    <div
                      className="absolute h-px bg-white/40"
                      style={{ top, left: childX, width: TRACK_W }}
                    />
                  ) : null}
                  <ActivityBlock
                    segment={child}
                    top={top}
                    height={height}
                    left={childX}
                    width={TRACK_W}
                    className={cn(isFirst && "rounded-tr-lg", isLast && "rounded-br-lg")}
                    onClick={() => onDrillDown(child)}
                  />
                </div>
              );
            })}
          </>
        ) : (
          <>
            {children.map((child) => {
              const top = toY(child.start, segment.start, pxPerMin);
              const height = Math.max(toY(child.end, segment.start, pxPerMin) - top, 3);
              return (
                <ActivityBlock
                  key={child.id}
                  segment={child}
                  top={top}
                  height={height}
                  left={DIGITAL_X}
                  width={TRACK_W}
                  onClick={() => onDrillDown(child)}
                />
              );
            })}

            {sensorInRange.map((item) => {
              const top = toY(item.start, segment.start, pxPerMin);
              const height = Math.max(toY(item.end, segment.start, pxPerMin) - top, 3);
              return (
                <ActivityBlock
                  key={item.id}
                  segment={item}
                  top={top}
                  height={height}
                  left={SENSOR_X}
                  width={TRACK_W}
                  onClick={() => onDrillDown(item)}
                />
              );
            })}
          </>
        )}
      </div>
    </div>
  );
}
