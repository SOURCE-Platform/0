# Phase H — Chrome extension (design and build notes, revision 1)

Spec §9; tests BR-01…BR-19, NM-01…NM-06. Owner decision 2026-10-16: no
review during development — reviewed in the final review. Synthetic data
only.

## Built

| Piece | Where | Notes |
|---|---|---|
| §9.4 canonicalization | `vault-engine/src/browser/origin.rs` | `scheme://host[:port]`; http/https only; no userinfo, no path, no default port; IP-literal tricks and dotless names refused. **IDN:** the helper accepts ASCII letter-digit-hyphen labels only — Chrome reports `location.origin` and `tab.url` in punycode already, and the app converts a typed host before saving. An IDN library (UTS-46) would have taken the helper from 98 to 122 dependencies, over the §17.4 gate of 120 |
| §9.7 Public Suffix List | `vault-engine/src/browser/psl.rs`, `public_suffix_list.dat` (2026-10-16) | owner decision 2026-10-16: the official list vendored, no crate; the list's own algorithm; a punycode suffix has no registrable domain (fail closed) |
| §9.5 matching | `vault-engine/src/browser/matching.rs` | exact → eligible; subdomain → eligible only with `match:"domain"`, otherwise a "similar" hint; https→http refused except local hosts with `allow_http`; punycode only for itself. `match:"domain"` covers the stored host's registrable domain as the PSL decides; a public suffix covers nobody |
| §9.3 ops | `vault/browser_ops.rs`, `vault/browser_save.rs` | `browser_hello`, `_candidates` (origins must agree — BR-11; 20 a minute; metadata only; conflicted/corrupt never offered), `_authorize` (local: presence naming the origin; iphone: a §6.5 approval and an `approval_pending` event the app turns into a push), `_fill` (after the iPhone), `_save_new` (never overwrites; an existing host+username answers `needs_update`), `_save_update` (local presence naming account and site, never a password; or an iPhone `update_password` approval bound to the exact site, action and login), `_save_update_finish` |
| One-shot fill (§9.6) | `browser_ops.rs` | a request lives 120 s; its fill goes out once; again → `APPROVAL_REPLAY` |
| Who may call | `vault-helper/src/ipc/conn.rs` | the `nm-host` class may send only the browser ops; the app class never gets them |
| nm-host broker | `vault-helper/src/nm_host.rs`, `src/bin/source-vault-nm-host.rs` | layer 2 caller-origin check (production id in release; compiled-in dev id in debug only); frames ≤ 32 KiB, documented ops and fields only, strings/small numbers only; relays as class `nm-host`; exits when stdin closes |
| Extension (MV3) | `browser-extension/` | permissions `nativeMessaging`, `tabs`, `storage`; pinned dev key; popup lists title and username only; fill with Touch ID or iPhone; the content script fills only if `location.origin` still equals the approved origin, by assignment plus `input`/`change` events, and keeps no value |
| App: push for approvals | `src/core/vault_client.rs` (`wake_for_approval`) | on the helper's `approval_pending` event, `wake_phones` |
| §9.5 confusables (BR-05/06) | `vault-engine/src/browser/confusable.rs` | the helper sees ASCII hosts only, so the skeleton uses the ASCII-to-ASCII entries of `confusables.txt` (`rn`→`m`, `vv`→`w`, `cl`→`d`, `0`→`o`, `1`/`|`→`l`); a skeleton collision with a *different* stored registrable domain, or an unsaved punycode page, adds `warning:"lookalike"` to the candidates answer and the popup shows the banner; nothing fills |
| BR tests | `vault-helper/tests/browser_rules.rs` (one test per rule, named `br_…`), `browser-extension/test/fill-rules.test.mjs` (`node --test browser-extension/test/`) | BR-01…07, 09…14, 16…19 helper-side; BR-08 against the real `content.js`/`background.js` with stubbed Chrome objects: only the asking tab's top frame (`frameId: 0`), only while its own origin is the approved one. **BR-15 does not apply:** the extension makes no matching or PSL decision — it reports `new URL(tab.url).origin` and the helper canonicalizes, cross-checks (BR-11) and decides |
| NM-05/NM-06 prototype | `src-tauri/poc/nm-probe/` | a probe host that logs Chrome's argv and the parent chain with a `SecCode`-by-pid check against the §9.2 requirement, and a probe extension. **Not yet run:** Chrome 155 ignores `--load-extension`; it needs the unpacked extension loaded by hand in a throwaway profile |

## Not yet (carried to the final review unless noted)

- **NM-05/NM-06 verdict**, then layer 3 (parent code identity) in nm-host.
- The native host manifest install by the app (§9.2 paths for Chrome,
  Chromium, Edge, Arc, Brave), signing nm-host as `com.racker.zero.nm-host`
  and bundling it in SOURCE.app.
- A login form inside an iframe is not filled at all (only the top
  frame is); the save badge flow after navigation (§9.8),
  `request_unlock`, the audit view of update decisions.
- The Phase H gate script.
