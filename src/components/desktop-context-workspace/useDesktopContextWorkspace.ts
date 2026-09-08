import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { endOfDay, startOfDay } from "date-fns";
import {
  AppUsageOverview,
  ChannelStatus,
  ContextSlice,
  ContextSliceDetail,
  ContextTimelineData,
  DesktopCaptureStatus,
  OcrReviewItem,
  PiiEntity,
  TimelineRail,
} from "@/types/contextTimeline";
import { useTimelineInteraction } from "@/components/desktop-context-workspace/useTimelineInteraction";

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
  const [appUsage, setAppUsage] = useState<AppUsageOverview | null>(null);
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
    appUsage?.items.forEach((item) => {
      const appName = item.appName?.trim();
      if (appName) names.add(appName);
    });
    return Array.from(names).sort();
  }, [timeline, appUsage]);

  async function loadWorkspace(silent = false) {
    if (!silent) setLoading(true);
    const currentRange = dateRangeRef.current;
    try {
      const [captureStatus, channels, timelineData, appOverview] = await Promise.all([
        invoke<DesktopCaptureStatus>("get_desktop_capture_status"),
        invoke<ChannelStatus[]>("get_channel_statuses"),
        invoke<ContextTimelineData>("get_context_timeline", {
          startTimestamp: currentRange.start,
          endTimestamp: currentRange.end,
        }),
        invoke<AppUsageOverview>("get_app_usage_overview", {
          startTimestamp: currentRange.start,
          endTimestamp: currentRange.end,
        }),
      ]);
      setStatus(captureStatus);
      setChannelStatuses(channels);
      setTimeline(timelineData);
      setAppUsage(appOverview);
    } catch (error) {
      setActionError(`Failed to load device context timeline: ${error}`);
    } finally {
      if (!silent) setLoading(false);
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
    void loadWorkspace();
  }, [dateRange.start]);

  useEffect(() => {
    setWindowEndTimestamp(isToday ? Date.now() : dateRange.end);
    setIsLiveFollowing(isToday);
  }, [dateRange.start, isToday]);

  useEffect(() => {
    void loadReviewData();
  }, [dateRange.start, appFilter, piiTypeFilter]);

  useEffect(() => {
    if (!isToday) return;
    const interval = setInterval(() => {
      setNowMs(Date.now());
    }, 1000);
    return () => clearInterval(interval);
  }, [isToday]);

  useEffect(() => {
    if (!isToday) return;
    const interval = setInterval(() => {
      void loadWorkspace(true);
    }, status?.isActive ? 2000 : 10000);
    return () => clearInterval(interval);
  }, [status?.isActive, isToday, dateRange.start]);

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

  async function handleSelectSlice(slice: ContextSlice) {    setSelectedSlice(slice);
    setLoadingDetail(true);
    setActionError(null);
    setDetailTab(
      slice.rail === "ocr" || slice.rail === "vision" || slice.rail === "attention"
        ? "visual"
        : "metadata",
    );
    try {
      const details = await invoke<ContextSliceDetail>("get_context_slice_detail", {
        sliceId: slice.id,
        railId: slice.rail,
        startTimestamp: dateRange.start,
        endTimestamp: dateRange.end,
      });
      setSliceDetail(details);
    } catch (error) {
      setActionError(`Failed to load block detail: ${error}`);
    } finally {
      setLoadingDetail(false);
    }
  }

  function clearSelectedSlice() {
    setSelectedSlice(null);
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
    appUsage,
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
    canStartCapture: displayId != null,
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
