# Phase C Verification Report — Local Mac Vault

Date: 2026-09-18. Spec: `credential-vault-implementation-spec.md` v0.3,
§18 Phase C. Scope limit honored: no Phase D+ work — no total-loss
recovery, recovery-finalize, RK generate/print/entry, remote backup,
enrollment, HPKE/Secure Enclave envelopes, iPhone approval, Chrome
extension, Dashlane import, or real credentials (every test, gate step,
and manual check uses synthetic values). MP never enters the Tauri/WebView
process: it is typed into the helper-owned panel and stays in the helper.

Status: **gate green (17/17); in-app capture/focus check passed
2026-09-18 23:10 → 2026-09-19 00:24 (§3).**

## What shipped

Helper (`src-tauri/vault-helper/src/`, every file ≤ 350 lines):

- `storage/` — rusqlite vault DB, `header.json` / `manifest.json` with
  the §3.5 generation/hash cross-check, record revisions + tombstones,
  schema versioning with FORMAT_TOO_NEW refusal.
- `vault/` — op dispatch for the Phase C subset (`setup_vault`,
  `begin_recovery_unlock{kind:"mp"}`, `list_items`, `add_item`,
  `update_item`, `delete_item`, `reveal`, `change_master_password`,
  `set_auto_lock_minutes`); UNLOCKED lifecycle, zeroize on lock.
- `panel/` — AppKit secure panel (native `NSSecureTextField`s, no close
  button) plus `watchdog.rs`, the in-session dismissal path (see Finding 1).
- `la.rs` (LocalAuthentication presence per mutating op and reveal),
  `keychain.rs` (§2.8 rollback generation + prefs), `notify.rs` (sleep /
  screen-lock → lock), `ipc/` (hub, per-connection loop, executor).

Main app: `core/vault_client.rs` (signed helper launch, event pump,
§14.2 capture wiring, §14.4 capture-check answers), `app/commands/vault.rs`
(11 `vault_*` commands), `platform/activation.rs` (focus hand-off to the
panel), keystroke rule for the helper in `core/capture_exclusions.rs`.
Frontend: Vault tab (`src/components/vault/`, `src/lib/vault.ts`),
registered as a capture-sensitive surface while mounted.

## 1. Tests and results

Helper suite: **112 tests, 0 failed** (58 lib units incl. 17 storage
units; CR/RG/XV/IPC integration from Phases A/B; Phase C files below).

| Test file | Tests | Covers |
|---|---|---|
| `tests/vault_lifecycle.rs` | OP-01 lifecycle, OP-02 wrong MP + backoff, OP-03 presence denial, OP-05 panel cancel, OP-06 reveal fail-closed, OP-07 change MP | op-level flows through `vault::dispatch` |
| `tests/vault_contracts.rs` | OP-04 bad-state matrix, SC-01/SC-04 frame contract, panel flow kinds, auto-lock op | op surface |
| `tests/vault_failclosed.rs` | FORMAT_TOO_NEW, MANIFEST_MISMATCH, RECORD_CORRUPT, MANIFEST_ROLLBACK | §3.5/§3.6/§2.8 |
| `tests/ipc_capture.rs` | reveal's capture_check over a real socket (allow → released, deny → CAPTURE_UNSAFE, stale reply consumed) | Finding 2 regression |

The op-level tests were numbered `cs01…cs07` by the previous session and
have been renamed `op01…op07`: the spec's CS-xx IDs are the
capture-suppression tests in §3 below, a different thing.

Main app: `cargo test -p SOURCE --lib -- capture_exclusions vault_client`
**9 passed** (counter/title registry, keystroke decision matrix,
helper-frontmost rule, panel-event → suppression wiring).

## Gate evidence

`npm run gate:phase-c`, run of record 2026-09-18 on this machine:
**PHASE C GATE: PASS (17 checks)**.

