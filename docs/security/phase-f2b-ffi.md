# SOURCE Vault FFI catalogue (spec v0.5 §22.2)

Date: 2026-10-03. Normative for the `vault-ffi` crate: the complete list
of what SOURCE Vault (iPhone) may call in the vault engine, and of what
the engine calls back. A change here is a spec change. Enforced by
`vault-ffi/tests/catalogue.rs` (FFI-01).

## 1. Entry points (C ABI; the complete list)

| Symbol | Arguments | Result | Notes |
|---|---|---|---|
| `ov0_engine_open` | `vault_dir: *const c_char` (UTF-8 path), `callbacks: *const Ov0Callbacks` | `*mut Ov0Engine`, null on failure | boots the engine on the directory, like the helper's start-up |
| `ov0_engine_call` | `engine`, `request: *const u8`, `len: usize`, `out: *mut *mut u8`, `out_len: *mut usize` | `i32`: 0 with a JSON answer in `*out`; -1 unusable arguments | one §1.5 op as JSON `{"op": …}`, limited to §3; the answer is the op's §1.5 response |
| `ov0_engine_lock` | `engine` | — | locks at once; never waits behind an op |
| `ov0_engine_free` | `buf`, `len` from one answer | — | zeroes the answer, then frees it (answers may carry crossing (d)) |
| `ov0_engine_close` | `engine` | — | locks, then drops the handle |

No unwinding crosses the ABI: every entry point runs inside
`catch_unwind` and aborts the process on a panic. Ops run one at a time
(as the helper's executor runs them); `get_state` and `ov0_engine_lock`
are answered at once.

## 2. Callbacks (`Ov0Callbacks`, supplied by Swift)

| Field | Signature | Crossing | Contract |
|---|---|---|---|
| `ctx` | `*mut c_void` | — | Swift's; every callback must be safe from any thread |
| `secure_entry` | `(ctx, kind: u8, timeout_ms, a, a_len, b, b_len, cap) -> i32` | **(b) in** | kinds 0 create MP (Swift asks twice and compares), 1 MP, 2 MP change (old → `a`, new → `b`), 3 RK words. Swift writes into **engine-owned zeroizing buffers** of `cap` (1,024) bytes and zeroes its own copy; 0 = submitted, anything else = cancelled. Secure text entry, capture hidden |
| `recovery_sheet` | `(ctx, words, words_len, checkpoint, recovery, reason, timeout_ms) -> i32` | **(c) out** | shows the 24 words with the three non-secret lines; 0 only when the user confirmed they saved it; Swift keeps no copy |
| `presence` | `(ctx, reason) -> bool` | — | `LAPolicy.deviceOwnerAuthentication` (§22.4 per-operation presence) |
| `capture_suppressed` | `(ctx, surface) -> bool` | — | whether the surface is hidden from capture (§14.4) |
| `event` | `(ctx, json, len)` | — | the §1.5 events; never secret |

The Secure Enclave is not a callback here: the engine calls the §2.12
bridge (`ov0_se_*`, `ov0_hpke_*`) directly, and SOURCE Vault links the
same `Bridge.swift`. Crossing **(a)** — the VK as the envelope open's
plaintext — is written into an engine buffer, and the bridge zeroes its
own copy (`resetBytes` after the copy). No bridge function signs a digest
the engine did not build.

## 3. Ops allowed through `ov0_engine_call` (F.2b)

`get_state`, `unlock`, `begin_recovery_unlock`, `list_items`, `reveal`,
`add_item`, `update_item`, `delete_item`, `list_history`,
`list_deleted`, `restore_revision`, `resolve_conflict`,
`change_master_password` (incl. `mode: "reset"` with the RK),
`rotate_recovery_key`, `list_devices`, `registry_status`,
`set_auto_lock_minutes`, the provider ops (`backup_prepare`,
`backup_blob_list`, `backup_transition_body`, `backup_commit_result`,
`backup_state_offer`, `backup_apply`, `stream_read`, `stream_begin`,
`stream_write`, `stream_end`, `stream_cancel`, `sign_provider_request` —
the engine builds every signed field, §11.4 — `session_close`,
`quarantine_status`, `remote_update_status`).

**Not allowed (answer `UNKNOWN_OP`):** vault creation, peer serving,
enrollment authorization (`begin_enrollment`, `enroll_*`), device
revocation and total-loss recovery — Mac-only in F.2b; the iPhone's
revocation and reverse enrollment (F2-D4) come with their own reviews.
The first materialization from an enrollment bundle and the peer client
are added in step 5 with their own entries here.

## 4. What never crosses

PK, `RK_bytes`, `sk_c`, `ikm_c`, a signature over a caller-supplied
digest, and any raw key byte. Every op answer is the §1.5 response,
which already excludes them (the IPC never-list). Crossing (d) is one
record's plaintext per `reveal` answer and one record's fields per
`add_item` / `update_item` request (the request copy is zeroed once
parsed; the answer buffer is zeroed when freed); crossing (e) is the
`list_items` metadata.

## 5. Checks (FFI-01)

- the `#[no_mangle]` functions in the crate's sources are exactly §1;
- the built iOS static library defines no `ov0_` symbol beyond §1 (the
  macOS build also carries the §2.12 bridge symbols);
- §3 contains none of the Mac-only ops;
- a handle answers `UNKNOWN_OP` for everything outside §3 and
  `get_state` at once.
