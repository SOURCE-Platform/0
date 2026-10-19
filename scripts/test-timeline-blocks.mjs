import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer } from "vite";

// Exercise the real lane renderer without a browser or native app interaction.
const root = fileURLToPath(new URL("../", import.meta.url));
const server = await createServer({
  configFile: false, root, appType: "custom", logLevel: "error",
  optimizeDeps: { noDiscovery: true, entries: [] },
  resolve: { alias: { "@": `${root}src` } },
  server: { middlewareMode: true, hmr: false, ws: false, watch: null },
});
try {
  const { layoutTimelineSlices } = await server.ssrLoadModule(
    "/src/components/desktop-context-workspace/sliceGeometry.ts",
  );
  const { RailLane } = await server.ssrLoadModule(
    "/src/components/desktop-context-workspace/RailLane.tsx",
  );
  const minute = 60_000;
  const makeSlice = (id, start, end, sliceKind = "span") => ({
    id, startTimestamp: start, endTimestamp: end, sliceKind,
    title: id, rail: "audio", confidence: 1, tags: [], source: "ambient_audio",
    storageBytes: 0, storageExact: false,
  });
  const audio = Array.from({ length: 80 }, (_, index) =>
    makeSlice(`audio-${index}`, index * 61_000, index * 61_000 + minute));
  const ocr = Array.from({ length: 80 }, (_, index) =>
    makeSlice(`ocr-${index}`, index * 15_000, index * 15_000 + 5_000, "event"));

  for (const slices of [audio, ocr]) {
    for (const range of [2 * minute, 10 * minute, 60 * minute, 24 * 60 * minute]) {
      for (const pixels of [320, 800, 1800]) {
        const blocks = layoutTimelineSlices([...slices].reverse(), 0, range, pixels);
        for (let index = 0; index < blocks.length; index++) {
          const current = blocks[index];
          assert.ok(current.left >= 0 && current.width > 0);
          assert.ok(current.left + current.width <= 100 + 1e-9, "Stay inside the track");
          const next = blocks[index + 1];
          if (next) {
            assert.ok(current.left + current.width < next.left,
              `Separate captures must retain a gap at range ${range}, width ${pixels}`);
          }
        }
      }
      const rail = { id: "audio", label: "Audio", slices, children: [] };
      const markup = renderToStaticMarkup(React.createElement(RailLane, {
        rail, depth: 0, windowStart: 0, windowEnd: range,
        selectedSliceId: null, selectedRailId: null,
        appFilter: "all", interactionFilter: "all", onSelect() {}, onDeselect() {},
      }));
      const button = markup.match(/<button[^>]*style="([^"]+)"/)[1];
      const width = Number(button.match(/width:([\d.e+-]+)%/)[1]);
      assert.ok(Math.abs(width - slices[0].endTimestamp / range * 100) < 1e-9,
        "The real renderer must use the time-scaled width, including on cold layout");
      assert.ok(markup.includes("border-0") && markup.includes("ring-inset"),
        "Borders must not force tiny buttons wider than their calculated bounds");
    }
  }

  const points = [makeSlice("p1", 10, 10), makeSlice("p2", 10.1, 10.1)];
  const markers = layoutTimelineSlices(points, 0, 100, 100);
  assert.equal(markers.length, 2, "Instantaneous events still have visible markers");
  assert.ok(markers[0].left + markers[0].width < markers[1].left);
  assert.equal(layoutTimelineSlices([points[0], points[0]], 0, 100, 800).length, 2,
    "Simultaneous point events must not discard each other");
  const adjacent = layoutTimelineSlices([
    makeSlice("a", 10, 10.01), makeSlice("b", 10.01, 10.02),
  ], 0, 100, 800);
  assert.ok(Math.abs(adjacent[0].left + adjacent[0].width - adjacent[1].left) < 1e-9,
    "Touching intervals must remain touching without an invented overlap");
  const clipped = layoutTimelineSlices([
    makeSlice("before", -20, 0), makeSlice("left", -10, 10),
    makeSlice("right", 90, 110), makeSlice("after", 100, 110),
  ], 0, 100, 800);
  assert.deepEqual(clipped.map(({ slice, left, width }) => [slice.id, left, width]),
    [["left", 0, 10], ["right", 90, 10]]);
  const realOverlap = layoutTimelineSlices([
    makeSlice("a", 0, 60), makeSlice("b", 30, 90),
  ], 0, 100, 800);
  assert.equal(realOverlap[0].width, 60, "Do not truncate activity that genuinely overlaps");
  const tiny = layoutTimelineSlices([makeSlice("tiny", 10, 10.01)], 0, 100, 800)[0];
  assert.equal(tiny.width, 100 / 800, "An isolated tiny event uses a one-pixel marker");
  console.log("Timeline block geometry and rendering regression checks passed.");
} finally {
  await server.close();
}
