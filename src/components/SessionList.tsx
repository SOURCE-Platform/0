import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Card, CardContent } from "@/components/ui/card";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";

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
    if (!type) return '#6b7280';

    switch (type.toLowerCase()) {
      case 'development':
        return '#3b82f6';
      case 'communication':
        return '#22c55e';
      case 'research':
        return '#a855f7';
      case 'entertainment':
        return '#ec4899';
      case 'work':
        return '#eab308';
      default:
        return '#6b7280';
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
      <Card>
        <CardContent className="p-4">
          <h2 className="text-xl font-semibold mb-4">Session History</h2>
          <p className="text-muted-foreground">Loading sessions...</p>
        </CardContent>
      </Card>
    );
  }

  return (
    <div className="p-4 border border-border rounded-lg">
      <div className="flex justify-between items-center mb-4">
        <h2 className="text-xl font-semibold">Session History</h2>
        <Button size="sm" onClick={loadSessions}>
          Refresh
        </Button>
      </div>

      {error && (
        <Alert variant="destructive" className="mb-4">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      {/* Filters */}
      <div className="mb-4 flex gap-4">
        <div className="flex-1">
          <Input
            type="text"
            placeholder="Search by session ID..."
            value={searchTerm}
            onChange={(e) => setSearchTerm(e.target.value)}
          />
        </div>

        <Select value={filterType} onValueChange={setFilterType}>
          <SelectTrigger className="w-40">
            <SelectValue placeholder="All Types" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All Types</SelectItem>
            <SelectItem value="development">Development</SelectItem>
            <SelectItem value="communication">Communication</SelectItem>
            <SelectItem value="research">Research</SelectItem>
            <SelectItem value="entertainment">Entertainment</SelectItem>
            <SelectItem value="work">Work</SelectItem>
            <SelectItem value="unknown">Unknown</SelectItem>
          </SelectContent>
        </Select>

        <Select value={dateRange} onValueChange={(v) => setDateRange(v as typeof dateRange)}>
          <SelectTrigger className="w-40">
            <SelectValue placeholder="Last Week" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="day">Last 24 Hours</SelectItem>
            <SelectItem value="week">Last Week</SelectItem>
            <SelectItem value="month">Last Month</SelectItem>
            <SelectItem value="all">All Time</SelectItem>
          </SelectContent>
        </Select>
      </div>

      {/* Sessions List */}
      {filteredSessions.length === 0 ? (
        <p className="text-muted-foreground">
          {sessions.length === 0
            ? 'No sessions found. Start monitoring to create sessions.'
            : 'No sessions match your filters.'}
        </p>
      ) : (
        <div className="space-y-2">
          {filteredSessions.map((session) => (
            <div
              key={session.id}
              onClick={() => onSelectSession?.(session.id)}
              className={`p-3 border rounded cursor-pointer transition-colors ${
                session.is_active
                  ? 'border-primary bg-primary/5'
                  : 'border-border hover:bg-muted/50'
              }`}
            >
              <div className="flex items-center justify-between">
                <div className="flex-1">
                  <div className="flex items-center gap-2">
                    {session.is_active && (
                      <Badge>Active</Badge>
                    )}
                    {session.session_type && (
                      <Badge style={{ backgroundColor: getSessionTypeColor(session.session_type) }} className="text-white border-0">
                        {session.session_type.toUpperCase()}
                      </Badge>
                    )}
                    <span className="text-sm text-muted-foreground">
                      {formatDate(session.start_timestamp)} at {formatTime(session.start_timestamp)}
                    </span>
                  </div>
                  <div className="mt-1 text-xs text-muted-foreground">
                    ID: {session.id.substring(0, 8)}...
                  </div>
                </div>

                <div className="text-right">
                  <div className="font-semibold">{formatDuration(session.duration_hours)}</div>
                  <div className="text-xs text-muted-foreground">
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