| # | Check | Evidence |
|---|---|---|
| 1 | helper test suite | 112 tests, 0 failed |
| 2 | file-length audit | all files ≤ 350 lines |
| 3 | debug helper build + sign + verify | OU 9RGW34CMA2, deep-strict + DR + no entitlements |
| 4 | release helper: debug overrides compiled out | no `OV0_VAULT_*` string in the binary |
| 5 | E2E lifecycle over the signed socket | setup → unlock → add → list (metadata only) → update → reveal (v2) → delete → empty list |
| 6 | capture_check fails closed (CS-06) | CAPTURE_UNSAFE + `capture_unsafe` event, no secret in output |
| 7 | RK unlock kind | UNKNOWN_OP |
| 8 | auto-lock minutes op | 4 → INVALID_INPUT, 5 → OK |
| 9 | panel events (UI-01) | `secure_panel_visible` true with create/unlock titles, then false |
| 10 | change master password across restarts | old MP → WRONG_CREDENTIAL, new MP unlocks, canary record intact |
| 11 | LA presence denial | PRESENCE_DENIED, nothing written |
| 12 | auto-lock observed | `locked` event, reason `timeout`; state LOCKED |
| 13 | live panel (UI-01/03) | real window, `NSSecureTextField`, dismissed in seconds, no partial vault; probe `app_active:false, panel_key:false` (expected for the gate client, see §2) |
| 14 | main app | `cargo check` + 9 capture wiring/decision tests |
| 15 | frontend | `npm run build` green |
| 16 | supply chain | cargo audit + vet green; 96 helper deps (< 120); no rsa |
| 17 | Phase B gate regression | PASS (6 checks) — incl. Phase A 11/11 and 10 min TLV fuzz, no crash |

## 2. Helper secure-panel verification

| ID | Evidence | Status |
|---|---|---|
| UI-01 panel opens → `secure_panel_visible`, suppression held | gate checks 9 + 13 (events on the wire, titled); main-app wiring test | automated ✅ |
| UI-02 native secure fields; secure input while focused | gate 13 probe: `NSSecureTextField`; in-app: panel opened key and focused (active title bar, focus ring on the first field, typing without a click) | in-app ✅ |
| UI-03 cancel → counter down, buffers cleared, no partial state | gate 13 (visible:false, no header written, dismissed in <10 s); OP-05 | automated ✅ |
| UI-04 recovery-sheet print | no RK print path exists in Phase C | deferred to Phase D |
| UI-05 no MP/RK/VK-bearing op field | SC-01/SC-04 test: frame claiming `master_password` is ignored, panel still driven; MP never in any response/event | automated ✅ |

## 3. Capture / OCR / keyboard suppression evidence

How it works: while the Vault tab is mounted or a helper panel is
visible, a process-wide counter is held; the recorder drops every frame
while it is held (whole-display capture, so fail-closed rather than
masking), and OCR only ever sees frames the recorder captured.
Keystrokes are dropped when the focused field is secure, macOS Secure
Event Input is on, SOURCE is frontmost with the vault visible, or the
vault helper is frontmost.

| ID | Automated evidence | Live evidence |
|---|---|---|
| CS-01 vault visible during recording → frames dropped | wiring + counter tests; `process_frame`/`capture_frame` gate on the counter | in-app check: no frames stored in steps 2–4, frames resume in step 5 |
| CS-02 vault content absent from OCR/search | no captured frame → nothing to OCR | in-app check: canary strings absent from OCR text |
| CS-03 typing into vault secure fields → zero events | decision matrix + helper-frontmost tests | zero keyboard events — **vacuous**: macOS keystroke capture is not implemented (see Finding 8) |
| CS-04 other apps' password fields (secure input) | decision matrix (secure input → drop) | not exercised in Phase C |
| CS-05 ambiguous focus → suppress | poisoned registry → fail closed; unknown element → counter rules | unit level only |
| CS-06 reveal with suppression unverifiable → CAPTURE_UNSAFE | OP-06, `ipc_capture.rs`, gate check 6 | — |
| CS-07 import wizard | no import in Phase C | N/A until import phase |

A recorder-level automated test was not added: the recorder's test setup
opens the user's real SOURCE database (`Database::init()`), which a gate
must not touch.

### In-app check (run once, ~5 minutes, synthetic data only)

Run the dev build (the installed SOURCE.app predates the vault) from a
normal Terminal window, after quitting the installed app so only one
recorder writes to the database. Canary strings are synthetic.

1. `npm run tauri dev`; in Settings make sure screen recording, "Read
   text from screenshots", and keyboard capture are on; start recording.
2. Vault tab → **Set up**. The password box must accept typing
   immediately, without a click (UI-02). Enter `canary-master-7Q4Z`
   twice → OK. Then **Unlock** with the same password.
3. Add login: title `CANARY-VAULT-TITLE-51`, user `canary@example.test`,
   host `example.test`, password `canary-pw-9X2K`; click reveal.
4. Stay on the Vault tab for 2 minutes (covers the 60 s OCR backstop).
5. Switch to Timeline for 1 minute (recording must resume), then stop.

Verification (read-only queries on `~/.observer_data/database/observer.db`):
no OCR text containing any canary; no keyboard events from SOURCE or the
vault helper during the vault window; no frames stored during the vault
window, frames present again afterwards.

