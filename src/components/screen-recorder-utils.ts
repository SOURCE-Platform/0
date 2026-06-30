export interface Display {
  id: number;
  name: string;
  width: number;
  height: number;
  is_primary: boolean;
}

export interface RecordingStatus {
  is_recording: boolean;
  display_id: number | null;
  display_name: string | null;
  has_consent: boolean;
  session_id: string | null;
  segment_count: number;
  total_frames: number;
  total_motion_percentage: number;
  is_paused: boolean;
  save_directory: string | null;
}

export function formatElapsed(seconds: number): string {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = seconds % 60;

  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;
  }

  return `${String(minutes).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;
}
