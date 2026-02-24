import { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { VideoPlayer } from './VideoPlayer';
import { Button } from './ui/button';
import { Input } from './ui/input';
import { Card } from './ui/card';
import { Badge } from './ui/badge';
import { ScrollArea } from './ui/scroll-area';
import { RefreshCw, Trash2, Film } from 'lucide-react';
import { cn } from '@/lib/utils';

interface RecordingInfo {
  session_id: string;
  start_timestamp: number;
  end_timestamp: number | null;
  segment_count: number;
  total_size_bytes: number;
  total_duration_ms: number;
  frame_count: number;
}

function formatSize(bytes: number): string {
  if (bytes >= 1_073_741_824) {
    return `${(bytes / 1_073_741_824).toFixed(1)} GB`;
  }
  if (bytes >= 1_048_576) {
    return `${(bytes / 1_048_576).toFixed(0)} MB`;
  }
  if (bytes >= 1024) {
    return `${(bytes / 1024).toFixed(0)} KB`;
  }
  return `${bytes} B`;
}

function formatDuration(ms: number): string {
  const totalSeconds = Math.floor(ms / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;

  if (hours > 0) {
    return `${hours}h ${minutes}m`;
  }
  if (minutes > 0) {
    return `${minutes}m ${seconds}s`;
  }
  return `${seconds}s`;
}

function formatDate(ts: number): string {
  const date = new Date(ts);
  return date.toLocaleString(undefined, {
    weekday: 'short',
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  });
}

const NAMES_KEY = 'recording-names';

function loadNames(): Record<string, string> {
  try {
    return JSON.parse(localStorage.getItem(NAMES_KEY) ?? '{}');
  } catch {
    return {};
  }
}

function saveName(sessionId: string, name: string) {
  const names = loadNames();
  names[sessionId] = name;
  localStorage.setItem(NAMES_KEY, JSON.stringify(names));
}

export default function Recordings() {
  const [recordings, setRecordings] = useState<RecordingInfo[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editValue, setEditValue] = useState('');
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [names, setNames] = useState<Record<string, string>>(loadNames);
  const editInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    loadRecordings();
  }, []);

  useEffect(() => {
    if (editingId && editInputRef.current) {
      editInputRef.current.focus();
    }
  }, [editingId]);

  const loadRecordings = async () => {
    try {
      setIsLoading(true);
      const data = await invoke<RecordingInfo[]>('get_recordings');
      setRecordings(data);
    } catch (error) {
      console.error('Failed to load recordings:', error);
    } finally {
      setIsLoading(false);
    }
  };

  const startEditing = (recording: RecordingInfo) => {
    setEditingId(recording.session_id);
    setEditValue(names[recording.session_id] ?? formatDate(recording.start_timestamp));
  };

  const commitEdit = (sessionId: string) => {
    const trimmed = editValue.trim();
    if (trimmed) {
      saveName(sessionId, trimmed);
      setNames(prev => ({ ...prev, [sessionId]: trimmed }));
    }
    setEditingId(null);
  };

  const handleEditKeyDown = (e: React.KeyboardEvent, sessionId: string) => {
    if (e.key === 'Enter') {
      commitEdit(sessionId);
    } else if (e.key === 'Escape') {
      setEditingId(null);
    }
  };

  const confirmDelete = async (sessionId: string) => {
    try {
      await invoke('delete_recording', { sessionId });
      if (selectedId === sessionId) {
        setSelectedId(null);
      }
      setDeletingId(null);
      await loadRecordings();
    } catch (error) {
      console.error('Failed to delete recording:', error);
      setDeletingId(null);
    }
  };

  const selectedRecording = recordings.find(r => r.session_id === selectedId) ?? null;

  return (
    <div className="flex gap-4 h-[calc(100vh-140px)]">
      {/* Left panel — recordings list */}
      <div className="w-80 flex-shrink-0 flex flex-col gap-2">
        <div className="flex items-center justify-between">
          <h2 className="text-lg font-medium">Recordings</h2>
          <Button variant="outline" size="sm" onClick={loadRecordings} disabled={isLoading}>
            <RefreshCw className={cn('h-4 w-4', isLoading && 'animate-spin')} />
          </Button>
        </div>

        <ScrollArea className="flex-1">
          {isLoading ? (
            <div className="flex items-center justify-center py-8 text-muted-foreground text-sm">
              Loading...
            </div>
          ) : recordings.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-8 gap-2 text-center">
              <Film className="h-8 w-8 text-muted-foreground" />
              <p className="text-sm text-muted-foreground">
                No recordings yet. Start recording from the Screen Recorder tab.
              </p>
            </div>
          ) : (
            <div className="flex flex-col gap-2 p-1">
              {recordings.map(recording => {
                const displayName = names[recording.session_id] ?? formatDate(recording.start_timestamp);
                const isSelected = selectedId === recording.session_id;
                const isDeleting = deletingId === recording.session_id;
                const isEditing = editingId === recording.session_id;

                return (
                  <Card
                    key={recording.session_id}
                    className={cn(
                      'p-3 cursor-pointer transition-colors',
                      isSelected
                        ? 'border-primary bg-primary/10 dark:bg-primary/10'
                        : 'hover:bg-accent'
                    )}
                    onClick={() => {
                      if (!isEditing && !isDeleting) {
                        setSelectedId(recording.session_id);
                      }
                    }}
                  >
                    {/* Name row */}
                    <div className="flex items-center gap-1 mb-1">
                      {isEditing ? (
                        <Input
                          ref={editInputRef}
                          value={editValue}
                          onChange={e => setEditValue(e.target.value)}
                          onBlur={() => commitEdit(recording.session_id)}
                          onKeyDown={e => handleEditKeyDown(e, recording.session_id)}
                          className="h-6 text-sm px-1 py-0"
                          onClick={e => e.stopPropagation()}
                        />
                      ) : (
                        <span
                          className="text-sm font-medium truncate flex-1 hover:underline cursor-text"
                          title="Click to rename"
                          onClick={e => {
                            e.stopPropagation();
                            startEditing(recording);
                          }}
                        >
                          {displayName}
                        </span>
                      )}
                    </div>

                    {/* Metadata row */}
                    <div className="flex items-center gap-1 flex-wrap">
                      <Badge variant="secondary" className="text-xs">
                        {formatDuration(recording.total_duration_ms)}
                      </Badge>
                      <span className="text-xs text-muted-foreground">
                        {formatSize(recording.total_size_bytes)}
                      </span>
                      <span className="text-xs text-muted-foreground">
                        {recording.segment_count} {recording.segment_count === 1 ? 'segment' : 'segments'}
                      </span>
                    </div>

                    {/* Delete row */}
                    <div className="flex items-center justify-end mt-1">
                      {isDeleting ? (
                        <div className="flex items-center gap-1" onClick={e => e.stopPropagation()}>
                          <span className="text-xs text-destructive font-medium">Delete?</span>
                          <Button
                            variant="destructive"
                            size="sm"
                            className="h-6 px-2 text-xs"
                            onClick={() => confirmDelete(recording.session_id)}
                          >
                            Yes
                          </Button>
                          <Button
                            variant="outline"
                            size="sm"
                            className="h-6 px-2 text-xs"
                            onClick={() => setDeletingId(null)}
                          >
                            Cancel
                          </Button>
                        </div>
                      ) : (
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-6 w-6 p-0 text-muted-foreground hover:text-destructive"
                          onClick={e => {
                            e.stopPropagation();
                            setDeletingId(recording.session_id);
                          }}
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                        </Button>
                      )}
                    </div>
                  </Card>
                );
              })}
            </div>
          )}
        </ScrollArea>
      </div>

      {/* Right panel — player */}
      <div className="flex-1 flex flex-col gap-3 min-w-0">
        {selectedId === null ? (
          <div className="flex-1 flex items-center justify-center text-muted-foreground">
            <div className="text-center">
              <Film className="h-12 w-12 mx-auto mb-3 opacity-30" />
              <p>Select a recording to play it here</p>
            </div>
          </div>
        ) : (
          <>
            <VideoPlayer sessionId={selectedId} showControls={true} />
            {selectedRecording && (
              <div className="flex items-center gap-4 text-xs text-muted-foreground px-1">
                <span>Started: {formatDate(selectedRecording.start_timestamp)}</span>
                {selectedRecording.end_timestamp && (
                  <span>Ended: {formatDate(selectedRecording.end_timestamp)}</span>
                )}
                <span>{selectedRecording.frame_count.toLocaleString()} frames</span>
                <span>{formatSize(selectedRecording.total_size_bytes)}</span>
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