Result, run 2026-09-18 23:10 → 2026-09-19 00:24 on the dev build
(`npm run tauri dev`, signed via `scripts/dev-run-signed.sh`):

- Setup, unlock, add login, and reveal all worked in the real app; the MP
  panel opened focused (UI-02). Reveal required Touch ID/Mac password each
  time, as specified.
- **CS-01/CS-02: pass.** OCR results per minute: 130 at 23:24 (before the
  vault), **none from 23:25 to 00:21** (vault tab open, including setup,
  unlock, add, and reveal), 96 at 00:22 once Timeline was shown — capture
  resumed immediately.
- Canary search over every OCR/context text column: 4 hits, **all of them
  this session's own chat instructions** visible in the Claude window or
  browser (23:10:05, 23:24:47 — both before the vault was opened or the
  login created — and 00:23:29 after leaving it). None from the vault UI.
  Lesson for re-runs: keep the canary values out of anything on screen.
- `frames` / `video_frame_samples` hold no rows in this configuration
  (video evidence off; OCR screenshots are deleted after reading), so OCR
  output is the capture evidence.
- Keyboard: `keyboard_events` holds 0 rows ever (Finding 8).

## 4. Storage / manifest corruption and fail-closed results

All green in the helper suite:

- Header: round trip; future `version` → FORMAT_TOO_NEW (unit + op-level:
  vault enters ERROR, no panel shown); corrupt header refused; unknown KDF
  refused as too new; `import_fp_salt` length enforced (SC-03).
- Manifest: round trip; header/manifest generation disagreement →
  MANIFEST_MISMATCH; corrupt or future manifest refused; op-level tamper
  of `rev_hash` → MANIFEST_MISMATCH, ERROR state, file left on disk.
- Rollback (§2.8): keychain remembers a newer generation → MANIFEST_ROLLBACK.
- DB: newer `user_version` → FORMAT_TOO_NEW; garbage file → DB_CORRUPT.
- Records: ciphertext corrupted in place → RECORD_CORRUPT on reveal and
  update, no plaintext, rest of the vault stays usable; CVV rejected
  outright; non-NFC titles rejected; history capped at 10.

## 5. Dependency and audit changes

- Helper adds `rusqlite 0.32` (bundled SQLite), `objc2 0.5`,
  `objc2-foundation`/`objc2-app-kit`/`objc2-local-authentication 0.2`
  (class-level feature gating), `block2 0.5`. No rsa. Keychain stays
  hand-rolled Security.framework FFI.
- Main app adds `source-vault-helper` by path (one implementation of
  framing, peer-auth, and op shapes; no secrets move into the app).
