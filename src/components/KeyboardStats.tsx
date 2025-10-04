import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface KeyboardStats {
  session_id: string;
  total_keystrokes: number;
  keys_per_minute: number;
  most_used_keys: [string, number][];
  shortcut_usage: [string, number][];
  typing_speed_wpm: number | null;
}

interface KeyboardStatsProps {
  sessionId: string;
}

export default function KeyboardStats({ sessionId }: KeyboardStatsProps) {
  const [stats, setStats] = useState<KeyboardStats | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    loadStats();
  }, [sessionId]);

  const loadStats = async () => {
    setLoading(true);
    setError(null);

    try {
      const data = await invoke<KeyboardStats>('get_keyboard_stats', { sessionId });
      setStats(data);
    } catch (err) {
      setError(`Error loading keyboard stats: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  if (loading) {
    return (
      <div className="p-4 border border-gray-300 dark:border-gray-700 rounded-lg">
        <div className="flex items-center justify-center py-8">
          <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-500"></div>
          <span className="ml-3 text-gray-600 dark:text-gray-400">Loading statistics...</span>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-4 border border-gray-300 dark:border-gray-700 rounded-lg">
        <div className="p-3 bg-red-100 dark:bg-red-900 text-red-900 dark:text-red-100 rounded">
          {error}
        </div>
        <button
          onClick={loadStats}
          className="mt-3 px-4 py-2 bg-blue-500 text-white rounded hover:bg-blue-600"
        >
          Retry
        </button>
      </div>
    );
  }

  if (!stats || stats.total_keystrokes === 0) {
    return (
      <div className="p-4 border border-gray-300 dark:border-gray-700 rounded-lg">
        <h2 className="text-xl font-semibold mb-4">Keyboard Statistics</h2>
        <div className="text-center py-8 text-gray-500 dark:text-gray-400">
          No keyboard data available for this session yet.
          <br />
          Start keyboard recording to collect statistics.
        </div>
      </div>
    );
  }

  return (
    <div className="p-4 border border-gray-300 dark:border-gray-700 rounded-lg">
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-xl font-semibold">Keyboard Statistics</h2>
        <button
          onClick={loadStats}
          className="px-3 py-1 text-sm bg-gray-200 dark:bg-gray-700 rounded hover:bg-gray-300 dark:hover:bg-gray-600"
          title="Refresh statistics"
        >
          ↻ Refresh
        </button>
      </div>

      {/* Overview Metrics */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4 mb-6">
        <div className="p-4 bg-blue-50 dark:bg-blue-900 rounded-lg">
          <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Total Keystrokes</div>
          <div className="text-2xl font-bold text-blue-700 dark:text-blue-300">
            {stats.total_keystrokes.toLocaleString()}
          </div>
        </div>

        <div className="p-4 bg-green-50 dark:bg-green-900 rounded-lg">
          <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Keys per Minute</div>
          <div className="text-2xl font-bold text-green-700 dark:text-green-300">
            {stats.keys_per_minute.toFixed(1)}
          </div>
        </div>

        <div className="p-4 bg-purple-50 dark:bg-purple-900 rounded-lg">
          <div className="text-sm text-gray-600 dark:text-gray-400 mb-1">Typing Speed</div>
          <div className="text-2xl font-bold text-purple-700 dark:text-purple-300">
            {stats.typing_speed_wpm ? `${stats.typing_speed_wpm} WPM` : 'N/A'}
          </div>
        </div>
      </div>

      {/* Most Used Keys */}
      {stats.most_used_keys.length > 0 && (
        <div className="mb-6">
          <h3 className="text-lg font-semibold mb-3">Most Used Keys</h3>
          <div className="space-y-2">
            {stats.most_used_keys.map(([key, count], index) => {
              const maxCount = stats.most_used_keys[0][1];
              const percentage = (count / maxCount) * 100;

              return (
                <div key={index} className="flex items-center">
                  <div className="w-12 text-sm font-mono text-gray-700 dark:text-gray-300">
                    {key === ' ' ? '⎵ Space' : key}
                  </div>
                  <div className="flex-1 mx-3 bg-gray-200 dark:bg-gray-700 rounded-full h-6 overflow-hidden">
                    <div
                      className="bg-blue-500 h-full flex items-center justify-end px-2 transition-all"
                      style={{ width: `${percentage}%` }}
                    >
                      <span className="text-xs text-white font-semibold">
                        {count}
                      </span>
                    </div>
                  </div>
                  <div className="w-16 text-right text-sm text-gray-600 dark:text-gray-400">
                    {percentage.toFixed(0)}%
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* Shortcut Usage */}
      {stats.shortcut_usage.length > 0 && (
        <div>
          <h3 className="text-lg font-semibold mb-3">Top Keyboard Shortcuts</h3>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
            {stats.shortcut_usage.map(([shortcut, count], index) => (
              <div
                key={index}
                className="p-3 bg-gray-50 dark:bg-gray-800 rounded-lg flex items-center justify-between"
              >
                <div className="flex items-center">
                  <div className="w-8 h-8 bg-blue-100 dark:bg-blue-900 text-blue-700 dark:text-blue-300 rounded flex items-center justify-center font-semibold text-sm mr-3">
                    #{index + 1}
                  </div>
                  <div>
                    <div className="font-mono text-sm font-semibold text-gray-800 dark:text-gray-200">
                      {shortcut}
                    </div>
                  </div>
                </div>
                <div className="text-right">
                  <div className="text-lg font-bold text-gray-700 dark:text-gray-300">
                    {count}
                  </div>
                  <div className="text-xs text-gray-500 dark:text-gray-400">uses</div>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      <div className="mt-4 text-xs text-gray-500 dark:text-gray-400">
        Session ID: {stats.session_id}
      </div>
    </div>
  );
}
