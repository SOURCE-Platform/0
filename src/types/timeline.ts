// Timeline data structures

export interface TimelineData {
  sessions: TimelineSession[];
  totalDuration: number;
  dateRange: {
    start: number;
    end: number;
  };
}

export interface TimelineSession {
  id: string;
  startTimestamp: number;
  endTimestamp: number | null;
  sessionType?: SessionType;
  applications: AppUsageSegment[];
  activityIntensity: number; // 0.0 - 1.0
  hasScreenRecording: boolean;
  hasInputRecording: boolean;
}

export interface AppUsageSegment {
  appName: string;
  bundleId: string;
  startTimestamp: number;
  endTimestamp: number;
  focusDuration: number;
  color: string;
}

export enum SessionType {
  Work = "Work",
  Development = "Development",
  Communication = "Communication",
  Research = "Research",
  Entertainment = "Entertainment",
  Unknown = "Unknown"
}

export enum TimelineZoom {
  Hour = "Hour",
  Day = "Day",
  Week = "Week",
  Month = "Month"
}
