import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface Session {
  id: string;
  start_timestamp: number;
  end_timestamp: number | null;
  session_type: string | null;
  device_id: string;
}

interface SessionWithMetrics extends Session {
  duration_hours: number;
  is_active: boolean;
}

interface SessionListProps {
  onSelectSession?: (sessionId: string) => void;
}

export default function SessionList({ onSelectSession }: SessionListProps) {
  const [sessions, setSessions] = useState<SessionWithMetrics[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filterType, setFilterType] = useState<string>('all');
  const [searchTerm, setSearchTerm] = useState('');
  const [dateRange, setDateRange] = useState<'day' | 'week' | 'month' | 'all'>('week');

  useEffect(() => {
    loadSessions();
  }, [dateRange]);

  const loadSessions = async () => {
    try {
      setLoading(true);
      setError(null);

      const now = Date.now();
      let start = 0;

      switch (dateRange) {
        case 'day':
          start = now - 24 * 60 * 60 * 1000;
          break;
        case 'week':
          start = now - 7 * 24 * 60 * 60 * 1000;
          break;
        case 'month':
          start = now - 30 * 24 * 60 * 60 * 1000;
          break;
        case 'all':
        default:
          start = 0;
      }

      const fetchedSessions = await invoke<Session[]>('get_session_history', {
        start,
        end: now,
      });

      // Enhance sessions with computed properties
      const enhanced = fetchedSessions.map((session) => {
        const startTime = session.start_timestamp;
        const endTime = session.end_timestamp || Date.now();
        const duration_ms = endTime - startTime;
        const duration_hours = duration_ms / (1000 * 60 * 60);

        return {
          ...session,
          duration_hours,
          is_active: session.end_timestamp === null,
        };
      });

      setSessions(enhanced);
    } catch (err) {
      setError(`Error loading sessions: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  const formatDate = (timestamp: number): string => {
    return new Date(timestamp).toLocaleDateString('en-US', {
      month: 'short',
      day: 'numeric',
      year: 'numeric',
    });
  };

  const formatTime = (timestamp: number): string => {
    return new Date(timestamp).toLocaleTimeString('en-US', {
      hour: '2-digit',
      minute: '2-digit',
    });
  };

  const formatDuration = (hours: number): string => {
    if (hours < 1) {
      const minutes = Math.floor(hours * 60);
      return `${minutes}m`;
    }
    const h = Math.floor(hours);
    const m = Math.floor((hours - h) * 60);
    return m > 0 ? `${h}h ${m}m` : `${h}h`;
  };

  const getSessionTypeColor = (type: string | null): string => {
    if (!type) return 'bg-gray-500';

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

  const filteredSessions = sessions
    .filter((session) => {
      if (filterType !== 'all' && session.session_type !== filterType) {
        return false;
      }
      if (searchTerm && !session.id.toLowerCase().includes(searchTerm.toLowerCase())) {
        return false;
      }
      return true;
    })
    .sort((a, b) => b.start_timestamp - a.start_timestamp);

  if (loading && sessions.length === 0) {
    return (
      <div className="p-4 border border-gray-300 dark:border-gray-700 rounded-lg">
        <h2 className="text-xl font-semibold mb-4">Session History</h2>
        <div className="text-gray-600 dark:text-gray-400">Loading sessions...</div>
      </div>
    );
  }

  return (
    <div className="p-4 border border-gray-300 dark:border-gray-700 rounded-lg">
      <div className="flex justify-between items-center mb-4">
        <h2 className="text-xl font-semibold">Session History</h2>
        <button
          onClick={loadSessions}
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

      {/* Filters */}
      <div className="mb-4 flex gap-4">
        <div className="flex-1">
          <input
            type="text"
            placeholder="Search by session ID..."
            value={searchTerm}
            onChange={(e) => setSearchTerm(e.target.value)}
            className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded bg-white dark:bg-gray-800"
          />
        </div>

        <select
          value={filterType}
          onChange={(e) => setFilterType(e.target.value)}
          className="px-3 py-2 border border-gray-300 dark:border-gray-600 rounded bg-white dark:bg-gray-800"
        >
          <option value="all">All Types</option>
          <option value="development">Development</option>
          <option value="communication">Communication</option>
          <option value="research">Research</option>
          <option value="entertainment">Entertainment</option>
          <option value="work">Work</option>
          <option value="unknown">Unknown</option>
        </select>

        <select
          value={dateRange}
          onChange={(e) => setDateRange(e.target.value as any)}
          className="px-3 py-2 border border-gray-300 dark:border-gray-600 rounded bg-white dark:bg-gray-800"
        >
          <option value="day">Last 24 Hours</option>
          <option value="week">Last Week</option>
          <option value="month">Last Month</option>
          <option value="all">All Time</option>
        </select>
      </div>

      {/* Sessions List */}
      {filteredSessions.length === 0 ? (
        <div className="text-gray-600 dark:text-gray-400">
          {sessions.length === 0
            ? 'No sessions found. Start monitoring to create sessions.'
            : 'No sessions match your filters.'}
        </div>
      ) : (
        <div className="space-y-2">
          {filteredSessions.map((session) => (
            <div
              key={session.id}
              onClick={() => onSelectSession?.(session.id)}
              className={`p-3 border rounded cursor-pointer transition-colors ${
                session.is_active
                  ? 'border-blue-500 bg-blue-50 dark:bg-blue-900'
                  : 'border-gray-300 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-800'
              }`}
            >
              <div className="flex items-center justify-between">
                <div className="flex-1">
                  <div className="flex items-center gap-2">
                    {session.is_active && (
                      <span className="px-2 py-1 bg-green-500 text-white text-xs rounded">
                        ACTIVE
                      </span>
                    )}
                    {session.session_type && (
                      <span
                        className={`px-2 py-1 ${getSessionTypeColor(
                          session.session_type
                        )} text-white text-xs rounded`}
                      >
                        {session.session_type.toUpperCase()}
                      </span>
                    )}
                    <span className="text-sm text-gray-600 dark:text-gray-400">
                      {formatDate(session.start_timestamp)} at {formatTime(session.start_timestamp)}
                    </span>
                  </div>
                  <div className="mt-1 text-xs text-gray-500 dark:text-gray-500">
                    ID: {session.id.substring(0, 8)}...
                  </div>
                </div>

                <div className="text-right">
                  <div className="font-semibold">{formatDuration(session.duration_hours)}</div>
                  <div className="text-xs text-gray-500 dark:text-gray-500">
                    {session.device_id}
                  </div>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
