import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { AlertCircle, Circle, Monitor, StopCircle } from "lucide-react";

interface Display {
  id: number;
  name: string;
  width: number;
  height: number;
  is_primary: boolean;
}

interface RecordingStatus {
  is_recording: boolean;
  display_id: number | null;
  display_name: string | null;
  has_consent: boolean;
}

export default function ScreenRecorder() {
  const [displays, setDisplays] = useState<Display[]>([]);
  const [selectedDisplay, setSelectedDisplay] = useState<number | null>(null);
  const [status, setStatus] = useState<RecordingStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    loadDisplaysAndStatus();
  }, []);

  async function loadDisplaysAndStatus() {
    setLoading(true);
    setError(null);

    try {
      // Load displays
      const availableDisplays = await invoke<Display[]>("get_available_displays");
      setDisplays(availableDisplays);

      // Select primary display by default
      const primaryDisplay = availableDisplays.find((d) => d.is_primary);
      if (primaryDisplay && !selectedDisplay) {
        setSelectedDisplay(primaryDisplay.id);
      }

      // Load status
      const currentStatus = await invoke<RecordingStatus>("get_recording_status");
      setStatus(currentStatus);
    } catch (err) {
      console.error("Failed to load displays:", err);
      setError(`Failed to initialize screen recorder: ${err}`);
    } finally {
      setLoading(false);
    }
  }

  async function handleStartRecording() {
    if (selectedDisplay === null) {
      setError("Please select a display to record");
      return;
    }

    setError(null);

    try {
      await invoke("start_screen_recording", { displayId: selectedDisplay });
      await loadDisplaysAndStatus();
    } catch (err) {
      console.error("Failed to start recording:", err);
      setError(`Failed to start recording: ${err}`);
    }
  }

  async function handleStopRecording() {
    setError(null);

    try {
      await invoke("stop_screen_recording");
      await loadDisplaysAndStatus();
    } catch (err) {
      console.error("Failed to stop recording:", err);
      setError(`Failed to stop recording: ${err}`);
    }
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center min-h-[400px]">
        <p className="text-muted-foreground">Loading screen recorder...</p>
      </div>
    );
  }

  if (error && displays.length === 0) {
    return (
      <div className="w-full max-w-5xl mx-auto p-6">
        <Card className="border-red-500 bg-red-50 dark:bg-red-950">
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-red-900 dark:text-red-100">
              <AlertCircle className="h-5 w-5" />
              Screen Recorder Unavailable
            </CardTitle>
          </CardHeader>
          <CardContent className="text-red-800 dark:text-red-200">
            <p>{error}</p>
            <p className="mt-2 text-sm">
              This may occur on platforms without screen capture support (e.g., Linux Wayland, headless environments).
            </p>
          </CardContent>
        </Card>
      </div>
    );
  }

  const isRecording = status?.is_recording || false;
  const hasConsent = status?.has_consent || false;

  return (
    <div className="w-full max-w-5xl mx-auto p-6 space-y-6">
      <div className="flex items-center justify-between">
        <div className="space-y-1">
          <h1 className="text-3xl font-bold tracking-tight text-foreground">Screen Recorder</h1>
          <p className="text-muted-foreground">Capture your screen activity</p>
        </div>
        <div className="flex items-center gap-3">
          {isRecording && (
            <div className="flex items-center gap-2 px-3 py-2 bg-red-100 dark:bg-red-950 rounded-md">
              <Circle className="h-3 w-3 fill-red-600 text-red-600 animate-pulse" />
              <span className="text-sm font-medium text-red-900 dark:text-red-100">Recording</span>
            </div>
          )}
        </div>
      </div>

      {error && (
        <Card className="border-red-500 bg-red-50 dark:bg-red-950">
          <CardContent className="pt-6">
            <div className="flex items-center gap-2">
              <AlertCircle className="h-4 w-4 text-red-600 dark:text-red-400" />
              <p className="text-red-900 dark:text-red-100">{error}</p>
            </div>
          </CardContent>
        </Card>
      )}

      {!hasConsent && (
        <Card className="border-yellow-500 bg-yellow-50 dark:bg-yellow-950">
          <CardHeader>
            <CardTitle className="text-yellow-900 dark:text-yellow-100">Consent Required</CardTitle>
            <CardDescription className="text-yellow-800 dark:text-yellow-200">
              You need to grant screen recording consent before you can start recording.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <p className="text-sm text-yellow-800 dark:text-yellow-200">
              Please go to the <strong>Privacy & Consent</strong> tab and enable <strong>Screen Recording</strong>.
            </p>
          </CardContent>
        </Card>
      )}

      <div className="grid grid-cols-3 gap-4 items-start">
        <Card className="col-span-2">
          <CardHeader>
            <CardTitle>Display Selection</CardTitle>
            <CardDescription>Choose which display to record</CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="space-y-2">
              <label className="text-sm font-medium">Display</label>
              <Select
                value={selectedDisplay?.toString() || ""}
                onValueChange={(value) => setSelectedDisplay(parseInt(value, 10))}
                disabled={isRecording}
              >
                <SelectTrigger>
                  <SelectValue placeholder="Select a display" />
                </SelectTrigger>
                <SelectContent>
                  {displays.map((display) => (
                    <SelectItem key={display.id} value={display.id.toString()}>
                      <div className="flex items-center gap-2">
                        <Monitor className="h-4 w-4" />
                        <span>
                          {display.name} ({display.width}x{display.height})
                          {display.is_primary && " - Primary"}
                        </span>
                      </div>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {selectedDisplay !== null && (
              <div className="p-4 bg-muted rounded-lg">
                <p className="text-sm text-muted-foreground">
                  Selected:{" "}
                  {displays.find((d) => d.id === selectedDisplay)?.name || "Unknown"}
                </p>
                <p className="text-sm text-muted-foreground">
                  Resolution:{" "}
                  {displays.find((d) => d.id === selectedDisplay)?.width}x
                  {displays.find((d) => d.id === selectedDisplay)?.height}
                </p>
              </div>
            )}
          </CardContent>
        </Card>

        <Card className="col-span-1">
          <CardHeader>
            <CardTitle>Recording Controls</CardTitle>
            <CardDescription>
              {isRecording
                ? "Recording is active. Click stop to end the session."
                : "Click start to begin recording your screen."}
            </CardDescription>
          </CardHeader>
          <CardContent>
            <div className="flex gap-3">
              {!isRecording ? (
                <Button
                  onClick={handleStartRecording}
                  disabled={selectedDisplay === null || !hasConsent}
                  size="lg"
                  className="gap-2 w-full"
                >
                  <Circle className="h-4 w-4 fill-current" />
                  Start Recording
                </Button>
              ) : (
                <Button
                  onClick={handleStopRecording}
                  variant="destructive"
                  size="lg"
                  className="gap-2 w-full"
                >
                  <StopCircle className="h-4 w-4" />
                  Stop Recording
                </Button>
              )}
            </div>

            {isRecording && status?.display_name && (
              <div className="mt-4 p-4 bg-muted rounded-lg">
                <p className="text-sm font-medium">Currently recording:</p>
                <p className="text-sm text-muted-foreground">{status.display_name}</p>
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      <Card className="border-blue-200 dark:border-blue-900 bg-blue-50 dark:bg-blue-950">
        <CardHeader>
          <CardTitle className="text-blue-900 dark:text-blue-100">Important Notes</CardTitle>
        </CardHeader>
        <CardContent className="space-y-2 text-sm text-blue-800 dark:text-blue-200">
          <p>• Recording captures raw frames - no video encoding yet</p>
          <p>• Frames are not saved to disk in this phase</p>
          <p>• This is infrastructure testing - full recording service coming soon</p>
          <p>• Check console logs to see capture activity</p>
        </CardContent>
      </Card>
    </div>
  );
}
