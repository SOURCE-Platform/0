import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface CommandStats {
  most_used_shortcuts: [string, number][];
  shortcuts_by_app: Record<string, [string, number][]>;
  total_shortcuts: number;
  unique_shortcuts: number;
}

interface CommandStatsProps {
  sessionId?: string;
}

export const CommandStats: React.FC<CommandStatsProps> = ({ sessionId }) => {
  const [stats, setStats] = useState<CommandStats | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selectedApp, setSelectedApp] = useState<string | null>(null);

  useEffect(() => {
    loadStats();
  }, [sessionId]);

  const loadStats = async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<CommandStats>('get_command_stats', {
        sessionId: sessionId || null,
      });
      setStats(result);
    } catch (err) {
      setError(err as string);
    } finally {
      setLoading(false);
    }
  };

  if (loading) {
    return <div className="text-gray-400">Loading command statistics...</div>;
  }

  if (error) {
    return <div className="text-red-400">Error: {error}</div>;
  }

  if (!stats || stats.total_shortcuts === 0) {
    return (
      <div className="text-gray-400">
        No keyboard shortcuts detected yet. Start typing commands to see statistics.
      </div>
    );
  }

  const displayedShortcuts = selectedApp
    ? stats.shortcuts_by_app[selectedApp] || []
    : stats.most_used_shortcuts;

  return (
    <div className="space-y-6">
      {/* Summary Stats */}
      <div className="grid grid-cols-2 gap-4">
        <div className="bg-gray-800 rounded-lg p-4">
          <div className="text-gray-400 text-sm">Total Shortcuts</div>
          <div className="text-2xl font-bold text-white">{stats.total_shortcuts}</div>
        </div>
        <div className="bg-gray-800 rounded-lg p-4">
          <div className="text-gray-400 text-sm">Unique Shortcuts</div>
          <div className="text-2xl font-bold text-white">{stats.unique_shortcuts}</div>
        </div>
      </div>

      {/* App Filter */}
      <div className="space-y-2">
        <label className="text-sm text-gray-400">Filter by Application</label>
        <div className="flex gap-2 flex-wrap">
          <button
            onClick={() => setSelectedApp(null)}
            className={`px-3 py-1.5 rounded text-sm transition-colors ${
              selectedApp === null
                ? 'bg-blue-600 text-white'
                : 'bg-gray-800 text-gray-300 hover:bg-gray-700'
            }`}
          >
            All Apps
          </button>
          {Object.keys(stats.shortcuts_by_app).map((app) => (
            <button
              key={app}
              onClick={() => setSelectedApp(app)}
              className={`px-3 py-1.5 rounded text-sm transition-colors ${
                selectedApp === app
                  ? 'bg-blue-600 text-white'
                  : 'bg-gray-800 text-gray-300 hover:bg-gray-700'
              }`}
            >
              {app}
            </button>
          ))}
        </div>
      </div>

      {/* Top Shortcuts */}
      <div className="space-y-2">
        <h3 className="text-lg font-semibold text-white">
          {selectedApp ? `Top Shortcuts in ${selectedApp}` : 'Top Shortcuts'}
        </h3>
        <div className="space-y-2">
          {displayedShortcuts.slice(0, 20).map(([shortcut, count], index) => {
            const maxCount = displayedShortcuts[0]?.[1] || 1;
            const percentage = (count / maxCount) * 100;

            return (
              <div key={shortcut} className="bg-gray-800 rounded-lg p-3">
                <div className="flex items-center justify-between mb-2">
                  <div className="flex items-center gap-3">
                    <span className="text-gray-500 text-sm font-mono w-6">
                      #{index + 1}
                    </span>
                    <span className="font-mono text-white bg-gray-700 px-2 py-1 rounded text-sm">
                      {shortcut}
                    </span>
                  </div>
                  <span className="text-gray-300 font-semibold">{count}</span>
                </div>
                <div className="w-full bg-gray-700 rounded-full h-2">
                  <div
                    className="bg-blue-600 h-2 rounded-full transition-all"
                    style={{ width: `${percentage}%` }}
                  />
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {/* Shortcuts by App (only show if no filter is active) */}
      {!selectedApp && Object.keys(stats.shortcuts_by_app).length > 0 && (
        <div className="space-y-3">
          <h3 className="text-lg font-semibold text-white">Shortcuts by Application</h3>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {Object.entries(stats.shortcuts_by_app).map(([app, shortcuts]) => {
              const totalForApp = shortcuts.reduce((sum, [, count]) => sum + count, 0);
              const topShortcut = shortcuts[0];

              return (
                <div key={app} className="bg-gray-800 rounded-lg p-4">
                  <div className="text-white font-semibold mb-2">{app}</div>
                  <div className="text-gray-400 text-sm mb-2">
                    {totalForApp} shortcut{totalForApp !== 1 ? 's' : ''} used
                  </div>
                  {topShortcut && (
                    <div className="flex items-center gap-2">
                      <span className="text-xs text-gray-500">Most used:</span>
                      <span className="font-mono text-white bg-gray-700 px-2 py-0.5 rounded text-xs">
                        {topShortcut[0]}
                      </span>
                      <span className="text-gray-400 text-xs">({topShortcut[1]}x)</span>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* Refresh Button */}
      <div className="flex justify-end">
        <button
          onClick={loadStats}
          className="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded transition-colors"
        >
          Refresh Stats
        </button>
      </div>
    </div>
  );
};
