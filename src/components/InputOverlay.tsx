import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface InputOverlayProps {
  sessionId: string;
  currentTimestamp: number;
  enabled: boolean;
  opacity?: number;
}

interface KeyboardEvent {
  id: string;
  timestamp: number;
  eventType: string;
  keyChar: string | null;
  keyCode: number;
  modifiers: {
    ctrl: boolean;
    shift: boolean;
    alt: boolean;
    meta: boolean;
  };
  appName: string;
}

interface MouseEvent {
  id: string;
  timestamp: number;
  eventType: string;
  position: {
    x: number;
    y: number;
  };
  button: string | null;
}

interface KeyDisplay {
  key: string;
  timestamp: number;
  position: { x: number; y: number };
  isCommand: boolean;
}

interface ClickAnimation {
  position: { x: number; y: number };
  type: string;
  timestamp: number;
  progress: number;
}

export function InputOverlay({
  sessionId,
  currentTimestamp,
  enabled,
  opacity = 0.8
}: InputOverlayProps) {
  const [visibleKeys, setVisibleKeys] = useState<KeyDisplay[]>([]);
  const [mousePosition, setMousePosition] = useState<{ x: number, y: number } | null>(null);
  const [mouseClicks, setMouseClicks] = useState<ClickAnimation[]>([]);

  useEffect(() => {
    if (!enabled) {
      setVisibleKeys([]);
      setMousePosition(null);
      setMouseClicks([]);
      return;
    }

    loadEventsAroundTimestamp(currentTimestamp);
  }, [currentTimestamp, enabled, sessionId]);

  const loadEventsAroundTimestamp = async (timestamp: number) => {
    try {
      const windowMs = 1000; // Show events ±1 second

      // Get keyboard events
      const kbEvents = await invoke<KeyboardEvent[]>('get_keyboard_events_in_range', {
        sessionId,
        startTime: timestamp - windowMs,
        endTime: timestamp + windowMs
      }).catch(() => [] as KeyboardEvent[]);

      // Get mouse events
      const mouseEvents = await invoke<MouseEvent[]>('get_mouse_events_in_range', {
        sessionId,
        startTime: timestamp - windowMs,
        endTime: timestamp + windowMs
      }).catch(() => [] as MouseEvent[]);

      processKeyboardEvents(kbEvents, timestamp);
      processMouseEvents(mouseEvents, timestamp);
    } catch (error) {
      console.error('Failed to load input events:', error);
    }
  };

  const processKeyboardEvents = (events: KeyboardEvent[], currentTime: number) => {
    // Show keys pressed within last 500ms
    const recentKeys = events.filter(e =>
      e.eventType === 'KeyDown' &&
      currentTime - e.timestamp < 500 &&
      currentTime >= e.timestamp
    );

    const keyDisplays: KeyDisplay[] = recentKeys.map(e => ({
      key: formatKeyDisplay(e),
      timestamp: e.timestamp,
      position: { x: window.innerWidth / 2, y: window.innerHeight - 100 },
      isCommand: e.modifiers.ctrl || e.modifiers.meta || e.modifiers.alt
    }));

    setVisibleKeys(keyDisplays);
  };

  const processMouseEvents = (events: MouseEvent[], currentTime: number) => {
    // Find most recent mouse position
    const positionEvents = events.filter(e =>
      e.timestamp <= currentTime
    ).sort((a, b) => b.timestamp - a.timestamp);

    if (positionEvents.length > 0) {
      setMousePosition({
        x: positionEvents[0].position.x,
        y: positionEvents[0].position.y
      });
    }

    // Show click animations
    const recentClicks = events.filter(e =>
      (e.eventType === 'LeftClick' ||
       e.eventType === 'RightClick' ||
       e.eventType === 'MiddleClick') &&
      currentTime - e.timestamp < 500 &&
      currentTime >= e.timestamp
    );

    const clickAnims: ClickAnimation[] = recentClicks.map(e => ({
      position: e.position,
      type: e.eventType,
      timestamp: e.timestamp,
      progress: (currentTime - e.timestamp) / 500 // 0 to 1
    }));

    setMouseClicks(clickAnims);
  };

  const formatKeyDisplay = (event: KeyboardEvent): string => {
    const parts: string[] = [];

    if (event.modifiers.ctrl) parts.push('Ctrl');
    if (event.modifiers.shift) parts.push('Shift');
    if (event.modifiers.alt) parts.push('Alt');
    if (event.modifiers.meta) parts.push('⌘');

    if (event.keyChar) {
      parts.push(event.keyChar.toUpperCase());
    }

    return parts.join('+');
  };

  if (!enabled) return null;

  return (
    <div
      className="input-overlay absolute inset-0 pointer-events-none"
      style={{ opacity }}
    >
      {/* Keyboard overlay - bottom center */}
      {visibleKeys.length > 0 && (
        <div className="absolute bottom-8 left-1/2 transform -translate-x-1/2 flex gap-2">
          {visibleKeys.map((keyDisplay, i) => (
            <div
              key={i}
              className={`px-4 py-2 rounded-lg shadow-lg font-mono text-lg ${
                keyDisplay.isCommand
                  ? 'bg-purple-500 text-white'
                  : 'bg-gray-800 text-white'
              }`}
              style={{
                animation: 'fadeIn 0.2s ease-out'
              }}
            >
              {keyDisplay.key}
            </div>
          ))}
        </div>
      )}

      {/* Mouse position indicator */}
      {mousePosition && (
        <div
          className="absolute w-6 h-6 border-2 border-red-500 rounded-full transform -translate-x-1/2 -translate-y-1/2"
          style={{
            left: mousePosition.x,
            top: mousePosition.y
          }}
        />
      )}

      {/* Click animations */}
      {mouseClicks.map((click, i) => {
        const scale = 1 + click.progress * 2;
        const opacity = 1 - click.progress;

        return (
          <div
            key={i}
            className="absolute transform -translate-x-1/2 -translate-y-1/2"
            style={{
              left: click.position.x,
              top: click.position.y
            }}
          >
            <div
              className={`w-12 h-12 rounded-full border-4 ${
                click.type === 'LeftClick' ? 'border-blue-500' :
                click.type === 'RightClick' ? 'border-green-500' :
                'border-yellow-500'
              }`}
              style={{
                transform: `scale(${scale})`,
                opacity
              }}
            />
          </div>
        );
      })}

      <style>{`
        @keyframes fadeIn {
          from {
            opacity: 0;
            transform: translateY(10px);
          }
          to {
            opacity: 1;
            transform: translateY(0);
          }
        }
      `}</style>
    </div>
  );
}
