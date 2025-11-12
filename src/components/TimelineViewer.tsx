import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { startOfDay, endOfDay, startOfWeek, endOfWeek, startOfMonth, endOfMonth } from 'date-fns';
import { Timeline } from './Timeline';
import { TimelineData, TimelineZoom } from '../types/timeline';
import { Button } from './ui/button';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './ui/select';

export function TimelineViewer() {
  const [zoom, setZoom] = useState<TimelineZoom>(TimelineZoom.Day);
  const [dateRange, setDateRange] = useState({
    start: startOfDay(new Date()).getTime(),
    end: endOfDay(new Date()).getTime()
  });
  const [data, setData] = useState<TimelineData | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    loadTimelineData();
  }, [dateRange]);

  const loadTimelineData = async () => {
    setLoading(true);
    try {
      const timelineData = await invoke<TimelineData>('get_timeline_data', {
        startTimestamp: dateRange.start,
        endTimestamp: dateRange.end
      });
      setData(timelineData);
    } catch (error) {
      console.error('Failed to load timeline data:', error);
    } finally {
      setLoading(false);
    }
  };

  const handleZoomChange = (newZoom: TimelineZoom) => {
    setZoom(newZoom);

    // Adjust date range based on zoom level
    const now = new Date();
    switch (newZoom) {
      case TimelineZoom.Hour:
        setDateRange({
          start: new Date(now.getTime() - 3600000).getTime(),
          end: now.getTime()
        });
        break;
      case TimelineZoom.Day:
        setDateRange({
          start: startOfDay(now).getTime(),
          end: endOfDay(now).getTime()
        });
        break;
      case TimelineZoom.Week:
        setDateRange({
          start: startOfWeek(now).getTime(),
          end: endOfWeek(now).getTime()
        });
        break;
      case TimelineZoom.Month:
        setDateRange({
          start: startOfMonth(now).getTime(),
          end: endOfMonth(now).getTime()
        });
        break;
    }
  };

  const handlePanLeft = () => {
    const duration = dateRange.end - dateRange.start;
    setDateRange({
      start: dateRange.start - duration,
      end: dateRange.end - duration
    });
  };

  const handlePanRight = () => {
    const duration = dateRange.end - dateRange.start;
    setDateRange({
      start: dateRange.start + duration,
      end: dateRange.end + duration
    });
  };

  const handleJumpToToday = () => {
    const now = new Date();
    switch (zoom) {
      case TimelineZoom.Hour:
        setDateRange({
          start: new Date(now.getTime() - 3600000).getTime(),
          end: now.getTime()
        });
        break;
      case TimelineZoom.Day:
        setDateRange({
          start: startOfDay(now).getTime(),
          end: endOfDay(now).getTime()
        });
        break;
      case TimelineZoom.Week:
        setDateRange({
          start: startOfWeek(now).getTime(),
          end: endOfWeek(now).getTime()
        });
        break;
      case TimelineZoom.Month:
        setDateRange({
          start: startOfMonth(now).getTime(),
          end: endOfMonth(now).getTime()
        });
        break;
    }
  };

  const handleTimeClick = (timestamp: number) => {
    console.log('Clicked on timestamp:', new Date(timestamp));
    // TODO: Navigate to playback at this time
  };

  const handleSessionClick = (sessionId: string) => {
    console.log('Clicked on session:', sessionId);
    // TODO: Navigate to session detail
  };

  return (
    <div className="timeline-viewer p-4">
      <div className="timeline-controls mb-4 flex items-center gap-4">
        <Button onClick={handlePanLeft} variant="outline" size="sm">
          ← Previous
        </Button>

        <Button onClick={handleJumpToToday} variant="outline" size="sm">
          Today
        </Button>

        <Button onClick={handlePanRight} variant="outline" size="sm">
          Next →
        </Button>

        <Select value={zoom} onValueChange={(value) => handleZoomChange(value as TimelineZoom)}>
          <SelectTrigger className="w-32">
            <SelectValue placeholder="Zoom level" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={TimelineZoom.Hour}>Hour View</SelectItem>
            <SelectItem value={TimelineZoom.Day}>Day View</SelectItem>
            <SelectItem value={TimelineZoom.Week}>Week View</SelectItem>
            <SelectItem value={TimelineZoom.Month}>Month View</SelectItem>
          </SelectContent>
        </Select>

        {loading && <span className="text-sm text-gray-500">Loading...</span>}
      </div>

      {data ? (
        <Timeline
          data={data}
          zoom={zoom}
          onTimeClick={handleTimeClick}
          onSessionClick={handleSessionClick}
        />
      ) : (
        <div className="text-center py-12 text-gray-500">
          {loading ? 'Loading timeline...' : 'No data available'}
        </div>
      )}
    </div>
  );
}
