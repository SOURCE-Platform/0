// The popup: the logins saved for this page, and how to fill one —
// Touch ID on this Mac, or approval on your iPhone. Usernames and titles
// only; passwords never reach the popup.

const list = document.getElementById("list");
const note = document.getElementById("note");
const ask = (m) => new Promise((r) => chrome.runtime.sendMessage(m, r));

function say(text) {
  note.textContent = text;
}

function waitForIphone(requestId) {
  say("Approve on your iPhone…");
  const timer = setInterval(async () => {
    const f = await ask({ type: "fill_after_iphone", request_id: requestId });
    if (f.ok) {
      clearInterval(timer);
      window.close();
    } else if (!f.pending) {
      clearInterval(timer);
      say(f.error === "APPROVAL_EXPIRED" ? "The request expired — try again." : "Not approved.");
    }
  }, 1500);
}

(async () => {
  const c = await ask({ type: "candidates" });
  if (!c.ok) return say(c.error === "VAULT_UNAVAILABLE" ? "Open SOURCE on your Mac to use the vault." : "Not available here.");
  if (c.locked) return say("Your vault is locked. Unlock it in SOURCE.");
  if (!c.accounts.length) return say(c.similar ? "A similar login exists for a related site — open SOURCE to check it." : "No saved login for this site.");
  for (const a of c.accounts) {
    const box = document.createElement("div");
    box.className = "account";
    const name = document.createElement("div");
    name.textContent = `${a.title} — ${a.username}`;
    const row = document.createElement("div");
    row.className = "row";
    const local = document.createElement("button");
    local.textContent = "Fill with Touch ID";
    local.onclick = async () => {
      const r = await ask({ type: "fill", request_id: c.request_id, ref: a.ref, method: "local" });
      r.ok ? window.close() : say("Not filled.");
    };
    const phone = document.createElement("button");
    phone.textContent = "Approve on iPhone";
    phone.onclick = async () => {
      const r = await ask({ type: "fill", request_id: c.request_id, ref: a.ref, method: "iphone" });
      r.approval_id ? waitForIphone(c.request_id) : say("Your iPhone isn't available — use Touch ID.");
    };
    row.append(local, phone);
    box.append(name, row);
    list.append(box);
  }
})();