- `cargo vet`: 16 new `safe-to-deploy` audit entries (objc2 family,
  rusqlite's hashing/iterator deps, framework stubs). As in Phase B they
  were written by the agent under the repo owner's name; they record the
  agent's review notes, not an independent human review.
- **Dependency-count method changed.** `audit-deps.sh` now counts unique
  `name vX.Y.Z` lines, removing cargo's `(*)` duplicate markers. True
  unique count: **96 (< 120)**. The old method counted 26 duplicates and
  would read **122**, over the gate. The new method is the correct reading
  of the rule, but the rule's arithmetic changed in this phase.

## 6. Deviations from v0.3

- `set_auto_lock_minutes` is a Phase C internal op (§1.5 has no prefs op;
  §1.6 requires the 5–60 minute dial).
- Panel is an `NSWindow` (Titled, no close button), not `NSPanel` (objc2
  0.5 init chain); §1.7 behavior preserved.
- Item ops accept flat fields plus `hosts:[…]`/`host:"…"` shorthands in
  addition to canonical nested `fields`/`urls` (the management UI uses them).
- Unlock is `begin_recovery_unlock` with `kind:"mp"` until Phase E;
  `kind:"rk"` → UNKNOWN_OP (RK is Phase D).
- UI-04 deferred to Phase D (no RK print path in Phase C).
- Release helper is built but not signed on this machine
  (`build-helper.sh` requires the production team identity for release);
  the gate proves every `OV0_VAULT_*` debug override is absent from the
  release binary. Signed E2E runs use the debug bundle, as in Phase A.
- Debug-only env overrides (compiled out of release, verified):
  `OV0_VAULT_PANEL_SCRIPT`, `OV0_VAULT_PANEL_PROBE`, `OV0_VAULT_LA_STUB`,
  `OV0_VAULT_AUTO_LOCK_SECS`, `OV0_VAULT_KEYCHAIN_PREFIX`, `OV0_VAULT_DIR`,
  `OV0_VAULT_SOCKET_PATH`, `OV0_VAULT_IDLE_SECS`; main app:
  `OV0_VAULT_HELPER_PATH`.

## 7. Blockers and findings

Found and fixed during gate work. None was caught by the op-level tests,
because each lives in the real AppKit/socket layer those tests stub out.

1. **Panel could not be dismissed while open** (security). Lock, sleep,
   screen-lock, and the 120 s timeout queued the dismissal on the main
   dispatch queue, behind the block running the panel's modal loop. The
   op returned Cancelled and emitted `secure_panel_visible:false`, so the
   main app released capture suppression while the MP panel stayed on
   screen. Fixed: `panel/watchdog.rs` polls a per-job abort flag from a
   run-loop timer inside the modal session and calls `abortModal`; the
   runner waits for the real dismissal before reporting. Also fixed a
   `dispatch_after_f` call that passed a raw delay as an absolute time.
2. **Every production reveal failed CAPTURE_UNSAFE.** The connection loop
   blocked on the executor during `reveal` and could not read the app's
   capture_check reply on the same socket; the 500 ms check always timed
   out, and the late reply was then run as an op. Fail-safe but
   non-functional, and the gate's deny-path check passed vacuously.
   Fixed: a dedicated read-side thread routes `reply_to` frames
   immediately. `tests/ipc_capture.rs` fails on the old loop, passes now.
3. **Panel opened without keyboard focus** (UI-02). Since macOS 14 an app
   cannot activate itself; the helper's `activate()` was a no-op, so the
   panel appeared unfocused and Secure Event Input never engaged until
   clicked. Fixed: SOURCE yields activation to the helper bundle before
   every panel op (`platform/activation.rs`), and the keystroke recorder
   treats the helper as a sensitive surface whenever it is frontmost.
   Confirmed only by the in-app check.
4. **No change-master-password in the app.** The helper supported it but
   the app had no command or button. Added `vault_change_master_password`
   and a Vault-tab button.
5. **Phase A gate regression** from the previous session: the test
   client's `handshake` output labels changed (`LOCK_STATE=` → `LOCK=`).
   Restored; Phase A gate 11/11.
6. **Vault unusable in dev builds.** The app tried cargo's unsigned
   `target/debug/source-vault-helper` before the signed dev bundle, failed
   the (correct) signature check, and never launched the helper; and the
   helper rejected the ad-hoc signed `tauri dev` app. Fixed: the signed
   bundle is tried first, and `src-tauri/.cargo/config.toml` runs the dev
   SOURCE binary through `scripts/dev-run-signed.sh`, which signs it with
   the local Apple Development identity as `com.racker.zero`. Release
   builds are unaffected.
7. **Reveal never displayed.** During the Touch ID check the vault is in
   AUTHORIZING; the Vault tab swapped its item list for a status card in
   that state, unmounting the component awaiting the reveal, so the
   secret was dropped. Fixed: the list stays mounted through AUTHORIZING.
8. **macOS keystroke capture does not exist.** `MacOSKeyboardListener` is
   a stub that never installs an event tap (`keyboard_events` has 0 rows
   ever). CS-03's "zero events" therefore proves nothing today; the
   suppression rules are unit-tested and wired, and must be re-verified
   the day a real tap lands.

Carried forward, not Phase C blockers:

- Argon2id `m=64 MiB, t=3, p=1` remains the **provisional** v1 tuple;
  measured on the M2 Air only. Final freeze needs the supported iPhone
  floor and any other required hardware class, before Phase E.
- The recovery-ordering inconsistency must be patched in the spec before
  Phase D is authorized.
- Observed once, not investigated: an orphaned debug helper (its app had
  been killed mid-rejection loop) did not exit on SIGTERM within 10 s and
  needed SIGKILL. The Phase A gate's SIGTERM check passes, so this is a
  specific state worth reproducing before Phase D.
- Pre-existing, noted only: an explicit `lock` sent on the same connection
  as an in-flight panel op queues behind it (the op loop is sequential per
  connection); lock from sleep/screen-lock/timeout is unaffected.

## Reproduce

```bash
npm run gate:phase-c
```

Requires the debug signing identity (as Phase A), cargo-fuzz + nightly
(Phase B regression). Shows the real panel on screen for under half a
second (check 13). `PHASE_C_SKIP_REGRESSION=1` skips the Phase B/A
regression for iteration and marks that check failed.
