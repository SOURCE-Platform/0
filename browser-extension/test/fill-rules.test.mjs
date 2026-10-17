// BR-08 and the §9.6 delivery rules, run against the real extension
// scripts with stubbed `chrome` / DOM objects (no browser needed).
// Run: node --test browser-extension/test/
// Synthetic values only.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";

const dir = new URL("../", import.meta.url);
const read = (f) => readFileSync(new URL(f, dir), "utf8");

function field(type) {
  return { type, value: "", events: [], focus() {}, dispatchEvent(e) { this.events.push(e.type); } };
}

// One frame running content.js at `origin`; returns its fields and listener.
function frame(origin) {
  const user = field("email");
  const password = field("password");
  password.form = { querySelector: () => user };
  let listener;
  const ctx = {
    window: { location: { origin } },
    document: { querySelector: (q) => (q.includes("password") ? password : null) },
    Event: class { constructor(type) { this.type = type; } },
    chrome: { runtime: { onMessage: { addListener: (f) => (listener = f) } } },
  };
  vm.runInNewContext(read("content.js"), ctx);
  return { user, password, send: (m) => listener(m) };
}

const fill = (origin) => ({ type: "fill", origin, fill: { username: "alice@example.test", password: "synthetic-pw" } });

test("BR-08: only a frame whose own origin is the approved one fills", () => {
  const top = frame("https://example.test");
  top.send(fill("https://example.test"));
  assert.equal(top.password.value, "synthetic-pw");
  assert.equal(top.user.value, "alice@example.test");
  assert.deepEqual(top.password.events, ["input", "change"]);
  const child = frame("https://ads.evil.test"); // a cross-origin iframe
  child.send(fill("https://example.test"));
  assert.equal(child.password.value, "", "never a child on the parent's behalf");
  const moved = frame("https://example.test.evil.test"); // navigated away
  moved.send(fill("https://example.test"));
  assert.equal(moved.password.value, "");
});

// The background worker with a scripted native host and tabs.
function background({ tabs, answers }) {
  const sent = [];
  let onMessage;
  const ctx = {
    URL,
    Map,
    Promise,
    chrome: {
      runtime: {
        lastError: null,
        sendNativeMessage: (_h, m, cb) => cb(answers[m.op]),
        onMessage: { addListener: (f) => (onMessage = f) },
      },
      tabs: {
        query: async () => [tabs.active],
        get: async (id) => tabs.byId[id],
        sendMessage: (tabId, msg, opts) => sent.push({ tabId, msg, opts }),
      },
    },
  };
  vm.runInNewContext(read("background.js"), ctx);
  const ask = (m) => new Promise((r) => onMessage(m, {}, r));
  return { sent, ask, tabs };
}

test("§9.6: the fill goes to the asking tab's top frame, only while it shows the approved origin", async () => {
  const tab = { id: 7, url: "https://example.test/login" };
  const answers = {
    get_candidates: { ok: true, request_id: "r1", accounts: [{ ref: "a" }] },
    authorize: { ok: true, origin: "https://example.test", fill: { password: "synthetic-pw" } },
  };
  const bg = background({ tabs: { active: tab, byId: { 7: tab } }, answers });
  await bg.ask({ type: "candidates" });
  assert.equal((await bg.ask({ type: "fill", request_id: "r1", ref: "a", method: "local" })).ok, true);
  assert.equal(bg.sent.length, 1);
  assert.deepEqual([bg.sent[0].tabId, bg.sent[0].opts.frameId], [7, 0], "BR-08: top frame of the asking tab");
  // Once: a second fill for the same request has no tab to go to.
  assert.equal((await bg.ask({ type: "fill", request_id: "r1", ref: "a", method: "local" })).ok, false);
  assert.equal(bg.sent.length, 1);
});

test("§9.6: a tab that navigated away gets nothing", async () => {
  const tab = { id: 7, url: "https://example.test/login" };
  const answers = {
    get_candidates: { ok: true, request_id: "r2", accounts: [{ ref: "a" }] },
    authorize: { ok: true, origin: "https://example.test", fill: { password: "synthetic-pw" } },
  };
  const bg = background({ tabs: { active: tab, byId: { 7: tab } }, answers });
  await bg.ask({ type: "candidates" });
  tab.url = "https://evil.test/";
  assert.equal((await bg.ask({ type: "fill", request_id: "r2", ref: "a", method: "local" })).ok, false);
  assert.equal(bg.sent.length, 0);
});
