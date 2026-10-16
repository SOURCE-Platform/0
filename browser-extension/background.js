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

async function activeTab() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  return tab;
}

// The popup asks; the background answers with metadata only.
chrome.runtime.onMessage.addListener((msg, sender, reply) => {
  (async () => {
    const tab = await activeTab();
    if (!tab || !tab.url) return reply({ ok: false, error: "NO_TAB" });
    const origin = new URL(tab.url).origin; // punycode, as Chrome reports it
    switch (msg.type) {
      case "candidates":
        return reply(await native({ op: "get_candidates", origin, tab_url: tab.url }));
      case "fill": {
        const a = await native({ op: "authorize", request_id: msg.request_id, ref: msg.ref, method: msg.method });
        if (a.ok && a.fill) {
          // §9.6: the content script re-checks the origin before filling.
          chrome.tabs.sendMessage(tab.id, { type: "fill", origin, fill: a.fill });
          return reply({ ok: true });
        }
        return reply(a); // e.g. {approval_id} while the iPhone decides
      }
      case "fill_after_iphone": {
        const f = await native({ op: "fill", request_id: msg.request_id });
        if (f.ok && f.fill) {
          chrome.tabs.sendMessage(tab.id, { type: "fill", origin, fill: f.fill });
          return reply({ ok: true });
        }
        return reply(f);
      }
      case "save":
        return reply(await native({ op: "save_new", origin, tab_url: tab.url, username: msg.username, password: msg.password }));
      default:
        return reply({ ok: false, error: "UNKNOWN" });
    }
  })();
  return true; // async reply
});
