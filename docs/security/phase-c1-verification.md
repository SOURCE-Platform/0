# Phase C.1 Verification Report — Pre-Phase-D Correction Pass

Date: 2026-09-19. Spec: `credential-vault-implementation-spec.md` v0.3
(with the dated recovery-order correction below). Scope honored: the five
correction items only. Not started: Recovery Key UI, recovery-finalize
implementation, remote backup, enrollment, HPKE, iPhone approval, browser
work, Dashlane import, real credentials. No keyboard event tap was added.
The provisional Argon2id tuple (`m=64 MiB, t=3, p=1`) is unchanged; final
cross-device calibration remains a pre-Phase-E gate.

## 1. Recovery ordering (spec only)

Total-loss recovery is now, everywhere in the implementation spec:

`recover old VK → create recovery_epoch → generate fresh VK → re-encrypt
current vault under fresh VK → build/upload new objects and wraps →
recovery-finalize atomically installs the already-rotated manifest/
registry/device credential → enter UNLOCKED`

Edits in `credential-vault-implementation-spec.md`:

| Where | Before | After |
|---|---|---|
| Header | — | dated v0.3 correction note |
| §4.5 | "recovering device immediately rotates VK" (after the epoch) | fresh VK generated and vault re-encrypted **before** finalize; the old VK (proof key) is retired at the moment the epoch commits |
| §11.8 body `new_manifest` | `vk_generation` = new | also: every referenced record/wrap already sealed under the fresh VK |
| §11.8 step 5 | objects uploaded first | re-encrypted objects **and new wraps** uploaded first |
| §11.8 client side | on success RECOVERING → ROTATING_KEYS → UNLOCKED | full ordered sequence; finalize installs already-rotated state; RECOVERING → UNLOCKED; "no post-finalize VK rotation" |
| §12 scenario 3 | step 4 finalize, step 5 rotate VK | step 4 epoch entry, step 5 fresh VK + re-encrypt + wraps, step 6 upload + finalize → UNLOCKED (scenario 4 inherits) |
| §13.1 state machine | LOCKED → RECOVERING → ROTATING_KEYS → UNLOCKED | LOCKED → RECOVERING → UNLOCKED (re-encryption inside RECOVERING) |
| §13.2 RECOVERING row | "transient post-unwrap" | old VK transient; fresh VK before finalize; old zeroized after re-encryption; upload of re-encrypted objects |
| §13.3 timeout/crash | — | re-encryption inside RECOVERING is resumable, no timeout; interrupted recovery restarts with a newly generated fresh VK, abandoned uploads GC'd |
| §16.15 / gate item 12 | RF-01…RF-07 | new **RF-08**: finalize carries `vk_generation` = old+1, everything opens only under the fresh VK, UNLOCKED directly, no second rotation |

`credential-vault-security-architecture.md` does not prescribe a
post-finalize rotation and needed no change.

## 2. Explicit lock during an in-flight panel op

Problem: a `lock` sent on the same connection queued behind the open panel
twice over — in the client (`op_lock` held one request at a time) and in
the helper's sequential per-connection op loop — so it took effect only
when the panel closed (up to the 120 s timeout). The Vault tab also hid or
disabled "Lock now" while an op was busy.

Fix:
- Helper (`ipc/conn.rs`): the connection's read side applies `lock` the
  moment it arrives — panel-cancel flag, zeroize (VK/store dropped), LOCKED,
  `locked`/`state` events. Only its response waits its turn, so replies
  stay in request order; the wire protocol is unchanged.
- Client (`ipc/client.rs`): requests no longer serialize; waiters are a
  FIFO registered under the writer lock in wire order.
- The panel then aborts through the Phase C watchdog (`abortModal`,
  fields cleared to "" first); the runner waits for the real dismissal, so
  `secure_panel_visible:false` — which releases the app's capture
  suppression — is emitted only after the panel is off screen.
- Vault tab: "Lock now" stays visible during AUTHORIZING and is never
  disabled by a busy op.
