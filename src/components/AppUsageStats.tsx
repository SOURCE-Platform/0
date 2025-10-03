import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface AppUsageStats {
  app_name: string;
  bundle_id: string;
  total_focus_duration_ms: number;
  total_background_duration_ms: number;
  launch_count: number;
  first_launch: number;
  last_terminate: number | null;
}

interface AppUsageStatsProps {
  sessionId: string;
  autoRefresh?: boolean;
  refreshInterval?: number; // milliseconds
}

export default function AppUsageStats({
  sessionId,
  autoRefresh = false,
  refreshInterval = 5000,
}: AppUsageStatsProps) {
  const [stats, setStats] = useState<AppUsageStats[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [searchTerm, setSearchTerm] = useState('');
  const [sortBy, setSortBy] = useState<'focus' | 'launches' | 'name'>('focus');

  useEffect(() => {
    loadStats();

    if (autoRefresh) {
      const interval = setInterval(loadStats, refreshInterval);
      return () => clearInterval(interval);
    }
  }, [sessionId, autoRefresh, refreshInterval]);

  const loadStats = async () => {
    try {
      setLoading(true);
      const data = await invoke<AppUsageStats[]>('get_app_usage_stats', {
        sessionId,
      });
      setStats(data);
      setError(null);
    } catch (err) {
      setError(`Error loading stats: ${err}`);
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

  const formatTimestamp = (timestamp: number): string => {
    return new Date(timestamp).toLocaleString();
  };

  const filteredStats = stats
    .filter(
      (stat) =>
        stat.app_name.toLowerCase().includes(searchTerm.toLowerCase()) ||
        stat.bundle_id.toLowerCase().includes(searchTerm.toLowerCase())
    )
    .sort((a, b) => {
      switch (sortBy) {
        case 'focus':
          return b.total_focus_duration_ms - a.total_focus_duration_ms;
        case 'launches':
          return b.launch_count - a.launch_count;
        case 'name':
          return a.app_name.localeCompare(b.app_name);
        default:
          return 0;
      }
    });

  const maxFocusDuration = Math.max(
    ...filteredStats.map((s) => s.total_focus_duration_ms),
    1
  );

  if (loading && stats.length === 0) {
    return (
      <div className="p-4 border border-gray-300 dark:border-gray-700 rounded-lg">
        <h2 className="text-xl font-semibold mb-4">App Usage Statistics</h2>
        <div className="text-gray-600 dark:text-gray-400">Loading...</div>
      </div>
    );
  }

  return (
    <div className="p-4 border border-gray-300 dark:border-gray-700 rounded-lg">
      <div className="flex justify-between items-center mb-4">
        <h2 className="text-xl font-semibold">App Usage Statistics</h2>
        <button
          onClick={loadStats}
          className="px-3 py-1 bg-blue-500 text-white rounded hover:bg-blue-600 text-sm"
        >
          Refresh
        </button>
      </div>

      {error && (
        <div className="mb-4 p-3 bg-red-100 dark:bg-red-900 text-red-900 dark:text-red-100 rounded">
          {error}
        </div>
      )}

      <div className="mb-4 flex gap-4">
        <input
          type="text"
          placeholder="Search by app name or bundle ID..."
          value={searchTerm}
          onChange={(e) => setSearchTerm(e.target.value)}
          className="flex-1 px-3 py-2 border border-gray-300 dark:border-gray-600 rounded bg-white dark:bg-gray-800"
        />
        <select
          value={sortBy}
          onChange={(e) => setSortBy(e.target.value as 'focus' | 'launches' | 'name')}
          className="px-3 py-2 border border-gray-300 dark:border-gray-600 rounded bg-white dark:bg-gray-800"
        >
          <option value="focus">Sort by Focus Time</option>
          <option value="launches">Sort by Launch Count</option>
          <option value="name">Sort by Name</option>
        </select>
      </div>

      {filteredStats.length === 0 ? (
        <div className="text-gray-600 dark:text-gray-400">
          {stats.length === 0
            ? 'No usage data available. Start monitoring to see app usage statistics.'
            : 'No apps match your search.'}
        </div>
      ) : (
        <div className="space-y-4">
          {/* Bar Chart */}
          <div className="space-y-2">
            {filteredStats.slice(0, 10).map((stat) => {
              const percentage =
                (stat.total_focus_duration_ms / maxFocusDuration) * 100;
              return (
                <div key={stat.bundle_id}>
                  <div className="flex justify-between text-sm mb-1">
                    <span className="font-medium">{stat.app_name}</span>
                    <span className="text-gray-600 dark:text-gray-400">
                      {formatDuration(stat.total_focus_duration_ms)}
                    </span>
                  </div>
                  <div className="w-full bg-gray-200 dark:bg-gray-700 rounded-full h-2">
                    <div
                      className="bg-blue-500 h-2 rounded-full transition-all duration-300"
                      style={{ width: `${percentage}%` }}
                    />
                  </div>
                </div>
              );
            })}
          </div>

          {/* Detailed Table */}
          <div className="mt-6">
            <h3 className="font-semibold mb-2">Detailed Statistics</h3>
            <div className="overflow-x-auto border border-gray-200 dark:border-gray-700 rounded">
              <table className="w-full text-sm">
                <thead className="bg-gray-100 dark:bg-gray-800">
                  <tr>
                    <th className="text-left p-2">App Name</th>
                    <th className="text-left p-2">Focus Time</th>
                    <th className="text-left p-2">Background Time</th>
                    <th className="text-left p-2">Launches</th>
                    <th className="text-left p-2">First Launch</th>
                  </tr>
                </thead>
                <tbody>
                  {filteredStats.map((stat) => (
                    <tr
                      key={stat.bundle_id}
                      className="border-t border-gray-200 dark:border-gray-700"
                    >
                      <td className="p-2">
                        <div className="font-medium">{stat.app_name}</div>
                        <div className="text-xs text-gray-600 dark:text-gray-400">
                          {stat.bundle_id}
                        </div>
                      </td>
                      <td className="p-2">
                        {formatDuration(stat.total_focus_duration_ms)}
                      </td>
                      <td className="p-2">
                        {formatDuration(stat.total_background_duration_ms)}
                      </td>
                      <td className="p-2">{stat.launch_count}</td>
                      <td className="p-2 text-xs text-gray-600 dark:text-gray-400">
                        {formatTimestamp(stat.first_launch)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
