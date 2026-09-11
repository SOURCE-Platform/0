import { useLayoutEffect, useRef, useState } from "react";
import { ContextSlice, TimelineRail } from "@/types/contextTimeline";
import {
  formatBytes,
  getSnappedTicks,
  getTimelineTickMs,
  railTone,
  safeFormatDate,
  sliceIsVisible,
} from "@/components/desktop-context-workspace/utils";

interface RailLaneProps {
  rail: TimelineRail;
  depth: number;
  windowStart: number;
  windowEnd: number;
  selectedSliceId: string | null;
  selectedRailId: string | null;
  onSelect: (slice: ContextSlice) => void;
  onDeselect: () => void;
  appFilter: string;
  interactionFilter: string;
}

export function RailLane({
  rail,
  depth,
  windowStart,
  windowEnd,
  selectedSliceId,
  selectedRailId,
  onSelect,
  onDeselect,
  appFilter,
  interactionFilter,
}: RailLaneProps) {
  const range = Math.max(1, windowEnd - windowStart);
  const tickMs = getTimelineTickMs(range);
  const ticks = getSnappedTicks(windowStart, windowEnd, tickMs);
  const [hoveredId, setHoveredId] = useState<string | null>(null);
  const trackRef = useRef<HTMLDivElement>(null);
  const [trackWidth, setTrackWidth] = useState(0);
  useLayoutEffect(() => {
    const el = trackRef.current;
    if (!el) return;
    setTrackWidth(el.clientWidth);
    const observer = new ResizeObserver((entries) => {
      setTrackWidth(entries[0]?.contentRect.width ?? 0);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  const slices = rail.slices.filter((slice) =>
    sliceIsVisible(slice, windowStart, windowEnd, appFilter, interactionFilter, rail.id),
  );
  const waveformSamples = (rail.waveform?.samples ?? []).filter(
    (sample) => sample.timestamp >= windowStart && sample.timestamp <= windowEnd,
  );
  const hasWaveform = waveformSamples.length > 0;
  const hasCapturedSlicesOutsideView =
    (rail.slices.length > 0 || (rail.waveform?.samples.length ?? 0) > 0) &&
    slices.length === 0 &&
    !hasWaveform;
  const railPadding = depth * 16;

  return (
    <div className="-mb-px grid gap-2 md:grid-cols-[8rem_minmax(0,1fr)]">
      <div
        className="flex min-w-0 items-center py-1 pr-3"
        style={{ paddingLeft: railPadding }}
      >
        <span className="text-xs font-normal leading-5 text-muted-foreground">{rail.label}</span>
      </div>

      <div ref={trackRef} className="relative h-16 overflow-x-clip border border-border/70 bg-background/65">
        <div
          className="relative h-full w-full"
          onClick={(event) => {
            if (event.target === event.currentTarget) onDeselect();
          }}
        >
          {ticks.map((timestamp) => {
            const left = ((timestamp - windowStart) / range) * 100;
            return (
              <div
                key={`${rail.id}-${timestamp}`}
                className="pointer-events-none absolute inset-y-0 z-0 border-l border-white/10"
                style={{ left: `${left}%` }}
              />
            );
          })}

          {hasWaveform ? <ContinuousWaveform samples={waveformSamples} windowStart={windowStart} range={range} /> : null}
          {slices.length === 0 && !hasWaveform ? (
            <div className="flex h-full items-center justify-center text-xs text-muted-foreground">
              {hasCapturedSlicesOutsideView
                ? "Captured blocks are outside this time view. Jump to Now to see them."
                : "No slices for the current filters."}
            </div>
          ) : (
            slices.map((slice) => {
              const clippedStart = Math.max(slice.startTimestamp, windowStart);
              const clippedEnd = Math.min(Math.max(slice.endTimestamp, slice.startTimestamp + 1), windowEnd);
              const left = ((clippedStart - windowStart) / range) * 100;
              const rawWidth = ((Math.max(clippedEnd, clippedStart + 1) - clippedStart) / range) * 100;
              const fullWidth = slice.sliceKind === "event" ? Math.max(1.25, rawWidth) : Math.max(2.5, rawWidth);
              // Minimum widths keep slivers clickable, but a block must never
              // draw past the lane edge: clamp the stub inside the boundary
              // and drop slices that sit entirely outside it.
              const clampedLeft = Math.min(Math.max(left, 0), 100);
              const width = Math.min(fullWidth, 100 - clampedLeft);
              if (width <= 0) return null;
              const anchor = Math.min(92, Math.max(8, clampedLeft + width / 2));
              // Tooltips center over their block. Only when centering would
              // push past the lane's right edge does the tooltip pin its
              // right edge to the block's right edge instead.
              const tipHalfPx = 128;
              const tipOverflowsRight =
                trackWidth > 0 && (anchor / 100) * trackWidth + tipHalfPx > trackWidth;
              const storageLabel = `${formatBytes(slice.storageBytes)} ${slice.storageExact ? "Exact" : "Estimated"}`;
              const layout = getSliceLayout(rail, slice);
              const isSelected = selectedSliceId === slice.id && selectedRailId === rail.id;
              const sourceLabel = getAudioSourceLabel(slice);
              const transcript = slice.tags.includes("asr")
                ? (slice.ocrPreview ?? slice.title).trim()
                : "";

              return (
                <div key={slice.id}>
                  <button
                    type="button"
                    onClick={() => onSelect(slice)}
                    onMouseEnter={() => setHoveredId(slice.id)}
                    onMouseLeave={() => setHoveredId((current) => (current === slice.id ? null : current))}
                    className={`group absolute top-2 h-12 cursor-pointer overflow-hidden border text-left shadow-sm transition ${
                      isSelected
                        ? "z-10 border-white/70 ring-1 ring-white/30"
                        : "border-white/10 hover:border-white/35"
                    } ${railTone(rail.id, slice.interactionState)}`}
                    style={{
                      left: `${clampedLeft}%`,
                      width: `${width}%`,
                      top: `${layout.topPx}px`,
                      height: `${layout.heightPx}px`,
                      opacity: layout.opacity,
                    }}
                  >
                    {transcript && layout.showInlineTitle ? (
                      <div className="line-clamp-4 px-1.5 pt-0.5 text-[10px] font-normal leading-[11px] text-white/90">
                        {transcript}
                      </div>
                    ) : null}
                  </button>

                  {hoveredId === slice.id ? (
                    <div
                      className={`pointer-events-none absolute bottom-[calc(100%+0.4rem)] z-30 w-max max-w-[16rem] rounded-xl border border-white/15 bg-black/85 px-3 py-2 text-left shadow-2xl backdrop-blur ${
                        tipOverflowsRight ? "" : "-translate-x-1/2"
                      }`}
                      style={
                        tipOverflowsRight
                          ? { right: `${100 - (clampedLeft + width)}%` }
                          : { left: `${anchor}%` }
                      }
                    >
                      <div className="truncate text-[11px] font-semibold text-white">{slice.title}</div>
                      {sourceLabel ? (
                        <div className="pt-0.5 text-[10px] uppercase tracking-wide text-white/75">
                          {sourceLabel} source
                        </div>
                      ) : null}
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

function ContinuousWaveform({
  samples,
  windowStart,
  range,
}: {
  samples: Array<{ timestamp: number; level: number }>;
  windowStart: number;
  range: number;
}) {
  const maxPoints = 300;
  const step = Math.max(1, Math.ceil(samples.length / maxPoints));
  const bars = samples.filter((_, index) => index % step === 0);
  return (
    <div className="pointer-events-none absolute inset-x-1 inset-y-2 z-[1] overflow-hidden bg-emerald-400/[0.03]">
      <svg className="h-full w-full" preserveAspectRatio="none" viewBox="0 0 100 100">
        {bars.map((sample, index) => {
          const x = ((sample.timestamp - windowStart) / range) * 100;
          const amplitude = Math.max(3, Math.min(44, sample.level * 48));
          return (
            <line
              key={`${sample.timestamp}-${index}`}
              x1={x}
              x2={x}
              y1={50 - amplitude}
              y2={50 + amplitude}
              stroke="rgba(74, 222, 128, 0.9)"
              strokeWidth="0.35"
            />
          );
        })}
      </svg>
    </div>
  );
}

function getSliceLayout(rail: TimelineRail, slice: ContextSlice) {
  const confidence = Math.max(0.2, Math.min(1, slice.confidence));
  const isEmotionSummary = rail.id === "audio_emotion_summary_lane";
  const isEmotionDetail = rail.id.startsWith("audio_emotion_") && rail.id !== "audio_emotion_summary_lane";
  const isAsrEvent = rail.id === "audio_speech" && slice.tags.includes("asr");

  if (isAsrEvent) {
    return { topPx: 20, heightPx: 22, opacity: Math.max(0.65, confidence), showInlineTitle: true, showInlineSubtitle: false };
  }

  if (isEmotionSummary) {
    const polarity = slice.tags.includes("positive")
      ? "positive"
      : slice.tags.includes("negative")
        ? "negative"
        : slice.tags.includes("centered")
          ? "centered"
          : "uncertain";
    if (polarity === "positive") {
      const heightPx = Math.max(10, confidence * 20);
      return { topPx: 28 - heightPx, heightPx, opacity: Math.max(0.6, confidence), showInlineTitle: false, showInlineSubtitle: false };
    }
    if (polarity === "negative") {
      const heightPx = Math.max(10, confidence * 20);
      return { topPx: 28, heightPx, opacity: Math.max(0.6, confidence), showInlineTitle: false, showInlineSubtitle: false };
    }
    if (slice.tags.includes("uncertain")) {
      return { topPx: 24, heightPx: 10, opacity: 0.45, showInlineTitle: false, showInlineSubtitle: false };
    }
    const heightPx = Math.max(8, confidence * 14);
    return { topPx: 28 - heightPx / 2, heightPx, opacity: Math.max(0.55, confidence), showInlineTitle: false, showInlineSubtitle: false };
  }

  if (isEmotionDetail) {
    const heightPx = Math.max(10, confidence * 38);
    return { topPx: 50 - heightPx, heightPx, opacity: Math.max(0.55, confidence), showInlineTitle: false, showInlineSubtitle: false };
  }

  return { topPx: 8, heightPx: 48, opacity: Math.max(0.55, confidence), showInlineTitle: true, showInlineSubtitle: true };
}

function getAudioSourceLabel(slice: ContextSlice): string | null {
  if (!slice.tags.includes("audio")) return null;
  if (slice.tags.includes("dictation")) return "Right Option";
  if (slice.tags.includes("mobile") || slice.source === "source-mobile") return "Mobile";
  if (slice.source.startsWith("desktop_output:")) return "Desktop";
  if (slice.source.startsWith("microphone-name:")) {
    const name = slice.source.slice("microphone-name:".length).trim();
    return name || "Mic";
  }
  if (slice.source.startsWith("microphone:")) return "Mic";
  return "Audio";
}
