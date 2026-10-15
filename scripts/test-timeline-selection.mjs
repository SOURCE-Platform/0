import assert from "node:assert/strict";
import { mergeOverviewSlice, mergeOverviewDetail } from "../src/components/desktop-context-workspace/timelineSelection.ts";

// A lightweight overview refresh must never erase the user's loaded dictation.
const loaded = {
  id: "dictation-1", rail: "audio_dictation", startTimestamp: 100, endTimestamp: 200,
  confidence: 0.9, title: "Words I need to copy", ocrPreview: "Words I need to copy",
  storageBytes: 4200, storageExact: true, rowCount: 8, tags: ["asr", "final"],
};
const overview = {
  ...loaded, title: "Dictation", ocrPreview: null, storageBytes: 0,
  storageExact: false, rowCount: 0,
};
assert.equal(mergeOverviewSlice(loaded, overview), loaded);
const refreshed = mergeOverviewSlice(loaded, { ...overview, endTimestamp: 300 });
assert.equal(refreshed.ocrPreview, loaded.ocrPreview);
assert.equal(refreshed.title, loaded.title);
assert.equal(refreshed.storageBytes, 4200);
assert.equal(refreshed.storageExact, true);
assert.equal(refreshed.endTimestamp, 300);

const detail = { slice: loaded, occurredAt: 100, durationMs: 100, storageBytes: 4200, rawPayloads: [{ rawJson: "saved" }] };
const refreshedDetail = mergeOverviewDetail(detail, { ...overview, endTimestamp: 300 });
assert.equal(refreshedDetail.slice.ocrPreview, loaded.ocrPreview);
assert.equal(refreshedDetail.durationMs, 200);
assert.equal(refreshedDetail.rawPayloads, detail.rawPayloads);
assert.equal(refreshedDetail.storageBytes, 4200);

// A refresh for an old selection cannot overwrite the block now being read.
assert.equal(mergeOverviewDetail(detail, { ...overview, id: "different" }), detail);
assert.equal(mergeOverviewDetail(null, overview), null);
// Keep polling a live selection until ASR itself delivers the final text.
const live = { ...loaded, tags: ["asr", "live"] };
assert.deepEqual(mergeOverviewSlice(live, { ...overview, endTimestamp: 300 }).tags, ["asr", "live"]);
console.log("Timeline selection regression checks passed.");
