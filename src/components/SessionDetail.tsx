import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface SessionMetrics {
  total_duration_ms: number;
  active_duration_ms: number;
  idle_duration_ms: number;
  app_switches: number;
  unique_apps: number;
  most_used_app: string;
  productivity_score: number;
}

interface Session {
  id: string;
  start_timestamp: number;
  end_timestamp: number | null;
  session_type: string | null;
  device_id: string;
}

interface AppUsageStats {
  app_name: string;
  bundle_id: string;
  total_focus_duration_ms: number;
  total_background_duration_ms: number;
  launch_count: number;
  first_launch: number;
  last_terminate: number | null;
}

interface SessionDetailProps {
  sessionId: string;
  onClose?: () => void;
}

export default function SessionDetail({ sessionId, onClose }: SessionDetailProps) {
  const [metrics, setMetrics] = useState<SessionMetrics | null>(null);
  const [sessionType, setSessionType] = useState<string>('unknown');
  const [appStats, setAppStats] = useState<AppUsageStats[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    loadSessionDetails();
  }, [sessionId]);

  const loadSessionDetails = async () => {
    try {
      setLoading(true);
      setError(null);

      // Load metrics, session type, and app usage in parallel
      const [metricsData, typeData, statsData] = await Promise.all([
        invoke<SessionMetrics>('get_session_metrics', { sessionId }),
        invoke<string>('classify_session', { sessionId }),
        invoke<AppUsageStats[]>('get_app_usage_stats', { sessionId }),
      ]);

      setMetrics(metricsData);
      setSessionType(typeData);
      setAppStats(statsData);
    } catch (err) {
      setError(`Error loading session details: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  const formatDuration = (ms: number): string => {
    const seconds = Math.floor(ms / 1000);
    const minutes = Math.floor(seconds / 60);
    const hours = Math.floor(minutes / 60);

    if (hours > 0) {
      return `${hours}h ${minutes % 60}m`;
    } else if (minutes > 0) {
      return `${minutes}m ${seconds % 60}s`;
    } else {
      return `${seconds}s`;
    }
  };

  const formatPercentage = (value: number): string => {
    return `${(value * 100).toFixed(0)}%`;
  };

  const getProductivityColor = (score: number): string => {
    if (score >= 0.7) return 'text-green-600 dark:text-green-400';
    if (score >= 0.4) return 'text-yellow-600 dark:text-yellow-400';
    return 'text-red-600 dark:text-red-400';
  };

  const getSessionTypeColor = (type: string): string => {
    switch (type.toLowerCase()) {
      case 'development':
        return 'bg-blue-500';
      case 'communication':
        return 'bg-green-500';
      case 'research':
        return 'bg-purple-500';
      case 'entertainment':
        return 'bg-pink-500';
      case 'work':
        return 'bg-yellow-500';
      default:
        return 'bg-gray-500';
    }
  };

  if (loading) {
    return (
      <div className="p-4 border border-gray-300 dark:border-gray-700 rounded-lg">
        <h2 className="text-xl font-semibold mb-4">Session Details</h2>
        <div className="text-gray-600 dark:text-gray-400">Loading session details...</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-4 border border-gray-300 dark:border-gray-700 rounded-lg">
        <h2 className="text-xl font-semibold mb-4">Session Details</h2>
        <div className="p-3 bg-red-100 dark:bg-red-900 text-red-900 dark:text-red-100 rounded">
          {error}
        </div>
      </div>
    );
  }

  if (!metrics) {
    return null;
  }

  const activePercentage = (metrics.active_duration_ms / metrics.total_duration_ms) * 100;
  const idlePercentage = (metrics.idle_duration_ms / metrics.total_duration_ms) * 100;

  return (
    <div className="p-4 border border-gray-300 dark:border-gray-700 rounded-lg">
      <div className="flex justify-between items-center mb-4">
        <h2 className="text-xl font-semibold">Session Details</h2>
        {onClose && (
          <button
            onClick={onClose}
            className="px-3 py-1 bg-gray-500 text-white rounded hover:bg-gray-600 text-sm"
          >
            Close
          </button>
        )}
      </div>

      {/* Session Type Badge */}
      <div className="mb-4">
        <span className={`px-3 py-1 ${getSessionTypeColor(sessionType)} text-white rounded`}>
          {sessionType.toUpperCase()}
        </span>
        <span className="ml-3 text-sm text-gray-600 dark:text-gray-400">
          Session ID: {sessionId.substring(0, 16)}...
        </span>
      </div>

      {/* Metrics Grid */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-4 mb-6">
        <div className="p-3 bg-gray-50 dark:bg-gray-800 rounded">
          <div className="text-xs text-gray-600 dark:text-gray-400 mb-1">Total Duration</div>
          <div className="text-lg font-semibold">{formatDuration(metrics.total_duration_ms)}</div>
        </div>

        <div className="p-3 bg-gray-50 dark:bg-gray-800 rounded">
          <div className="text-xs text-gray-600 dark:text-gray-400 mb-1">Active Time</div>
          <div className="text-lg font-semibold">{formatDuration(metrics.active_duration_ms)}</div>
          <div className="text-xs text-gray-500">{activePercentage.toFixed(0)}%</div>
        </div>

        <div className="p-3 bg-gray-50 dark:bg-gray-800 rounded">
          <div className="text-xs text-gray-600 dark:text-gray-400 mb-1">Idle Time</div>
          <div className="text-lg font-semibold">{formatDuration(metrics.idle_duration_ms)}</div>
          <div className="text-xs text-gray-500">{idlePercentage.toFixed(0)}%</div>
        </div>

        <div className="p-3 bg-gray-50 dark:bg-gray-800 rounded">
          <div className="text-xs text-gray-600 dark:text-gray-400 mb-1">Productivity</div>
          <div className={`text-lg font-semibold ${getProductivityColor(metrics.productivity_score)}`}>
            {formatPercentage(metrics.productivity_score)}
          </div>
        </div>

        <div className="p-3 bg-gray-50 dark:bg-gray-800 rounded">
          <div className="text-xs text-gray-600 dark:text-gray-400 mb-1">App Switches</div>
          <div className="text-lg font-semibold">{metrics.app_switches}</div>
        </div>

        <div className="p-3 bg-gray-50 dark:bg-gray-800 rounded">
          <div className="text-xs text-gray-600 dark:text-gray-400 mb-1">Unique Apps</div>
          <div className="text-lg font-semibold">{metrics.unique_apps}</div>
        </div>

        <div className="p-3 bg-gray-50 dark:bg-gray-800 rounded col-span-2">
          <div className="text-xs text-gray-600 dark:text-gray-400 mb-1">Most Used App</div>
          <div className="text-lg font-semibold truncate">{metrics.most_used_app}</div>
        </div>
      </div>

      {/* Active/Idle Duration Bar */}
      <div className="mb-6">
        <h3 className="font-semibold mb-2">Time Distribution</h3>
        <div className="w-full bg-gray-200 dark:bg-gray-700 rounded-full h-4 flex overflow-hidden">
          <div
            className="bg-green-500"
            style={{ width: `${activePercentage}%` }}
            title={`Active: ${activePercentage.toFixed(1)}%`}
          />
          <div
            className="bg-gray-400 dark:bg-gray-600"
            style={{ width: `${idlePercentage}%` }}
            title={`Idle: ${idlePercentage.toFixed(1)}%`}
          />
        </div>
        <div className="flex justify-between text-xs text-gray-600 dark:text-gray-400 mt-1">
          <span>Active: {activePercentage.toFixed(1)}%</span>
          <span>Idle: {idlePercentage.toFixed(1)}%</span>
        </div>
      </div>

      {/* Application Usage */}
      {appStats.length > 0 && (
        <div>
          <h3 className="font-semibold mb-2">Application Usage</h3>
          <div className="overflow-x-auto border border-gray-200 dark:border-gray-700 rounded">
            <table className="w-full text-sm">
              <thead className="bg-gray-100 dark:bg-gray-800">
                <tr>
                  <th className="text-left p-2">App Name</th>
                  <th className="text-left p-2">Focus Time</th>
                  <th className="text-left p-2">Launches</th>
                </tr>
              </thead>
              <tbody>
                {appStats.map((app) => (
                  <tr key={app.bundle_id} className="border-t border-gray-200 dark:border-gray-700">
                    <td className="p-2">
                      <div className="font-medium">{app.app_name}</div>
                      <div className="text-xs text-gray-600 dark:text-gray-400">{app.bundle_id}</div>
                    </td>
                    <td className="p-2">{formatDuration(app.total_focus_duration_ms)}</td>
                    <td className="p-2">{app.launch_count}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}
