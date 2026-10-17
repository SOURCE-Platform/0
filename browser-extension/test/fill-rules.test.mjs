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
  const native = [];
  const badges = [];
  const tabEvents = {};
  let onMessage;
  const ctx = {
    URL,
    Map,
    Promise,
    chrome: {
      runtime: {
        lastError: null,
        sendNativeMessage: (_h, m, cb) => {
          native.push(m);
          cb(answers[m.op]);
        },
        onMessage: { addListener: (f) => (onMessage = f) },
      },
      tabs: {
        query: async () => [tabs.active],
        get: async (id) => tabs.byId[id],
        sendMessage: (tabId, msg, opts) => sent.push({ tabId, msg, opts }),
        onUpdated: { addListener: (f) => (tabEvents.updated = f) },
        onRemoved: { addListener: (f) => (tabEvents.removed = f) },
      },
      action: { setBadgeText: (b) => badges.push(b) },
    },
    Date,
  };
  vm.runInNewContext(read("background.js"), ctx);
  const ask = (m, sender = {}) => new Promise((r) => onMessage(m, sender, r));
  const from = (m, sender) => onMessage(m, sender, () => {});
  return { sent, ask, from, tabs, native, badges, tabEvents };
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

test("§9.8: a submitted login is offered after the page moves on; the popup never sees the password", async () => {
  const tab = { id: 3, url: "https://shop.example.test/login" };
  const answers = { save_new: { ok: true, ref: "n1" } };
  const bg = background({ tabs: { active: tab, byId: { 3: tab } }, answers });
  const typed = { type: "captured", username: "alice@example.test", password: "synthetic-typed" };
  bg.from(typed, { tab, frameId: 2, url: "https://ads.evil.test/" }); // an iframe: ignored
  bg.from(typed, { tab, frameId: 0, url: "https://shop.example.test/login" });
  assert.equal((await bg.ask({ type: "pending_save" })).ok, false, "not before the page moved on");
  tab.url = "https://shop.example.test/home";
  bg.tabEvents.updated(3, { url: tab.url });
  bg.tabEvents.updated(3, { status: "complete" });
  assert.deepEqual([bg.badges.at(-1).tabId, bg.badges.at(-1).text], [3, "+"]);
  const p = await bg.ask({ type: "pending_save" });
  assert.deepEqual([p.ok, p.origin, p.username, p.password], [true, "https://shop.example.test", "alice@example.test", undefined]);
  assert.equal((await bg.ask({ type: "save_pending" })).ok, true);
  const save = bg.native.at(-1);
  assert.deepEqual([save.op, save.origin, save.tab_url, save.password], ["save_new", "https://shop.example.test", "https://shop.example.test/login", "synthetic-typed"], "the origin and URL of the submit");
  assert.equal((await bg.ask({ type: "pending_save" })).ok, false, "dropped once saved");
});

test("§9.8: an existing login becomes an update that needs Touch ID or the iPhone", async () => {
  const tab = { id: 4, url: "https://shop.example.test/login" };
  const answers = { save_new: { ok: true, needs_update: "r9" }, save_update: { ok: true } };
  const bg = background({ tabs: { active: tab, byId: { 4: tab } }, answers });
  bg.from({ type: "captured", username: "alice@example.test", password: "synthetic-new" }, { tab, frameId: 0, url: tab.url });
  bg.tabEvents.updated(4, { url: "https://shop.example.test/home" });
  assert.equal((await bg.ask({ type: "save_pending" })).needs_update, "r9");
  await bg.ask({ type: "update_pending", ref: "r9", method: "local" });
  const u = bg.native.at(-1);
  assert.deepEqual([u.op, u.ref, u.method, u.password], ["save_update", "r9", "local", "synthetic-new"]);
  assert.equal((await bg.ask({ type: "pending_save" })).ok, false);
});
