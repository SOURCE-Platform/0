import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface AppInfo {
  name: string;
  bundle_id: string;
  process_id: number;
  version?: string;
  executable_path?: string;
}

interface ActivityMonitorProps {
  sessionId: string;
}

export default function ActivityMonitor({ sessionId }: ActivityMonitorProps) {
  const [isMonitoring, setIsMonitoring] = useState(false);
  const [runningApps, setRunningApps] = useState<AppInfo[]>([]);
  const [currentApp, setCurrentApp] = useState<AppInfo | null>(null);
  const [hasConsent, setHasConsent] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Check consent status on mount
  useEffect(() => {
    checkConsent();
  }, []);

  // Poll current app and running apps while monitoring
  useEffect(() => {
    if (!isMonitoring) return;

    const interval = setInterval(async () => {
      try {
        const apps = await invoke<AppInfo[]>('get_running_applications');
        setRunningApps(apps);

        const current = await invoke<AppInfo | null>('get_current_application');
        setCurrentApp(current);
      } catch (err) {
        console.error('Error fetching app info:', err);
      }
    }, 2000); // Poll every 2 seconds

    return () => clearInterval(interval);
  }, [isMonitoring]);

  const checkConsent = async () => {
    try {
      const consent = await invoke<boolean>('check_consent_status', {
        feature: 'OsActivity'
      });
      setHasConsent(consent);
    } catch (err) {
      setError(`Error checking consent: ${err}`);
    }
  };

  const requestConsent = async () => {
    try {
      await invoke('request_consent', { feature: 'OsActivity' });
      setHasConsent(true);
      setError(null);
    } catch (err) {
      setError(`Error requesting consent: ${err}`);
    }
  };

  const handleStartMonitoring = async () => {
    if (!hasConsent) {
      setError('OS Activity consent required');
      return;
    }

    try {
      await invoke('start_os_monitoring', { sessionId });
      setIsMonitoring(true);
      setError(null);

      // Initial fetch of running apps and current app
      const apps = await invoke<AppInfo[]>('get_running_applications');
      setRunningApps(apps);

      const current = await invoke<AppInfo | null>('get_current_application');
      setCurrentApp(current);
    } catch (err) {
      setError(`Error starting monitoring: ${err}`);
    }
  };

  const handleStopMonitoring = async () => {
    try {
      await invoke('stop_os_monitoring');
      setIsMonitoring(false);
      setRunningApps([]);
      setCurrentApp(null);
      setError(null);
    } catch (err) {
      setError(`Error stopping monitoring: ${err}`);
    }
  };

  return (
    <div className="p-4 border border-gray-300 dark:border-gray-700 rounded-lg">
      <h2 className="text-xl font-semibold mb-4">OS Activity Monitor</h2>

      {error && (
        <div className="mb-4 p-3 bg-red-100 dark:bg-red-900 text-red-900 dark:text-red-100 rounded">
          {error}
        </div>
      )}

      {!hasConsent ? (
        <div className="mb-4">
          <p className="text-gray-700 dark:text-gray-300 mb-2">
            OS Activity monitoring requires consent to track running applications and focus time.
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
              onClick={isMonitoring ? handleStopMonitoring : handleStartMonitoring}
              className={`px-4 py-2 rounded ${
                isMonitoring
                  ? 'bg-red-500 hover:bg-red-600 text-white'
                  : 'bg-green-500 hover:bg-green-600 text-white'
              }`}
            >
              {isMonitoring ? 'Stop Monitoring' : 'Start Monitoring'}
            </button>
            <span className="ml-3 text-sm text-gray-600 dark:text-gray-400">
              Status: {isMonitoring ? 'Recording' : 'Stopped'}
            </span>
          </div>

          {isMonitoring && (
            <>
              {currentApp && (
                <div className="mb-4 p-3 bg-blue-50 dark:bg-blue-900 rounded">
                  <h3 className="font-semibold mb-2">Current Focused App</h3>
                  <div className="text-sm">
                    <div className="font-medium">{currentApp.name}</div>
                    <div className="text-gray-600 dark:text-gray-400">
                      {currentApp.bundle_id}
                    </div>
                    <div className="text-gray-500 dark:text-gray-500 text-xs">
                      PID: {currentApp.process_id}
                    </div>
                  </div>
                </div>
              )}

              <div className="mt-4">
                <h3 className="font-semibold mb-2">
                  Running Applications ({runningApps.length})
                </h3>
                <div className="max-h-96 overflow-y-auto border border-gray-200 dark:border-gray-700 rounded">
                  <table className="w-full text-sm">
                    <thead className="bg-gray-100 dark:bg-gray-800 sticky top-0">
                      <tr>
                        <th className="text-left p-2">Name</th>
                        <th className="text-left p-2">Bundle ID</th>
                        <th className="text-left p-2">PID</th>
                      </tr>
                    </thead>
                    <tbody>
                      {runningApps.map((app) => (
                        <tr
                          key={app.process_id}
                          className={`border-t border-gray-200 dark:border-gray-700 ${
                            currentApp?.process_id === app.process_id
                              ? 'bg-blue-50 dark:bg-blue-900'
                              : ''
                          }`}
                        >
                          <td className="p-2">{app.name}</td>
                          <td className="p-2 text-gray-600 dark:text-gray-400">
                            {app.bundle_id}
                          </td>
                          <td className="p-2 text-gray-500 dark:text-gray-500">
                            {app.process_id}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            </>
          )}
        </>
      )}
    </div>
  );
}
