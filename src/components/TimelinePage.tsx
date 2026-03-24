import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { Circle, StopCircle, AlertCircle } from "lucide-react";
import Recordings from "./Recordings";
import VerticalTimeline from "./VerticalTimeline";

export const DISPLAY_KEY = "selected-display-id";

interface RecordingStatus {
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

export default function TimelinePage() {
  const [status, setStatus] = useState<RecordingStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  const recordingStartRef = useRef<number | null>(null);

  useEffect(() => {
    loadStatus();
  }, []);

  useEffect(() => {
    if (status?.is_recording) {
      if (recordingStartRef.current === null) {
        recordingStartRef.current = Date.now();
        setElapsedSeconds(0);
      }
      const interval = setInterval(() => {
        setElapsedSeconds(Math.floor((Date.now() - recordingStartRef.current!) / 1000));
      }, 1000);
      return () => clearInterval(interval);
    } else {
      recordingStartRef.current = null;
      setElapsedSeconds(0);
    }
  }, [status?.is_recording]);

  useEffect(() => {
    if (!status?.is_recording) return;
    const interval = setInterval(async () => {
      try {
        const s = await invoke<RecordingStatus>("get_recording_status");
        setStatus(s);
      } catch {}
    }, 2000);
    return () => clearInterval(interval);
  }, [status?.is_recording]);

  async function loadStatus() {
    try {
      const s = await invoke<RecordingStatus>("get_recording_status");
      setStatus(s);
    } catch (err) {
      setError(`Failed to load status: ${err}`);
    } finally {
      setLoading(false);
    }
  }

  async function handleStartRecording() {
    const raw = localStorage.getItem(DISPLAY_KEY);
    const displayId = raw !== null ? parseInt(raw, 10) : NaN;
    if (isNaN(displayId)) {
      setError("No display selected — choose one in Settings → Recording.");
      return;
    }
    setError(null);
    try {
      await invoke("start_screen_recording", { displayId });
      await loadStatus();
    } catch (err) {
      setError(`Failed to start recording: ${err}`);
    }
  }

  async function handleStopRecording() {
    setError(null);
    try {
      await invoke("stop_screen_recording");
      await loadStatus();
    } catch (err) {
      setError(`Failed to stop recording: ${err}`);
    }
  }

  function formatElapsed(seconds: number): string {
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    const s = seconds % 60;
    if (h > 0) return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
    return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  }

  const isRecording = status?.is_recording ?? false;
  const hasConsent = status?.has_consent ?? false;

  return (
    <div className="w-full max-w-5xl mx-auto space-y-4">
      {/* Recording controls bar */}
      <div className="flex items-center gap-3 min-h-[2.5rem]">
        {error && (
          <div className="flex items-center gap-1.5 text-sm text-red-600 dark:text-red-400">
            <AlertCircle className="h-4 w-4 shrink-0" />
            {error}
          </div>
        )}
        {!hasConsent && !loading && !error && (
          <p className="text-sm text-yellow-600 dark:text-yellow-400">
            Screen recording consent required — enable it in Settings → Privacy.
          </p>
        )}

        <div className="ml-auto flex items-center gap-3">
          {isRecording && (
            <div className="flex items-center gap-2 px-3 py-1.5 bg-red-100 dark:bg-red-950 rounded-md">
              <Circle className="h-3 w-3 fill-red-600 text-red-600 animate-pulse" />
              <span className="text-sm font-mono font-medium tabular-nums text-red-900 dark:text-red-100">
                {formatElapsed(elapsedSeconds)}
              </span>
              <span className="text-xs text-red-700 dark:text-red-300">
                {status?.total_frames?.toLocaleString()} frames · {status?.segment_count} seg
              </span>
            </div>
          )}

          {!isRecording ? (
            <Button
              onClick={handleStartRecording}
              disabled={loading || !hasConsent}
              className="gap-2"
            >
              <Circle className="h-3.5 w-3.5 fill-current" />
              Start Recording
            </Button>
          ) : (
            <Button onClick={handleStopRecording} variant="destructive" className="gap-2">
              <StopCircle className="h-3.5 w-3.5" />
              Stop Recording
            </Button>
          )}
        </div>
      </div>

      {/* Vertical timeline */}
      <VerticalTimeline />

      {/* Recordings list + video player — hidden until Timeline UI is ready */}
      <div className="hidden">
        <Recordings />
      </div>
    </div>
  );
}
