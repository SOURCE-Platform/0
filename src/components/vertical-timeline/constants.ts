import { ActivityType } from "@/data/mockTimeline";

export const PX_PER_HOUR = 64;
export const PX_PER_MIN = PX_PER_HOUR / 60;
export const DAY_PX = PX_PER_HOUR * 24;
export const AXIS_X = 72;
export const TRACK_W = 160;
export const TRACK_GAP = 8;
export const DIGITAL_X = AXIS_X + 8;
export const SENSOR_X = DIGITAL_X + TRACK_W + TRACK_GAP;

export const DAYS: Date[] = Array.from({ length: 7 }, (_, index) => new Date(2026, 2, 23 - index));
export const TODAY = DAYS[0];

export type ChunkMode = "calendar" | "clock-24h" | "day-night" | "sleep-awake";

export const CHUNK_MODES: { value: ChunkMode; label: string }[] = [
  { value: "calendar", label: "Calendar day" },
  { value: "clock-24h", label: "24h clock" },
  { value: "day-night", label: "Day / Night" },
  { value: "sleep-awake", label: "Sleep / Awake" },
];

export const NIGHT_BANDS = [
  { start: 0, end: 6, cls: "bg-indigo-950/40" },
  { start: 6, end: 8, cls: "bg-indigo-950/15" },
  { start: 20, end: 22, cls: "bg-indigo-950/15" },
  { start: 22, end: 24, cls: "bg-indigo-950/40" },
];

export const BG_STYLES: Partial<Record<ActivityType, string>> = {
  sleep: "bg-slate-800/50 dark:bg-slate-900/60",
  away: "border border-dashed border-border/50 bg-transparent",
};

export const FG_STYLES: Partial<Record<ActivityType, string>> = {
  desktop: "bg-blue-500/90",
  phone: "bg-emerald-500/90",
  tablet: "bg-violet-500/90",
  physical: "bg-amber-500/90",
  sensor: "bg-orange-400/80",
};

export const LABEL_COLOR: Partial<Record<ActivityType, string>> = {
  desktop: "text-white",
  phone: "text-white",
  tablet: "text-white",
  physical: "text-white",
  sensor: "text-white",
  sleep: "text-slate-400 dark:text-slate-500",
  away: "text-muted-foreground",
};

export const TYPE_DISPLAY: Record<ActivityType, string> = {
  desktop: "Desktop",
  phone: "Phone",
  tablet: "Tablet",
  physical: "Physical",
  sleep: "Sleep",
  away: "Away",
  sensor: "Space",
};

export const LEGEND_DOT: Record<ActivityType, string> = {
  desktop: "bg-blue-500",
  phone: "bg-emerald-500",
  tablet: "bg-violet-500",
  physical: "bg-amber-500",
  sensor: "bg-orange-400",
  sleep: "bg-slate-600",
  away: "border border-dashed border-border",
};
