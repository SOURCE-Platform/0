import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer } from "vite";

const root = fileURLToPath(new URL("../", import.meta.url));
// SSR only: no browser, HTTP listener, microphone, or native commands.
const server = await createServer({
  configFile: false, root, appType: "custom", logLevel: "error",
  optimizeDeps: { noDiscovery: true, entries: [] },
  resolve: { alias: { "@": `${root}src` } },
  server: { middlewareMode: true, hmr: false, watch: null },
});
try {
  const { DeviceContextTimelineSection } = await server.ssrLoadModule(
    "/src/components/desktop-context-workspace/DeviceContextTimelineSection.tsx",
  );
  const slice = {
    id: "zoom-test", rail: "audio_dictation", sliceKind: "span",
    startTimestamp: 600_000, endTimestamp: 900_000, title: "Zoom regression block",
    subtitle: null, source: "fluid-voice-prompt", confidence: 1, sessionId: null,
    appName: null, windowTitle: null, interactionState: null, reasons: [],
    visibleWindows: [], ocrPreview: null, piiCount: 0, evidenceFramePath: null,
    storageBytes: 0, storageExact: false, rowCount: 1, fileCount: 0,
    hasDetailView: true, tags: ["audio", "dictation", "asr", "final"],
  };
  const timeline = { rails: [{ id: "audio_dictation", kind: "lane", label: "Right Option Dictation",
    description: "", confidenceNote: "", defaultExpanded: true, slices: [slice], children: [] }] };
  const controller = {
    timeline, loading: false, selectedSlice: null, isToday: false, isLiveFollowing: false,
    dayStart: 0, effectiveWindowStart: 0, effectiveWindowEnd: 3_600_000,
    dateRange: { start: 0, end: 7_200_000 }, status: { isActive: true },
    timelineSurfaceRef: { current: null }, appFilter: "all", interactionFilter: "all",
  };
  const render = (changes) => renderToStaticMarkup(React.createElement(
    DeviceContextTimelineSection, { controller: { ...controller, ...changes } },
  ));
  const initial = render({ timeline: null, loading: true });
  assert.ok(initial.includes("Loading live device timeline"), "Cold start still needs a loading state");
  const ready = render({});
  assert.ok(ready.includes("Zoom regression block"));
  // The old implementation replaced the whole timeline with a loading box here.
  const refreshing = render({ loading: true });
  assert.ok(!refreshing.includes("Loading live device timeline"), "A refresh must not blank a populated timeline");
  assert.ok(refreshing.includes("Zoom regression block"), "Existing blocks remain visible during the request");
  assert.equal(refreshing, ready, "Refresh state must not change the timeline's rendered structure");
  const zoomed = render({ loading: true, effectiveWindowEnd: 7_200_000 });
  assert.ok(zoomed.includes("Zoom regression block"), "Cached blocks remain visible at the new zoom scale");
  console.log("Timeline zoom rendering regression checks passed.");
} finally {
  await server.close();
}
