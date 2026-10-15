import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { endOfDay, startOfDay } from "date-fns";
import { bufferedTimelineRange, timelineCoversRange } from "./timelineRange";
import {
  ChannelStatus,
  ContextSlice,
  ContextSliceDetail,
  ContextTimelineData,
  DesktopCaptureStatus,
  OcrReviewItem,
  PiiEntity,
  TimelineRail,
} from "@/types/contextTimeline";

/** Channel health and the visible block overview refresh on this cadence. */
const SLOW_REFRESH_MS = 30_000;
import { useTimelineInteraction } from "@/components/desktop-context-workspace/useTimelineInteraction";
import {
  immediateSliceDetail,
  useLiveTranscript,
} from "@/components/desktop-context-workspace/useLiveTranscript";

function walkRails(rails: TimelineRail[], visit: (rail: TimelineRail) => void) {
  rails.forEach((rail) => {
    visit(rail);
    if (rail.children.length > 0) walkRails(rail.children, visit);
  });
}

export function useDesktopContextWorkspace(displayId: number | null) {
  const [status, setStatus] = useState<DesktopCaptureStatus | null>(null);
  const [channelStatuses, setChannelStatuses] = useState<ChannelStatus[]>([]);
  const [timeline, setTimeline] = useState<ContextTimelineData | null>(null);
  const [ocrItems, setOcrItems] = useState<OcrReviewItem[]>([]);
  const [piiItems, setPiiItems] = useState<PiiEntity[]>([]);
  const [selectedSlice, setSelectedSlice] = useState<ContextSlice | null>(null);
  const [sliceDetail, setSliceDetail] = useState<ContextSliceDetail | null>(null);
  const [detailTab, setDetailTab] = useState("metadata");
  const [activeView, setActiveView] = useState("timeline");
  const [loading, setLoading] = useState(true);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [appFilter, setAppFilter] = useState("all");
  const [interactionFilter, setInteractionFilter] = useState("all");
  const [ocrQuery, setOcrQuery] = useState("");
  const [piiTypeFilter, setPiiTypeFilter] = useState("all");
  const [dayStart, setDayStart] = useState(startOfDay(new Date()).getTime());
  const [isLiveFollowing, setIsLiveFollowing] = useState(true);
  const [windowDurationMs, setWindowDurationMs] = useState<number>(60 * 60 * 1000);
  const [windowEndTimestamp, setWindowEndTimestamp] = useState<number | null>(null);
  const [nowMs, setNowMs] = useState(() => Date.now());
  const dateRangeRef = useRef<{ start: number; end: number }>({ start: dayStart, end: Date.now() });
  const timelineCacheRef = useRef<ContextTimelineData | null>(null);
  const timelineLoadedAtRef = useRef(0);
  const loadInFlightCountRef = useRef(0);
  const loadGenerationRef = useRef(0);
  const detailGenerationRef = useRef(0);
  const slowRefreshAtRef = useRef(0);

  const isToday = useMemo(() => startOfDay(new Date()).getTime() === dayStart, [dayStart]);
  const dateRange = useMemo(
    () => ({ start: dayStart, end: isToday ? nowMs : endOfDay(dayStart).getTime() }),
    [dayStart, isToday, nowMs],
  );

  useEffect(() => {
    dateRangeRef.current = dateRange;
  }, [dateRange]);

  const effectiveWindowEnd = useMemo(() => {
    if (isLiveFollowing && isToday) return dateRange.end;
    if (windowEndTimestamp != null) return Math.min(windowEndTimestamp, dateRange.end);
    return dateRange.end;
  }, [dateRange.end, isLiveFollowing, isToday, windowEndTimestamp]);

  const effectiveWindowStart = useMemo(
    () => Math.max(dateRange.start, effectiveWindowEnd - windowDurationMs),
    [dateRange.start, effectiveWindowEnd, windowDurationMs],
  );

  const appOptions = useMemo(() => {
    const names = new Set<string>();
    if (timeline?.rails) {
      walkRails(timeline.rails, (rail) => {
        rail.slices.forEach((slice) => {
          const appName = slice.appName?.trim();
          if (appName) names.add(appName);
        });
      });
    }
    return Array.from(names).sort();
  }, [timeline]);

  async function loadWorkspace(
    silent = false,
    range = { start: effectiveWindowStart, end: effectiveWindowEnd },
    reuseCached = false,
  ) {
    if (silent && loadInFlightCountRef.current > 0) return;
    const generation = ++loadGenerationRef.current;
    const now = Date.now();
    if (reuseCached && now - timelineLoadedAtRef.current < SLOW_REFRESH_MS
      && timelineCoversRange(timelineCacheRef.current, range)) {
      setLoading(false);
      return;
    }
    loadInFlightCountRef.current += 1;
    if (!timelineCacheRef.current) setLoading(true);
    const refreshSlow = !silent || now - slowRefreshAtRef.current >= SLOW_REFRESH_MS;
    const fetchRange = bufferedTimelineRange(range, {
      start: dateRangeRef.current.start,
      end: isToday ? now : dateRangeRef.current.end,
    });
    try {
      const [captureStatus, timelineData, channels] = await Promise.all([
        invoke<DesktopCaptureStatus>("get_desktop_capture_status"),
        invoke<ContextTimelineData>("get_context_timeline", {
          startTimestamp: fetchRange.start,
          endTimestamp: fetchRange.end,
        }),
        refreshSlow ? invoke<ChannelStatus[]>("get_channel_statuses") : Promise.resolve(null),
      ]);
      if (generation !== loadGenerationRef.current) return;
      setStatus(captureStatus);
      timelineCacheRef.current = timelineData;
      timelineLoadedAtRef.current = Date.now();
      setTimeline(timelineData);
      if (channels) setChannelStatuses(channels);
      if (refreshSlow) slowRefreshAtRef.current = now;
    } catch (error) {
      if (generation === loadGenerationRef.current) {
        setActionError(`Failed to load device context timeline: ${error}`);
      }
    } finally {
      loadInFlightCountRef.current -= 1;
      if (generation === loadGenerationRef.current) setLoading(false);
    }
  }

  async function loadReviewData() {
    const currentRange = dateRangeRef.current;
    try {
      const [ocrReview, piiReview] = await Promise.all([
        invoke<OcrReviewItem[]>("get_ocr_review", {
          startTimestamp: currentRange.start,
          endTimestamp: currentRange.end,
          appFilter: appFilter === "all" ? null : appFilter,
          query: ocrQuery.trim() || null,
          piiOnly: false,
        }),
        invoke<PiiEntity[]>("get_pii_review", {
          startTimestamp: currentRange.start,
          endTimestamp: currentRange.end,
          appFilter: appFilter === "all" ? null : appFilter,
          entityTypeFilter: piiTypeFilter === "all" ? null : piiTypeFilter,
          confidenceThreshold: 0.6,
        }),
      ]);
      setOcrItems(ocrReview);
      setPiiItems(piiReview);
    } catch (error) {
      setActionError(`Failed to load OCR/PII review data: ${error}`);
    }
  }

  useEffect(() => {
    const timer = window.setTimeout(() => void loadWorkspace(false, {
      start: effectiveWindowStart, end: effectiveWindowEnd,
    }, true), 200);
    return () => window.clearTimeout(timer);
  }, [dateRange.start, windowDurationMs, windowEndTimestamp, isLiveFollowing]);

  useEffect(() => {
    setWindowEndTimestamp(isToday ? Date.now() : dateRange.end);
    setIsLiveFollowing(isToday);
  }, [dateRange.start, isToday]);

  useEffect(() => {
    if (!isToday) return;
    const interval = setInterval(() => {
      setNowMs(Date.now());
    }, SLOW_REFRESH_MS);
    return () => clearInterval(interval);
  }, [isToday]);

  useEffect(() => {
    if (!isToday || !isLiveFollowing || !status?.isActive) return;
    const interval = setInterval(() => {
      const end = Date.now();
      void loadWorkspace(true, {
        start: Math.max(dayStart, end - windowDurationMs),
        end,
      });
    }, SLOW_REFRESH_MS);
    return () => clearInterval(interval);
  }, [status?.isActive, isToday, isLiveFollowing, dayStart, windowDurationMs]);

  useLiveTranscript({ timeline, selectedSlice, setSelectedSlice, setSliceDetail });

  const { isPanning, timelineSurfaceRef } = useTimelineInteraction({
    dateRange,
    effectiveWindowEnd,
    effectiveWindowStart,
    isToday,
    windowDurationMs,
    setWindowDurationMs,
    setWindowEndTimestamp,
    setIsLiveFollowing,
    onEscape: clearSelectedSlice,
  });

  async function handleStartCapture() {
    setActionError(null);
    try {
      const nextStatus = await invoke<DesktopCaptureStatus>("start_desktop_capture", { displayId });
      const freshNow = Date.now();
      dateRangeRef.current = { start: dayStart, end: freshNow };
      setStatus(nextStatus);
      setNowMs(freshNow);
      setIsLiveFollowing(true);
      setWindowEndTimestamp(freshNow);
      await loadWorkspace();
    } catch (error) {
      setActionError(`Failed to start capture: ${error}`);
    }
  }

  async function handleStopCapture() {
    setActionError(null);
    try {
      const nextStatus = await invoke<DesktopCaptureStatus>("stop_desktop_capture");
      setStatus(nextStatus);
      await loadWorkspace();
    } catch (error) {
      setActionError(`Failed to stop capture: ${error}`);
    }
  }

  async function handleSelectSlice(slice: ContextSlice) {
    const generation = ++detailGenerationRef.current;
    setSelectedSlice(slice);
    // Keep the selected block visible while its stored content loads.
    setSliceDetail(immediateSliceDetail(slice));
    setLoadingDetail(true);
    setActionError(null);
    setDetailTab(
      slice.rail === "ocr" || slice.rail === "vision" || slice.rail === "attention"
        ? "visual"
        : slice.tags.includes("asr")
          ? "transcript"
          : "metadata",
    );
    try {
      const details = await invoke<ContextSliceDetail>("get_context_slice_detail", {
        slice,
      });
      if (generation === detailGenerationRef.current) setSliceDetail(details);
    } catch (error) {
      if (generation === detailGenerationRef.current) {
        setActionError(`Failed to load block detail: ${error}`);
      }
    } finally {
      if (generation === detailGenerationRef.current) setLoadingDetail(false);
    }
  }

  function clearSelectedSlice() {
    detailGenerationRef.current += 1;
    setSelectedSlice(null);
    setSliceDetail(null);
  }

  function shiftWindow(direction: -1 | 1) {
    const nextEnd = Math.min(
      dateRange.end,
      Math.max(dateRange.start + windowDurationMs, effectiveWindowEnd + direction * windowDurationMs),
    );
    setWindowEndTimestamp(nextEnd);
    setIsLiveFollowing(isToday && nextEnd >= dateRange.end - 1000);
  }

  function handleJumpToNow() {
    setIsLiveFollowing(true);
    setWindowEndTimestamp(dateRange.end);
  }

  return {
    status,
    channelStatuses,
    timeline,
    ocrItems,
    piiItems,
    selectedSlice,
    sliceDetail,
    detailTab,
    activeView,
    loading,
    loadingDetail,
    actionError,
    appFilter,
    interactionFilter,
    ocrQuery,
    piiTypeFilter,
    dayStart,
    isLiveFollowing,
    windowDurationMs,
    windowEndTimestamp,
    isPanning,
    timelineSurfaceRef,
    isToday,
    dateRange,
    effectiveWindowEnd,
    effectiveWindowStart,
    appOptions,
    // Audio capture does not require a display. The backend only requires one
    // when the explicitly enabled screen channel needs it.
    canStartCapture: true,
    setActiveView,
    setAppFilter,
    setInteractionFilter,
    setOcrQuery,
    setPiiTypeFilter,
    setDayStart,
    setDetailTab,
    loadReviewData,
    handleStartCapture,
    handleStopCapture,
    handleSelectSlice,
    clearSelectedSlice,
    shiftWindow,
    handleJumpToNow,
  };
}
