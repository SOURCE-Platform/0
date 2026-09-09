import { Dispatch, SetStateAction, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  AsrSegment,
  ContextSlice,
  ContextSliceDetail,
  ContextTimelineData,
  TimelineRail,
} from "@/types/contextTimeline";

interface LiveTranscriptOptions {
  timeline: ContextTimelineData | null;
  selectedSlice: ContextSlice | null;
  setSelectedSlice: Dispatch<SetStateAction<ContextSlice | null>>;
  setSliceDetail: Dispatch<SetStateAction<ContextSliceDetail | null>>;
}

function findSlice(rails: TimelineRail[], railId: string, sliceId: string): ContextSlice | null {
  for (const rail of rails) {
    if (rail.id === railId) {
      const slice = rail.slices.find((item) => item.id === sliceId);
      if (slice) return slice;
    }
    const nested = findSlice(rail.children, railId, sliceId);
    if (nested) return nested;
  }
  return null;
}

function mergeSegment(slice: ContextSlice, segment: AsrSegment): ContextSlice {
  const transcript = segment.transcript.trim();
  const stateTag = segment.isFinal ? "final" : "live";
  return {
    ...slice,
    endTimestamp: segment.endTimestamp,
    title: transcript.length > 96 ? `${transcript.slice(0, 93)}…` : transcript,
    confidence: segment.confidence ?? slice.confidence,
    ocrPreview: transcript,
    tags: [...slice.tags.filter((tag) => tag !== "live" && tag !== "final"), stateTag],
  };
}

function applySlice(
  latest: ContextSlice,
  setSelectedSlice: LiveTranscriptOptions["setSelectedSlice"],
  setSliceDetail: LiveTranscriptOptions["setSliceDetail"],
) {
  setSelectedSlice((current) => {
    if (!current || current.id !== latest.id || current.rail !== latest.rail) return current;
    if (current.endTimestamp === latest.endTimestamp && current.ocrPreview === latest.ocrPreview) {
      return current;
    }
    return latest;
  });
  setSliceDetail((current) => current ? {
    ...current,
    slice: latest,
    occurredAt: latest.startTimestamp,
    durationMs: Math.max(1, latest.endTimestamp - latest.startTimestamp),
    storageBytes: latest.storageBytes,
    storageExact: latest.storageExact,
    rowCount: latest.rowCount,
    fileCount: latest.fileCount,
    interactionReasons: latest.reasons,
  } : current);
}

export function useLiveTranscript({
  timeline,
  selectedSlice,
  setSelectedSlice,
  setSliceDetail,
}: LiveTranscriptOptions) {
  useEffect(() => {
    if (!timeline || !selectedSlice) return;
    const latest = findSlice(timeline.rails, selectedSlice.rail, selectedSlice.id);
    if (latest) applySlice(latest, setSelectedSlice, setSliceDetail);
  }, [timeline, selectedSlice?.id, selectedSlice?.rail]);

  useEffect(() => {
    if (!selectedSlice?.tags.includes("asr")) return;
    let stopped = false;
    let pending = false;
    const refresh = async () => {
      if (pending) return;
      pending = true;
      try {
        const segment = await invoke<AsrSegment | null>("get_asr_segment", {
          asrSegmentId: selectedSlice.id,
        });
        if (!stopped && segment) {
          applySlice(mergeSegment(selectedSlice, segment), setSelectedSlice, setSliceDetail);
          setSliceDetail((current) => current ? {
            ...current,
            rawPayloads: [{
              label: "ASR segment",
              rawJson: JSON.stringify(segment, null, 2),
            }],
          } : current);
        }
      } catch {
        // The app can briefly restart while the Rust backend recompiles.
      } finally {
        pending = false;
      }
    };
    void refresh();
    const interval = window.setInterval(() => void refresh(), 500);
    return () => {
      stopped = true;
      window.clearInterval(interval);
    };
  }, [selectedSlice?.id, selectedSlice?.rail]);
}

export function immediateSliceDetail(slice: ContextSlice): ContextSliceDetail {
  const railLabel = {
    audio_dictation: "Right Option Dictation",
    audio_ambient_speech: "Ambient Audio",
    audio_sound_events: "Sound Events",
    audio_mobile: "Source Mobile",
  }[slice.rail] ?? "Audio";
  return {
    slice,
    railLabel,
    occurredAt: slice.startTimestamp,
    durationMs: Math.max(1, slice.endTimestamp - slice.startTimestamp),
    storageBytes: slice.storageBytes,
    storageExact: slice.storageExact,
    rowCount: slice.rowCount,
    fileCount: slice.fileCount,
    linkedFilePaths: [],
    focusedApp: slice.appName,
    focusedBundleId: null,
    visibleWindows: slice.visibleWindows,
    interactionReasons: slice.reasons,
    piiEntities: [],
    rawPayloads: [],
    ocrReconstruction: null,
    nearbySystemEvents: [],
  };
}
