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
  onEscape: () => void;
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
  onEscape,
}: TimelineInteractionOptions) {
  const [isPanning, setIsPanning] = useState(false);
  const timelineSurfaceRef = useRef<HTMLDivElement | null>(null);
  const commandPressedRef = useRef(false);
  const suppressClickRef = useRef(false);
  const lastGestureScaleRef = useRef(1);
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
    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onEscape();
    };
    const handleBlur = () => {
      commandPressedRef.current = false;
    };

    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("keyup", handleKeyUp);
    window.addEventListener("keydown", handleEscape);
    window.addEventListener("blur", handleBlur);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("keyup", handleKeyUp);
      window.removeEventListener("keydown", handleEscape);
      window.removeEventListener("blur", handleBlur);
    };
  }, [onEscape]);

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

    const panByPixels = (deltaX: number, surface: HTMLDivElement) => {
      const width = Math.max(surface.getBoundingClientRect().width, 1);
      const deltaMs = Math.round((deltaX / width) * windowDurationMs);
      const unclampedEnd = effectiveWindowEnd - deltaMs;
      const nextEnd = Math.min(dateRange.end, Math.max(dateRange.start + windowDurationMs, unclampedEnd));
      setWindowEndTimestamp(nextEnd);
      setIsLiveFollowing(isToday && nextEnd >= dateRange.end - 1000);
    };

    const handleNativeWheel = (event: WheelEvent) => {
      const surface = timelineSurfaceRef.current;
      const target = event.target;
      if (!surface || !(target instanceof Node) || !surface.contains(target)) return;
      // Pinch / Cmd+scroll zooms at the pointer.
      if (commandPressedRef.current || event.metaKey || event.ctrlKey) {
        event.preventDefault();
        event.stopPropagation();
        zoomTimelineAtPoint(event.clientX, event.deltaY, surface);
        return;
      }
      // Two-finger horizontal swipe pans. Vertical scroll is left alone
      // so the page keeps scrolling normally.
      if (Math.abs(event.deltaX) > Math.abs(event.deltaY) && Math.abs(event.deltaX) > 0) {
        event.preventDefault();
        event.stopPropagation();
        panByPixels(event.deltaX, surface);
      }
    };

    // Safari/WKWebView trackpad pinch gestures (more reliable than
    // ctrl+wheel synthesis for pinch-to-zoom).
    const handleGestureStart = (event: Event) => {
      const surface = timelineSurfaceRef.current;
      const target = event.target;
      if (!surface || !(target instanceof Node) || !surface.contains(target)) return;
      event.preventDefault();
      lastGestureScaleRef.current = 1;
    };

    const handleGestureChange = (event: Event) => {
      const surface = timelineSurfaceRef.current;
      const target = event.target;
      if (!surface || !(target instanceof Node) || !surface.contains(target)) return;
      event.preventDefault();
      event.stopPropagation();
      const scale = (event as unknown as { scale?: number }).scale ?? 1;
      const lastScale = lastGestureScaleRef.current;
      lastGestureScaleRef.current = scale;
      if (!Number.isFinite(scale) || scale <= 0 || !Number.isFinite(lastScale) || lastScale <= 0) return;
      // deltaY sign convention matches zoomTimelineAtPoint: positive zooms out.
      const gesture = event as unknown as { scale?: number; clientX?: number };
      const rect = surface.getBoundingClientRect();
      const clientX = typeof gesture.clientX === "number" ? gesture.clientX : rect.left + rect.width / 2;
      zoomTimelineAtPoint(clientX, (lastScale - scale) * 400, surface);
    };

    const handleNativeMouseDown = (event: MouseEvent) => {
      const surface = timelineSurfaceRef.current;
      const target = event.target;
      if (!surface || !(target instanceof Node) || !surface.contains(target) || event.button !== 1) return;
      event.preventDefault();
      event.stopPropagation();
      // A middle press must never toggle a tooltip on release.
      suppressClickRef.current = true;
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

    const handleSuppressClick = (event: MouseEvent) => {
      if (!suppressClickRef.current || event.button !== 1) return;
      suppressClickRef.current = false;
      const surface = timelineSurfaceRef.current;
      const target = event.target;
      if (!surface || !(target instanceof Node) || !surface.contains(target)) return;
      event.preventDefault();
      event.stopPropagation();
    };

    window.addEventListener("wheel", handleNativeWheel, { passive: false, capture: true });
    window.addEventListener("gesturestart", handleGestureStart, { passive: false, capture: true });
    window.addEventListener("gesturechange", handleGestureChange, { passive: false, capture: true });
    window.addEventListener("mousedown", handleNativeMouseDown, true);
    window.addEventListener("auxclick", handleAuxClick, true);
    window.addEventListener("click", handleSuppressClick, true);
    return () => {
      window.removeEventListener("wheel", handleNativeWheel, true);
      window.removeEventListener("gesturestart", handleGestureStart, true);
      window.removeEventListener("gesturechange", handleGestureChange, true);
      window.removeEventListener("mousedown", handleNativeMouseDown, true);
      window.removeEventListener("auxclick", handleAuxClick, true);
      window.removeEventListener("click", handleSuppressClick, true);
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
