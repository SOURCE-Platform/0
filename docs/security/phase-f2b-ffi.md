# SOURCE Vault FFI catalogue (spec v0.5 §22.2)

Date: 2026-10-03 (revised after the steps 1–3 review, §6). Normative for
the `vault-ffi` crate: the complete list of what SOURCE Vault (iPhone) may
call in the vault engine, what the engine calls back, and what the engine
imports from the §2.12 bridge. A change here is a spec change. Enforced by
`vault-ffi/tests/catalogue.rs` (FFI-01) and `tests/handle.rs`.

## 1. Entry points (C ABI; the complete list)

| Symbol | Arguments | Result | Notes |
|---|---|---|---|
| `ov0_engine_open` | `vault_dir: *const c_char` (UTF-8 path), `callbacks: *const Ov0Callbacks` | handle, null on failure (a missing path or callback) | boots the engine and its auto-lock tick |
| `ov0_engine_call` | `engine`, `request: *const u8`, `len`, `out: *mut *mut u8`, `out_len: *mut usize` | `i32`: 0 with a JSON answer; -1 unusable arguments | one §1.5 op `{"op": …}`, limited to §3; requests over 64 KiB (the §1.4 frame cap) answer `INVALID_INPUT` |
| `ov0_engine_lock` | `engine` | — | locks at once; never waits behind an op |
| `ov0_engine_free` | `buf` (an answer) | — | zeroes, then frees; the length is read from the answer's own 8-byte prefix, never from the caller |
| `ov0_engine_close` | `engine` | — | locks, waits for the op in flight, ends the tick, releases the handle |

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
  the iOS forms of §1.6's sleep and screen-lock triggers. The §1.6
  auto-lock window (since the last authorization, `set_auto_lock_minutes`)
  is enforced by the engine itself: a tick once a second, and a check
  before every op.
- On a `locked` event Swift dismisses any secure entry or Recovery Key
  sheet still on screen; the interrupted op fails closed at its re-lock
  checkpoint.
- `ov0_engine_close` only after every other call has returned.

## 2. Callbacks (`Ov0Callbacks`, supplied by Swift; every member non-null)

| Field | Signature | Crossing | Contract |
|---|---|---|---|
| `ctx` | `*mut c_void` | — | Swift's; every callback safe from any thread |
| `secure_entry` | `(ctx, kind: u8, timeout_ms, a, a_len, b, b_len, cap) -> i32` | **(b) in** | kinds 0 new MP (Swift asks twice and compares), 1 MP, 2 MP change (old → `a`, new → `b`), 3 RK words. Swift writes raw UTF-8 into **engine-owned zeroizing buffers** of `cap` (4,096) bytes and zeroes its own copy; 0 = submitted, anything else = cancelled. A **new** MP (kind 0, or `b` of kind 2) shorter than 8 characters or not UTF-8 is refused as a cancel — the Mac panel's rule. Secure text entry, hidden from capture |
| `recovery_sheet` | `(ctx, words, words_len, checkpoint, recovery, reason, timeout_ms) -> i32` | **(c) out** | the 24 words with the three non-secret lines; 0 only when the user confirmed they saved it; Swift keeps no copy |
| `presence` | `(ctx, reason) -> bool` | — | `LAPolicy.deviceOwnerAuthentication` (§22.4) |
| `capture_suppressed` | `(ctx, surface) -> bool` | — | whether the surface is hidden from capture (§14.4) |
| `event` | `(ctx, json, len)` | — | the §1.5 events, never secret; delivered only after the engine has released its locks (so a handler may block, but must not call back into the engine) |

## 3. Bridge imports (`Bridge.swift`, linked into the app; the complete list)

`ov0_se_key_create`, `ov0_se_key_create_bio`, `ov0_se_key_needs_user`,
`ov0_se_key_public`, `ov0_se_key_delete`, `ov0_se_sign_create`,
`ov0_se_sign_public`, `ov0_se_sign_digest`, `ov0_hpke_seal`,
`ov0_hpke_open_se_auth`. The engine passes `ov0_se_sign_digest` only
digests it built (§2.7 prehashes, §11.4 requests); no entry point lets
Swift supply one. Crossing **(a)** is the VK as the plaintext of
`ov0_hpke_open_se_auth`, written into an engine buffer; the bridge zeroes
its own copy. `ov0_hpke_seal` receives a VK only in a key rotation, which
no §4 op performs on the phone in F.2b; the bridge zeroes that copy too.

## 4. Ops allowed through `ov0_engine_call` (F.2b)

`get_state` (with `vault_open` and §22.14's `behind`), `unlock`,
`begin_recovery_unlock`, `list_items`, `reveal`, `add_item`,
`update_item`, `delete_item`, `list_history`, `list_deleted`,
`restore_revision`, `resolve_conflict`, `change_master_password` (incl.
`mode: "reset"` with the RK), `list_devices`, `registry_status`,
`set_auto_lock_minutes`, and the provider ops (`backup_prepare`,
`backup_blob_list`, `backup_transition_body`, `backup_commit_result`,
`backup_state_offer`, `backup_apply`, `stream_read`, `stream_begin`,
`stream_write`, `stream_end`, `stream_cancel`, `sign_provider_request` —
the engine builds every signed field, §11.4 — `session_close`,
`quarantine_status`, `remote_update_status`).

**Not allowed (`UNKNOWN_OP`):** vault creation, peer serving, enrollment
authorization, device revocation, total-loss recovery, and
`rotate_recovery_key` — its phone form (rotation staged before the sheet,
in a background task, the sheet saying whether the key is live, §22.10,
IO-04) does not exist yet; it comes with F.2d. The first materialization
from an enrollment bundle and the peer client come in step 5 with their
own entries here.

## 5. What never crosses

PK, `RK_bytes`, `sk_c`, `ikm_c`, a signature over a caller-supplied
digest, and any raw key byte. Every op answer is the §1.5 response, which
already excludes them. Crossing (d) is one record's plaintext per
`reveal` answer and one record's fields per `add_item` / `update_item`
request; crossing (e) is the `list_items` metadata. **Zeroing, stated
exactly:** the request bytes are zeroed once parsed and the answer buffer
when freed; the parsed request and the answer's in-memory JSON value are
dropped without zeroing, as in the helper's IPC (residual).

## 6. Checks (FFI-01)

- the unmangled exports in the crate's sources (`no_mangle`,
  `export_name`, any file) are exactly §1;
- the iOS static library (`llvm-nm` from the Rust toolchain) defines
  exactly §1 and imports exactly §3; the library must exist (the Phase F
  gate builds it);
- §4 holds none of the Mac-only ops, and a handle answers `UNKNOWN_OP`
  for each of them;
- `get_state` and lock answer while an op holds the lane;
- the secure-entry kinds, the MP-change pair, cancels and the new-MP rule;
- events wait for the engine to be free;
- an unlocked vault locks itself when its window runs out.

Review history: steps 1–3 milestone review (security + verification) —
dispositions in `phase-f2-verification.md` §6.
