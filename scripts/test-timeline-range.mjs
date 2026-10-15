import assert from "node:assert/strict";
import { bufferedTimelineRange, timelineCoversRange } from "../src/components/desktop-context-workspace/timelineRange.ts";

const minute = 60_000;
const bounds = { start: 0, end: 24 * 60 * minute };
const view = { start: 120 * minute, end: 130 * minute };
const buffer = bufferedTimelineRange(view, bounds);
assert.deepEqual(buffer, { start: 115 * minute, end: 135 * minute });
const cached = { startTimestamp: buffer.start, endTimestamp: buffer.end };
assert.ok(timelineCoversRange(cached, { start: 118 * minute, end: 133 * minute }), "A nearby zoom should reuse loaded blocks");
assert.ok(!timelineCoversRange(cached, { start: 100 * minute, end: 133 * minute }), "An exposed range beyond the buffer must load data");
assert.ok(!timelineCoversRange(null, view));
assert.ok(!timelineCoversRange(cached, { start: 1500 * minute, end: 1510 * minute }), "A different day cannot reuse this overview");
assert.deepEqual(bufferedTimelineRange({ start: 0, end: 10 * minute }, bounds), { start: 0, end: 15 * minute });
assert.deepEqual(bufferedTimelineRange({ start: 130 * minute, end: 140 * minute }, { start: 0, end: 140 * minute }), { start: 125 * minute, end: 140 * minute });
const wide = { start: 180 * minute, end: 900 * minute };
assert.deepEqual(bufferedTimelineRange(wide, bounds), { start: 150 * minute, end: 930 * minute }, "Extra preload is capped, even for a wide zoom");
console.log("Timeline range buffer checks passed.");
