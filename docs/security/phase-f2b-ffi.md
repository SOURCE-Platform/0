# SOURCE Vault FFI catalogue (spec v0.5 §22.2)

Date: 2026-10-03 (revised after the steps 1–3 review, §6). Normative for
the `vault-ffi` crate: the complete list of what SOURCE Vault (iPhone) may
call in the vault engine, what the engine calls back, and what the engine
imports from the §2.12 bridge. A change here is a spec change. Enforced by
`vault-ffi/tests/catalogue.rs` (FFI-01) and `tests/handle.rs`.

## 1. Entry points (C ABI; the complete list)

| Symbol | Arguments | Result | Notes |
|---|---|---|---|
| `ov0_engine_open` | `vault_dir: *const c_char` (UTF-8 path), `callbacks: *const Ov0Callbacks` | handle, null on failure (a missing or non-UTF-8 path, a missing callback) | boots the engine and its auto-lock tick |
| `ov0_engine_call` | `engine`, `request: *const u8`, `len`, `out: *mut *mut u8`, `out_len: *mut usize` | `i32`: 0 with a JSON answer; -1 unusable arguments | one §1.5 op `{"op": …}`, limited to §3; requests over 64 KiB (the §1.4 frame cap) answer `INVALID_INPUT` |
| `ov0_engine_lock` | `engine` | — | locks at once; never waits behind an op — except the instant of a rotation's commit, which holds the core mutex (F.2d step 5, §4) |
| `ov0_engine_free` | `buf` (an answer) | — | zeroes, then frees; the length is read from the answer's own 8-byte prefix, never from the caller |
| `ov0_engine_close` | `engine` | — | sets the provider cancel flag (no further `http_send`; one already in Swift ends within its own timeout), stops and joins the tick, locks, waits for the op in flight, locks again, releases the handle; no engine thread calls Swift after it returns |

No unwinding crosses: every entry point runs inside `catch_unwind` and
aborts the process on a panic (the spec's `panic = "abort"`, met per
entry point; the workspace profile is shared with the macOS app).

**Calling rules (normative for Swift).**
- One handle per vault directory; open it only while protected data is
  available (a header read under file protection fails closed for the
  life of the handle).
- `ov0_engine_call` blocks while an op waits on the user: never on the
  main thread, never from inside a callback. Ops run one at a time;
  `get_state` and `ov0_engine_lock` answer at once.
- **Swift must call `ov0_engine_lock`** on
  `protectedDataWillBecomeUnavailable` and on entering the background —
  the iOS forms of §1.6's sleep and screen-lock triggers. **One exception
  (owner decision 2026-10-13):** once the new Recovery Key of a total-loss
  recovery has been confirmed, entering the background lets
  `provider_recover_finish` return — at most 30 s, inside a background
  task, the app covered — and locks right after, so the user always
  learns whether that key is in effect. Before the confirmation, and on
  `protectedDataWillBecomeUnavailable`, the lock is immediate. The §1.6
  auto-lock window (since the last authorization, `set_auto_lock_minutes`)
  is enforced by the engine itself: a tick once a second, and a check
  before every op.
- On a `locked` event Swift dismisses any secure entry or Recovery Key
  sheet still on screen; the interrupted op fails closed at its re-lock
  checkpoint. Swift discards, unseen, the answer of any op that began
  before a `locked` event (a reveal may still return), and treats
  `get_state` as authoritative.
- Every call holds its own reference to the handle; the auto-lock tick
  holds none (it ends when the handle closes) and runs under the same
  abort-on-panic rule.
- `ov0_engine_close` only after every other call has returned.

## 2. Callbacks (`Ov0Callbacks`, supplied by Swift; every function non-null)

