import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

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
    <div className="p-4 border border-gray-300 dark:border-gray-700 rounded-lg">
      <h2 className="text-xl font-semibold mb-4">Keyboard Monitor</h2>

      {error && (
        <div className="mb-4 p-3 bg-red-100 dark:bg-red-900 text-red-900 dark:text-red-100 rounded">
          {error}
        </div>
      )}

      <div className="mb-4 p-3 bg-yellow-50 dark:bg-yellow-900 text-yellow-900 dark:text-yellow-100 rounded">
        <div className="flex items-start">
          <svg className="w-5 h-5 mr-2 mt-0.5 flex-shrink-0" fill="currentColor" viewBox="0 0 20 20">
            <path fillRule="evenodd" d="M8.257 3.099c.765-1.36 2.722-1.36 3.486 0l5.58 9.92c.75 1.334-.213 2.98-1.742 2.98H4.42c-1.53 0-2.493-1.646-1.743-2.98l5.58-9.92zM11 13a1 1 0 11-2 0 1 1 0 012 0zm-1-8a1 1 0 00-1 1v3a1 1 0 002 0V6a1 1 0 00-1-1z" clipRule="evenodd" />
          </svg>
          <div>
            <div className="font-semibold mb-1">Privacy Notice</div>
            <p className="text-sm">
              Keyboard monitoring captures keystroke statistics and patterns for productivity analysis.
              Sensitive fields (passwords, credit cards, etc.) are automatically filtered and never logged.
              All data is stored locally on your device.
            </p>
          </div>
        </div>
      </div>

      {!hasConsent ? (
        <div className="mb-4">
          <p className="text-gray-700 dark:text-gray-300 mb-2">
            Keyboard monitoring requires consent to track typing patterns and statistics.
          </p>
          <button
            onClick={requestConsent}
            className="px-4 py-2 bg-blue-500 text-white rounded hover:bg-blue-600"
          >
            Grant Consent
          </button>
        </div>
      ) : (
        <>
          <div className="mb-4">
            <button
              onClick={isRecording ? handleStopRecording : handleStartRecording}
              className={`px-4 py-2 rounded ${
                isRecording
                  ? 'bg-red-500 hover:bg-red-600 text-white'
                  : 'bg-green-500 hover:bg-green-600 text-white'
              }`}
            >
              {isRecording ? 'Stop Recording' : 'Start Recording'}
            </button>
            <span className="ml-3 text-sm text-gray-600 dark:text-gray-400">
              Status: {isRecording ? 'Recording' : 'Stopped'}
            </span>
          </div>

          {isRecording && (
            <div className="p-3 bg-green-50 dark:bg-green-900 rounded">
              <div className="flex items-center">
                <div className="w-2 h-2 bg-green-500 rounded-full mr-2 animate-pulse"></div>
                <span className="text-sm text-gray-700 dark:text-gray-300">
                  Actively recording keyboard events for session {sessionId.substring(0, 8)}...
                </span>
              </div>
              <div className="mt-2 text-xs text-gray-600 dark:text-gray-400">
                <ul className="list-disc list-inside space-y-1">
                  <li>Keystroke count and typing speed</li>
                  <li>Most used keys and shortcuts</li>
                  <li>Per-application typing patterns</li>
                </ul>
              </div>
            </div>
          )}

          {!isRecording && (
            <div className="text-sm text-gray-500 dark:text-gray-400">
              Click "Start Recording" to begin tracking keyboard statistics for this session.
            </div>
          )}
        </>
      )}
    </div>
  );
}
