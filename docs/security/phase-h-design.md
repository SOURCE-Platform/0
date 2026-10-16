# Phase H — Chrome extension (design and build notes, revision 1)

Spec §9; tests BR-01…BR-19, NM-01…NM-06. Owner decision 2026-10-16: no
review during development — reviewed in the final review. Synthetic data
only.

## Built

| Piece | Where | Notes |
|---|---|---|
| §9.4 canonicalization | `vault-engine/src/browser/origin.rs` | `scheme://host[:port]`; http/https only; no userinfo, no path, no default port; IP-literal tricks and dotless names refused. **IDN:** the helper accepts ASCII letter-digit-hyphen labels only — Chrome reports `location.origin` and `tab.url` in punycode already, and the app converts a typed host before saving. An IDN library (UTS-46) would have taken the helper from 98 to 122 dependencies, over the §17.4 gate of 120 |
| §9.5 matching | `vault-engine/src/browser/matching.rs` | exact → eligible; subdomain → eligible only with `match:"domain"`, otherwise a "similar" hint; https→http refused except local hosts with `allow_http`; punycode only for itself. **Until the PSL lands (§9.7):** `match:"domain"` is honoured only below a stored host of three or more labels, so a public suffix (`github.io`, `co.uk`) can never cover strangers' sites |
| §9.3 ops | `vault/browser_ops.rs`, `vault/browser_save.rs` | `browser_hello`, `_candidates` (origins must agree — BR-11; 20 a minute; metadata only; conflicted/corrupt never offered), `_authorize` (local: presence naming the origin; iphone: a §6.5 approval and an `approval_pending` event the app turns into a push), `_fill` (after the iPhone), `_save_new` (never overwrites; an existing host+username answers `needs_update`), `_save_update` (local presence naming account and site, never a password; or an iPhone `update_password` approval bound to the exact site, action and login), `_save_update_finish` |
| One-shot fill (§9.6) | `browser_ops.rs` | a request lives 120 s; its fill goes out once; again → `APPROVAL_REPLAY` |
| Who may call | `vault-helper/src/ipc/conn.rs` | the `nm-host` class may send only the browser ops; the app class never gets them |
| nm-host broker | `vault-helper/src/nm_host.rs`, `src/bin/source-vault-nm-host.rs` | layer 2 caller-origin check (production id in release; compiled-in dev id in debug only); frames ≤ 32 KiB, documented ops and fields only, strings/small numbers only; relays as class `nm-host`; exits when stdin closes |
| Extension (MV3) | `browser-extension/` | permissions `nativeMessaging`, `tabs`, `storage`; pinned dev key; popup lists title and username only; fill with Touch ID or iPhone; the content script fills only if `location.origin` still equals the approved origin, by assignment plus `input`/`change` events, and keeps no value |
| App: push for approvals | `src/core/vault_client.rs` (`wake_for_approval`) | on the helper's `approval_pending` event, `wake_phones` |
| NM-05/NM-06 prototype | `src-tauri/poc/nm-probe/` | a probe host that logs Chrome's argv and the parent chain with a `SecCode`-by-pid check against the §9.2 requirement, and a probe extension. **Not yet run:** Chrome 155 ignores `--load-extension`; it needs the unpacked extension loaded by hand in a throwaway profile |

## Not yet (carried to the final review unless noted)

- **The vendored Public Suffix List (§9.7)** — owner decision pending:
  download the official list once and commit it, or add the `psl` crate.
- **NM-05/NM-06 verdict**, then layer 3 (parent code identity) in nm-host.
- The native host manifest install by the app (§9.2 paths for Chrome,
  Chromium, Edge, Arc, Brave), signing nm-host as `com.racker.zero.nm-host`
  and bundling it in SOURCE.app.
- Confusable skeletons (§9.5), cross-origin iframe rules beyond "the
  frame's own origin", the save badge flow after navigation (§9.8),
  `request_unlock`, the audit view of update decisions.
- BR-01…BR-19 against a local synthetic site server (IDN/punycode/iframe
  fixtures); the Phase H gate.
