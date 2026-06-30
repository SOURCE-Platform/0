import { ActivitySegment } from "@/data/mockTimeline";
import { ActivityBlock } from "@/components/vertical-timeline/ActivityBlock";
import {
  AXIS_X,
  ChunkMode,
  DAY_PX,
  DAYS,
  DIGITAL_X,
  NIGHT_BANDS,
  PX_PER_HOUR,
  PX_PER_MIN,
  SENSOR_X,
  TODAY,
  TRACK_W,
} from "@/components/vertical-timeline/constants";
import { activitiesForDay, dayStart, hourLabel, toY } from "@/components/vertical-timeline/helpers";
import { cn } from "@/lib/utils";

interface WeekViewProps {
  mode: ChunkMode;
  onDrillDown: (segment: ActivitySegment) => void;
  activities: ActivitySegment[];
}

export function WeekView({ mode, onDrillDown, activities }: WeekViewProps) {
  return (
    <div className="space-y-8 pb-16">
      <div className="relative h-5">
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
      </div>

      {DAYS.map((day, dayIndex) => {
        const start = dayStart(day);
        const { digital, sensor } = activitiesForDay(day, activities);
        const isToday = day.getTime() === TODAY.getTime();
        const sleepBands = sensor
          .filter((activity) => activity.type === "sleep")
          .map((activity) => ({
            start: toY(activity.start, start, PX_PER_MIN),
            end: toY(activity.end, start, PX_PER_MIN),
          }));

        return (
          <div key={dayIndex}>
            <div className="mb-2 pl-1">
              <span className={cn("text-sm font-medium", isToday ? "text-foreground" : "text-muted-foreground")}>
                {day.toLocaleDateString("en-US", { weekday: "long", month: "short", day: "numeric" })}
                {isToday ? <span className="ml-2 text-xs font-normal text-muted-foreground">Today</span> : null}
              </span>
            </div>

            <div className="relative" style={{ height: DAY_PX }}>
              {mode === "day-night"
                ? NIGHT_BANDS.map((band, index) => (
                    <div
                      key={index}
                      className={cn("pointer-events-none absolute", band.cls)}
                      style={{
                        top: band.start * PX_PER_HOUR,
                        height: (band.end - band.start) * PX_PER_HOUR,
                        left: AXIS_X,
                        right: 0,
                      }}
                    />
                  ))
                : null}

              {mode === "sleep-awake"
                ? sleepBands.map((band, index) => (
                    <div
                      key={index}
                      className="pointer-events-none absolute bg-slate-900/30 dark:bg-slate-950/50"
                      style={{ top: band.start, height: band.end - band.start, left: AXIS_X, right: 0 }}
                    />
                  ))
                : null}

              {Array.from({ length: 25 }, (_, hour) => (
                <div key={hour} className="absolute inset-x-0" style={{ top: hour * PX_PER_HOUR }}>
                  {hour < 24 && hour % 2 === 0 ? (
                    <span
                      className="absolute select-none text-[11px] leading-none text-muted-foreground/60"
                      style={{ right: `calc(100% - ${AXIS_X - 6}px)`, top: -1 }}
                    >
                      {hourLabel(hour, mode)}
                    </span>
                  ) : null}
                  <div
                    className={cn("absolute h-px", hour % 2 === 0 ? "bg-border/40" : "bg-border/20")}
                    style={{ left: AXIS_X, right: 0 }}
                  />
                </div>
              ))}

              {Array.from({ length: 24 }, (_, hour) => (
                <div
                  key={hour}
                  className="absolute h-px bg-border/10"
                  style={{ top: hour * PX_PER_HOUR + PX_PER_HOUR / 2, left: AXIS_X, right: 0 }}
                />
              ))}

              <div className="absolute bottom-0 top-0 w-px bg-border/50" style={{ left: AXIS_X }} />

              {digital.map((segment) => {
                const top = toY(segment.start, start, PX_PER_MIN);
                const height = Math.max(toY(segment.end, start, PX_PER_MIN) - top, 3);
                return (
                  <ActivityBlock
                    key={segment.id}
                    segment={segment}
                    top={top}
                    height={height}
                    left={DIGITAL_X}
                    width={TRACK_W}
                    onClick={() => onDrillDown(segment)}
                  />
                );
              })}

              {sensor.map((segment) => {
                const top = toY(segment.start, start, PX_PER_MIN);
                const height = Math.max(toY(segment.end, start, PX_PER_MIN) - top, 3);
                return (
                  <ActivityBlock
                    key={segment.id}
                    segment={segment}
                    top={top}
                    height={height}
                    left={SENSOR_X}
                    width={TRACK_W}
                    onClick={() => onDrillDown(segment)}
                  />
                );
              })}
            </div>
          </div>
        );
      })}
    </div>
  );
}
