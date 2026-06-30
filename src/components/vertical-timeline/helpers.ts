import { ActivitySegment } from "@/data/mockTimeline";
import { ActivityType, mockActivities } from "@/data/mockTimeline";

export function dayStart(date: Date): number {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

export function activitiesForDay(day: Date, activities: ActivitySegment[] = mockActivities) {
  const start = dayStart(day);
  const end = start + 24 * 60 * 60 * 1000;
  const clipped = activities
    .filter((activity) => activity.start < end && activity.end > start)
    .map((activity) => ({
      ...activity,
      start: Math.max(activity.start, start),
      end: Math.min(activity.end, end),
    }))
    .sort((first, second) => first.start - second.start);

  return {
    digital: clipped.filter((activity) => activity.track !== "sensor" && activity.type !== "physical"),
    sensor: clipped.filter((activity) => activity.track === "sensor" || activity.type === "physical"),
  };
}

export function toY(timestamp: number, refStart: number, pxPerMin: number): number {
  return ((timestamp - refStart) / 60000) * pxPerMin;
}

export function hourLabel(hour: number, mode: "calendar" | "clock-24h" | "day-night" | "sleep-awake") {
  if (mode === "clock-24h") return `${String(hour).padStart(2, "0")}:00`;
  if (hour === 0) return "12am";
  if (hour < 12) return `${hour}am`;
  if (hour === 12) return "12pm";
  return `${hour - 12}pm`;
}

export function formatTime(timestamp: number): string {
  const date = new Date(timestamp);
  const hours = date.getHours();
  const minutes = date.getMinutes();
  const ampm = hours >= 12 ? "PM" : "AM";
  const displayHour = hours % 12 || 12;
  return minutes === 0
    ? `${displayHour} ${ampm}`
    : `${displayHour}:${String(minutes).padStart(2, "0")} ${ampm}`;
}

export function durationLabel(segment: ActivitySegment): string {
  const minutes = Math.round((segment.end - segment.start) / 60000);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const remainder = minutes % 60;
  return remainder === 0 ? `${hours}h` : `${hours}h ${remainder}m`;
}

export function isBackground(type: ActivityType) {
  return type === "sleep" || type === "away";
}

export function hasChildren(segment: ActivitySegment) {
  return Boolean(segment.children && segment.children.length > 0);
}

export function getDetailTicks(segment: ActivitySegment): number[] {
  const durationMin = (segment.end - segment.start) / 60000;
  const intervalMin =
    durationMin <= 30 ? 5 : durationMin <= 120 ? 15 : durationMin <= 360 ? 30 : 60;
  const ticks: number[] = [];
  for (let timestamp = segment.start; timestamp <= segment.end; timestamp += intervalMin * 60000) {
    ticks.push(timestamp);
  }
  return ticks;
}

export function getDetailPxPerMin(segment: ActivitySegment) {
  const durationMin = (segment.end - segment.start) / 60000;
  return Math.max(5, 600 / durationMin);
}
