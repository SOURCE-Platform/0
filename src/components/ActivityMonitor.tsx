import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from "@/components/ui/button";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";

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
    <div className="p-4 border border-border rounded-lg">
      <h2 className="text-xl font-medium mb-4">OS Activity Monitor</h2>

      {error && (
        <Alert variant="destructive" className="mb-4">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      {!hasConsent ? (
        <div className="mb-4">
          <p className="text-muted-foreground mb-2">
            OS Activity monitoring requires consent to track running applications and focus time.
          </p>
          <Button onClick={requestConsent}>Grant Consent</Button>
        </div>
      ) : (
        <>
          <div className="mb-4 flex items-center gap-3">
            <Button
              onClick={isMonitoring ? handleStopMonitoring : handleStartMonitoring}
              variant={isMonitoring ? "destructive" : "default"}
            >
              {isMonitoring ? 'Stop Monitoring' : 'Start Monitoring'}
            </Button>
            <span className="text-sm text-muted-foreground">
              Status: {isMonitoring ? 'Recording' : 'Stopped'}
            </span>
          </div>

          {isMonitoring && (
            <>
              {currentApp && (
                <div className="mb-4 p-3 bg-primary/5 border border-primary/20 rounded-lg">
                  <h3 className="font-medium mb-2">Current Focused App</h3>
                  <div className="text-sm">
                    <div className="font-medium">{currentApp.name}</div>
                    <div className="text-muted-foreground">{currentApp.bundle_id}</div>
                    <div className="text-muted-foreground text-xs">PID: {currentApp.process_id}</div>
                  </div>
                </div>
              )}

              <div className="mt-4">
                <h3 className="font-medium mb-2">
                  Running Applications ({runningApps.length})
                </h3>
                <div className="max-h-96 overflow-y-auto border border-border rounded-lg">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Name</TableHead>
                        <TableHead>Bundle ID</TableHead>
                        <TableHead>PID</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {runningApps.map((app) => (
                        <TableRow
                          key={app.process_id}
                          className={currentApp?.process_id === app.process_id ? 'bg-primary/5' : ''}
                        >
                          <TableCell>{app.name}</TableCell>
                          <TableCell className="text-muted-foreground">{app.bundle_id}</TableCell>
                          <TableCell className="text-muted-foreground">{app.process_id}</TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
              </div>
            </>
          )}
        </>
      )}
    </div>
  );
}
