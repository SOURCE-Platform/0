import { useEffect, useMemo, useRef, useState } from "react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { addDays, endOfDay, format, startOfDay } from "date-fns";
import {
  Activity,
  AlertCircle,
  CircleHelp,
  ChevronLeft,
  ChevronRight,
  Circle,
  Eye,
  FileSearch,
  PanelRightOpen,
  Shield,
  StopCircle,
  Layers3,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import {
  AppUsageOverview,
  ChannelStatus,
  ContextSlice,
  ContextSliceDetail,
  ContextTimelineData,
  DesktopCaptureStatus,
  OcrReconstruction,
  OcrReviewItem,
  PiiEntity,
  TimelineRail,
} from "@/types/contextTimeline";

const MIN_WINDOW_MS = 2 * 60 * 1000;

function safeFormatDate(value: number | Date | null | undefined, pattern: string, fallback = "Time unavailable") {
  if (value == null) return fallback;

  try {
    return format(value, pattern);
  } catch {
    return fallback;
  }
}

function formatDuration(ms: number) {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const remainder = seconds % 60;

  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m ${remainder}s`;
  return `${remainder}s`;
}

function formatBytes(bytes: number) {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let index = 0;
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024;
    index += 1;
  }
  return `${value >= 10 || index === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`;
}

function formatWindowScale(ms: number) {
  const minutes = Math.round(ms / 60_000);
  if (minutes < 60) return `${minutes}m`;
  if (minutes % 60 === 0) return `${minutes / 60}h`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

function getTimelineTickMs(range: number) {
  if (range <= 15 * 60 * 1000) return 60 * 1000;
  if (range <= 60 * 60 * 1000) return 5 * 60 * 1000;
  return 30 * 60 * 1000;
}

function railTone(railId: string, interactionState?: string | null) {
  if (railId === "system") return "from-slate-500/85 to-slate-400/80";
  if (railId === "focus") return "from-blue-500/85 to-sky-400/80";
  if (railId === "visible_windows") return "from-cyan-500/80 to-cyan-300/70";
  if (railId === "ocr") return "from-amber-500/85 to-orange-400/80";
  if (railId === "evidence") return "from-violet-500/85 to-fuchsia-400/75";
  if (interactionState === "active_typing") return "from-emerald-500/85 to-emerald-300/75";
  if (interactionState === "active_pointer") return "from-sky-500/85 to-blue-300/75";
  if (interactionState === "voice_input_inferred") return "from-orange-500/85 to-orange-300/80";
  if (interactionState === "mixed") return "from-fuchsia-500/85 to-pink-300/80";
  return "from-zinc-500/85 to-zinc-300/75";
}

function parseBoundingBox(raw: unknown): { x: number; y: number; width: number; height: number } | null {
  if (!raw || typeof raw !== "object") return null;
  const candidate = raw as Record<string, unknown>;
  const x = Number(candidate.x);
  const y = Number(candidate.y);
  const width = Number(candidate.width);
  const height = Number(candidate.height);
  if ([x, y, width, height].some((value) => Number.isNaN(value))) return null;
  return { x, y, width, height };
}

function healthTone(health: string) {
  if (health === "healthy") return "default";
  if (health === "off") return "secondary";
  if (health === "warming_up") return "outline";
  return "destructive";
}

function TimelineRuler({
  startTimestamp,
  endTimestamp,
}: {
  startTimestamp: number;
  endTimestamp: number;
}) {
  const range = Math.max(1, endTimestamp - startTimestamp);
  const tickMs = getTimelineTickMs(range);
  const tickCount = Math.max(1, Math.ceil(range / tickMs));

  return (
    <div className="relative h-10 border-b border-border/70 bg-background/60">
      <div className="relative h-full w-full">
        {Array.from({ length: tickCount + 1 }).map((_, index) => {
          const timestamp = startTimestamp + index * tickMs;
          if (timestamp > endTimestamp) return null;
          const left = ((timestamp - startTimestamp) / range) * 100;
          const isLastTick = index === tickCount || timestamp + tickMs > endTimestamp;
          return (
            <div
              key={timestamp}
              className="absolute bottom-0 top-0 border-l border-white/10"
              style={{ left: `${left}%` }}
            >
              <div
                className="absolute top-1 text-[10px] uppercase tracking-wide text-muted-foreground"
                style={isLastTick ? { right: "0.35rem" } : { left: "0.5rem" }}
              >
                {safeFormatDate(timestamp, "p", "--")}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function RailLane({
  rail,
  windowStart,
  windowEnd,
  selectedSliceId,
  onSelect,
  appFilter,
  interactionFilter,
}: {
  rail: TimelineRail;
  windowStart: number;
  windowEnd: number;
  selectedSliceId: string | null;
  onSelect: (slice: ContextSlice) => void;
  appFilter: string;
  interactionFilter: string;
}) {
  const range = Math.max(1, windowEnd - windowStart);
  const tickMs = getTimelineTickMs(range);
  const tickCount = Math.max(1, Math.ceil(range / tickMs));
  const slices = rail.slices.filter((slice) => {
    if (appFilter !== "all" && slice.appName !== appFilter) return false;
    if (interactionFilter !== "all" && rail.id === "interaction" && slice.interactionState !== interactionFilter) return false;
    if (slice.endTimestamp < windowStart || slice.startTimestamp > windowEnd) return false;
    return true;
  });

  return (
    <div className="grid gap-2 md:grid-cols-[10.5rem_minmax(0,1fr)]">
      <div className="sticky left-0 z-10 flex items-center gap-2 bg-muted/10 py-1 backdrop-blur-sm">
        <div className="min-w-0">
          <span className="text-sm font-semibold text-foreground">{rail.label}</span>
        </div>
        <Badge variant="outline" className="text-[10px] uppercase tracking-wide">
            {rail.slices.length}
        </Badge>
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              className="inline-flex h-6 w-6 items-center justify-center rounded-full text-muted-foreground transition hover:text-foreground"
            >
              <CircleHelp className="h-4 w-4" />
            </button>
          </TooltipTrigger>
          <TooltipContent side="right" className="max-w-[32ch] text-left leading-6">
            <div className="space-y-2">
              <p>{rail.description}</p>
              <p>{rail.confidenceNote}</p>
            </div>
          </TooltipContent>
        </Tooltip>
      </div>

      <div className="relative h-16 rounded-2xl border border-border/70 bg-background/65">
        <div className="pointer-events-none absolute inset-y-0 right-0 z-20 w-px bg-blue-400/90 shadow-[0_0_18px_rgba(59,130,246,0.45)]" />
        <div className="relative h-full w-full">
          {Array.from({ length: tickCount + 1 }).map((_, index) => {
            const timestamp = windowStart + index * tickMs;
            if (timestamp > windowEnd) return null;
            const left = ((timestamp - windowStart) / range) * 100;
            return (
              <div
                key={`${rail.id}-${timestamp}`}
                className="pointer-events-none absolute inset-y-0 z-0 border-l border-white/10"
                style={{ left: `${left}%` }}
              />
            );
          })}

          {slices.length === 0 ? (
            <div className="flex h-full items-center justify-center text-xs text-muted-foreground">
              No slices for the current filters.
            </div>
          ) : (
            slices.map((slice) => {
              const clippedStart = Math.max(slice.startTimestamp, windowStart);
              const clippedEnd = Math.min(Math.max(slice.endTimestamp, slice.startTimestamp + 1), windowEnd);
              const left = ((clippedStart - windowStart) / range) * 100;
              const rawWidth = ((Math.max(clippedEnd, clippedStart + 1) - clippedStart) / range) * 100;
              const width = slice.sliceKind === "event" ? Math.max(1.25, rawWidth) : Math.max(2.5, rawWidth);
              const anchor = Math.min(92, Math.max(8, left + width / 2));
              const storageLabel = `${formatBytes(slice.storageBytes)} ${slice.storageExact ? "Exact" : "Estimated"}`;

              return (
                <div key={slice.id}>
                  <button
                    type="button"
                    onClick={() => onSelect(slice)}
                    className={`group absolute top-2 h-12 overflow-hidden rounded-xl border bg-gradient-to-r text-left shadow-sm transition ${
                      selectedSliceId === slice.id
                        ? "border-white/70 ring-1 ring-white/30"
                        : "border-white/10 hover:border-white/35"
                    } ${railTone(rail.id, slice.interactionState)}`}
                    style={{ left: `${left}%`, width: `${width}%`, opacity: Math.max(0.55, slice.confidence) }}
                    title={`${slice.title} • ${safeFormatDate(slice.startTimestamp, "p")} • ${storageLabel}`}
                  >
                    <div className="px-2 py-1.5 text-[11px] font-semibold text-white">
                      {width > 12 ? <div className="truncate">{slice.title}</div> : null}
                      {width > 18 && slice.subtitle ? (
                        <div className="truncate pt-0.5 text-[10px] text-white/85">{slice.subtitle}</div>
                      ) : null}
                    </div>
                  </button>

                  {selectedSliceId === slice.id ? (
                    <div
                      className="pointer-events-none absolute bottom-[calc(100%+0.4rem)] z-30 w-max max-w-[16rem] -translate-x-1/2 rounded-xl border border-white/15 bg-black/85 px-3 py-2 text-left shadow-2xl backdrop-blur"
                      style={{ left: `${anchor}%` }}
                    >
                      <div className="truncate text-[11px] font-semibold text-white">{slice.title}</div>
                      <div className="pt-0.5 text-[10px] uppercase tracking-wide text-white/75">
                        {safeFormatDate(slice.startTimestamp, "p")} • {storageLabel}
                      </div>
                      {slice.subtitle ? (
                        <div className="pt-1 text-[10px] leading-4 text-white/85">{slice.subtitle}</div>
                      ) : null}
                    </div>
                  ) : null}
                </div>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
}

function OcrReconstructionView({ reconstruction }: { reconstruction: OcrReconstruction }) {
  const backdropSrc = reconstruction.framePath ? convertFileSrc(reconstruction.framePath) : null;

  return (
    <div className="space-y-3">
      <div className="text-xs uppercase tracking-wide text-muted-foreground">Reconstructed scene</div>
      <div className="overflow-hidden rounded-2xl border border-border/70 bg-zinc-950/80">
        <div
          className="relative mx-auto w-full"
          style={{
            aspectRatio: `${Math.max(reconstruction.width, 1)} / ${Math.max(reconstruction.height, 1)}`,
          }}
        >
          {backdropSrc && reconstruction.backdropAvailable ? (
            <img
              src={backdropSrc}
              alt="OCR evidence backdrop"
              className="absolute inset-0 h-full w-full object-cover opacity-25 grayscale"
            />
          ) : (
            <div className="absolute inset-0 bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.08),transparent_50%),linear-gradient(180deg,rgba(255,255,255,0.03),rgba(0,0,0,0.08))]" />
          )}

          {reconstruction.blocks.map((block) => {
            const bbox = parseBoundingBox(block.boundingBox);
            if (!bbox) return null;
            return (
              <div
                key={block.id}
                className="absolute overflow-hidden rounded-lg border border-amber-300/45 bg-black/30 shadow-[0_0_0_1px_rgba(255,255,255,0.05)] backdrop-blur-[1px]"
                style={{
                  left: `${(bbox.x / reconstruction.width) * 100}%`,
                  top: `${(bbox.y / reconstruction.height) * 100}%`,
                  width: `${(bbox.width / reconstruction.width) * 100}%`,
                  height: `${(bbox.height / reconstruction.height) * 100}%`,
                }}
              >
                <div className="line-clamp-3 px-2 py-1 text-[10px] leading-4 text-white">
                  {block.text}
                </div>
                {block.piiEntities.length > 0 ? (
                  <div className="absolute inset-x-1 bottom-1 flex flex-wrap gap-1">
                    {block.piiEntities.slice(0, 2).map((entity) => (
                      <span
                        key={entity.id}
                        className="rounded bg-red-500/25 px-1 py-0.5 text-[9px] uppercase tracking-wide text-red-100"
                      >
                        {entity.entityType}
                      </span>
                    ))}
                  </div>
                ) : null}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

export default function DesktopContextWorkspace({ displayId }: { displayId: number | null }) {
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
  const [isPanning, setIsPanning] = useState(false);
  const [nowMs, setNowMs] = useState(() => Date.now());
  const timelineSurfaceRef = useRef<HTMLDivElement | null>(null);
  const dateRangeRef = useRef<{ start: number; end: number }>({
    start: dayStart,
    end: Date.now(),
  });
  const commandPressedRef = useRef(false);
  const panStateRef = useRef<{
    pointerStartX: number;
    windowEndAtDragStart: number;
    width: number;
  } | null>(null);

  const isToday = useMemo(() => startOfDay(new Date()).getTime() === dayStart, [dayStart]);

  const dateRange = useMemo(() => {
    const end = isToday ? nowMs : endOfDay(dayStart).getTime();
    return {
      start: dayStart,
      end,
    };
  }, [dayStart, isToday, nowMs]);

  useEffect(() => {
    dateRangeRef.current = dateRange;
  }, [dateRange]);

  const effectiveWindowEnd = useMemo(() => {
    if (isLiveFollowing && isToday) {
      return dateRange.end;
    }
    if (windowEndTimestamp != null) {
      return Math.min(windowEndTimestamp, dateRange.end);
    }
    return dateRange.end;
  }, [dateRange.end, isLiveFollowing, isToday, windowEndTimestamp]);

  const effectiveWindowStart = useMemo(
    () => Math.max(dateRange.start, effectiveWindowEnd - windowDurationMs),
    [dateRange.start, effectiveWindowEnd, windowDurationMs],
  );

  const appOptions = useMemo(() => {
    const names = new Set<string>();

    timeline?.rails.forEach((rail) =>
      rail.slices.forEach((slice) => {
        const appName = slice.appName?.trim();
        if (appName) {
          names.add(appName);
        }
      }),
    );

    appUsage?.items.forEach((item) => {
      const appName = item.appName?.trim();
      if (appName) {
        names.add(appName);
      }
    });

    return Array.from(names).sort();
  }, [timeline, appUsage]);

  async function loadWorkspace(silent = false) {
    if (!silent) {
      setLoading(true);
    }
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
      if (!silent) {
        setLoading(false);
      }
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

  useEffect(() => {
    const isCommandEvent = (event: KeyboardEvent) =>
      event.key === "Meta" ||
      event.key === "OS" ||
      event.code === "MetaLeft" ||
      event.code === "MetaRight" ||
      event.code === "OSLeft" ||
      event.code === "OSRight";

    const handleKeyDown = (event: KeyboardEvent) => {
      if (isCommandEvent(event)) {
        commandPressedRef.current = true;
      }
    };

    const handleKeyUp = (event: KeyboardEvent) => {
      if (isCommandEvent(event)) {
        commandPressedRef.current = false;
      }
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
      const relativeX =
        rect.width > 0 ? Math.min(Math.max((clientX - rect.left) / rect.width, 0), 1) : 1;
      const currentStart = effectiveWindowStart;
      const currentEnd = effectiveWindowEnd;
      const currentDuration = currentEnd - currentStart;
      const anchorTime = currentStart + currentDuration * relativeX;
      const zoomFactor = deltaY > 0 ? 1.15 : 0.85;
      const maxWindowMs = Math.max(MIN_WINDOW_MS, dateRange.end - dateRange.start);
      const nextDuration = Math.min(
        maxWindowMs,
        Math.max(MIN_WINDOW_MS, Math.round(currentDuration * zoomFactor)),
      );

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
      if (!surface || !(target instanceof Node) || !surface.contains(target)) return;
      if (event.button !== 1) return;
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
      if (!surface || !(target instanceof Node) || !surface.contains(target)) return;
      if (event.button !== 1) return;
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
  }, [dateRange.end, dateRange.start, effectiveWindowEnd, effectiveWindowStart, isToday]);

  async function handleStartCapture() {
    setActionError(null);
    try {
      const nextStatus = await invoke<DesktopCaptureStatus>("start_desktop_capture", {
        displayId,
      });
      setStatus(nextStatus);
      setIsLiveFollowing(true);
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
    setSelectedSlice(slice);
    setLoadingDetail(true);
    setActionError(null);
    setDetailTab(slice.rail === "ocr" ? "visual" : "metadata");
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
  }, [dateRange.end, dateRange.start, isToday, windowDurationMs]);

  const selectedEvidenceSrc = useMemo(() => {
    const linkedPath = sliceDetail?.linkedFilePaths[0] ?? sliceDetail?.slice.evidenceFramePath ?? null;
    return linkedPath ? convertFileSrc(linkedPath) : null;
  }, [sliceDetail]);

  const showVisualTab = !!sliceDetail?.ocrReconstruction || !!selectedEvidenceSrc;

  return (
    <div className="space-y-6">
      {actionError ? (
        <div className="flex items-start gap-2 rounded-xl border border-red-300 bg-red-50 px-4 py-3 text-sm text-red-800 dark:border-red-900 dark:bg-red-950/60 dark:text-red-200">
          <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
          {actionError}
        </div>
      ) : null}

      {status?.warnings?.length ? (
        <div className="rounded-xl border border-amber-300 bg-amber-50 px-4 py-3 text-sm text-amber-900 dark:border-amber-900 dark:bg-amber-950/60 dark:text-amber-100">
          {status.warnings.join(" ")}
        </div>
      ) : null}

      <TooltipProvider>
      <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_360px]">
        <div className="space-y-4">
          <section className="space-y-5">
              <div className="flex flex-wrap items-start justify-between gap-4 px-1">
                <div className="space-y-2">
                  <h2 className="text-2xl font-semibold tracking-tight text-foreground">Device Context Timeline</h2>
                  <p className="max-w-[58ch] text-base leading-8 text-muted-foreground">
                    The live edge is pinned to the far right. New capture
                    blocks appear there and drift left as your device
                    context accumulates over time.
                  </p>
                </div>

                <div className="flex items-center gap-3">
                  <Badge variant={status?.isActive ? "destructive" : "outline"} className="gap-2 px-3 py-1.5">
                    <Circle className={`h-3 w-3 ${status?.isActive ? "fill-current animate-pulse" : ""}`} />
                    {status?.isActive
                      ? status.displayName
                        ? `Capturing on ${status.displayName}`
                        : "Capture running"
                      : "Capture stopped"}
                  </Badge>
                  {!status?.isActive ? (
                    <Button onClick={handleStartCapture} className="gap-2" disabled={displayId == null}>
                      <Circle className="h-3.5 w-3.5 fill-current" />
                      Start Capture
                    </Button>
                  ) : (
                    <Button onClick={handleStopCapture} variant="destructive" className="gap-2">
                      <StopCircle className="h-3.5 w-3.5" />
                      Stop Capture
                    </Button>
                  )}
                </div>
              </div>

              <div className="flex flex-wrap items-center gap-3 px-1">
                <Button variant="outline" size="sm" onClick={() => setDayStart(addDays(dayStart, -1).getTime())}>
                  Previous day
                </Button>
                <Button variant="outline" size="sm" onClick={() => setDayStart(startOfDay(new Date()).getTime())}>
                  Today
                </Button>
                <Button variant="outline" size="sm" onClick={() => setDayStart(addDays(dayStart, 1).getTime())}>
                  Next day
                </Button>
                <Badge variant="outline" className="text-xs">
                  {safeFormatDate(dayStart, "EEEE, MMMM d", "Selected day unavailable")}
                </Badge>

                <div className="ml-auto flex flex-wrap items-center gap-2">
                  <Button variant="outline" size="sm" onClick={() => shiftWindow(-1)} className="gap-2">
                    <ChevronLeft className="h-3.5 w-3.5" />
                    Back
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => shiftWindow(1)}
                    className="gap-2"
                    disabled={effectiveWindowEnd >= dateRange.end}
                  >
                    Forward
                    <ChevronRight className="h-3.5 w-3.5" />
                  </Button>
                  {!isLiveFollowing && isToday ? (
                    <Button variant="secondary" size="sm" onClick={handleJumpToNow}>
                      Jump to Now
                    </Button>
                  ) : null}
                  <Badge variant="outline" className="px-3 py-1.5 text-xs uppercase tracking-wide">
                    Zoom {formatWindowScale(windowDurationMs)}
                  </Badge>
                  <Select value={appFilter} onValueChange={setAppFilter}>
                    <SelectTrigger className="w-[210px]">
                      <SelectValue placeholder="All apps" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="all">All apps</SelectItem>
                      {appOptions.map((app) => (
                        <SelectItem key={app} value={app}>
                          {app}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <Select value={interactionFilter} onValueChange={setInteractionFilter}>
                    <SelectTrigger className="w-[210px]">
                      <SelectValue placeholder="All interaction states" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="all">All interaction states</SelectItem>
                      <SelectItem value="active_typing">Typing</SelectItem>
                      <SelectItem value="active_pointer">Mouse / pointer</SelectItem>
                      <SelectItem value="passive_viewing">Passive viewing</SelectItem>
                      <SelectItem value="voice_input_inferred">Inferred voice input</SelectItem>
                      <SelectItem value="mixed">Mixed</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              </div>

              {loading || !timeline ? (
                <div className="rounded-2xl border border-border/70 bg-muted/25 py-20 text-center text-sm text-muted-foreground">
                  Loading live device timeline...
                </div>
              ) : (
                <div
                  ref={timelineSurfaceRef}
                  className={`border-y border-border/70 bg-transparent px-1 py-4 ${
                    isPanning ? "cursor-grabbing select-none" : "cursor-default"
                  }`}
                >
                  <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border/70 pb-3">
                      <div className="text-sm text-muted-foreground">
                        Showing {safeFormatDate(effectiveWindowStart, "p")} to{" "}
                        {safeFormatDate(effectiveWindowEnd, "p")}
                      </div>
                      <div className="text-sm text-muted-foreground">
                        {status?.isActive
                          ? "Hold Command and scroll to zoom all tracks."
                          : "Capture is stopped. You are reviewing previously recorded data for this day."}
                      </div>
                  </div>
                  <div className="pt-3">
                      <TimelineRuler startTimestamp={effectiveWindowStart} endTimestamp={effectiveWindowEnd} />
                      <div className="space-y-2 pt-2">
                        {timeline.rails.map((rail) => (
                          <RailLane
                            key={rail.id}
                            rail={rail}
                            windowStart={effectiveWindowStart}
                            windowEnd={effectiveWindowEnd}
                            selectedSliceId={selectedSlice?.id ?? null}
                            onSelect={handleSelectSlice}
                            appFilter={appFilter}
                            interactionFilter={interactionFilter}
                          />
                        ))}
                      </div>
                  </div>
                </div>
              )}
          </section>

          <Tabs value={activeView} onValueChange={setActiveView}>
            <TabsList variant="line">
              <TabsTrigger value="timeline" className="gap-2">
                <Activity className="h-4 w-4" />
                Timeline
              </TabsTrigger>
              <TabsTrigger value="apps" className="gap-2">
                <Eye className="h-4 w-4" />
                App Usage
              </TabsTrigger>
              <TabsTrigger value="ocr" className="gap-2">
                <FileSearch className="h-4 w-4" />
                OCR Review
              </TabsTrigger>
              <TabsTrigger value="pii" className="gap-2">
                <Shield className="h-4 w-4" />
                PII Review
              </TabsTrigger>
            </TabsList>

            <TabsContent value="timeline" className="pt-4">
              <Card>
                <CardHeader>
                  <CardTitle>Channel Health</CardTitle>
                  <CardDescription className="max-w-[58ch]">
                    Empty rails should be explainable. These signals show
                    whether each channel is enabled, sampling, and writing
                    new data.
                  </CardDescription>
                </CardHeader>
                <CardContent className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
                  {channelStatuses.map((channel) => (
                    <div key={channel.channel} className="rounded-xl border border-border/70 px-4 py-3">
                      <div className="flex items-center justify-between gap-3">
                        <div>
                          <div className="font-medium capitalize">{channel.channel.split("_").join(" ")}</div>
                          <div className="max-w-[28ch] text-xs leading-5 text-muted-foreground">{channel.details}</div>
                        </div>
                        <Badge variant={healthTone(channel.health) as "default" | "secondary" | "outline" | "destructive"}>
                          {channel.health}
                        </Badge>
                      </div>
                      <div className="mt-3 flex flex-wrap gap-2 text-xs text-muted-foreground">
                        <span>Permission: {channel.permissionState}</span>
                        <span>Samples: {channel.sampleCount}</span>
                        <span>Rate: {channel.throughputPerMinute.toFixed(2)}/min</span>
                      </div>
                    </div>
                  ))}
                </CardContent>
              </Card>
            </TabsContent>

            <TabsContent value="apps" className="pt-4">
              <Card>
                <CardHeader>
                  <CardTitle>App Usage</CardTitle>
                  <CardDescription className="max-w-[58ch]">
                    Focused time, visible-on-screen time, and
                    interaction-qualified time stay separate so “app was
                    open” does not equal “I was actively using it.”
                  </CardDescription>
                </CardHeader>
                <CardContent>
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>App</TableHead>
                        <TableHead>Focused</TableHead>
                        <TableHead>Visible</TableHead>
                        <TableHead>Interacted</TableHead>
                        <TableHead>OCR</TableHead>
                        <TableHead className="text-right">Action</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {appUsage?.items.map((item) => (
                        <TableRow key={`${item.appName}-${item.bundleId}`}>
                          <TableCell>
                            <div className="font-medium">{item.appName}</div>
                            <div className="text-xs text-muted-foreground">{item.bundleId || "Bundle unknown"}</div>
                          </TableCell>
                          <TableCell>{formatDuration(item.focusedTimeMs)}</TableCell>
                          <TableCell>{formatDuration(item.visibleTimeMs)}</TableCell>
                          <TableCell>{formatDuration(item.interactionTimeMs)}</TableCell>
                          <TableCell>{item.ocrHitCount}</TableCell>
                          <TableCell className="text-right">
                            <Button size="sm" variant="outline" onClick={() => { setAppFilter(item.appName); setActiveView("timeline"); }}>
                              Filter timeline
                            </Button>
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </CardContent>
              </Card>
            </TabsContent>

            <TabsContent value="ocr" className="space-y-4 pt-4">
              <Card>
                <CardHeader>
                  <CardTitle>OCR Review</CardTitle>
                  <CardDescription className="max-w-[58ch]">
                    Searchable captured text with app attribution and
                    inline PII badges where detected.
                  </CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                  <Input
                    placeholder="Search OCR text..."
                    value={ocrQuery}
                    onChange={(event) => setOcrQuery(event.target.value)}
                    onBlur={() => void loadReviewData()}
                  />
                  <div className="space-y-3">
                    {ocrItems.map((item) => (
                      <div key={item.id} className="rounded-xl border border-border/70 px-4 py-4">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="text-sm font-medium">{item.appName ?? "Unknown app"}</span>
                          <Badge variant="outline">{safeFormatDate(item.timestamp, "p")}</Badge>
                          <Badge variant="outline">Confidence {Math.round(item.confidence * 100)}%</Badge>
                          {item.piiEntities.map((entity) => (
                            <Badge key={entity.id} variant="secondary">
                              {entity.entityType}
                            </Badge>
                          ))}
                        </div>
                        <p className="mt-3 max-w-[60ch] text-sm leading-6 text-foreground">{item.text}</p>
                      </div>
                    ))}
                    {ocrItems.length === 0 ? (
                      <div className="rounded-xl border border-dashed border-border/70 px-4 py-10 text-center text-sm text-muted-foreground">
                        No OCR text matched the current filters.
                      </div>
                    ) : null}
                  </div>
                </CardContent>
              </Card>
            </TabsContent>

            <TabsContent value="pii" className="space-y-4 pt-4">
              <Card>
                <CardHeader>
                  <CardTitle>PII Review</CardTitle>
                  <CardDescription className="max-w-[58ch]">
                    Detect-only by default. This is the user-facing
                    trust surface for what SOURCE recognized as sensitive.
                  </CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                  <Select value={piiTypeFilter} onValueChange={setPiiTypeFilter}>
                    <SelectTrigger className="w-[240px]">
                      <SelectValue placeholder="All entity types" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="all">All entity types</SelectItem>
                      <SelectItem value="email">Emails</SelectItem>
                      <SelectItem value="phone">Phones</SelectItem>
                      <SelectItem value="government_id">Government IDs</SelectItem>
                      <SelectItem value="credit_card">Credit cards</SelectItem>
                      <SelectItem value="ip_address">IP addresses</SelectItem>
                    </SelectContent>
                  </Select>

                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Timestamp</TableHead>
                        <TableHead>App</TableHead>
                        <TableHead>Type</TableHead>
                        <TableHead>Preview</TableHead>
                        <TableHead>Confidence</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {piiItems.map((item) => (
                        <TableRow key={item.id}>
                          <TableCell>{safeFormatDate(item.timestamp, "p")}</TableCell>
                          <TableCell>{item.appName ?? "Unknown"}</TableCell>
                          <TableCell>{item.entityType}</TableCell>
                          <TableCell className="font-mono text-xs">{item.redactedPreview}</TableCell>
                          <TableCell>{Math.round(item.confidence * 100)}%</TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                  {piiItems.length === 0 ? (
                    <div className="rounded-xl border border-dashed border-border/70 px-4 py-10 text-center text-sm text-muted-foreground">
                      No PII entities matched the current filters.
                    </div>
                  ) : null}
                </CardContent>
              </Card>
            </TabsContent>
          </Tabs>
        </div>

        <Card className="h-fit border-border/70 xl:sticky xl:top-20">
          <CardHeader>
            <div className="flex items-center gap-2">
              <PanelRightOpen className="h-4 w-4 text-muted-foreground" />
              <CardTitle>Block Detail</CardTitle>
            </div>
            <CardDescription className="max-w-[30ch]">
              Click any timeline block to inspect what it captured, how
              large it is, and the exact stored payload.
            </CardDescription>
          </CardHeader>
          <CardContent>
            {!selectedSlice ? (
              <div className="rounded-xl border border-dashed border-border/70 px-4 py-12 text-center text-sm text-muted-foreground">
                No timeline block selected yet.
              </div>
            ) : loadingDetail || !sliceDetail ? (
              <div className="rounded-xl border border-border/70 px-4 py-12 text-center text-sm text-muted-foreground">
                Loading block detail...
              </div>
            ) : (
              <div className="space-y-4">
                <div className="space-y-2">
                  <div className="flex flex-wrap items-center gap-2">
                    <Badge variant="outline">{sliceDetail.railLabel}</Badge>
                    <Badge variant="outline">{sliceDetail.slice.sliceKind}</Badge>
                    <Badge variant="outline">{sliceDetail.storageExact ? "Exact" : "Estimated"}</Badge>
                    {sliceDetail.slice.piiCount > 0 ? (
                      <Badge variant="secondary">{sliceDetail.slice.piiCount} PII matches</Badge>
                    ) : null}
                  </div>
                  <h3 className="text-lg font-semibold">{sliceDetail.slice.title}</h3>
                  <p className="text-sm text-muted-foreground">
                    {safeFormatDate(sliceDetail.occurredAt, "PPpp")} • {formatDuration(sliceDetail.durationMs)}
                  </p>
                </div>

                <div className="grid gap-3 sm:grid-cols-2">
                  <div className="rounded-xl border border-border/70 bg-muted/20 px-3 py-3">
                    <div className="text-xs uppercase tracking-wide text-muted-foreground">Storage</div>
                    <div className="mt-1 text-lg font-semibold text-foreground">{formatBytes(sliceDetail.storageBytes)}</div>
                    <p className="mt-1 text-xs text-muted-foreground">
                      {sliceDetail.storageExact ? "Exact bytes tied to this block." : "Estimated bytes tied to this span."}
                    </p>
                  </div>
                  <div className="rounded-xl border border-border/70 bg-muted/20 px-3 py-3">
                    <div className="text-xs uppercase tracking-wide text-muted-foreground">Rows / Files</div>
                    <div className="mt-1 text-lg font-semibold text-foreground">
                      {sliceDetail.rowCount} rows • {sliceDetail.fileCount} files
                    </div>
                    <p className="mt-1 text-xs text-muted-foreground">Source-backed storage accounting for this block.</p>
                  </div>
                </div>

                <Tabs value={detailTab} onValueChange={setDetailTab}>
                  <TabsList variant="line">
                    {showVisualTab ? <TabsTrigger value="visual">Visual</TabsTrigger> : null}
                    <TabsTrigger value="metadata">Metadata</TabsTrigger>
                    <TabsTrigger value="json">Raw JSON</TabsTrigger>
                  </TabsList>

                  {showVisualTab ? (
                    <TabsContent value="visual" className="space-y-4 pt-4">
                      {sliceDetail.ocrReconstruction ? (
                        <OcrReconstructionView reconstruction={sliceDetail.ocrReconstruction} />
                      ) : selectedEvidenceSrc ? (
                        <div className="space-y-3">
                          <div className="text-xs uppercase tracking-wide text-muted-foreground">Linked evidence frame</div>
                          <img
                            src={selectedEvidenceSrc}
                            alt="Evidence frame"
                            className="w-full rounded-xl border border-border/70 object-cover"
                          />
                        </div>
                      ) : (
                        <div className="rounded-xl border border-dashed border-border/70 px-4 py-10 text-center text-sm text-muted-foreground">
                          No visual reconstruction is available for this block.
                        </div>
                      )}
                    </TabsContent>
                  ) : null}

                  <TabsContent value="metadata" className="space-y-4 pt-4">
                    <div className="space-y-3">
                      <div>
                        <div className="text-xs uppercase tracking-wide text-muted-foreground">Focused app</div>
                        <div className="mt-1 text-sm text-foreground">{sliceDetail.focusedApp ?? "Unknown"}</div>
                      </div>

                      <div>
                        <div className="text-xs uppercase tracking-wide text-muted-foreground">Source</div>
                        <div className="mt-1 text-sm text-foreground">{sliceDetail.slice.source}</div>
                      </div>

                      <div>
                        <div className="flex items-center justify-between gap-2">
                          <div className="text-xs uppercase tracking-wide text-muted-foreground">Visible windows</div>
                          <Badge variant="outline" className="gap-1 text-[10px] uppercase tracking-wide">
                            <Layers3 className="h-3 w-3" />
                            {sliceDetail.visibleWindows.length}
                          </Badge>
                        </div>
                        {sliceDetail.visibleWindows.length === 0 ? (
                          <p className="mt-1 text-sm text-muted-foreground">No visible-window snapshot was available for this moment.</p>
                        ) : (
                          <div className="mt-2 max-h-72 space-y-2 overflow-y-auto pr-1">
                            {sliceDetail.visibleWindows.map((window) => (
                              <div
                                key={`${window.bundleId}-${window.processId}`}
                                className="rounded-lg border border-border/60 bg-muted/25 px-3 py-2 text-sm"
                              >
                                <div className="flex items-start justify-between gap-2">
                                  <div className="min-w-0">
                                    <div className="truncate font-medium">{window.appName}</div>
                                    <div className="truncate text-[11px] text-muted-foreground">
                                      {window.bundleId || "Bundle unknown"}
                                    </div>
                                  </div>
                                  <Badge variant={window.isFrontmost ? "default" : "outline"} className="shrink-0 text-[10px] uppercase tracking-wide">
                                    {window.isFrontmost ? "Frontmost" : `${Math.round(window.confidence * 100)}%`}
                                  </Badge>
                                </div>
                              </div>
                            ))}
                          </div>
                        )}
                      </div>

                      <div>
                        <div className="text-xs uppercase tracking-wide text-muted-foreground">Reasons</div>
                        <ul className="mt-2 space-y-1 text-sm text-foreground">
                          {sliceDetail.interactionReasons.map((reason) => (
                            <li key={reason}>{reason}</li>
                          ))}
                        </ul>
                      </div>

                      {sliceDetail.piiEntities.length > 0 ? (
                        <div>
                          <div className="text-xs uppercase tracking-wide text-muted-foreground">Detected PII</div>
                          <div className="mt-2 space-y-2">
                            {sliceDetail.piiEntities.map((entity) => (
                              <div key={entity.id} className="rounded-lg bg-muted/35 px-3 py-2 text-sm">
                                <div className="font-medium">
                                  {entity.entityType} • {entity.redactedPreview}
                                </div>
                                <div className="text-xs text-muted-foreground">
                                  Confidence {Math.round(entity.confidence * 100)}%
                                </div>
                              </div>
                            ))}
                          </div>
                        </div>
                      ) : null}

                      {sliceDetail.linkedFilePaths.length > 0 ? (
                        <div>
                          <div className="text-xs uppercase tracking-wide text-muted-foreground">Linked files</div>
                          <div className="mt-2 space-y-2">
                            {sliceDetail.linkedFilePaths.map((path) => (
                              <p key={path} className="break-all rounded-lg bg-muted/35 px-3 py-2 font-mono text-xs text-foreground">
                                {path}
                              </p>
                            ))}
                          </div>
                        </div>
                      ) : null}
                    </div>
                  </TabsContent>

                  <TabsContent value="json" className="space-y-4 pt-4">
                    {sliceDetail.rawPayloads.map((payload) => (
                      <div key={payload.label} className="space-y-2">
                        <div className="text-xs uppercase tracking-wide text-muted-foreground">{payload.label}</div>
                        <pre className="overflow-x-auto rounded-xl border border-border/70 bg-black/20 p-4 text-xs leading-6 text-foreground">
{payload.rawJson}
                        </pre>
                      </div>
                    ))}
                  </TabsContent>
                </Tabs>
              </div>
            )}
          </CardContent>
        </Card>
      </div>
      </TooltipProvider>
    </div>
  );
}