- Unchanged safety net: every panel op re-checks state after the panel
  (e.g. unlock refuses to install VK if a lock landed mid-unwrap).

Regression test — `tests/lock_preempts_panel.rs` (real socket, real AppKit
panel, `harness = false`, runs with `OV0_VAULT_APPKIT_TESTS=1` from the
gate): create a vault, open the real unlock panel, send `lock` on the same
connection. Measured: `locked` event +0.2 ms, panel off screen and
`visible:false` +64 ms, unlock → `PANEL_CANCELLED`, lock → `{ok, state:
"locked"}`, panel confirmed off screen when `visible:false` arrived. With
the lock moved back into the op queue the test fails (times out at 10 s).

Not covered by this change: the LocalAuthentication sheet (Touch ID /
password) cannot be cancelled by the flag; a lock during it still zeroizes
immediately and the op fails closed at its post-LA state check.

## 3. Helper ignoring SIGTERM

Reproduced deterministically. Cause: the helper logged with `eprintln!`,
which **panics** when stderr cannot be written. A helper launched by
`tauri dev` inherits a stderr pipe; once that parent is gone every log
write fails. The shutdown drain logs "grace expired with clients attached"
and the idle path logs "idle timeout … exiting" — both **before**
`process::exit` on the server thread. The panic killed that thread, and the
AppKit main thread kept the process alive: SIGTERM ignored, idle exit never
happens. (The other orphan in the incident exited because it had no client
attached and so never logged on that path.)

Fix:
- `log.rs`: `hlog!` — same shape as `eprintln!`, write errors dropped. All
  15 helper/client log sites use it; no `eprintln!` remains outside `src/bin`.
- `main.rs`: the server thread runs under `catch_unwind` and always calls
  `process::exit` (70 on panic), so no future panic on that thread can
  strand the process.

Regression tests:
- `tests/helper_termination.rs` (runs in `cargo test`): the real helper
  binary as a child with stderr broken after startup — idle exit must
  happen within 10 s; SIGTERM after rejected (unsigned) peers must exit
  within 10 s. With `eprintln!` restored the idle test fails (alive at 10 s).
- Gate check 13b: the exact incident — signed client attached, broken
  stderr, SIGTERM → exit within 10 s.

Not changed: the socket file is still left behind on exit (Phase A
behavior; the next helper unlinks and rebinds).

## 4–5. Held constant

- No macOS keyboard event tap. CS-03 remains vacuous and must be
  re-verified the day real keystroke capture lands (Phase C report,
  Finding 8).
- Argon2id tuple unchanged.

## Tests and gate

`npm run gate:phase-c`, run of record 2026-09-19 on this machine:
**PHASE C GATE: PASS (19 checks)** — the 17 Phase C checks plus:

| # | Check | Evidence |
|---|---|---|
| 1 | helper test suite | **114 tests, 0 failed** (112 + 2 new termination tests; the AppKit lock test reports a skip here and runs as 13a) |
| 13a | explicit lock preempts live panel, same connection | `locked` event +0.18 ms; panel off screen and `visible:false` +70 ms |
| 13b | SIGTERM, signed client attached, broken stderr | exited in 5 s (the §1.6 grace); before the fix: never |
| 14 | main app | `cargo check` + 9 capture wiring/decision tests |
| 17 | Phase B gate regression | PASS (6 checks) — full suite again, vector freshness, supply chain, Phase A 11/11, 10 min TLV fuzz |

All other Phase C checks unchanged and PASS (E2E lifecycle, capture-check
deny, RK kind, auto-lock op, panel events, change-MP, presence denial,
auto-lock, live panel, frontend build, supply chain: 96 helper deps < 120,
no rsa). No dependency or `cargo vet` changes in this pass.

## Remaining deviations / blockers

- All Phase C deviations stand (see `phase-c-verification.md` §6).
- Pre-Phase-E: cross-device Argon2id calibration.
- Phase D may now be considered; it still requires separate authorization.
