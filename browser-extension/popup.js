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

// §9.8: a login just submitted on this tab, offered for saving. The popup
// shows the site and username only; the password stays in the worker.
async function offerSave() {
  const p = await ask({ type: "pending_save" });
  if (!p.ok) return;
  const box = document.createElement("div");
  box.className = "account";
  const text = document.createElement("div");
  text.textContent = `Save the login for ${p.username || "this account"} on ${p.origin}?`;
  const row = document.createElement("div");
  row.className = "row";
  const button = (label, onclick) => {
    const b = document.createElement("button");
    b.textContent = label;
    b.onclick = onclick;
    return b;
  };
  const done = (t) => { text.textContent = t; row.replaceChildren(); };
  const update = (ref) => {
    text.textContent = "A login for this account is already saved. Update its password?";
    row.replaceChildren(
      button("Update with Touch ID", async () => {
        const r = await ask({ type: "update_pending", ref, method: "local" });
        done(r.ok ? "Password updated." : "Not updated.");
      }),
      button("Approve on iPhone", async () => {
        const r = await ask({ type: "update_pending", ref, method: "iphone" });
        if (!r.approval_id) return done("Your iPhone isn't available — use Touch ID.");
        done("Approve on your iPhone…");
        const timer = setInterval(async () => {
          const f = await ask({ type: "update_after_iphone", ref, approval_id: r.approval_id });
          if (f.ok && f.pending) return;
          clearInterval(timer);
          done(f.ok ? "Password updated." : "Not updated.");
        }, 1500);
      }),
    );
  };
  row.append(button("Save", async () => {
    const r = await ask({ type: "save_pending" });
    if (r.needs_update) return update(r.needs_update);
    done(r.ok ? "Saved to Source Vault." : r.error === "BAD_STATE" ? "Unlock your vault in SOURCE first." : "Not saved.");
  }));
  box.append(text, row);
  list.before(box);
}

(async () => {
  await offerSave();
  const c = await ask({ type: "candidates" });
  if (!c.ok) return say(c.error === "VAULT_UNAVAILABLE" ? "Open SOURCE on your Mac to use the vault." : "Not available here.");
  if (c.locked) {
    say("Your vault is locked.");
    const open = document.createElement("button");
    open.textContent = "Unlock in SOURCE";
    open.onclick = async () => {
      await ask({ type: "request_unlock" });
      say("Unlock in the SOURCE window, then open this again.");
    };
    list.append(open);
    return;
  }
  if (c.warning === "lookalike") {
    const w = document.createElement("div");
    w.className = "warn";
    w.textContent = "Careful: this site's address looks like one of your saved sites but is a different address. Nothing will be filled here.";
    list.before(w);
  }
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
