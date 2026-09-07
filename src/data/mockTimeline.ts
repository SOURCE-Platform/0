export type ActivityType = 'desktop' | 'phone' | 'tablet' | 'physical' | 'sleep' | 'away' | 'sensor';
export type TrackType = 'digital' | 'sensor';

export interface ActivitySegment {
  id: string;
  type: ActivityType;
  track?: TrackType; // defaults to 'digital'
  start: number;
  end: number;
  label?: string;
  children?: ActivitySegment[];
}

// No demo segments ship with the app. Real activity will populate this list;
// the WeekView/DetailView/RecordingView templates in
// src/components/vertical-timeline/ are kept for that work.
export const mockActivities: ActivitySegment[] = [];
