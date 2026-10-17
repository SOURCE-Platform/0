// Source Vault background worker (spec §9.1–§9.6; Phase H). It talks to
// the native host one message at a time and never stores a password: a
// fill goes straight to the tab that asked, once. No remote code, no
// telemetry, nothing in chrome.storage but the UI's last state.

const HOST = "com.racker.zero.vault";

function native(message) {
  return new Promise((resolve) => {
    chrome.runtime.sendNativeMessage(HOST, message, (answer) => {
      resolve(chrome.runtime.lastError ? { ok: false, error: "VAULT_UNAVAILABLE" } : answer);
    });
  });
}

// Which tab asked for each fill request, and the origin it showed then.
// A fill is delivered only to that tab, only while it still shows the
// origin the vault approved it for; otherwise the secret is dropped.
const asked = new Map();

async function deliver(requestId, answer) {
  const at = asked.get(requestId);
  asked.delete(requestId);
  if (!at || !answer.fill || answer.origin !== at.origin) return false;
  const tab = await chrome.tabs.get(at.tabId).catch(() => null);
  if (!tab || !tab.url || new URL(tab.url).origin !== answer.origin) return false;
  chrome.tabs.sendMessage(at.tabId, { type: "fill", origin: answer.origin, fill: answer.fill }, { frameId: 0 });
  return true;
}

// §9.8: a submitted login per tab, in this worker's memory only (never
// chrome.storage), offered for saving once the page has moved on. It is
// dropped after two minutes, when saved, or when the tab closes.
const captured = new Map();
const KEEP_MS = 120000;

function pendingFor(tabId) {
  const c = captured.get(tabId);
  if (c && Date.now() - c.at > KEEP_MS) {
    captured.delete(tabId);
    return null;
  }
  return c && c.navigated ? c : null;
}

chrome.tabs.onUpdated.addListener((tabId, change) => {
  const c = captured.get(tabId);
  if (!c) return;
  if (change.url) c.navigated = true; // the success heuristic: the page moved on
  if (change.status === "complete" && c.navigated) chrome.action.setBadgeText({ tabId, text: "+" });
});
chrome.tabs.onRemoved.addListener((tabId) => captured.delete(tabId));

function dropCaptured(tabId) {
  captured.delete(tabId);
  chrome.action.setBadgeText({ tabId, text: "" });
}

async function activeTab() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  return tab;
}

// The popup asks; the background answers with metadata only.
chrome.runtime.onMessage.addListener((msg, sender, reply) => {
  if (msg.type === "captured") {
    // From a content script: only a tab's top frame, http(s) only.
    if (!sender.tab || sender.frameId !== 0 || !/^https?:/.test(sender.url || "")) return false;
    const url = sender.url;
    captured.set(sender.tab.id, { origin: new URL(url).origin, url, username: msg.username, password: msg.password, at: Date.now(), navigated: false });
    return false;
  }
  (async () => {
    const tab = await activeTab();
    if (!tab || !tab.url) return reply({ ok: false, error: "NO_TAB" });
    const origin = new URL(tab.url).origin; // punycode, as Chrome reports it
    switch (msg.type) {
      case "candidates": {
        const c = await native({ op: "get_candidates", origin, tab_url: tab.url });
        if (c.ok && c.request_id) asked.set(c.request_id, { tabId: tab.id, origin });
        return reply(c);
      }
      case "fill": {
        const a = await native({ op: "authorize", request_id: msg.request_id, ref: msg.ref, method: msg.method });
        if (a.ok && a.fill) return reply({ ok: await deliver(msg.request_id, a) });
        return reply(a); // e.g. {approval_id} while the iPhone decides
      }
      case "fill_after_iphone": {
        const f = await native({ op: "fill", request_id: msg.request_id });
        if (f.ok && f.fill) return reply({ ok: await deliver(msg.request_id, f) });
        return reply(f);
      }
      case "request_unlock":
        return reply(await native({ op: "request_unlock" }));
      case "pending_save": {
        // The popup sees where and who — never the password.
        const c = pendingFor(tab.id);
        return reply(c ? { ok: true, origin: c.origin, username: c.username } : { ok: false });
      }
      case "save_pending": {
        // Origin and URL as they were when the form was submitted.
        const c = pendingFor(tab.id);
        if (!c) return reply({ ok: false, error: "NOTHING_TO_SAVE" });
        const r = await native({ op: "save_new", origin: c.origin, tab_url: c.url, username: c.username, password: c.password });
        if (r.ok && !r.needs_update) dropCaptured(tab.id);
        return reply(r.needs_update ? { ok: true, needs_update: r.needs_update } : { ok: r.ok, error: r.error });
      }
      case "update_pending": {
        // Never applied on the extension's say-so: Touch ID or the iPhone.
        const c = pendingFor(tab.id);
        if (!c) return reply({ ok: false, error: "NOTHING_TO_SAVE" });
        const r = await native({ op: "save_update", ref: msg.ref, origin: c.origin, tab_url: c.url, password: c.password, method: msg.method });
        if (r.ok && !r.approval_id) dropCaptured(tab.id);
        return reply(r);
      }
      case "update_after_iphone": {
        const c = pendingFor(tab.id);
        if (!c) return reply({ ok: false, error: "NOTHING_TO_SAVE" });
        const r = await native({ op: "save_update_finish", ref: msg.ref, origin: c.origin, tab_url: c.url, password: c.password, approval_id: msg.approval_id });
        if (r.ok && !r.pending) dropCaptured(tab.id);
        return reply(r);
      }
      default:
        return reply({ ok: false, error: "UNKNOWN" });
    }
  })();
  return true; // async reply
});