| Field | Signature | Crossing | Contract |
|---|---|---|---|
| `ctx` | `*mut c_void` (may be null) | — | Swift's; every callback safe from any thread, including the thread calling `ov0_engine_lock` / `ov0_engine_close` (so it must not wait synchronously on that thread) |
| `secure_entry` | `(ctx, kind: u8, timeout_ms, a, a_len, b, b_len, cap) -> i32` | **(b) in** | kinds 0 new MP (Swift asks twice and compares), 1 MP, 2 MP change (old → `a`, new → `b`), 3 RK words, 4 the MP for adopting a key change (§2.7 master-password adoption; its own title, raised only by a user-started sync). Swift writes raw UTF-8 into **engine-owned zeroizing buffers** of `cap` (4,096) bytes and zeroes its own copy; 0 = submitted, anything else = cancelled. A **new** MP (kind 0, or `b` of kind 2) shorter than 8 characters or not UTF-8 is refused as a cancel — the Mac panel's rule. Secure text entry, hidden from capture |
| `recovery_sheet` | `(ctx, words, words_len, checkpoint, recovery, reason, timeout_ms) -> i32` | **(c) out** | the 24 words with the three non-secret lines; 0 only when the user confirmed they saved it; Swift keeps no copy |
| `presence` | `(ctx, reason) -> bool` | — | `LAPolicy.deviceOwnerAuthentication` (§22.4) |
| `capture_suppressed` | `(ctx, surface) -> bool` | — | whether the surface is hidden from capture (§14.4) |
| `http_send` | `(ctx, origin, method, path, auth (null = none), body, body_len, timeout_ms, out, out_cap, out_len, status: *mut u16, date: *mut u64) -> i32` | — (ciphertext, public data, engine-signed requests) | F.2d: provider HTTPS for the `provider_*` ops. The engine calls it only from the op thread, during a `provider_*` op, and only for the vault header's own provider origin — or, for `provider_recover_*` (no vault yet), the engine's allowlisted default origin. `auth` goes in the `Ov0-Auth` header. Swift writes the answer body into the **engine-owned buffer** of `out_cap` (the per-flow cap, 8 MiB + 4 KiB) and its HTTP `Date` (Unix seconds, 0 if absent); 0 = answered (any non-3xx status), 1 = unreachable (no complete answer within `timeout_ms` — a deadline on the whole exchange, not on the gaps between bytes — TLS failure, a refused redirect), 2 = the body would exceed `out_cap`. Swift's session: ephemeral, no URL cache, standard certificate validation (no pinning, D-12), redirects refused, `https` only (debug builds also allow `http://localhost` for a local test provider). The cancel flag is the engine's: it stops further calls; one already in Swift ends by its own deadline. It must not call back into the engine (a call meanwhile answers `BAD_STATE`) |
| `event` | `(ctx, json, len)` | — | the §1.5 events, never secret; delivered in order, only after the engine has released its locks (a handler may block, but must not call back into the engine); an op's own events arrive before its call returns |

## 3. Bridge imports (`Bridge.swift`, linked into the app; the complete list)

`ov0_se_key_create`, `ov0_se_key_create_bio`, `ov0_se_key_needs_user`,
`ov0_se_key_public`, `ov0_se_key_delete`, `ov0_se_sign_create`,
`ov0_se_sign_public`, `ov0_se_sign_digest`, `ov0_hpke_seal`,
`ov0_hpke_open_se_auth`. The engine passes `ov0_se_sign_digest` only
digests it built (§2.7 prehashes, §11.4 requests); no entry point lets
Swift supply one. Crossing **(a)** is the VK as the plaintext of
`ov0_hpke_open_se_auth`, written into an engine buffer; the bridge zeroes
its own copy (on every unlock, and on an adoption that opens the served
envelope). `ov0_hpke_seal` receives a VK when a vault is created, a device
is enrolled, or the key is rotated — since F.2d step 5 the phone rotates
(`rotate_recovery_key`, `revoke_device`), sealing the new VK to every
surviving device's agreement key; the bridge zeroes that copy too, and
copies it last so no error path skips the zeroing. (The plan's separate
phone seal-crossing review is folded into the chunk-2 review, which
found nothing in the seal path; it is listed for the final audit.)

## 4. Ops allowed through `ov0_engine_call` (F.2b–F.2d)

