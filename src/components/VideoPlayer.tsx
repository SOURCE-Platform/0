import { useRef, useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { convertFileSrc } from '@tauri-apps/api/core';
import { PlaybackInfo, SeekInfo } from '../types/playback';
import { Button } from './ui/button';
import { Slider } from './ui/slider';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './ui/select';

interface VideoPlayerProps {
  sessionId: string;
  startTimestamp?: number;
  showControls?: boolean;
  onTimeUpdate?: (timestamp: number) => void;
}

export function VideoPlayer({
  sessionId,
  startTimestamp,
  showControls = true,
  onTimeUpdate
}: VideoPlayerProps) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const [playbackInfo, setPlaybackInfo] = useState<PlaybackInfo | null>(null);
  const [currentSegmentIndex, setCurrentSegmentIndex] = useState(0);
  const [isPlaying, setIsPlaying] = useState(false);
  const [playbackSpeed, setPlaybackSpeed] = useState(1.0);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    loadPlaybackInfo();
  }, [sessionId]);

  useEffect(() => {
    if (startTimestamp && playbackInfo) {
      seekToTimestamp(startTimestamp);
    }
  }, [startTimestamp, playbackInfo]);

  const loadPlaybackInfo = async () => {
    try {
      setIsLoading(true);
      const info = await invoke<PlaybackInfo>('get_playback_info', {
        sessionId
      });
      setPlaybackInfo(info);
      setDuration(info.totalDurationMs / 1000);

      if (info.segments.length > 0) {
        await loadSegment(0);
      }
    } catch (error) {
      console.error('Failed to load playback info:', error);
    } finally {
      setIsLoading(false);
    }
  };

  const loadSegment = async (index: number) => {
    if (!playbackInfo || index >= playbackInfo.segments.length) {
      return;
    }

    const segment = playbackInfo.segments[index];
    const videoPath = convertFileSrc(segment.path);

    if (videoRef.current) {
      videoRef.current.src = videoPath;
      videoRef.current.load();
      setCurrentSegmentIndex(index);

      if (isPlaying) {
        await videoRef.current.play();
      }
    }
  };

  const handleVideoEnded = () => {
    // Move to next segment
    if (playbackInfo && currentSegmentIndex < playbackInfo.segments.length - 1) {
      loadSegment(currentSegmentIndex + 1);
    } else {
      setIsPlaying(false);
    }
  };

  const handlePlay = async () => {
    if (videoRef.current) {
      await videoRef.current.play();
      setIsPlaying(true);
    }
  };

  const handlePause = () => {
    if (videoRef.current) {
      videoRef.current.pause();
      setIsPlaying(false);
    }
  };

  const handleSpeedChange = (speed: string) => {
    const speedValue = parseFloat(speed);
    setPlaybackSpeed(speedValue);
    if (videoRef.current) {
      videoRef.current.playbackRate = speedValue;
    }
  };

  const handleTimeUpdate = () => {
    if (videoRef.current && playbackInfo) {
      const segment = playbackInfo.segments[currentSegmentIndex];
      const segmentTime = videoRef.current.currentTime * 1000;
      const absoluteTime = segment.startTimestamp + segmentTime;

      setCurrentTime(videoRef.current.currentTime);

      if (onTimeUpdate) {
        onTimeUpdate(absoluteTime);
      }
    }
  };

  const seekToTimestamp = async (timestamp: number) => {
    try {
      const seekInfo = await invoke<SeekInfo>('seek_to_timestamp', {
        sessionId,
        timestamp
      });

      if (!playbackInfo) return;

      // Find segment index
      const segmentIndex = playbackInfo.segments.findIndex(
        s => s.path === seekInfo.videoPath
      );

      if (segmentIndex !== -1 && segmentIndex !== currentSegmentIndex) {
        await loadSegment(segmentIndex);
      }

      if (videoRef.current) {
        videoRef.current.currentTime = seekInfo.offsetMs / 1000;
      }
    } catch (error) {
      console.error('Failed to seek:', error);
    }
  };

  const handleSeek = (value: number[]) => {
    const seekTime = value[0];

    if (!playbackInfo) return;

    // Calculate which segment this time falls into
    let accumulatedTime = 0;
    let targetSegmentIndex = 0;
    let offsetInSegment = seekTime;

    for (let i = 0; i < playbackInfo.segments.length; i++) {
      const segmentDuration = playbackInfo.segments[i].durationMs / 1000;

      if (accumulatedTime + segmentDuration >= seekTime) {
        targetSegmentIndex = i;
        offsetInSegment = seekTime - accumulatedTime;
        break;
      }

      accumulatedTime += segmentDuration;
    }

    if (targetSegmentIndex !== currentSegmentIndex) {
      loadSegment(targetSegmentIndex).then(() => {
        if (videoRef.current) {
          videoRef.current.currentTime = offsetInSegment;
        }
      });
    } else if (videoRef.current) {
      videoRef.current.currentTime = offsetInSegment;
    }
  };

  const formatTime = (seconds: number): string => {
    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    const secs = Math.floor(seconds % 60);

    if (hours > 0) {
      return `${hours}:${minutes.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`;
    }
    return `${minutes}:${secs.toString().padStart(2, '0')}`;
  };

  const toggleFullscreen = () => {
    if (videoRef.current) {
      if (!document.fullscreenElement) {
        videoRef.current.requestFullscreen();
      } else {
        document.exitFullscreen();
      }
    }
  };

  if (isLoading) {
    return (
      <div className="video-player flex items-center justify-center p-8">
        <div className="text-center">
          <div className="text-lg">Loading recording...</div>
        </div>
      </div>
    );
  }

  if (!playbackInfo) {
    return (
      <div className="video-player flex items-center justify-center p-8">
        <div className="text-center text-gray-500">
          No recording available for this session
        </div>
      </div>
    );
  }

  return (
    <div className="video-player flex flex-col gap-2">
      <div className="video-container relative bg-black rounded-lg overflow-hidden">
        <video
          ref={videoRef}
          onEnded={handleVideoEnded}
          onTimeUpdate={handleTimeUpdate}
          className="w-full h-auto"
        />
      </div>

      {showControls && (
        <div className="player-controls flex flex-col gap-2 p-2 bg-gray-100 dark:bg-gray-800 rounded-lg">
          <div className="flex items-center gap-2">
            <Button
              onClick={isPlaying ? handlePause : handlePlay}
              variant="outline"
              size="sm"
            >
              {isPlaying ? '⏸' : '▶'}
            </Button>

            <Slider
              value={[currentTime]}
              min={0}
              max={duration}
              step={0.1}
              onValueChange={handleSeek}
              className="flex-1"
            />

            <span className="text-sm font-mono text-gray-600 dark:text-gray-400 min-w-24 text-right">
              {formatTime(currentTime)} / {formatTime(duration)}
            </span>

            <Select
              value={playbackSpeed.toString()}
              onValueChange={handleSpeedChange}
            >
              <SelectTrigger className="w-20">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="0.25">0.25x</SelectItem>
                <SelectItem value="0.5">0.5x</SelectItem>
                <SelectItem value="1">1x</SelectItem>
                <SelectItem value="1.5">1.5x</SelectItem>
                <SelectItem value="2">2x</SelectItem>
                <SelectItem value="4">4x</SelectItem>
              </SelectContent>
            </Select>

            <Button onClick={toggleFullscreen} variant="outline" size="sm">
              ⛶
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
