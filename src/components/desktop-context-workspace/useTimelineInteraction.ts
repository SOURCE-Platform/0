import { useEffect, useRef, useState } from "react";
import { MIN_WINDOW_MS } from "@/components/desktop-context-workspace/utils";

interface TimelineInteractionOptions {
  dateRange: { start: number; end: number };
  effectiveWindowEnd: number;
  effectiveWindowStart: number;
  isToday: boolean;
  windowDurationMs: number;
  setWindowDurationMs: (value: number) => void;
  setWindowEndTimestamp: (value: number) => void;
  setIsLiveFollowing: (value: boolean) => void;
}

export function useTimelineInteraction({
  dateRange,
  effectiveWindowEnd,
  effectiveWindowStart,
  isToday,
  windowDurationMs,
  setWindowDurationMs,
  setWindowEndTimestamp,
  setIsLiveFollowing,
}: TimelineInteractionOptions) {
  const [isPanning, setIsPanning] = useState(false);
  const timelineSurfaceRef = useRef<HTMLDivElement | null>(null);
  const commandPressedRef = useRef(false);
  const panStateRef = useRef<{ pointerStartX: number; windowEndAtDragStart: number; width: number } | null>(null);

  useEffect(() => {
    const isCommandEvent = (event: KeyboardEvent) =>
      event.key === "Meta" ||
      event.key === "OS" ||
      event.code === "MetaLeft" ||
      event.code === "MetaRight" ||
      event.code === "OSLeft" ||
      event.code === "OSRight";

    const handleKeyDown = (event: KeyboardEvent) => {
      if (isCommandEvent(event)) commandPressedRef.current = true;
    };
    const handleKeyUp = (event: KeyboardEvent) => {
      if (isCommandEvent(event)) commandPressedRef.current = false;
    };
    const handleBlur = () => {
      commandPressedRef.current = false;
    };

    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("keyup", handleKeyUp);
    window.addEventListener("blur", handleBlur);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("keyup", handleKeyUp);
      window.removeEventListener("blur", handleBlur);
    };
  }, []);

  useEffect(() => {
    const zoomTimelineAtPoint = (clientX: number, deltaY: number, surface: HTMLDivElement) => {
      const rect = surface.getBoundingClientRect();
      const relativeX = rect.width > 0 ? Math.min(Math.max((clientX - rect.left) / rect.width, 0), 1) : 1;
      const currentDuration = effectiveWindowEnd - effectiveWindowStart;
      const anchorTime = effectiveWindowStart + currentDuration * relativeX;
      const zoomFactor = deltaY > 0 ? 1.15 : 0.85;
      const maxWindowMs = Math.max(MIN_WINDOW_MS, dateRange.end - dateRange.start);
      const nextDuration = Math.min(maxWindowMs, Math.max(MIN_WINDOW_MS, Math.round(currentDuration * zoomFactor)));

      let nextStart = Math.round(anchorTime - nextDuration * relativeX);
      let nextEnd = nextStart + nextDuration;

      if (nextStart < dateRange.start) {
        nextStart = dateRange.start;
        nextEnd = nextStart + nextDuration;
      }
      if (nextEnd > dateRange.end) {
        nextEnd = dateRange.end;
        nextStart = nextEnd - nextDuration;
      }

      setWindowDurationMs(nextDuration);
      setWindowEndTimestamp(nextEnd);
      setIsLiveFollowing(isToday && nextEnd >= dateRange.end - 1000);
    };

    const handleNativeWheel = (event: WheelEvent) => {
      const surface = timelineSurfaceRef.current;
      const target = event.target;
      if (!surface || !(target instanceof Node) || !surface.contains(target)) return;
      if (!commandPressedRef.current && !event.metaKey && !event.ctrlKey) return;
      event.preventDefault();
      event.stopPropagation();
      zoomTimelineAtPoint(event.clientX, event.deltaY, surface);
    };

    const handleNativeMouseDown = (event: MouseEvent) => {
      const surface = timelineSurfaceRef.current;
      const target = event.target;
      if (!surface || !(target instanceof Node) || !surface.contains(target) || event.button !== 1) return;
      event.preventDefault();
      event.stopPropagation();
      panStateRef.current = {
        pointerStartX: event.clientX,
        windowEndAtDragStart: effectiveWindowEnd,
        width: surface.getBoundingClientRect().width,
      };
      setIsPanning(true);
    };

    const handleAuxClick = (event: MouseEvent) => {
      const surface = timelineSurfaceRef.current;
      const target = event.target;
      if (!surface || !(target instanceof Node) || !surface.contains(target) || event.button !== 1) return;
      event.preventDefault();
      event.stopPropagation();
    };

    window.addEventListener("wheel", handleNativeWheel, { passive: false, capture: true });
    window.addEventListener("mousedown", handleNativeMouseDown, true);
    window.addEventListener("auxclick", handleAuxClick, true);
    return () => {
      window.removeEventListener("wheel", handleNativeWheel, true);
      window.removeEventListener("mousedown", handleNativeMouseDown, true);
      window.removeEventListener("auxclick", handleAuxClick, true);
    };
  }, [
    dateRange.end,
    dateRange.start,
    effectiveWindowEnd,
    effectiveWindowStart,
    isToday,
    setIsLiveFollowing,
    setWindowDurationMs,
    setWindowEndTimestamp,
  ]);

  useEffect(() => {
    const handleMouseMove = (event: MouseEvent) => {
      const panState = panStateRef.current;
      if (!panState) return;
      event.preventDefault();
      const deltaX = event.clientX - panState.pointerStartX;
      const width = Math.max(panState.width, 1);
      const deltaMs = Math.round((deltaX / width) * windowDurationMs);
      const unclampedEnd = panState.windowEndAtDragStart - deltaMs;
      const nextEnd = Math.min(dateRange.end, Math.max(dateRange.start + windowDurationMs, unclampedEnd));
      setWindowEndTimestamp(nextEnd);
      setIsLiveFollowing(isToday && nextEnd >= dateRange.end - 1000);
    };

    const handleMouseUp = () => {
      panStateRef.current = null;
      setIsPanning(false);
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
    return () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    };
  }, [dateRange.end, dateRange.start, isToday, setIsLiveFollowing, setWindowEndTimestamp, windowDurationMs]);

  return { isPanning, timelineSurfaceRef };
}