`get_state` (with `vault_open` and §22.14's `behind`), `unlock`,
`begin_recovery_unlock`, `list_items`, `reveal`, `add_item`,
`update_item`, `delete_item`, `list_history`, `list_deleted`,
`restore_revision`, `resolve_conflict`, `change_master_password` (incl.
`mode: "reset"` with the RK), `list_devices`, `registry_status`,
`set_auto_lock_minutes`, the stream ops (`stream_read`, `stream_begin`,
`stream_write`, `stream_end`, `stream_cancel`; pairing and large peer
answers), `session_close`, `quarantine_status`, `remote_update_status`,
the provider flows (below), and the phone's side of
enrollment (§5, §22.10; `vault/join_ops.rs`): `join_begin {qr, name}`
(the QR checked first, including `mac_key`; then fresh Secure Enclave
keys — refused if this phone has no Face ID, whose envelope could never
open; the hello to send over TLS pinned to the QR `fp`), `join_hello
{reply}` (the SAS, computed here from transcript v2 and never received;
the route proof for the bundle and ACK requests), `join_bundle_begin
{sha256, size}` (a session the bundle streams into, ≤ 256 MiB),
`join_complete {session}` (the §22.10 checks in order — the registry
ending in the QR's Mac, whose key hashes to `mac_key`, enrolling exactly
this device's keys under the id it assigned; the manifest signature; this
device's envelope **as the signed index names it**, bound to the hello's
nonce; the checkpoint under that VK — then the vault written, the VK's SE
commitment signed, `peer_endpoint` validated and kept **in the Keychain
only**, the vault left LOCKED; answers the ENROLL_ACK signature),
`join_finish` (the Mac accepted the ACK) and `join_abort`. From
`join_begin` until `join_finish` an attempt marker sits in the vault
directory; an attempt that fails, is aborted, or is interrupted (found at
the next start) is removed whole — files, Secure Enclave keys and the
Keychain item.

**Security changes (F.2d step 5, §22.10):** `rotate_recovery_key` and
`revoke_device {device_id}` run in their staged-first form
(`vault/staged_sheet.rs`): the rotation is fully staged with the store out
of the core and no mutex held; then the `recovery_sheet` callback; then,
with the core mutex held so a lock waits for it, only the commit. A lock
during the staging or the sheet discards the staged rotation; the §22.7
set-aside that precedes it has already committed (as §22.7 requires) and
is not undone. The sheet is refused while the screen is captured
(`CAPTURE_UNSAFE`, before anything is shown) and waits up to 600 s. A
marker naming the next key generation is written before the sheet
appears and removed only by the commit: any sheet shown that did not go
live — cancel, timeout, lock, kill — makes `get_state` report
`rk_sheet_void` ("the key on that sheet does not work; the earlier one
still does") until `dismiss_rk_sheet_notice`. Either op answers
`ROTATION_PENDING`, before any prompt, while a rotation of this device's
own still waits to publish. Swift publishes first before
`rotate_recovery_key` only — never before `revoke_device`, whose cutoff
an ordinary publication would defeat — runs the op inside a background
task (it covers the commit), and publishes again; a security change still
waiting to upload keeps a banner until "Sync now" lands it. Swift locks
the engine before it dismisses a sheet, dismisses a sheet the engine
stopped waiting for, and hides the sheet from capture itself. The words
become a Swift `String` for display, which cannot be zeroed (stated
residual of crossing c).

**Remove this vault (F.2d step 6, §22.9):** `remove_vault_preview` →
`{local_only, unknown, access_lost, removed}` (revisions with no provider
source; `unknown` while LOCKED); `remove_vault {confirm: true}` → presence,
then the files, Secure Enclave keys and Keychain items (floor,
`peer_endpoint`) are removed and the engine is UNINITIALIZED. Phone only;
the Mac's helper answers `UNKNOWN_OP`.

**Total-loss recovery onto the phone (F.2d chunk 3, §11.5, §11.8):**
`provider_recover_start {handle, kind: "mp"|"rk"}` (UNINITIALIZED only;
the origin is the engine's allowlisted default, as there is no vault yet)
runs the coordinator's locate, `recovery_begin` (its secure entry) and
download-and-verify; `recovery_preview` returns the FR-01 preview;
`provider_recover_finish` (RECOVERING only) runs `recovery_complete` — a
Face ID-bound `iPhone` identity (refused, `DEVICE_NOT_AUTHORIZED`, on a
phone without Face ID), the new Recovery Key sheet — and the finalize,
leaving the phone UNLOCKED on the fresh key with a fresh floor. The
step-wise `recovery_begin` / `recovery_complete` are the flows' alone.
Stated: no Mac can join a phone-recovered vault until F.2e (§22.13).

**A staged upload while LOCKED (§22.10, §22.11):** `provider_publish` is
also accepted while LOCKED when a fully staged publication survives (a
security change whose upload was cut short); it signs only what is staged
and needs no key. A staging is resumed — LOCKED or UNLOCKED — only while
it is still the one `vault.db` records: an authority change since
supersedes it (chunk-3 reviews SEC-I2 / VER-I1). The foreground trigger
sends `{resume: true}`, which only ever finishes such an upload and never
stages one. Recovery is refused while a pairing is in progress (its
marker owns the directory), and its Recovery Key sheet, like a
rotation's, is refused while the screen is captured. `get_state` reports `upload_waiting`, and Swift runs it
at every foreground. (LOCKED with a staged publication reports
`backing_up`, §13.) No `BGProcessingTask`: §22.10's "or the next
foreground" is the trigger.

**Not allowed (`UNKNOWN_OP`):** vault creation, peer serving, and
enrollment authorization.

**Provider flows (F.2d step 2, §11.1):**

- `provider_sync {interactive}` and `provider_publish {interactive}` run
  the coordinator's sync and "back up now" flows inside the call
  (`vault-ffi/src/provider.rs`). UNLOCKED only (`BAD_STATE` otherwise),
  except the LOCKED finish of a staged upload below. `provider_publish`
  with nothing of the phone's own to publish runs the sync instead, as
  the Mac's "Back up now" does: "Sync now" always checks the provider
  (§22.7). A publication carries every pending local change — edits, and
  a master-password change or reset made on the phone. `interactive` is set only for "Sync now"
  or an edit the user just saved; it decides whether the §2.7 adoption
  panel (kind 4) may appear.
- Sub-ops are the fixed list `SUB_OPS` (state, `backup_*`, stream,
  `sign_provider_request`, `session_close`, `remote_update_status`, and
  since chunk 3 `recovery_begin` / `recovery_preview` /
  `recovery_complete`); each
  frame is held to the 64 KiB cap, and events are flushed between them.
  The step-wise `backup_*` and `sign_provider_request` ops are **not** in
  this section's list any more: only the flows reach them (review
  SEC-O2).
- Answer: `{ok:true, result}` or `{ok:false, error, status}` —
  `BACKUP_UNAVAILABLE`, `BACKUP_CONFLICT`, `MP_ADOPTION_REQUIRED`, a
  provider code (only in the §15 shape, `[A-Z0-9_]{1,48}`; otherwise
  `PROVIDER_<status>`), or an engine code.
- After each flow the engine updates the persisted §11.3.2 access count
  (a 401 runs the locate probe first, outside the core mutex; a success
  counts only when the provider answered a request with a 2xx) and sets
  `access_lost` in `get_state` (reported while the vault is open); a state that revokes this phone records
  the removal lock, and one in which it is still active lifts it (§22.9).
- A Mac helper refuses every `provider_*` op (the Mac's coordinator runs
  in its main process).

**Peer sync (F.2c phone side, §22.8, §22.9):**

- `peer_sync_begin` returns the first signed request (a carriage entry,
  base64url) and the paired Mac's `peer_endpoint` (host hints, port, pin,
  token). The token authorizes nothing in the vault.
- `peer_sync_step {response | refused | session}` returns the next request,
  `done` (counts only), or `removed`.
- `peer_sync_receive {sha256, size}` opens a session for a Mac answer too
  large for one 64 KiB frame. The size cap is 8 MiB + 4 KiB, and the call
  needs an exchange in flight. The bytes arrive through the §1.3 stream
  ops, which check length and SHA-256; `peer_sync_step {session}` consumes
  them. A lock or 60 s idle clears it. The answer is then verified like any
  other (review SEC-I1).
- The removal lock applies only in SOURCE Vault's engine, set by
  `boot_phone`.
- The engine builds and signs every request field, so no caller digest is
  signed. It checks each answer in the requester order and admits
  revisions under §22.7.
- UNLOCKED only.
- The Mac helper refuses all three ops.

## 5. What never crosses

PK, `RK_bytes`, `sk_c`, `ikm_c`, a signature over a caller-supplied
digest, and any raw key byte. Every op answer is the §1.5 response, which
already excludes them. Crossing (d) is one record's plaintext per
`reveal` answer and one record's fields per `add_item` / `update_item`
request; crossing (e) is the `list_items` metadata. **Zeroing, stated
exactly:** the request bytes are zeroed once parsed and the answer buffer
when freed; the parsed request and the answer's in-memory JSON value are
dropped without zeroing, and `serde_json` may leave intermediate copies
of an answer as its buffer grows — as in the helper's IPC (residual).

## 6. Checks (FFI-01)

- the unmangled exports in the FFI's and the engine's sources
  (`no_mangle` on its own line or the item's, `export_name`) are exactly
  §1;
- the iOS static library (`llvm-nm` from the Rust toolchain): among its
  `ov0_` symbols, the defined ones are exactly §1 and the imported ones
  exactly §3; the library must exist (the Phase F gate builds it in the
  same step);
- §4 holds none of the Mac-only ops, and a handle answers `UNKNOWN_OP`
  for each of them (now including the step-wise `backup_*` and
  `sign_provider_request`);
- a callback table without `http_send` is refused at open;
- `get_state` and lock answer while an op holds the lane;
- the secure-entry kinds, the MP-change pair, cancels and the new-MP rule;
- events wait for the engine to be free, and an op's events arrive before
  its call returns;
- an unlocked vault locks itself when its window runs out, and Swift is
  told at once; with the tick stopped, the check before an op still
  locks it.

Review history: steps 1–3 milestone review and its bounded re-review —
dispositions in `phase-f2-verification.md` §6 and §6.1.
