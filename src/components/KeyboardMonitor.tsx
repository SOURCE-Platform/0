import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from "@/components/ui/button";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { TriangleAlert } from "lucide-react";

interface KeyboardMonitorProps {
  sessionId: string;
}

export default function KeyboardMonitor({ sessionId }: KeyboardMonitorProps) {
  const [isRecording, setIsRecording] = useState(false);
  const [hasConsent, setHasConsent] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Check consent and recording status on mount
  useEffect(() => {
    checkConsent();
    checkRecordingStatus();
  }, []);

  const checkConsent = async () => {
    try {
      const consent = await invoke<boolean>('check_consent_status', {
        feature: 'KeyboardRecording'
      });
      setHasConsent(consent);
    } catch (err) {
      setError(`Error checking consent: ${err}`);
    }
  };

  const checkRecordingStatus = async () => {
    try {
      const recording = await invoke<boolean>('is_keyboard_recording');
      setIsRecording(recording);
    } catch (err) {
      console.error('Error checking recording status:', err);
    }
  };

  const requestConsent = async () => {
    try {
      await invoke('request_consent', { feature: 'KeyboardRecording' });
      setHasConsent(true);
      setError(null);
    } catch (err) {
      setError(`Error requesting consent: ${err}`);
    }
  };

  const handleStartRecording = async () => {
    if (!hasConsent) {
      setError('Keyboard Recording consent required');
      return;
    }

    try {
      await invoke('start_keyboard_recording', { sessionId });
      setIsRecording(true);
      setError(null);
    } catch (err) {
      setError(`Error starting keyboard recording: ${err}`);
    }
  };

  const handleStopRecording = async () => {
    try {
      await invoke('stop_keyboard_recording');
      setIsRecording(false);
      setError(null);
    } catch (err) {
      setError(`Error stopping keyboard recording: ${err}`);
    }
  };

  return (
    <div className="p-4 border border-border rounded-lg">
      <h2 className="text-xl font-medium mb-4">Keyboard Monitor</h2>

      {error && (
        <Alert variant="destructive" className="mb-4">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      <Alert className="mb-4">
        <TriangleAlert className="h-4 w-4" />
        <AlertTitle>Privacy Notice</AlertTitle>
        <AlertDescription>
          Keyboard monitoring captures keystroke statistics and patterns for productivity analysis.
          Sensitive fields (passwords, credit cards, etc.) are automatically filtered and never logged.
          All data is stored locally on your device.
        </AlertDescription>
      </Alert>

      {!hasConsent ? (
        <div className="mb-4">
          <p className="text-muted-foreground mb-2">
            Keyboard monitoring requires consent to track typing patterns and statistics.
          </p>
          <Button onClick={requestConsent}>Grant Consent</Button>
        </div>
      ) : (
        <>
          <div className="mb-4 flex items-center gap-3">
            <Button
              onClick={isRecording ? handleStopRecording : handleStartRecording}
              variant={isRecording ? "destructive" : "default"}
            >
              {isRecording ? 'Stop Recording' : 'Start Recording'}
            </Button>
            <span className="text-sm text-muted-foreground">
              Status: {isRecording ? 'Recording' : 'Stopped'}
            </span>
          </div>

          {isRecording && (
            <div className="p-3 bg-green-50 dark:bg-green-950 border border-green-200 dark:border-green-800 rounded-lg">
              <div className="flex items-center mb-2">
                <div className="w-2 h-2 bg-green-500 rounded-full mr-2 animate-pulse"></div>
                <span className="text-sm text-foreground">
                  Actively recording keyboard events for session {sessionId.substring(0, 8)}...
                </span>
              </div>
              <ul className="text-xs text-muted-foreground list-disc list-inside space-y-1">
                <li>Keystroke count and typing speed</li>
                <li>Most used keys and shortcuts</li>
                <li>Per-application typing patterns</li>
              </ul>
            </div>
          )}

          {!isRecording && (
            <p className="text-sm text-muted-foreground">
              Click "Start Recording" to begin tracking keyboard statistics for this session.
            </p>
          )}
        </>
      )}
    </div>
  );
}
