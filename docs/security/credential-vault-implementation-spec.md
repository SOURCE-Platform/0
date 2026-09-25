# O / Source Credential Vault — Implementation Specification

**Status:** Implementation specification **v0.4** (2026-09-24). Phases A–E
and E.1 are implemented and verified against v0.3.1; **Phase F is specified
here and not yet implemented or authorized.** This document is normative;
it does not by itself authorize implementation. Supersedes v0.3.1.
**v0.4 changes (Phase F design closure, owner-approved; source:
`docs/security/phase-f-design-closure.md` revision 3, commit `cfc6b80`):**
provider requests are authenticated by P-256 signatures over a canonical
`ProviderRequest` — Secure Enclave device keys, and MP/RK recovery-auth keys
derived via RFC 9180 `DeriveKeyPair` — so **no symmetric backup credential
exists** (`device_backup_cred`, `creds.bin`, locators, `device_register`, the
revoke endpoint and finalize tag `0x0B` are removed; §2.2, §11.4);
content-addressed immutable blobs plus one CAS-only vault-state object and a
single atomic state transition for create/publish/finalize (§11.2–§11.3,
§11.8); revocation is one state transition (§11.4); total-loss recovery
revokes every prior device (§4.4, §11.8); stable 256-bit `revision_id`s with
blob hashes separate from logical identity, a heads merge model and an exact
counter algorithm (§3.2, §3.7); refusal of unseen revisions from a revoked
author (§3.2); chunked ciphertext IPC streams (§1.3); explicit
remote-completion status (§11.3); recovery handle instead of email, with
KDF-downgrade protection and a provider-wide MP-class recovery throttle, with RK-class recovery exempt so it cannot be locked out (§11.4–§11.5,
§12); device envelopes in the backup and iPhone envelope catch-up (§2.10,
§4.7); new states BACKING_UP / SYNCING / RECOVERING / COMPROMISED and internal
ROTATING_KEYS (§13); v2 formats with no migration of synthetic development
vaults. Full iPhone record sync is deferred to Phase F.2 (§18).
**v0.3 changes (historical correction pass):**
helper-mediated backup request signing so no backup credential ever crosses
IPC (§1.5, §11.4); fully specified atomic `recovery-finalize` transaction
(§11.8); the `recovery_epoch` entry itself installs the replacement device —
no separate epoch-start enroll, no key-substitution gap (§4.3–4.5); HPKE
decapsulation prefers Apple's native Secure-Enclave HPKE path, with the
custom adapter demoted to a gated fallback (§2.12, §17.4); stale MP/RK
process-boundary wording removed (§1.2 and sweep); Source ID extension text
aligned with the v2 registry (§20); recovery-credential storage wording
corrected (§11.4); `update_password` approvals require `credential_ref`
(§6.5); `header.json` gains `import_fp_salt` (§3.1); RFC 9106 attribution
corrected (§2.3); Secure Notes added as an explicit owner decision
checkpoint (§10.8); new test families BA/RF/SC and extensions (§16).
**v0.3.1 corrections (2026-09-20, Phase D.1):** current-VK registry
checkpoint (§4.8) replaces the requirement that a fresh device hold
historical VK-derived proof keys; the landed Phase D mechanics are now
normative here — rotation commit journal and revision-hash remap (§2.10),
backup-object flags/trailer (§3.7), object index covering header/registry/
wraps (§11.2), RK replacement collecting the current MP (§12 scenario 6),
MP-only recovery issuing a new RK and RK-only recovery setting a new MP
(§12 scenarios 3–4), `change_master_password {mode:"reset"}` (§1.5), and
the acknowledged Recovery Key sheet before setup commits (§1.7, §5.4).
**v0.3 correction (2026-09-19, Phase C.1):** total-loss recovery order
fixed. The recovering helper generates a fresh VK and re-encrypts the
vault **before** `recovery-finalize`; finalize installs the already-rotated
state and the vault enters UNLOCKED. There is no post-finalize VK rotation
(§4.5, §11.8, §12 scenario 3, §13.1–13.2, RF-08).
**macOS floor:** macOS 14+ (CryptoKit HPKE availability, §2.12).
**Canonical architecture:** `docs/security/credential-vault-security-architecture.md`
v0.3. Where this specification and v0.3 conflict, v0.3 controls and this
document must be corrected — except where this document records a genuine
platform constraint (§21), which requires owner decision before proceeding.
**Companion documents:** `docs/security/macos-signing-and-hardening.md`
(signing identities, designated requirements), plus the hardening mechanisms
already landed in this repository (capture exclusions, asset scope, CSP,
audit/vet) referenced in §14.
**Scope rule:** a separate implementation agent must be able to build the
system from this document without making new security architecture decisions.
Where this document is silent on a security-relevant choice, the correct
action is to stop and ask, not to choose.

**Terminology:** MP = master password, PK = password-derived key, RK =
recovery key, VK = vault key, SE = Secure Enclave, LA = LocalAuthentication,
HPKE = RFC 9180, AAD = AEAD additional authenticated data.

**Document conventions:**

- All byte strings in schemas are lowercase hex unless stated otherwise.
- All integers are unsigned, decimal in text form, big-endian in binary form.
- `uuid` = RFC 4122 UUIDv4, lowercase, hyphenated, random.
- Timestamps are Unix seconds unless stated otherwise.
- "TLV" refers to the canonical binary encoding defined in §4.2.
- Wire formats are versioned; any change requires a version bump and new
  cross-language test vectors.

---

## 1. Module and process architecture

### 1.1 Processes and binaries

v1 ships three signed executables, all inside `SOURCE.app`, all signed with
the team identity `9RGW34CMA2` defined in
`docs/security/macos-signing-and-hardening.md`:

| Binary | Bundle identifier | Role |
|---|---|---|
| `SOURCE.app` (existing Tauri app) | `com.racker.zero` | Main process: UI, capture, mobile server, backup client |
| `SOURCE.app/Contents/Library/vault-helper/SourceVaultHelper.app` | `com.racker.zero.vault-helper` | Vault core: keys, storage, policy |
| `SOURCE.app/Contents/Library/vault-nm-host/source-vault-nm-host` | `com.racker.zero.nm-host` | Chrome native-messaging broker (thin client of the helper) |

The two helpers are **separate Cargo packages** in a new Cargo workspace
(`src-tauri/vault-helper/`, `src-tauri/vault-nm-host/`), not modules of
`zero_lib`. This keeps their dependency graphs physically distinct from the
~500-crate main graph and makes `cargo tree -p source-vault-helper` a
meaningful audit gate.

**Helper bundle shape.** `SourceVaultHelper.app` is a proper `.app` bundle
with `LSUIElement = true` (no Dock icon, no menu bar) so that
LocalAuthentication prompts are attributed to a stable, signable bundle
identity. It is launched by the main app through LaunchServices
(`NSWorkspace`/`open -g --hide`), never `posix_spawn` of a bare executable,
so it becomes its own TCC/LA responsible process. It carries **no**
entitlements except `com.apple.security.cs.disable-library-validation` =
**absent** (v0.3 §15.3 requirement: the helper must not carry it) and no
sandbox entitlement (it is not sandboxed; it needs Keychain/SE access under
the user's existing grants).

The helper contains no Source functionality: no capture, no OCR, no mobile
server, no agent code, no FFmpeg/Tesseract, no HTTP client, no WebView. It
links no C libraries. Target dependency count is in §17.

**Phase F crates (v0.4).** Three further Rust packages join the workspace;
none is linked into the main app's crypto path:

| Package | Kind | Role |
|---|---|---|
| `vault-proto` | library, pure Rust (no rusqlite/objc2/Swift) | wire types and secret-free verification shared by helper, provider and tests: TLV, object/index/manifest v2, checkpoint, registry decode + structural chain verification, `ProviderRequest`, state commitment, handle normalization. Moved out of `vault-helper` without behaviour change |
| `vault-provider-core` | library | every provider semantic (§11) over three storage traits — `StateStore` (load → state + ETag, create-if-absent, replace-if-match, delete-if-match for the §11.3.1 rollback), `BlobStore` (put-if-absent, get, exists, GC delete) and `OpsStore` (get → value + ETag, create-if-absent, replace-if-match, delete, list-prefix: nonces, handle claims, rate-limit slots) — plus `FsStores` for tests and rehearsals |
| `vault-provider` | binary, separate deployable (not in `SOURCE.app`) | axum HTTP adapter + S3 stores; a portable container; no APNs in Phase F (a push module boundary is reserved for Phase G) |

`FsBackupStore` (the Phase D rehearsal backend, still present in the Phase E code) is to be retired in Phase F; its semantics move
into `vault-provider-core`, so rehearsals run the production provider logic
over `FsStores`.

### 1.2 Responsibility matrix

| Responsibility | Main process | Vault helper | nm-host |
|---|---|---|---|
| Vault Key residency | never | yes (unlocked only) | never |
| Master password / RK handling | **never enters this process** — collected only inside helper-owned native secure UI (§1.7); main sees status codes only | collects via its own panel, derives/uses, zeroizes | never |
| Provider request authentication (v0.4) | supplies only the typed operation, typed parameters and body hash; transports the helper-built `ProviderRequest` and signature verbatim | builds the canonical `ProviderRequest` and signs it (SE device key, or transient recovery-auth key) via `sign_provider_request` (§11.4); **no symmetric backup credential exists** | never |
| Encrypted vault storage (`vault.db`, wraps, registry, manifest) | never opens | exclusive owner | never |
| Cryptography (wraps, records, envelopes, signatures) | none | all | none |
| Device registry verification | none | all | none |
| Origin canonicalization / match policy | none | all | sends raw origin only |
| User-presence decisions (LA, iPhone approval) | none | all | none |
| Network access (mobile server, backup client, APNs relay client) | yes | **never** | no |
| Browser extension contact | no | no (via nm-host) | yes |
| iPhone protocol contact | yes (relays opaque frames) | verifies/signs | no |
| Capture suppression registration | yes (existing commands) | checks state via IPC before display-class releases | no |
| Vault management UI (item list, settings) | yes | serves metadata over IPC | no |

The main process must **not** become a general-purpose secret retrieval
layer: it never receives record passwords except the single approved fill,
and even that path is helper → nm-host → Chrome, bypassing the main process
entirely (§9). The main process's own vault UI receives metadata (titles,
usernames, hosts) only, after unlock.

### 1.3 IPC transport

- **Transport:** AF_UNIX `SOCK_STREAM` socket at
  `$HOME/.observer_data/vault/helper.sock`. Parent directory mode `0700`
  (already created by `core/vault_dir.rs`), socket file mode `0600`.
  Same-UID processes outside the signed set are additionally rejected by
  peer authentication (below).
- **Framing:** 4-byte big-endian length prefix + JSON frame. Maximum frame
  64 KiB; a larger declared length closes the connection. One request →
  one response; the helper may emit unsolicited `event` frames on the main
  app's connection only.
- **Protocol version:** `hello`/`hello_ok` handshake carries `proto: 1`.
  Mismatched major version → connection closed.
- **Concurrency:** the helper accepts at most one connection per client
  class (`app`, `nm-host`); a second connection of the same class replaces
  and closes the first (guards against a wedged client holding the slot).
- **Ciphertext streams (v0.4).** Data larger than a frame (backup blobs, the
  §5 enrollment bundle) moves in chunked streams; the frame cap is
  unchanged. Only ciphertext and public data ever travel this way.
  - Chunk ≤ 24 KiB raw (base64 32 768 B), so a frame is ≈ 33 KiB — about
    half the cap.
  - **Outbound (helper → main), pull-based:** `stream_read {session,
    sha256, offset}` → `{data, offset, total_len, eof}`; main requests the
    next chunk only when ready (backpressure). Only blobs listed in that
    session are readable. Main verifies SHA-256 before upload.
  - **Inbound (main → helper), acknowledged:** `stream_begin {session,
    sha256, size}` (only for a hash on the session's need list, size within
    the role cap and the session budget) → `{stream_id}`; `stream_write
    {stream_id, seq, offset, data}` strictly contiguous, one outstanding
    write per stream, ≤ 4 open streams per session; `stream_end` checks
    total length and incremental SHA-256 and atomically moves the blob into
    session staging; any violation → `TRANSFER_INVALID`, partial data
    deleted.
  - `session` and `stream_id` are 128-bit OsRng values bound to the
    opening connection. Caps: blob 1 MiB (registry 4 MiB, index 8 MiB);
    session 512 MiB; idle stream 60 s; idle session 10 min.
  - Staging lives in `vault/staging/<session>/` (0700, helper-owned), is
    deleted on `stream_cancel`, `session_close`, disconnect, lock (except a
    fully staged publication, §11.3), TTL expiry, and swept at helper start.

### 1.4 Helper peer authentication

Both directions authenticate via code identity, per the signing model in
`docs/security/macos-signing-and-hardening.md`:

1. **Before launch**, the main app statically verifies the helper bundle:
   `SecStaticCodeCreateWithPath` + `SecStaticCodeCheckValidityWithErrors`
   against designated requirement
   `anchor apple generic and certificate leaf[subject.OU] = "9RGW34CMA2" and identifier "com.racker.zero.vault-helper"`.
   Failure → helper is not launched; vault stays unavailable; event logged.
2. **On connection accept**, the helper obtains the peer pid
   (`getsockopt(LOCAL_PEEREPID)`), then
   `SecCodeCopyGuestWithAttributes(kSecGuestAttributePid)` +
   `SecCodeCheckValidityWithErrors` against:

   ```text
   anchor apple generic and certificate leaf[subject.OU] = "9RGW34CMA2"
     and (identifier "com.racker.zero" or identifier "com.racker.zero.nm-host")
   ```

   (Debug builds may relax to same-team, never to "any process".)
3. **Reverse direction:** the main app applies the same pid→SecCode check
   to the socket's accepted peer identity after connecting, requiring
   `identifier "com.racker.zero.vault-helper"`.
4. **PID-reuse mitigation (honest residual risk):** the check runs once at
   connect time and the connection is then pinned — a verified connection
   that drops is never "resumed"; a new connection re-verifies. A local
   attacker who can already inject code into the main process is outside
   this boundary (v0.3 §17). This is the accepted cost of socket-based IPC
   without literal XPC, as authorized by the v0.3 §15.3 clarification.
5. `getpeereid` UID equality is checked as a cheap first gate before the
   SecCode call.

### 1.5 IPC message catalog

All frames are JSON objects with an `op` or `event` discriminator.
`ref` = record identifier (§3.3). Every response carries
`{"ok": bool, "error": <code from §15> | null}` plus op-specific fields.
No error frame ever carries secret material, only codes and
human-safe context.

**Main app → helper (`app` class):**

| op | Purpose | State required | Notes |
|---|---|---|---|
| `hello` | handshake | any | `{proto, client:"app"}` |
| `get_state` | state machine state | any | |
| `unlock` | unlock via LA presence or phone approval | LOCKED | no secret material involved |
| `setup_vault` | first-device creation | UNINITIALIZED | `{handle}` (the public recovery handle, §11.5; not a secret). The helper's own secure panel collects the new MP and displays/prints the RK (§1.7); the helper stages the `create` state transition with its inline bootstrap blobs (§11.3) and returns `{session}`; main posts it via `backup_transition_body` + `sign_provider_request` (LOCKED, fully staged). The normalized handle is kept in helper `kv` (never in `header.json`, which the provider stores) for sheet reprints; the local vault commits at setup and the `create` is tracked as `pending_remote {op: vault_create}` until it commits. On `HANDLE_TAKEN` see `setup_retry_handle` |
| `setup_retry_handle` | replace a taken handle before the vault first reaches the provider | UNLOCKED + fresh presence, only while the `create` is pending | `{handle}`. The failed `create` already delivered its bootstrap blobs — including `recovery.wrap` sealing the current VK under the old RK — to the provider (§11.3), so the old RK must stop opening anything current. Because `RK_bytes` is not retained (§2.11), the helper: collects the current MP in its panel (verified against `password.wrap`); issues a **new** Recovery Key and shows the new sheet (with the new handle) for acknowledgement; then performs one §2.10 journaled **VK rotation** — every local record re-sealed, `password.wrap` re-sealed under the same PK, `recovery.wrap` under the new RK with a new `auth_salt_rk`, the genesis device's envelope re-sealed, and the header, the `kv` handle and the `pending_remote` update committed in the same journal — and re-stages `create` with the new bootstrap blobs. The old sheet then opens only the retired VK, which protects nothing (no record was ever published under it), and the window says the old sheet is void. Each further `HANDLE_TAKEN` repeats this |
| `begin_recovery_unlock` | MP or RK entry for local unlock/fallback | LOCKED | `{kind:"mp"\|"rk"}` only — the helper collects the secret in its own panel; response is a status code, never an echo. Total-loss recovery uses `recovery_begin` (v0.4) |
| `lock` | immediate lock | any | |
| `list_items` | metadata list | UNLOCKED | `[{ref, kind, title, username, hosts}]`, no secrets |
| `add_item` / `update_item` | create/modify record | UNLOCKED + fresh presence | one record's fields cross IPC once, main→helper only |
| `delete_item` | delete record (tombstone revision) | UNLOCKED + fresh presence | §3.2 revision model |
| `resolve_conflict` | pick/merge a conflicted record | UNLOCKED + fresh presence | `{ref, chosen_rev, edits?}` → new merge revision (§3.2) |
| `reveal` | show one password in capture-suppressed UI | UNLOCKED + fresh presence + capture check | one-shot, §14 fail-closed |
| `change_master_password` | re-wrap VK under new PK | UNLOCKED + fresh presence | helper panel collects old+new MP; main sees status only |
| `change_master_password {mode:"reset"}` | set a new MP without the old one | UNLOCKED + fresh presence | §12 scenario 5 on this device (e.g. after an RK unlock): the panel collects new+confirm only; no VK rotation; `password.wrap` is replaced atomically |
| `rotate_recovery_key` | new RK + VK rotation | UNLOCKED + fresh presence | helper panel collects the **current MP** (the MP wrap is re-sealed under the new VK, §12 scenario 6), then displays/prints the new RK and only commits once the user acknowledges it; main sees status only |
| `list_devices` / `revoke_device` | registry view / revocation | UNLOCKED + fresh presence | the panel collects the MP (verified against the committed wrap, or a new MP is set) and shows a new Recovery Key for acknowledgement; then registry `revoke` + VK rotation + both recovery classes re-keyed are committed locally, followed by one `publish` state transition that cuts the device off at the provider (§11.4); tracked as `REMOTE_UPDATE_PENDING` until committed (§11.3). The confirmation lists every device the target itself authorized (enroll entries with `authorizer` = target) and recommends revoking them too |
| `registry_status` | signed registry for a paired device's status refresh (§4.7) | LOCKED or UNLOCKED | read-only; returns the registry and `vault_id` and nothing else. Deliberately answers while locked: the registry involves no VK, and a revoked device must be able to find that out without the vault being unlocked. |
| `begin_enrollment` | start §5 flow | UNLOCKED | `{fp}` (the ephemeral server's certificate fingerprint) → `{secret, mac_device_id, vault_id, expires_in}`; main renders the QR (§5.2) |
| `enroll_hello` | the phone's ENROLL_HELLO, relayed | UNLOCKED | helper verifies the single-use secret, assigns the new `device_id`, fixes the transcript → `{reply, sas}`; the SAS is shown on the Mac and **never** sent to the phone |
| `enroll_confirm` | the user compared the SAS | UNLOCKED + fresh presence | signs the enroll entry and seals the envelope → `{session, manifest, checkpoint, blob list}`; main pulls the blobs with `stream_read` (§1.3). All ciphertext/public; nothing is written to the registry yet |
| `enroll_ack` | the phone's ENROLL_ACK, relayed | UNLOCKED | `{signature}` over §5.2's ACK digest; verifying it is what appends the entry |
| `cancel_enrollment` | tear the session down | any | secret zeroized; a cancelled attempt leaves no registry trace |
| `relay_to_device` / `relay_from_device` | opaque vault-protocol frames for iPhone approvals | any | main is a dumb pipe (§6). **v0.3.1 Phase E:** enrollment uses the typed ops above instead — the helper has to route those frames into its session state machine anyway, and a typed schema is something it can validate; approvals keep the opaque relay. |
| `backup_prepare` | stage a `publish` state transition | UNLOCKED → BACKING_UP | → `{session, expected_state, new_state, blob_count, body_sha256}` (§11.3) |
| `backup_blob_list` | list a session's blobs | BACKING_UP, RECOVERING, LOCKED (fully staged only); UNLOCKED for an `enroll_confirm` bundle session only | `{session, page}` → ≤ 400 `{sha256, size, role}` |
| `stream_read` | outbound ciphertext chunk | as `backup_blob_list` | §1.3 |
| `backup_transition_body` | the staged `StateTransition` bytes | as `backup_blob_list` | ≤ 8 KiB, except a `create` with inline bootstrap blobs (≤ 1 MiB + 8 KiB), which is pulled through `stream_read` like a blob |
| `backup_commit_result` | report the provider outcome | as `backup_blob_list` | committed → persist last-seen state; `STATE_MOVED` → sync then re-stage (≤ 3, then `BACKUP_CONFLICT`); failure → retry policy |
| `backup_state_offer` | verify a fetched provider state | UNLOCKED → SYNCING; RECOVERING | `{state}` → signature/rollback/fork checked → `{session, need pages}` |
| `stream_begin` / `stream_write` / `stream_end` / `stream_cancel` | inbound ciphertext | SYNCING, RECOVERING | §1.3 |
| `backup_apply` | merge a verified state | SYNCING → UNLOCKED | §3.2 merge + envelope refresh; counts only |
| `recovery_begin` | start total-loss recovery | UNINITIALIZED/LOCKED → RECOVERING | `{kind, locate_response}`; the KDF-policy check (§11.5) runs **before** the panel collects MP/RK; → `{session}` |
| `recovery_preview` | FR-01 data | RECOVERING | generation, date, item count, sheet comparison |
| `recovery_complete` | epoch + re-encryption + stage `finalize` | RECOVERING | §11.8 |
| `sign_provider_request` | sign one provider request | per §11.4 policy table | `{session?, operation, typed params, body_sha256}` → `{request_tlv, signature}`; the helper builds every canonical field itself (§11.4) |
| `session_close` | abort a session | any | staging deleted |
| `quarantine_status` | counts of refused revisions | UNLOCKED | counts only (§3.2) |
| `import_dashlane` | §10 import from user-picked path | UNLOCKED | helper opens the file itself |
| `approval_result` | deliver signed iPhone approval | AUTHORIZING | §6.5 |

**nm-host → helper (`nm-host` class):**

| op | Purpose | Notes |
|---|---|---|
| `hello` | `{proto, client:"nm-host"}` | |
| `fill_candidates` | `{origin, tab_url}` → `{request_id, accounts:[{ref,title,username}]}` or `{locked:true}` | no secrets; origin checked against tab_url (§9.4); conflicted records excluded (§3.2) |
| `fill_authorize` | `{request_id, ref, method:"local"\|"iphone"}` → presence → `{username, password, expires_in}` | one-shot; consumed on response |
| `save_new` | `{origin, username, password, title}` → `{ref}` | requires UNLOCKED (user just typed the value) |
| `save_update` | `{ref, origin, password}` → held pending | requires UNLOCKED **plus trusted confirmation** (§9.8): LA or iPhone approval bound to origin/ref/action; the password is applied only after approval and never shown in the confirmation UI |

**Helper → main app events:** `state` (state transitions), `locked`,
`approval_requested` (main relays to iPhone, §6.5), `enrollment_progress`,
`backup_progress`, `backup_pending` (`{urgent}` — a staged publication is
waiting, §11.3), `remote_update` (remote-completion status, §11.3),
`rotation_progress` (ROTATING_KEYS is internal, §13), `registry_changed`,
`capture_unsafe` (a display-class release was refused),
`secure_panel_visible` (`{visible: bool}` — main bumps the
sensitive-surface counter so Source capture suppresses while a helper-owned
secret panel is up, §14.2).

**Never across IPC, in either direction, in any op:** VK, PK, MP, RK
plaintext, RK words, RecoveryWrapPayload/DeviceEnvelopePayload **plaintext**
bytes, device private keys, recovery-auth private keys or their HKDF inputs
(`sk_c`, `ikm_c`, §11.4), bulk record plaintext export, password-history
dumps, decrypted-notes search,
any "dump all" operation, any op returning more than one record's secret
fields. MP/RK enter and leave only through helper-owned native UI (§1.7).
**Ciphertext that does cross IPC (v0.4):** sealed device envelopes (in the
§5 enrollment bundle and in backup sessions), sealed `password.wrap` and
`recovery.wrap`, record objects, and the public registry, index, manifest,
checkpoint and header — all through §1.3 streams scoped to a helper
session. Envelopes are HPKE ciphertext addressed to a device's Secure
Enclave key; the wraps are AEAD ciphertext at Argon2id cost. This is no new
exposure: the provider stores the same bytes, and a compromised main
process already has same-UID read access to the 0600 files. The helper
never touches the network, so there is no other way for these bytes to
reach the provider or the enrolling device.
There is intentionally **no** `export_vault` op in v1, and no symmetric
backup credential exists anywhere (§11.4). Provider-request authorization
is mediated exclusively through `sign_provider_request`: the helper builds
the canonical request itself and signs it; no op signs arbitrary
caller-supplied byte strings.

### 1.6 Process lifecycle

- **Startup:** main app launch → verify + launch helper → connect →
  `hello`. Helper starts in `LOCKED` (or `UNINITIALIZED` on a machine with
  no vault). Unlock is user-initiated via the app UI or implicitly on first
  fill request.
- **Shutdown:** app quit → main sends `lock` → helper zeroizes and exits
  after all clients disconnect (5 s grace). The helper also exits after
  30 min with zero clients.
- **Helper crash:** VK dies with the process. Main detects the socket
  close, marks vault LOCKED, relaunches the helper once automatically;
  further crashes within 5 min → error surfaced, no restart loop.
- **Main app crash:** helper stays resident and retains its lock state;
  on reconnect the main app re-runs `hello` and reads `get_state`.
  Security note: an UNLOCKED helper surviving a main-app crash keeps VK
  resident until the idle timeout — the 15-minute auto-lock bounds this.
- **Auto-lock triggers (helper-side):** 15 min since last authorization
  (configurable 5–60), system sleep, screen lock / session switch
  (distributed notifications), explicit `lock`, `COMPROMISED` entry.
- **Lock behavior after helper restart:** always LOCKED; unlock requires
  the §6 user-presence path (device envelope unwrap is itself ACL-gated,
  §2.8).
- **Background provider work (v0.4):** a publication staged while unlocked
  may finish while LOCKED (uploads and the state transition need only the
  SE signing key and ciphertext, §11.3). Background provider signing never
  raises a Touch ID/LA prompt; if the Keychain or Secure Enclave is
  unavailable (for example, the machine is locked) the helper returns
  `KEYCHAIN_UNAVAILABLE` and the coordinator queues and retries. ACLs are
  never changed to make this work.

### 1.7 Helper-owned native secure UI

The main process's WebView never renders fields that collect or display
the root recovery secrets. The helper — an LSUIElement app that can
activate to present a modal panel (the `pinentry-mac` pattern) — owns
native, capture-protected UI for:

- initial master-password creation (with confirmation field);
- master-password entry (recovery/fallback unlock);
- master-password change (old + new);
- Recovery Key entry (24-word field with offline checksum validation);
- Recovery Key display and printing (NSPrintOperation from the helper);
- trusted confirmations that must show transaction details outside any
  browser/extension context (§9.8 update confirmation text).

Mechanics:

- Panels are `NSSecureTextField`-based wherever a secret is typed, which
  sets secure event input — Source's keyboard recorder already suppresses
  on that signal (`IsSecureEventInputEnabled` gate,
  `platform/input/keyboard_macos.rs`).
- While a helper secret panel is visible the helper emits
  `secure_panel_visible`; the main app raises the sensitive-surface
  counter (`core/capture_exclusions.rs`), so Source's screen capture and
  one-shot frame APIs suppress for the exact visibility window (§14).
- The panel activates the helper app (`NSApplication.activate`), presents
  modally, and resigns on completion. Focus theft is mitigated by
  activating only on an explicit user-initiated op and by the panel
  naming the requesting flow in its title.
- The main process receives status codes only: `success`, `cancelled`,
  `wrong_credential`, `recovery_complete`. It never receives MP, PK, RK
  plaintext, RK words, RecoveryWrapPayload/DeviceEnvelopePayload bytes, or VK.
- The Recovery Key window is **acknowledgement-gated**: it offers only
  "Print…" and "I've saved it"; the calling op commits nothing until the
  user acknowledges (a dismissed or aborted window leaves no vault at
  setup and no rotation at RK replacement). It is excluded from screen
  capture by the OS (`NSWindowSharingNone`) in addition to §14
  suppression, and the capture bracket stays up for the whole window
  lifetime including the print dialog.
- Printing: the recovery sheet is rendered by the helper and sent
  directly to `NSPrintOperation` (never via the WebView, never written to
  a temp PDF). v1 keeps the **standard** macOS print dialog; no custom
  print panel is built to remove its PDF menu. The window carries this
  copy verbatim: *"Print to paper. Saving as PDF creates an unencrypted
  copy of your Recovery Key."* Spooled print data is outside the helper's
  control and no erasure is claimed for it. The sheet includes the vault_id, current manifest
  generation, and an 8-hex-char prefix of the registry head hash as a
  user-held freshness checkpoint (§11.7), plus (v0.4) the normalized
  recovery handle and the provider origin (§11.5).
- **Remote-pending copy (v0.4).** After an RK replacement the window must
  not claim the old Recovery Key is dead until the provider transition has
  committed (§11.3 remote-completion status). While pending it states that
  the backup still accepts the previous key; for a security-driven
  replacement (suspected theft) a persistent warning stays up until the
  remote cutoff commits.
- iOS has no process split: the Source iOS app itself is the vault
  boundary on the phone (no capture stack, no agent surface there); MP/RK
  entry on iOS uses the app's own secure fields.

There is no macOS platform constraint forcing MP/RK through the main
process; the pinentry-style helper panel is the normative design, not a
fallback.

---

## 2. Cryptographic data model

No custom primitives. All constructions below are named standards composed
in the stated way; §16.8 defines shared Rust↔Swift test vectors for every
composition.

### 2.1 Primitives

| Use | Primitive | Implementation |
|---|---|---|
| Password KDF | Argon2id (RFC 9106) | RustCrypto `argon2` |
| Record/wrap/metadata AEAD | XChaCha20-Poly1305 (RFC 8439 + XChaCha draft) | RustCrypto `chacha20poly1305` |
| Subkey derivation / KEK derivation | HKDF-SHA-256 (RFC 5869) | RustCrypto `hkdf` + `sha2` |
| Device signatures | ECDSA over P-256 with SHA-256; randomized signing accepted; canonical low-S wire form (§2.7) | RustCrypto `p256` (verify/normalize) / Apple SE + CryptoKit (sign) |
| Device key agreement envelopes | HPKE base mode: DHKEM(P-256, HKDF-SHA-256), HKDF-SHA-256, ChaCha20-Poly1305 (RFC 9180); 65-byte uncompressed key serialization | Rust `hpke` crate (seal; software open for tests/vectors) + **Apple CryptoKit HPKE with SE-backed keys for production decapsulation** (direct on iOS; via the `vault-apple-crypto` Swift bridge on macOS) — §2.12 Path A; custom adapter only as a gated fallback (Path B) |
| Provider request signatures (v0.4) | ECDSA P-256/SHA-256 over the §11.4 prehash, low-S. Device class: Secure Enclave (randomized). Recovery class: software key, RFC 6979 deterministic | SE via the bridge; RustCrypto `p256` `SigningKey` |
| Recovery-auth key derivation (v0.4) | HKDF-SHA-256 → RFC 9180 §7.1.3 `DeriveKeyPair` for DHKEM(P-256, HKDF-SHA256) (§11.4) | RustCrypto `hkdf` (Extract/Expand) + `p256` — no new dependency |
| Hashes | SHA-256 | `sha2` / CryptoKit |
| RNG | OS CSPRNG | `rand::rngs::OsRng` / `SecRandomCopyBytes` |
| Constant-time compare | `subtle` / manual | tokens, secrets, SAS |

### 2.2 Key hierarchy

```text
                        Vault Key (VK, 32 bytes, OsRng)
                ┌───────────────┼────────────────────┐
                ▼               ▼                    ▼
        password.wrap    recovery.wrap        devices/<id>.wrap
        key: HKDF(PK)    key: HKDF(RK)        HPKE-sealed to device
                                              agreement public key

  PK  = Argon2id(MP, kdf_salt, params)          (32 bytes, transient)
  RK  = 32 bytes OsRng, shown as BIP-39 words   (never stored by Source)
```

Payloads (v0.4 — the v0.3 `device_backup_cred` is removed, because
provider authentication no longer uses any symmetric credential, §11.4):

```text
RecoveryWrapPayload (password.wrap / recovery.wrap):
  TLV { 0x01 vk: 32 B, 0x02 wrapped_at: u64, 0x03 vk_generation: u32 }

DeviceEnvelopePayload v2 (devices/<id>.wrap):
  TLV { 0x01 vk: 32 B, 0x02 wrapped_at: u64, 0x03 vk_generation: u32 }
  tag 0x04 (v0.3 device_backup_cred) is forbidden — decoding rejects it
```

- The two shapes are now identical in content. They are never confused:
  wraps are XChaCha20-Poly1305 under MP/RK-derived keys with the §2.5 AAD;
  envelopes are HPKE-sealed with `info = "ov0/envelope/v2" ‖ …` (§2.5).
- No wrap or envelope contains MP, PK, RK, device private keys, or any
  backup/provider credential.
- **Provider authentication keys (§11.4)** are not stored in any wrap:
  devices use their Secure Enclave signing key; MP/RK recovery uses keys
  derived transiently inside the helper from PK / RK_bytes, and only their
  public keys leave the helper. The MP recovery-auth key is in the same
  password-guessing class as `password.wrap` (§11.4 security
  qualification); only the RK-derived key has 256-bit strength.

### 2.3 Argon2id parameters and upgrade path

- v1 parameters: `m = 64 MiB`, `t = 3`, `p = 1`, output 32 bytes, random
  16-byte `kdf_salt`. **Attribution (corrected against the RFC text,
  v0.3 finding 10):** RFC 9106 §7.4 names exactly two recommended
  profiles — **first:** `t=1, m=2 GiB`; **second:** `t=3, m=64 MiB`
  "for memory-constrained environments" (the RFC's reference
  parameterizations use `p=4`). The often-quoted `m=19 MiB, t=2, p=1`
  minimum is **OWASP Cheat Sheet guidance, not RFC 9106** — do not cite
  it as such. Our tuple keeps the RFC's second-profile memory cost and
  iteration count (the dominant hardness parameters) and differs only in
  using one lane: an application-specific parallelism reduction chosen
  for predictable latency on memory-constrained devices.
- **Calibration gate (Phase B, before parameters freeze):** benchmark
  Argon2id on every supported Mac class and iPhone class; record unlock/
  recovery latency and memory pressure; choose the strongest parameters
  whose interactive cost stays ≤ 1 s on a 2020 MacBook Air and ≤ 2 s on
  the oldest supported iPhone; commit the benchmark table alongside the
  chosen tuple. Recovery security must not be weakened for speed — if a
  device class can't meet the latency target at `m=64 MiB`, prefer
  raising its latency budget over lowering memory cost, and document the
  choice. **Hardware floor (owner decision, confirmed 2026-09-21):** the v1
  minimum is **A15-class or newer** — the iPhone 13 family, iPhone SE
  3rd generation, and later. The owner's words were "we're not going to
  cover anything before iPhone 13"; it is recorded as a **chip** rather
  than a model year so the SE 3rd generation, which carries an A15, is
  included and the iPhone 12 (A14) is not. Rationale: no A12-class
  device was ever measured, and an untested support claim was not
  acceptable to ship.
  The slowest supported device is therefore the iPhone 13 mini measured
  at median 89 ms / worst 124 ms (`argon2-calibration.md`), inside the
  budget above, so `m=64 MiB, t=3, p=1` is **frozen** rather than
  provisional. The tuple was not weakened to reach this — the supported
  device set was narrowed instead. Older hardware may be added later
  only by measuring it against these same parameters.
- Storage (v0.4 header): `header.json` carries the exact block
  `kdf: {alg: "argon2id", version: 19, m_kib: 65536, t: 3, p: 1,
  out_len: 32, salt: hex16}` (`version` is the Argon2 version 0x13).
  `password.wrap` carries a copy of the same parameters.
- **Client allowlist (v0.4, normative).** The helper compiles in the set of
  supported tuples — in v1 exactly the frozen tuple above — and refuses any
  KDF parameters outside it **before** deriving anything from a master
  password, wherever they come from (the provider's locate response,
  §11.5; a downloaded header; a wrap). The helper never substitutes or
  "repairs" supplied parameters. A provider therefore cannot make a device
  run a cheaper-than-policy MP derivation; it can only deny service.
- Upgrade: a new tuple is introduced **only by a client release** that
  adds it to the allowlist; the provider can never introduce one. The next
  successful MP unwrap then re-derives PK with the new parameters and
  rewrites `password.wrap` (a remote-tracked MP change, §11.3). Old
  parameter sets remain readable until re-wrapped. There is no downgrade:
  every tuple outside the compiled-in allowlist is refused, and when the
  allowlist holds more than one tuple the helper also refuses one weaker
  than the tuple in the vault's current committed header. In v1 the
  allowlist holds exactly one tuple, so the allowlist check alone decides.
  The check applies to `password.wrap` too, whose public block (m, t, p,
  salt) must equal the header's `kdf` block; the version and output length
  are fixed by the allowlisted tuple.

### 2.4 Recovery Key representation

- RK = 32 bytes from OsRng.
- Display/entry encoding: BIP-39 English mnemonic, 24 words
  (256 bits entropy + 8-bit checksum). The BIP-39 encode/decode is
  **implemented in-house** (~100 lines: 11-bit packing over the official
  2048-word English list, SHA-256 checksum) in both Rust and Swift, checked
  against the published reference test vectors — this avoids a
  poorly-audited third-party mnemonic crate in the helper (§17).
- Entry normalisation: lowercase, trim, collapse whitespace, validate
  wordlist membership + checksum before use. Wrong words → §15 error
  `RECOVERY_KEY_INVALID`, no oracle beyond "not a valid recovery key".
- RK usage: `RK_bytes = mnemonic_decode(words)` (the 32-byte entropy,
  not a PBKDF2 expansion — BIP-39 seed stretching is not used; the entropy
  is already 256 bits).

### 2.5 Wrap constructions

```text
password.wrap (JSON):
{ "v":1, "kind":"mp", "kdf_version":1, "argon2id":{"m":65536,"t":3,"p":1,
  "salt":hex16}, "nonce":hex24, "ct":hex }

  wrap_key = HKDF-SHA256(ikm=PK, salt=argon2id.salt,
                         info="ov0/wrap/mp/v1")           // 32 bytes
  ct = XChaCha20-Poly1305-Seal(wrap_key, nonce=random24,
       plaintext=RecoveryWrapPayload, aad="ov0/wrap" || vault_id || "mp")

recovery.wrap: identical shape, kind="rk",
  wrap_key = HKDF-SHA256(ikm=RK_bytes, salt=random16-stored,
                         info="ov0/wrap/rk/v1")
  aad = "ov0/wrap" || vault_id || "rk"
```

```text
wraps/devices/<device_id>.wrap (JSON, v0.4):
{ "v":2, "kind":"device", "device_id":hex16, "enrollment_nonce":hex16,
  "enc":hex65, "ct":hex }

  HPKE base mode, suite per §2.12, sealed to the device's Secure Enclave
  agreement key; plaintext = DeviceEnvelopePayload v2 (§2.2);
  info = "ov0/envelope/v2" || vault_id || device_id || enrollment_nonce
```

v0.4 removes `wraps/devices/creds.bin` and its `ov0/device-creds/v1`
key: with no symmetric device credential (§11.4) there is nothing for the
authorizing device to preserve across rotations.

- Nonces: 24 bytes, OsRng, per seal operation. 192-bit random nonces make
  accidental reuse negligible at vault scale (≤ 2^20 seals).
- Wrong MP/RK produces an AEAD tag failure → `WRONG_CREDENTIAL`, fail
  closed, no partial state. There is no password-verifier oracle stored
  anywhere.

### 2.6 Record and metadata encryption

```text
record_key  = HKDF-SHA256(ikm=VK, salt=record_id_bytes,
                          info="ov0/record/v1")          // per-record subkey
record_ct   = XChaCha20-Poly1305-Seal(record_key, nonce=random24,
              plaintext=record JSON (§8), aad=record_aad)
graph_digest = SHA-256("ov0/rev-graph/v2" || record_id || revision_id || author
               || u64be(counter) || u8(flags) || u8(kind) || u8(n) || parents(sorted))
record_aad  = "ov0/record/v2" || vault_id || record_id_bytes || revision_id
              || u32be(schema_version) || u32be(vk_generation) || graph_digest

meta_key    = HKDF-SHA256(ikm=VK, salt=header.meta_salt,
                          info="ov0/meta/v1")
meta_ct     = Seal(meta_key, nonce=random24, plaintext=metadata JSON,
              aad="ov0/meta/v2" || vault_id || record_id_bytes || revision_id
                  || field_tag || graph_digest)
```

(v0.4) The AAD binds each ciphertext to its logical identity and graph
position (`revision_id`, record, author, counter, flags, kind, parents), so
moving a ciphertext to another revision, record or parent set — including
by tampering with a local `vault.db` — fails AEAD. It does not prevent a
current VK holder from authoring anything (§3.2).

Per-record subkeys mean a hypothetical future single-record key exposure
does not cascade. Moving a record's ciphertext to another `record_id`
fails AAD. Re-encryption at VK rotation rewrites `vk_generation` and all
nonces.

### 2.7 Device identity keys

- **Signing key:** P-256 ECDSA, SE-generated (`kSecAttrTokenIDSecureEnclave`,
  `SecKeyCreateRandomKey`, `privateKeyUsage` sign). Non-exportable.
- **Agreement key:** P-256 ECDH, SE-generated, distinct key. Used only via
  HPKE DHKEM decapsulation; the private DH operation runs inside the
  Secure Enclave, never in software — via Apple's own HPKE
  implementation with the SE key (§2.12 Path A); raw
  `SecKeyCopyKeyExchangeResult` is touched only if the §2.12 Path B
  fallback is ever triggered.
- Never one key for both roles (v0.3 C14).
- SE access control: `.privateKeyUsage` + `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`;
  the agreement key adds no biometric ACL of its own (presence is enforced
  explicitly by LA at the operation level, §6, so prompts are uniform and
  testable across devices).
- **Provider requests (v0.4):** the signing key also signs provider
  requests under the distinct prefix `ov0/provider/request/v2` (§11.4).
  One key serving several prehash domains is permitted; §2.7's rule
  separates *signing* from *agreement* roles.
- **Signature policy (corrected, v0.2 finding 5):** Secure Enclave /
  CryptoKit ECDSA signing is randomized, not RFC 6979 deterministic. The
  protocol therefore depends only on: valid P-256 ECDSA over the SHA-256
  domain-separated digest, and the canonical wire form below. Producers
  **normalize to low-S before embedding** (`s' = min(s, n−s)`; Rust:
  `p256::ecdsa::Signature::normalize_s()`; Swift: normalize the `s` half of
  `rawRepresentation` before TLV encoding). Verifiers **reject high-S** so
  the bytes inside hash-chained objects are canonical and a relay cannot
  re-encode an entry to a different hash. Repeated signing of the same
  message may yield different valid signatures; tests verify signatures,
  never compare them for equality.
- **Public keys on the wire (corrected, v0.2 finding 5):** both roles use
  the 65-byte uncompressed X9.63 form (`0x04 ‖ X ‖ Y`). RFC 9180
  DHKEM(P-256, HKDF-SHA256) serializes KEM public keys uncompressed, and
  using one encoding for both roles removes a conversion step that would
  otherwise live on the verification path. (CryptoKit's
  `x963Representation` is exactly this form.)

### 2.8 Keychain usage

| Item | Class | ACL | Purpose |
|---|---|---|---|
| `com.racker.zero.vault.state` | generic password, data blob | `WhenUnlockedThisDeviceOnly` | helper runtime bookkeeping (last manifest generation/head hash seen — rollback evidence) |
| SE keys (both) | key class | §2.7 | device identity |
| `com.racker.zero.vault.helper-prefs` | generic password | `WhenUnlockedThisDeviceOnly` | auto-lock minutes, non-secret prefs |

The Keychain never stores VK, wrap payloads, MP, RK, or backup
credentials. Unlock uses the
device envelope: HPKE-decapsulate `devices/<self>.wrap` with the SE
agreement key after an LA presence check.

**Implemented in Phase E.1 (`unlock`, §1.5).** The refusals are
normative, because an envelope on disk is not authority to open a vault:

- the **registry** decides whether this device may still unlock — a
  revoked device keeps its envelope file and is refused;
- an envelope whose `vk_generation` is not the header's is refused: it
  holds a key retired by a rotation (§2.10);
- a denied presence check leaves the vault LOCKED and is **not** counted
  as a wrong credential, so it never drives the §15 backoff;
- the master-password path is the fallback for a device with no usable
  envelope, and the main app falls back only on that condition — never
  on a refused presence check, which is the user declining. The
decapsulation yields the VK only (v0.4, §2.2). If the SE key is missing
(device restored from backup, key wiped), the device must re-enroll or
recover — documented behavior, not an error.

**Background provider signing (v0.4)** uses the SE signing key without a
presence prompt (the keys carry `.privateKeyUsage` but no user-presence
flag, §2.7). If the Keychain or SE is unavailable, requests are queued and
retried (§1.6); no ACL is changed.

### 2.9 HKDF context registry

| info string | Derives |
|---|---|
| `ov0/wrap/mp/v1` | MP wrap key from PK |
| `ov0/wrap/rk/v1` | RK wrap key from RK |
| `ov0/record/v1` | per-record key from VK |
| `ov0/meta/v1` | metadata key from VK |
| `ov0/recovery-auth/v1` | registry recovery-epoch proof key from VK (§4.5) |
| `ov0/provider-recovery-auth/mp/v2` ‖ `vault_id` | MP recovery-auth `ikm_mp` from PK (salt `auth_salt_mp`), input to `DeriveKeyPair` (§11.4) |
| `ov0/provider-recovery-auth/rk/v2` ‖ `vault_id` | RK recovery-auth `ikm_rk` from RK_bytes (salt `auth_salt_rk`) (§11.4) |
| `ov0/import-fingerprint/v1` | import idempotency HMAC key from VK (§10.3) |
| `ov0/enroll/sas/v1` | SAS display bytes from enrollment transcript |
| `ov0/approval/…` | not used — approvals are plain ECDSA over TLV (§6.5) |

Other v0.4 domain strings (hash/signature prefixes, not HKDF infos):
`ov0/provider/request/v2` (request prehash, §11.4), `ov0/vault-state/v2`
and `ov0/recovery-auth-set/v2` (state commitment, §11.2), `ov0/handle/v2`
(handle key, §11.5), `ov0/record/v2`, `ov0/meta/v2`, `ov0/rev-graph/v2`
(record AAD, §2.6/§3.2).

**Retired in v0.4:** `ov0/backup-auth/{mp,rk}/v1`, `ov0/locate/{mp,rk}/v1`,
`ov0/locator/v1`, `ov0/device-creds/v1`, `ov0/rev/v1`, `ov0/envelope/v1`,
`ov0/backup-req/v1`.

HPKE `info` strings (envelopes, v0.4): `"ov0/envelope/v2" || vault_id ||
device_id || enrollment_nonce`.

### 2.10 Versioning and rotation

- `header.json` fields (header v2, v0.4): `vault_id` (uuid, random at
  creation), `version` = 2, `kdf` (exact §2.3 block), `meta_salt`,
  `import_fp_salt`, `auth_salt_mp`, `auth_salt_rk` (16 B each, OsRng;
  replace v0.3's `locator_salt_*`, and are regenerated on every change of
  the corresponding recovery-auth key, §11.4), `provider` (the provider
  origin, e.g. `https://…`), `vk_generation`, `registry_head`,
  `manifest_generation`.
- `import_fp_salt` (v0.3 finding 9): **16 bytes, OsRng at vault
  creation**, non-secret (same classification as `kdf_salt`), persisted
  in `header.json` (0600, inside the vault directory). It versions with
  the header schema, not with VK: **rotation does not change it** — the
  derived `import_fp_key` already changes with VK (§10.3), and
  fingerprints are recomputed under the new key inside the rotation
  transaction below, so a salt change would buy nothing and would only
  widen the recompute's blast radius. If a future `version` bump ever
  rotates the salt, the same transaction must recompute every
  fingerprint before the manifest flips.
- Every record stores the `vk_generation` it was sealed under.
- Rotation (§12): new VK → `vk_generation += 1` → all records re-sealed →
  all wraps rewritten → **all `import_log` fingerprints recomputed under
  the new VK-derived key (§10.3), same SQLite transaction** → new
  manifest generation → old VK zeroized.
  **Rotation is staged and committed through a journal (normative):** the
  engine writes the complete rotated vault beside the live one
  (`vault.db.next`, `wraps/*.next`, `header.json.next`,
  `manifest.json.next`), then writes one commit marker
  (`rotation.commit`, atomic rename) naming the new `vk_generation`, the
  new manifest generation, and any wrap to remove — that marker is the
  commit point — and only then renames the staged files over the live
  ones. Every open runs recovery first: a marker present → roll forward
  (finish the renames, idempotent, then delete the marker); staged files
  without a marker → roll back (delete them). An opened vault is
  therefore entirely pre-rotation or entirely post-rotation. Re-running a
  rotation "from scratch" after a crash is **not** possible and must not
  be specified: the new VK exists only in the staged wraps.
- **Stable revision identity across rotation (normative, v0.4).** Every
  revision keeps its `revision_id`, parents, author and counter (§3.2);
  rotation re-seals `ct`/`meta_ct` with fresh nonces under the new VK and
  the new `vk_generation` in the AAD, which changes only each object's
  `blob_hash` (§3.7). The logical revision graph is never renamed; v0.3.1's
  revision-hash remap is removed. The pre-rotation blobs stay in the
  backup's retained generations and remain decryptable with the matching
  historical key material (§12 scenario 7 limitation, BK-10).
- **Device envelopes rotate with the VK (normative).** The per-device
  envelopes (§2.5) are staged in the same journal as the wraps, listed in
  the commit marker's `stage` array, and rolled forward in the same pass.
  A rotation can therefore never commit a new VK while leaving an enrolled
  device holding a wrap of the dead one. Each envelope keeps the
  enrollment nonce it was first bound to, so only the VK inside changes.
  Surviving devices' envelopes are part of every published state (§11.2),
  so a device that was offline at rotation time fetches its new envelope
  from the provider later (§4.7, §11.5).
- **Pending local work across a rotation (v0.4).** A device still at the
  old `vk_generation` that holds unsynced revisions of its own opens its
  new envelope, adopts the new state, re-seals those revisions under the
  new VK with the same `revision_id`s and parents, and then zeroizes the
  old VK. This is a transition, not retention (§2.11).
- Old VK/new state: AEAD failure everywhere; there is no fallback path.

### 2.11 Memory-lifetime rules

- All secret buffers (`VK`, `PK`, `RK_bytes`, wrap payloads, record
  plaintext, recovery-auth `ikm_c`/`sk_c`) live in
  `zeroize::Zeroizing`/`secrecy` types.
- Transient secrets cross a trust hop exactly once: derive → use →
  zeroize. The helper never caches MP/PK/RK beyond the call that used
  them (state machine §13 reflects this).
- The helper calls `setrlimit(RLIMIT_CORE, 0)` at startup and
  `mlock` on the VK buffer (best-effort; documented as non-guarantee,
  v0.3 §17).
- nm-host zeroizes fill responses after writing them to stdout; the
  Chrome extension nulls references after fill (JS cannot guarantee
  erasure — documented limitation, §9.9).
- Panic behavior: the helper does not catch panics around secret-bearing
  code; process death is the zeroization of last resort. Release builds
  compile the helper with `panic = "abort"`, `debug = false`, and no
  secret-bearing type implements `Debug`.

### 2.12 HPKE with Secure-Enclave-backed agreement keys

Problem (v0.2 finding 6): a non-exportable SE agreement key cannot be
handed to any HPKE library API that takes a software private key; the
decapsulation DH must happen inside the Secure Enclave. Resolution is
attempted in a fixed order (v0.3 finding 4). **The native Apple path is
the default; the custom adapter is a gated fallback, not the design.**

**Path A — preferred: CryptoKit native HPKE with the SE key.**
Verified against current Apple documentation (September 2026):
`SecureEnclave.P256.KeyAgreement.PrivateKey` conforms to
`HPKEDiffieHellmanPrivateKey`, and
`HPKE.Recipient.init(privateKey:ciphersuite:info:encapsulatedKey:)`
(iOS 17.0+, macOS 14.0+) is generic over that protocol — so Apple's own
HPKE can decapsulate directly against an SE-resident key. The required
RFC 9180 suite is expressible as
`HPKE.Ciphersuite(kem: .P256_HKDF_SHA256, kdf: .HKDF_SHA256,
aead: .chaChaPoly)` = DHKEM(P-256, HKDF-SHA256) / HKDF-SHA256 /
ChaCha20-Poly1305 (KEM 0x0010, KDF 0x0001, AEAD 0x0003).

- **iOS app:** `HPKE.Sender` / `HPKE.Recipient` used directly with
  `SecureEnclave.P256.KeyAgreement.PrivateKey`.
- **macOS helper (Rust):** same API through a tiny in-house Swift bridge,
  `vault-apple-crypto` (static library, C ABI, target ≤ 200 lines of
  Swift). Production surface (v0.3.1 Phase E, 8 symbols): key lifecycle
  per role — `ov0_se_key_create` / `ov0_se_key_public` /
  `ov0_se_sign_create` / `ov0_se_sign_public` / `ov0_se_key_delete`
  (deletes both roles for a tag) — `ov0_se_sign_digest(tag, digest32) →
  r‖s` (randomized; the caller normalizes to low-S per §2.7),
  `ov0_hpke_seal(pubkey65, info, plaintext, aad) → (enc, ct)` and
  `ov0_hpke_open_se(key_tag, info, enc, ct, aad) → plaintext`. `aad` is
  a v0.3.1 addition: RFC 9180 authenticates additional data per message,
  and the published vectors use it. The bridge is the only Swift linked
  into the helper; it holds no policy and no key material beyond SE
  references. The PoC-only software-open and blob-inspection entry points
  live in the PoC crate's own shim, not in the shipping bridge.
  Keychain note: the bridge stores SE key blobs in the same login
  keychain the helper's Rust Keychain code uses. The data-protection
  keychain would need a `keychain-access-groups` entitlement the gate and
  test binaries do not carry; the legacy keychain's global lock means
  callers must not hit it from several threads at once (see the Phase E
  verification report).
  A minimal bridge into Apple's reviewed implementation is strictly
  preferable to reimplementing HPKE's key schedule in Rust.
- **Rust `hpke` crate** is used by the §2.12 PoC only. The shipping
  helper adds **no** Rust HPKE dependency: both directions go through
  the CryptoKit bridge, so the helper's dependency graph is unchanged by
  Phase E (v0.3.1).

**Path B — only if A is impossible: external-DH adapter.** If the PoC
shows Path A cannot interoperate with the exact suite (byte-level
mismatch, conformance absent at the deployment floor, platform bug),
document the exact Apple API limitation with authoritative evidence
(documentation citation + failing vector), then fall back to the v0.2
adapter design: `hpke_se.rs` (≤ 150 lines) performing RFC 9180 KEM
ExtractAndExpand + base-mode key schedule over an SE-computed DH
(`SecKeyCopyKeyExchangeResult`), verified against official RFC 9180
vectors and independently reviewed (§19). Path B is never the default
and never ships without the documented impossibility of A.

**Path C — prohibited by default.** Any broader hand-composition of
HPKE requires a documented failure of A and B, explicit independent
security review, and an owner decision. It is not an automatic fallback.

**Hard pre-gate to Phase E (force unchanged):** with a real
SE-resident key, the PoC must demonstrate: (a) Rust `hpke` seal →
Path A open on macOS; (b) CryptoKit Sender seal → Rust open; (c)
CryptoKit Sender seal → Path A open on iOS with an SE key; all three at
the exact suite, cross-checked against RFC 9180 vectors (XV-HPKE-SE).
Phase E envelope code does not start until this PoC is green; if (a) or
(c) fails, the PoC report documents the precise limitation and Path B is
evaluated under §17.4.

**macOS floor:** CryptoKit HPKE requires macOS 14.0; the vault feature
floor is therefore **macOS 14+** (iOS 17+ per §21). SE keygen/signing
predates that floor, but no vault code path may rely on HPKE below it.

---

## 3. On-disk vault format

### 3.1 Layout and permissions

```text
~/.observer_data/vault/                 0700  (created by core/vault_dir.rs,
│                                             .metadata_never_index present)
├── helper.sock                       0600  IPC endpoint (§1.3)
├── header.json                       0600  §2.10 fields
├── vault.db                          0600  SQLite (WAL), records + tombstones
├── vault.db-wal / vault.db-shm       0600  never leaves this directory
├── manifest.json                     0600  signed state head (§11.3)
├── registry.json                     0600  append-only device log (§4)
├── device.json                       0600  this device's public identity
│                                           (id, name, platform, SE key
│                                           tag, both 65-byte public keys)
├── wraps/
│   ├── password.wrap                 0600  §2.5
│   ├── recovery.wrap                 0600  §2.5
│   └── devices/
│       └── <device_id>.wrap          0600  HPKE envelope per device (v2)
├── staging/                          0700  helper-owned §1.3 stream sessions
│                                           (ciphertext/public only; swept at start)
└── import/                           0700  transient; empty between imports
```

Only the helper opens anything under this directory. The main process
creates the directory (already landed) and knows paths for housekeeping,
but never opens `vault.db`, wraps, registry, or manifest. The directory is
structurally excluded from Source's asset protocol (config deny rule +
`core/asset_scope_guard.rs` tests + runtime probe), from capture/indexing
search roots, and from timeline/export paths (§14).

### 3.2 SQLite schema and revision model (vault.db, `user_version = 2`, v0.4)

```sql
CREATE TABLE record_revs (           -- every admitted revision of every record
  revision_id   BLOB PRIMARY KEY CHECK(length(revision_id)=32), -- stable logical id (below)
  record_id     TEXT NOT NULL,       -- uuid
  parent_ids    BLOB NOT NULL,       -- concatenated 32-byte parent revision_ids, sorted
  author_device TEXT NOT NULL,       -- registry device_id of the author (never all-zero)
  counter       INTEGER NOT NULL,    -- per-(record_id, author_device), §3.2 counters
  deleted       INTEGER NOT NULL DEFAULT 0,  -- 1 = tombstone revision
  kind_tag      INTEGER NOT NULL,    -- 1=login, 2=card (plaintext; §3.4)
  vk_generation INTEGER NOT NULL,
  schema_version INTEGER NOT NULL,
  nonce         BLOB NOT NULL CHECK(length(nonce)=24),
  ct            BLOB NOT NULL,       -- record JSON under record_key
  meta_nonce    BLOB NOT NULL CHECK(length(meta_nonce)=24),
  meta_ct       BLOB NOT NULL,       -- metadata JSON under meta_key
  created_at    INTEGER NOT NULL,    -- display/sort only, never merge authority
  updated_at    INTEGER NOT NULL     -- display/sort only, never merge authority
);
CREATE INDEX idx_revs_record ON record_revs(record_id);
CREATE TABLE record_tips (           -- the single head, when there is one
  record_id TEXT PRIMARY KEY,
  tip_rev   BLOB                     -- revision_id; NULL while conflicted
);
CREATE TABLE record_conflicts (      -- the heads while |heads| > 1
  record_id TEXT NOT NULL,
  revision_id BLOB NOT NULL,
  PRIMARY KEY (record_id, revision_id)
);
CREATE TABLE pending_revs (          -- received, parents not yet all admitted
  revision_id BLOB PRIMARY KEY, record_id TEXT NOT NULL, object BLOB NOT NULL
);
CREATE TABLE record_flags (          -- per-record freeze (equivocation / author fork)
  record_id TEXT PRIMARY KEY, frozen INTEGER NOT NULL, evidence BLOB NOT NULL
);
CREATE TABLE refused_revs (          -- counts only: tamper / revoked-author evidence
  record_id TEXT NOT NULL, reason INTEGER NOT NULL, count INTEGER NOT NULL,
  PRIMARY KEY (record_id, reason)
);
CREATE TABLE author_hwm (            -- highest counter THIS device ever authored
  record_id TEXT PRIMARY KEY, counter INTEGER NOT NULL
);
CREATE TABLE import_log (            -- §10.3; no plaintext-derived identity
  fingerprint BLOB PRIMARY KEY,      -- HMAC under a VK-derived key
  identity_ct BLOB NOT NULL          -- identity string sealed under meta_key
);
CREATE TABLE kv (                    -- helper-internal non-secret bookkeeping
  key   TEXT PRIMARY KEY,
  value BLOB NOT NULL
);
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
```

**Identity (v0.4).** `revision_id` is **256 bits from OsRng**, created once
when the revision is authored. Parent links are `revision_id`s. It carries
no derived semantics, never changes when the ciphertext changes, and is
**linkable across rotations by design** (`record_id` already exposes record
continuity). Storage and backup addressing use a separate
`blob_hash = SHA-256(serialized encrypted object)` (§3.7); the authenticated
index maps `revision_id → blob_hash` (§11.2). A VK rotation changes blob
hashes only. v0.3's content-committed `rev_hash` is removed.

**Integrity binding.** Provider-side substitution or re-parenting is
prevented by blob naming, the signed index, the manifest signature and the
checkpoint (§11.2); relabeling a ciphertext is prevented by the AEAD AAD
(§2.6). **Authorship is not non-repudiation (normative):** the per-revision
`author` and `counter` are not cryptographic proof against another
*currently trusted* VK holder, which can construct arbitrary validly
encrypted revisions with any author, counter and parents. They exist to
detect consistency problems and to drive synchronization, never to grant
anything. The boundary after revocation is the new VK plus the provider
authorization cutoff (§11.4). Per-revision signatures are not used.

**Authorship.** New revisions carry this device's registry `device_id`;
the all-zero id is invalid. `author_hwm` never decreases, and
`next_counter = max(author_hwm, max counter of own held revisions) + 1`.
A device whose local vault is behind its Keychain last-seen generation
(local rollback, e.g. a restored `vault.db`) refuses to author until it has
synced to at least that generation (§4.6).

**Heads model (deterministic; no timestamp ever decides).**

- A revision is **applicable** when every parent is admitted locally;
  otherwise it waits in `pending_revs`. Batches apply in topological order.
- Per record, `heads` = admitted revisions with no admitted child. Applying
  R sets `heads ← (heads \ ancestors(R)) ∪ {R}`. One head → `tip_rev`;
  more → `tip_rev = NULL`, `record_conflicts = heads`, the record is
  excluded from fills (`CONFLICT_PENDING`).
- **Resolution:** `resolve_conflict` (fresh presence) authors a revision
  whose parents are all current heads, collapsing them. A partial cover
  leaves the uncovered heads in conflict.
- **Tombstones:** a delete becomes the state only as the sole head; a
  non-deleted revision with a tombstone on the path that made it a head is
  a conflict against that tombstone (never a resurrection); authoring an
  edit on a tombstoned record is refused.
- **Duplicates:** the same `revision_id` at the same `vk_generation` with a
  different `blob_hash` is decrypted and compared — identical header and
  plaintext is benign (keep the lower `blob_hash`), anything else is
  revision equivocation (freeze). A lower-`vk_generation` representation of
  a known id is superseded, not a conflict.
- Published indexes must be **ancestor-closed structurally**: every parent
  a `rev` line names is itself listed in the same index (the provider
  enforces this, §11.3 step 5). A manifest whose index names a parent that
  is not listed, or whose listed blob is missing, is `MANIFEST_MISMATCH`.
  Revisions that this device **refuses** (revoked author, below) or
  **rejects** (counter regression) do not make a structurally valid state
  invalid: they are counted as evidence, their descendants are refused or
  re-authored as specified below, and the state remains mergeable and
  publishable. A device therefore never fails to merge, and never fails to
  publish its revocation, because another device published revisions it
  refuses.

**Counters — exact algorithm.** Honest-author invariant: an honest author A
never reuses a counter, and every revision A previously authored on record
X is an ancestor of A's next revision on X (A's parents are all current
heads, and A may only edit a single head or resolve all heads). When an
**applicable** R = (A, c, X) is applied, compare it with every admitted
P ≠ R by A on X:

| Relation | Condition | Result |
|---|---|---|
| P ∈ anc(R) | P.counter < c | normal |
| P ∈ anc(R) | P.counter ≥ c | **counter regression**: R rejected, not applied, counted as tamper evidence (`COUNTER_REGRESSION`) |
| P ∉ anc(R) | P.counter = c | **equivocation**: both kept as heads, record frozen |
| P ∉ anc(R) | P.counter ≠ c | **author fork**: both kept as heads, record frozen |

Because the check runs only on applicable revisions, whose ancestry is
complete, out-of-order delivery cannot trigger it: a lower-counter
ancestor that is missing keeps R pending, and a concurrent pair by one
author is detected identically whichever arrives first.

**Freeze.** `record_flags(frozen)`: the record is excluded from fills,
`update_item`/`delete_item` return `CONFLICT_PENDING`, the item shows a
tamper marker, and it unfreezes only via `resolve_conflict` with fresh
presence and an explicit acknowledgement. Vault-level COMPROMISED is
reserved for registry forks and vault-level tamper (§4.6).

**Revisions from a revoked author (normative, v0.4).** Let D be revoked by
the registry entry at seq `s_r`, and `M_rev` the first manifest in the
accepted chain whose registry includes it (the revoker's revocation
transition, §11.4). `Admit(D)` = the D-authored `revision_id`s listed in
`M_rev`'s index.

- The **revoker** defines `Admit(D)` as every D-authored revision in its
  local database at the moment of its *local* revocation commit — the
  history the person authorizing the revocation had accepted. The rotation
  re-seals them, so they appear in `M_rev`. From that commit on, any
  incoming D-authored `revision_id` it does not already hold is refused.
- **Every other device**, on accepting `M_rev`, keeps `Admit(D)` as ordinary
  history and refuses every other D-authored revision it holds, has
  pending, or receives later.
- **Refused** means: never admitted (not a head, parent-satisfier, fill
  candidate or conflict participant), never included in any index this
  device publishes, local ciphertext deleted after the device's rotation
  transition, and counted in `refused_revs` (count only; "N changes from
  removed device X were not accepted"). It is not deletion from committed
  provider history: older retained manifests keep referencing those blobs
  until normal GC.
- **Why refuse rather than quarantine:** every such revision is sealed
  under a VK the revocation's rotation retires, and invariant 7 forbids
  keeping that VK, so no device could ever show it for review.
- **Descendants:** a revision by a non-revoked device B with a refused
  ancestor can never become applicable. If B still holds it as pending
  local work, B re-authors it (new `revision_id`, author B, parents =
  current heads) and tells its user it was re-applied; other devices refuse
  it with the same count.
- **Trust never follows provider arrival order.** A provider-order cutoff
  would let a device compromised while unlocked keep injecting revisions
  until the revocation lands; the revoker-knowledge cutoff closes that
  window at the cost of losing (as a counted refusal) last-moment edits the
  revoker never saw.

`PRAGMA secure_delete = ON`. The DB file never leaves the device except as
per-record ciphertext objects in backups (§11); the DB itself is not
uploaded.

### 3.3 Record identifiers

Random UUIDv4, generated at creation, stable for the record's life,
unrelated to content. Nothing content-derived appears in `record_id`;
the backup index lists it (§11.2), which leaks only existence/size to the
provider — accepted and documented in v0.3 F15.

`revision_id` (v0.4): 32 bytes from OsRng per revision, created at
authoring and never changed (§3.2).

### 3.4 Plaintext vs encrypted fields

| Field | Where | Plaintext? | Why |
|---|---|---|---|
| `record_id` | db, backup index | yes | needed for sync/backup addressing; random, unlinkable |
| `revision_id`, `parent_ids`, `author_device`, `counter`, `deleted` | db, backup objects, backup index (ids and parents) | yes | the merge graph must be computable without decrypting, and the provider checks ancestor closure (§11.3); leaks edit-pattern metadata (who edited, roughly when, how often) — accepted, v0.3 F15 class. `revision_id` is linkable across rotations by design |
| `blob_hash` | backup storage key, index | yes | content addressing of the encrypted object (§3.7) |
| `kind_tag` | db | yes | chooser/list rendering without full metadata decrypt; leaks login-vs-card only |
| `created_at`, `updated_at` | db | yes | UI display/sorting only — explicitly **not** merge authority (§3.2) |
| ciphertext sizes | db/backup | yes | unavoidable; documented metadata leak |
| title, username, hosts/URLs, notes | `meta_ct` / `ct` | **no** | metadata minimization (v0.3 §7) |
| password, card fields | `ct` | **no** | — |
| `vault_id`, `vk_generation`, KDF params, `auth_salt_mp/rk`, `provider` | header.json | yes | required for unwrap / recovery lookup; not secret |
| device public keys, names | registry.json | yes | required for verification |
| vault item count | manifest | yes | provider metadata leak, accepted |

### 3.5 Versioning and migration

- `header.json.version` governs the directory format; SQLite
  `user_version` governs the schema. Migrations are forward-only,
  hand-written, each wrapped in a transaction; unknown newer versions →
  fail closed (`FORMAT_TOO_NEW`) — never silently open.
- **v0.4 is a clean v2 break** (header `version` 2, `user_version` 2,
  object `OV0OBJ02`, manifest v2, index v2, envelope v2). No real vault has
  shipped; synthetic development vaults are not migrated and v1 files are
  refused with `FORMAT_TOO_NEW`/`FORMAT_INVALID`.
- `manifest.json` (local copy) mirrors the §11.3 signed manifest: the
  helper refuses to open a vault whose local records don't match the
  manifest's object list (`MANIFEST_MISMATCH`), forcing the
  restore/verify path instead of trusting a swapped directory.

### 3.6 Corruption handling

- Open sequence: parse header → `PRAGMA integrity_check` → load manifest →
  verify manifest signature + registry head → spot-verify zero records
  (lazy: per-record AEAD failure surfaces at use as `RECORD_CORRUPT`).
- AEAD tag failure on a record → that record is quarantined (read as
  corrupt, never partially returned), error surfaced, record restorable
  from a backup copy (a peer copy from Phase F.2). Tampering never yields plaintext.
- `vault.db` unrecoverable → ERROR state, user offered "restore from
  backup" / "restore from peer device" (Phase F.2) (§12); the helper never deletes
  the file automatically.

### 3.7 Backup-compatible serialization (`OV0OBJ02`, v0.4)

Each revision's backup object is a byte-exact, self-describing binary:

```text
field            size      notes
magic            8         "OV0OBJ02" (v0.4; "OV0OBJ01" is refused)
kind_tag         1         1=login, 2=card
flags            1         bit0 = tombstone; bits 1–7 reserved, must be 0
vk_generation    4         u32 BE
counter          8         u64 BE
author           16        device_id (never all-zero)
record_id        16        uuid bytes
revision_id      32        §3.2
parent_count     1         0–8
parents          32×n      parent revision_ids, sorted ascending
nonce            24
ct_len           4         u32 BE
ct               ct_len
meta_nonce       24
meta_ct_len      4         u32 BE
meta_ct          meta_ct_len
trailer          20        schema_version u32 BE, created_at u64 BE, updated_at u64 BE
```

- Total object size ≤ 1 MiB; parsers enforce `parent_count ≤ 8`, sorted
  unique parents, length consistency, the reserved flag bits, and no
  trailing bytes. Unknown magic → `FORMAT_TOO_NEW`, never a best-effort
  parse.
- `schema_version` and the graph fields are bound by the record AAD
  (§2.6); the timestamps are plaintext display metadata (§3.4) that a
  restore preserves.
- `blob_hash = SHA-256(object bytes)` is the object's storage name
  (§11.2). A parser checks that the embedded `revision_id` and
  `record_id` equal the index line that named the blob.
- The helper emits objects byte-identical between local storage and
  upload; the provider needs no transformation and learns nothing new.

## 4. Device registry

### 4.1 Model

`registry.json` is an append-only log of entries, one JSON object per
line (JSONL, `\n`-terminated). The canonical form for hashing and signing
is the TLV encoding in §4.2 — the JSONL is a storage/inspection
representation; two files are equivalent iff their TLV decodings are
identical.

Entry kinds: `genesis` (first device, self-signed), `enroll`, `revoke`,
`recovery_epoch` (§4.5).

### 4.2 Canonical binary encoding (TLV)

Deterministic encoding used for signatures, hashes, and approval payloads
(§6.5). JSON canonicalization (JCS) is deliberately **not** used:
float/Unicode edge cases across Rust/Swift/JS are a known footgun, and a
fixed-schema binary encoding is trivial to implement identically in all
three languages.

```text
Document  := Tag(0x00) Len(u32be) Value(Entry)*      -- outer wrapper
Entry     := (Field)*  EndOfEntry(0xFF)
Field     := Tag(u8) Len(u32be) Value
  - tags are emitted in strictly ascending numeric order
  - Len counts Value bytes only
  - strings: UTF-8, NFC-normalized, no truncation at encode; decoders
    enforce documented max lengths
  - integers: minimal-length big-endian (no leading zero bytes)
  - byte strings: raw
  - no floats, no booleans (use u8 0/1), no optional absence ambiguity:
    a field is present or absent, never empty-with-meaning
```

### 4.3 Registry entry fields (tags)

| Tag | Field | Type | Present in |
|---|---|---|---|
| 0x01 | `entry_version` | u32 (=2) | all |
| 0x02 | `seq` | u64, monotonic | all |
| 0x03 | `prev_hash` | 32 B | all (genesis: 32 zero bytes) |
| 0x04 | `epoch` | u64 (new epoch on recovery_epoch) | all |
| 0x05 | `kind` | u8: 1 genesis, 2 enroll, 3 revoke, 4 recovery_epoch | all |
| 0x06 | `device_id` | 16 B uuid | all (recovery_epoch: the **new** device) |
| 0x07 | `device_name` | string ≤ 64 chars | genesis/enroll/recovery_epoch |
| 0x08 | `platform` | u8: 1 macos, 2 ios | genesis/enroll/recovery_epoch |
| 0x09 | `sign_pub` | 65 B uncompressed P-256 (§2.7) | genesis/enroll/recovery_epoch |
| 0x0A | `agree_pub` | 65 B uncompressed P-256 (§2.7) | genesis/enroll/recovery_epoch |
| 0x0B | `enrolled_at` | u64 | genesis/enroll/recovery_epoch |
| 0x0C | `authorizer` | 16 B device_id | genesis/enroll/revoke — **absent** in recovery_epoch |
| 0x0D | `revoked_at` | u64 | revoke |
| 0x0E | `recovery_proof` | 32 B (§4.5) | recovery_epoch only |
| 0x0F | `manifest_hash` | 32 B, head manifest the transition binds | recovery_epoch only |
| 0x10 | `signature` | 64 B r‖s, low-S (§2.7) | genesis/enroll/revoke — **absent** in recovery_epoch |
| 0x11 | `prior_epoch` | u64 | recovery_epoch only |
| 0x12 | `vault_id` | 16 B | recovery_epoch only |
| 0x13 | `recovery_nonce` | 16 B, OsRng by recovering device | recovery_epoch only |

```text
entry_hash = SHA-256("ov0/registry/entry/v1" || tlv(Entry))   -- includes signature/proof
sign_input = SHA-256("ov0/registry/sign/v1"  || tlv(Entry without field 0x10))
```

`entry_version = 2` reflects the v0.2 changes: 65-byte key encoding, and
recovery_epoch carrying a possession proof instead of a fake signature
(v0.2 findings 2 and 5). There is no v1 registry in the wild; no
migration exists.

### 4.4 Validity rules

A registry state is **valid** iff all hold:

1. seq starts at 0 and increments by exactly 1; no gaps (truncation).
2. Every `prev_hash` equals the previous entry's `entry_hash` (genesis:
   zeros).
3. Every **genesis/enroll/revoke** entry carries a `signature` that
   verifies with the `sign_pub` of the `authorizer` device, which must be
   enrolled and not revoked at that `seq`. This rule does not apply to
   recovery_epoch (rule 6 is its only authorization).
4. Genesis: `authorizer` == own `device_id`; self-signature must verify.
5. A revoked device appears in exactly one `revoke` entry; no enroll after
   revoke for the same `device_id` (re-enrollment = new device identity).
6. **Recovery transition + device installation (single entry, v0.3
   finding 3):** a recovery_epoch entry carries no `authorizer` and no
   `signature`. It is valid iff its `recovery_proof` verifies (§4.5),
   `epoch == prior_epoch + 1`, and `manifest_hash` names a manifest the
   verifier can independently confirm (§11.5). A valid recovery_epoch
   **itself enrolls the replacement device**: from its `seq` onward,
   `device_id` with exactly the entry's `sign_pub`/`agree_pub`/
   `platform`/`device_name` is an enrolled, non-revoked device and may
   authorize subsequent entries. There is no separate epoch-start enroll;
   genesis remains the only self-signed entry, and no self-signed
   `enroll` exists in v2 at all. Because every security-relevant identity
   field of the replacement device is bound by the proof, key or metadata
   substitution is impossible: any alteration invalidates the proof
   (RG-13…RG-15). No replacement device key becomes trusted merely by
   reusing the authorized `device_id`.
   **Total-loss recovery revokes every prior device (v0.4, owner decision
   S-4).** A `recovery_epoch` committed by a total-loss `finalize` (§11.8)
   is followed, in the same state transition, by one ordinary `revoke`
   entry for every device that was active immediately before the epoch,
   in ascending `device_id` order, each with `authorizer` = the device the
   epoch installed and signed by its `sign_pub`. These are normal named
   revokes, so rules 3 and 5 and the §4.7 revocation discipline apply
   unchanged. After the transition the only active device is the
   recovering one. A user who wants to keep an existing trusted device is
   not doing total-loss recovery and must use a trusted-device flow
   (§12 scenarios 1, 2, 5–7).
7. Exactly one tip: two distinct entries with the same `prev_hash` → fork
   (§4.6).
8. All public key fields are exactly 65-byte uncompressed points that lie
   on P-256; any other length or off-curve point invalidates the entry.

### 4.5 Recovery-epoch authorization

When no trusted device survives, the recovering device proves possession of
the vault itself (v0.3 §10.3: standard primitives, no Source escrow, MP-only
must work). The proof is an HMAC over the entry's TLV encoding, which binds
exactly these fields: `vault_id`, `prev_hash` (the prior registry head),
`manifest_hash` (the prior/current manifest the transition builds on),
`prior_epoch`, `epoch` (new), the new device's `device_id`, `sign_pub`,
`agree_pub`, and a fresh `recovery_nonce` (16 B OsRng, making each
transition instance unique and copied proofs unreplayable):

```text
recovery_key  = HKDF-SHA256(ikm=VK, salt=manifest_hash,
                            info="ov0/recovery-auth/v1")
recovery_proof = HMAC-SHA256(recovery_key,
                  "ov0/registry/recovery/v1" || tlv(entry without field 0x0E))
```

- Only a party that unwrapped the VK protecting the bound manifest (via MP,
  RK, or a surviving device) can produce a valid proof for it. (`VK` in the
  derivation above is that recovered VK.)
- Altering any bound field — including the new device's public keys —
  invalidates the proof (test RG-08).
- (v0.4) In a total-loss finalize the valid entry is immediately followed
  by named revokes of every prior active device, signed by the device it
  installs (§4.4 rule 6, S-4); the proof construction is unchanged.
- The valid entry itself installs the new device (rule 6). The recovering
  device has already generated a fresh VK and re-encrypted the vault
  before finalize (§12 scenario 3/4), so the manifest that finalize
  installs is sealed under the new `vk_generation`: the proof key (the
  old VK) is retired at the moment the epoch commits, with no window in
  which the new epoch runs on the old VK.
- An attacker holding an *old* VK/RK can forge an epoch bound only to the
  *old* `manifest_hash`; devices that have seen a newer manifest reject it
  (§4.6 rollback), and the recovery UI shows the bound manifest generation
  so a stale-epoch fork is visible to the user. The freshness limit this
  cannot fix on a fully fresh device is stated honestly in §11.7.

### 4.6 Fork, truncation, rollback

- **Fork:** two tips sharing `prev_hash`, or a downloaded registry whose
  chain diverges from the local one at any seq ≤ local head. Behavior:
  helper enters `COMPROMISED`-class error state (`REGISTRY_FORK`), refuses
  writes, surfaces both tips to the user (device names, dates, seqs).
  Never auto-resolve.
- **Truncation:** chain gap, bad `prev_hash`, or a presented head with
  seq < the seq this device has already accepted → reject, `REGISTRY_TRUNCATED`.
- **Rollback:** signed manifest with `generation` lower than the helper's
  persisted last-seen generation (Keychain state item, §2.8) → reject,
  `MANIFEST_ROLLBACK`. Devices persist every accepted head before applying
  state derived from it.
- **Manifest fork evidence (v0.4):** a manifest whose `prev_manifest_hash`
  is not the hash this device accepted at the previous generation is fork
  evidence, even though the provider's state CAS normally makes the chain
  linear (a malicious provider can serve different chains to different
  devices).
- **Local rollback:** a device whose local vault generation is below its
  Keychain last-seen generation refuses to author revisions until it has
  synced to at least that generation (§3.2).
- **COMPROMISED (v0.4):** entered on a registry fork, registry
  equivocation or confirmed vault-level tamper; writes are frozen and both
  tips are surfaced. The state persists across lock and restart. **How a
  user exits COMPROMISED is not specified by v0.4** (a pre-existing gap);
  implementations provide entry and surfacing only and must not invent a
  resolution procedure.
- **Conflict handling summary:** any ambiguity → stop, surface, let the
  user choose. The backup provider cannot authorise a device under any
  rule in this section: it holds no enrolled signing key and no VK.

### 4.7 Registry replication

The registry is public verification state: it is uploaded with every
backup manifest (§11) and exchanged during peer sync (Phase F.2). Its confidentiality
requirement is nil; its integrity requirement is total.

**Revocation-status refresh (normative, v0.3.1 Phase E, owner decision
2026-09-21).** Revocation is a local registry write on the authorizing
device, and the §5 enrollment channel is gone by the time it happens, so
an enrolled device has no way to learn it was revoked. It therefore
*asks*:

- The Mac serves the signed registry (and `vault_id`) read-only to an
  already-paired device, over the existing authenticated, certificate-
  pinned channel: `GET /v1/vault/registry`, backed by `registry_status`
  (§1.5). No vault records, no VK, no wraps, no recovery material, no
  backup credentials, no write operations, and no general sync.
- The asking device verifies the chain itself under §4.4 before changing
  any local state, and applies the §4.6 rollback rule against what it has
  already accepted: a registry with less history than it holds, or with
  different history at a seq it has accepted, is rejected as tampering.
- **Only a cryptographically valid `revoke` entry naming that exact
  device** may clear its local enrollment state, envelope and SE key
  references. An unreachable authorizer, a failed verification, or the
  device's own absence from an otherwise valid chain must never be read
  as revocation and must never delete key material.
- Event-driven: app launch/foreground, opening a screen that reports
  vault standing, reconnection, and manual refresh. **No background
  polling.**
- It is a *status refresh*, not a notification: delivery is not
  guaranteed and the authorizer never pushes. A device that never asks
  goes on holding a key that decrypts nothing, because revocation
  rotated the VK (§11.4).
- **The authorizer remains authoritative.** Nothing a device says alters
  the registry; in particular no device can cause a VK rotation, or
  invalidate a Recovery Key, remotely.

Devices that hold enrollment material must not present it as current
trust. A UI states what it has verified and when, distinguishing
"verified recently", "not recently verified", "unable to verify" and
"revoked".

**Provider responses are not revocation (v0.4).** A `401`/`403` from the
backup provider, an unreachable provider, or a missing envelope in a
published state means "unable to verify" — never revocation, and never a
reason to delete key material. Revoked keys are refused by the provider
before any operation, so a revoked device learns its status through the
refresh above or by re-enrolling, not through the provider.

**iPhone envelope catch-up from the provider (normative, v0.4 Phase F).**
An enrolled iPhone that was offline during a VK rotation obtains its new
envelope from the backup provider (not from a Mac route), at the same
event-driven refresh points (no background polling):

1. `state_get`, signed by the phone's SE signing key as a device-class
   `ProviderRequest` (§11.4). `401`/`403` → "unable to verify".
2. Decode manifest v2; require generation ≥ the phone's persisted manifest
   floor (equal generation with a different `manifest_hash` is fork
   evidence → "unable to verify").
3. Fetch the index (SHA-256 = `object_index_hash`), then the registry blob
   and its own `env` blob (SHA-256 per index lines).
4. HPKE-open the v2 envelope in the Secure Enclave; require its
   `vk_generation` = `manifest.vk_generation`.
5. Verify the checkpoint MAC under that VK **and** its binding to this
   manifest (`manifest_core_hash`, which the phone computes itself;
   generation; `vk_generation`) and to the served registry head (§4.8
   order: envelope → checkpoint → registry).
6. Verify the registry: the phone's accepted chain must be an exact prefix
   (by entry hash) of the served one; the new suffix entries are checked
   under §4.4 rules 1–5, 7, 8, with `recovery_epoch` entries checked
   structurally as in §4.8 (the checkpoint from step 5 anchors this head).
7. Verify the manifest signature under a signer active in that registry.
8. Atomically replace the stored envelope, manifest and checkpoint and
   raise both floors.

**On this provider path a `revoke` naming this phone is never acted on
destructively.** A provider (possibly colluding with a stolen, revoked
device) could serve a forked registry the phone cannot distinguish; an
honest provider never serves one, because a revoked key's `state_get` is
refused. The phone shows "unable to verify — possible tamper" and stops
using its enrollment; clearing enrollment state, envelopes or SE key
references happens **only** through the §4.7 Mac refresh above.

**Existing refresh must accept recovered chains (current-state defect,
fixed in Phase F).** The Phase E.1 phone refresh verifies the registry with
a policy that rejects any `recovery_epoch` entry, so a phone enrolled into
a recovered vault reports "unable to verify" on every refresh. In Phase F
the §4.7 Mac refresh verifies the suffix after the phone's accepted head
(prefix match by entry hash; rules 1–5, 7, 8 on the new entries;
`recovery_epoch` entries structurally), which is sound because the Mac is
the authority on that pinned channel.

The phone keeps no record store and performs no merge or publication in
Phase F; full iPhone record sync is Phase F.2 (§18).

---

### 4.8 Registry checkpoint (normative, v0.3.1 Phase D.1)

**Second consumer: enrollment (normative, Phase E.1).** A device joining
a vault that has been through total-loss recovery faces the same problem
as a recovering device, for the same reason: the registry contains
`recovery_epoch` entries whose §4.5 proofs are keyed by VKs retired at
the moment each epoch committed, so no newly enrolled device can ever
verify one. It resolves it the same way, and the **order of the §5.2
bundle checks is therefore normative**:

1. open the device envelope with the SE agreement key → current VK;
2. verify the §4.8 checkpoint under that VK;
3. verify the registry chain **anchored** on the checkpoint's head —
   §4.4 rules 1–5, 7 and 8 in full, signed entries still verifying under
   their authorizer, and `recovery_epoch` entries checked structurally
   (no authorizer, no signature, `epoch == prior_epoch + 1`, all §4.3
   recovery fields present, keys on-curve) rather than by a proof whose
   key is extinct;
4. confirm the chain head is the one the checkpoint and the bundle both
   name, and that the entry installing this device carries exactly the
   public keys this device generated.

No extinct VK is transmitted, used or stored at any point. Every entry
also declares the epoch it belongs to, and only a `recovery_epoch` may
advance it; an entry claiming any other epoch is rejected.

A `recovery_epoch` proof is keyed by the VK that protected the manifest it
binds (§4.5). That VK is retired by the rotation the same recovery
performs, so a *later* fresh device — which only ever learns the current
VK from MP or RK — cannot verify historical epochs. Devices must not keep
old VKs or old proof keys to work around this, and unverifiable epochs
must never be accepted on trust. The current-VK **registry checkpoint**
closes the gap:

```text
checkpoint_key = HKDF-SHA256(ikm=current_VK, salt=vault_id,
                             info="ov0/registry-checkpoint/v1")
registry_checkpoint = HMAC-SHA256(checkpoint_key,
    "ov0/registry-checkpoint/v1" ‖ tlv(checkpoint_body))
```

`checkpoint_body` is canonical TLV (§4.2 rules) with exactly these
fields, ascending:

| Tag | Field | Type |
|---|---|---|
| 0x01 | `version` | u32 = 1 |
| 0x02 | `vault_id` | 16 B |
| 0x03 | `epoch` | u64 — current registry epoch |
| 0x04 | `registry_head` | 32 B — `entry_hash` of the current tip |
| 0x05 | `manifest_core_hash` | 32 B (below) |
| 0x06 | `manifest_generation` | u64 |
| 0x07 | `vk_generation` | u32 |

The stored object appends tag 0x08 `mac` (32 B) to the same entry.

```text
manifest_core_hash = SHA-256("ov0/manifest/core/v1" ‖ tlv(SignedManifest
                              without field 0x10 signature))
```

**No recursion:** the manifest never contains the checkpoint, the object
index never lists it (§11.2), and `manifest_core_hash` excludes the
manifest signature — so nothing the checkpoint commits to is computed
over the checkpoint.

**Regeneration (normative).** The checkpoint is rebuilt, under the VK
then current, whenever any bound field changes: a recovery-epoch
transition, any registry mutation (enroll/revoke), a VK rotation, and any
manifest change covered by `manifest_core_hash` (which includes every
publication, since `generation` is bound). A publication or
recovery-finalize that does not carry a checkpoint describing exactly the
state being installed is refused by the provider (structurally — the
provider holds no VK and cannot verify the MAC).

**Fresh-device recovery (normative order).** A device with no prior state:

1. recovers the current VK through MP or RK (§12 scenarios 3/4);
2. verifies the served checkpoint's MAC under that VK;
3. requires its `registry_head` to equal the head of the downloaded
   registry exactly, and its `manifest_core_hash`, `manifest_generation`,
   `vk_generation`, `vault_id`, and `epoch` to equal the served
   manifest's — any mismatch → `MANIFEST_MISMATCH`;
4. only then treats that registry as the authorization anchor;
5. verifies ordinary device signatures and structural/hash-chain
   integrity as usual (§4.4 rules 1–5, 7, 8), and the manifest signature
   under a non-revoked device the chain installs.

Historical `recovery_epoch` entries remain **audit evidence**: a device
that holds the relevant transition state verifies their proofs when they
are created and whenever it holds that VK (§4.5); a later fresh device
must not need an extinct VK once the current checkpoint validates.

**Threat notes.** The checkpoint is a MAC, not a signature: only a holder
of the current VK can produce one, which is exactly the party the
recovering user has just proven to be. A provider that substitutes a
different registry (even with a manifest it signs with a device of its
own) cannot produce the matching checkpoint, and the served one will not
bind the substituted head. Altering historical epoch bytes changes the
registry object hash (caught by the index) and the head (caught by the
checkpoint). Rolling the whole account back to an older *complete,
internally consistent* state remains undetectable on a fresh device — the
§11.7 limitation is unchanged, and the printed recovery sheet remains the
user-held comparison. An attacker who holds the current VK already holds
the vault; the checkpoint adds no new exposure.

## 5. Mac ↔ iPhone enrollment protocol

Reuses the proven Source pairing UX (`core/mobile/qr.rs`,
`pair_requests.rs`, `tls.rs`) but produces asymmetric device identity
(v0.3 §8), not a bearer token. The helper does all cryptography; the main
process runs a **dedicated, ephemeral** enrollment server (fresh rcgen
cert, port 0, bound only for the session) so the always-on mobile server
is not enrollment attack surface. The helper never touches the network:
main relays frames between the socket and the TLS connection as opaque
bytes tagged by the framing below.

### 5.1 Sequence

```mermaid
sequenceDiagram
    participant U as User
    participant MA as Mac main app
    participant H as Mac vault helper
    participant IP as iPhone (new device)

    U->>MA: Settings → Security → Add Device
    MA->>H: begin_enrollment
    H->>H: enroll_secret = OsRng(16), nonce_e = OsRng(16), TTL 300 s, single-use
    H-->>MA: QR payload {v, host, port, fp, secret_b32, mac_device_id}
    MA->>MA: spawn ephemeral TLS server (rcgen cert, port 0)
    MA-->>U: show QR
    U->>IP: scan QR with Source iOS app
    IP->>IP: generate SE signing + agreement keypairs (non-exportable)
    IP->>MA: TLS connect, pin SHA-256(cert DER) == fp
    MA->>H: relay ENROLL_HELLO
    IP-->>H: {secret, nonce_n, sign_pub, agree_pub, name, platform, proto}
    H->>H: verify secret (constant-time, single-use, TTL)
    H->>H: transcript = SHA-256(fp‖secret‖nonce_e‖nonce_n‖device_ids‖pubkeys)
    H-->>U (via MA): SAS = sas8(transcript) shown on Mac
    IP-->>U: same SAS shown on iPhone
    U->>U: compare; confirm on both devices
    U->>MA: confirm → Mac LA presence (Touch ID)
    MA->>H: enroll_confirm
    H->>H: LA presence → sign enroll entry (Mac sign key)
    H->>H: envelope = HPKE-Seal(iPhone agree_pub, DeviceEnvelopePayload v2{VK})
    H-->>IP: relay {registry to head, envelope, manifest, checkpoint, blobs} (streamed via §1.3)
    IP->>IP: decapsulate envelope (SE agree key) → verify checkpoint → verify registry chain + manifest (§4.8 order)
    IP->>IP: store envelope, manifest, checkpoint; Keychain ThisDeviceOnly (no record store in Phase F)
    IP-->>H: ENROLL_ACK = iPhone signature over registry head hash
    H->>H: verify ACK → entry marked confirmed
    MA->>MA: teardown ephemeral server
```

### 5.2 Message details

| Field / message | Spec |
|---|---|
| QR payload | JSON `{v:2, host, port, fp, secret (base32), mac_device_id, name}` — same shape family as existing `EnrollmentPayload`, `v:2` distinguishes vault enrollment; rendered with existing `render_enrollment_qr` |
| TLS | rustls server (main), iOS URLSession/Network.framework with SPKI-pin = QR `fp`; hostname ignored (consistent with decision D6) |
| `enroll_secret` | 16 bytes, base32-no-pad in QR; verified with `subtle::ConstantTimeEq`; single-use; TTL 300 s from `begin_enrollment`; failure count ≥ 5 on a session → session torn down |
| Nonces | both sides 16 B random; both enter the transcript |
| Transcript | `SHA-256("ov0/enroll/transcript/v1" ‖ fp_bytes ‖ secret ‖ nonce_e ‖ nonce_n ‖ mac_device_id ‖ new_device_id ‖ sign_pub ‖ agree_pub)` — binds everything the user is about to approve |
| SAS | 8 chars, alphabet `23456789ABCDEFGHJKLMNPQRSTUVWXYZ` (repo's existing unambiguous alphabet), from `HKDF-SHA256(transcript, salt=nil, info="ov0/enroll/sas/v1")` → 5 bytes → 40 bits → 8×5-bit indices. Longer than the legacy 4-char pair tag on purpose. |
| SAS confirmation | explicit tap on **both** devices; either-side abort → session torn down, secret burned |
| Key exchange | iPhone sends only public keys (65-byte uncompressed, §2.7); private keys never leave SE |
| `device_id` assignment (v0.3.1 Phase E) | the **Mac** assigns the new device's id and returns it in the hello reply, together with `nonce_e` and `mac_device_id`; a new device cannot choose its own registry identity or collide with an enrolled one. The phone derives the SAS from that reply — the SAS itself is never transmitted. |
| Transport shape (v0.3.1 Phase E) | three routes on the ephemeral server: `POST /v1/vault/enroll/hello`, `GET /v1/vault/enroll/bundle` (the phone waits here while the user compares the SAS and confirms on the Mac), `POST /v1/vault/enroll/ack`. Per-message timeout 30 s; the bundle wait is bounded by the 300 s session. |
| Bundle contents (v0.4) | exactly a §11.2 v2 state — blobs, signed manifest v2 and §4.8 checkpoint — plus this device's envelope and the provider origin. The JSON `objects` entries are `[role, logical_id, sha256, data]`. The helper hands the bundle to main as a §1.3 stream session; main assembles the phone's response |
| Envelope | HPKE base (§2.7/§2.9), plaintext = `DeviceEnvelopePayload` v2 (VK only), `info = "ov0/envelope/v2" ‖ …` (§2.5) |
| Provider activation (v0.4; replaces "backup credential registration") | after ENROLL ACK the authorizer publishes a `publish` state transition whose registry contains the new `enroll` entry; the provider activates the new device's `sign_pub` when that transition commits (§11.4). No credential is registered. Tracked as `REMOTE_UPDATE_PENDING` until then (§11.3) |
| Initial vault transfer | registry to head + current record objects (§3.7) + wraps; all ciphertext; sent only after SAS + LA confirm. In Phase F the phone stores only its envelope, manifest and checkpoint; it keeps no record store (§4.7) |
| ENROLL_ACK | iPhone signs `SHA-256("ov0/enroll/ack/v1" ‖ registry_head_hash ‖ mac_device_id)` with its SE signing key; proves the private key exists in that device |
| Replay prevention | single-use secret + fresh nonces + transcript binding + ACK binds head hash; a captured session replays to nothing |
| Timeout | whole flow 300 s; per-message 30 s; expiry → teardown + fresh secret on retry |
| Cancellation | either side may cancel at any step; nothing is persisted on the new device before envelope decapsulation succeeds; the Mac registry entry is written only after ACK — a cancelled attempt leaves no registry trace |

### 5.3 Failure states and error UX

| Failure | Behavior | User sees |
|---|---|---|
| QR expired | teardown | "Code expired — generate a new one" |
| Wrong/guessed secret | close after 5 tries | "Pairing failed — rescan the code" |
| TLS fingerprint mismatch | abort before any frame | "Secure channel could not be established" |
| SAS mismatch | both sides warn, teardown | "Codes don't match — do not pair" (security UX, no retry button on same secret) |
| Network drop mid-transfer | resume unsupported → restart with fresh secret | "Connection lost — start over" |
| ACK invalid/never arrives | registry entry not written | "iPhone didn't confirm — try again" |
| iPhone OS < required version | polite refusal before keygen | "Update iOS to continue" (floor per §21) |

### 5.4 First device

Vault creation on the first Mac: helper generates VK, vault_id, writes
genesis registry entry (self-signed), creates MP wrap (user sets MP),
RK wrap, first manifest. The RK is shown in the §1.7 window and the
vault is committed **only** once the user acknowledges it; a dismissed
window removes everything created (no vault may exist whose Recovery Key
was never shown). The iPhone then enrolls via
§5.1 with the Mac as authorizer.

---

## 6. Routine credential authorization

Three supported local authorization paths (v0.3 §6.4, C17). All three end
in the same helper-side decision point: `authorize(action, origin, ref)`.

### 6.A Mac open + Touch ID available

```text
Chrome login page
 → extension: fill_candidates(origin, tab_url)
 → helper: origin policy check (§9.5) → matching accounts
 → extension popup: user picks "Sign in as alice@…"
 → helper: LA evaluatePolicy(.deviceOwnerAuthentication,
                             "Source Vault: fill <origin>")  → Touch ID sheet
 → success → helper decrypts exactly that record
 → helper → nm-host → extension fills the approved frame
 → fill authorization consumed (one-shot)
```

### 6.B Clamshell + enrolled iPhone reachable

```text
… same until account picked …
 → extension popup offers "Approve on iPhone"
 → helper: build ApprovalChallenge (§6.5), state → AUTHORIZING
 → helper → main: event approval_requested (opaque TLV blob)
 → main → iPhone: direct pinned channel if reachable, else APNs wake (§7)
 → iPhone UI: shows Mac name, origin, action, account title
 → Face ID / device passcode on iPhone
 → iPhone signs challenge with its SE signing key → returns signature
 → main → helper: approval_result
 → helper verifies (§6.6) → releases one credential → consumed
```

### 6.C Clamshell + iPhone unavailable

```text
… same until account picked …
 → helper: LA evaluatePolicy(.deviceOwnerAuthentication, …)
   (same policy as 6.A; with biometrics unavailable, macOS presents the
    native login-password sheet — no separate password UI is built)
 → success → release one credential → consumed
```

The Source master password is **not** offered in the fill UI. It appears
only inside helper-owned secure panels (§1.7): setup, recovery, and the
panel-based unlock path used when Keychain/SE state is lost
(`begin_recovery_unlock`).

### 6.4 User-presence policy resolution

Helper-side order for a fill (API model corrected, v0.2 finding 11):

1. Caller asked `method:"iphone"` → §6.B (with fallback offer on timeout).
2. Default: one LA call, `LAPolicy.deviceOwnerAuthentication`. Apple's
   behavior for this policy is exactly the intended UX: on a Touch ID Mac
   it presents the biometric sheet (with a password fallback button); on
   a clamshell / biometry-unavailable Mac it presents the native macOS
   login-password sheet directly. Paths 6.A and 6.C are literally the
   same code path; the OS picks the UI.
3. `LAPolicy.deviceOwnerAuthenticationWithBiometrics` is **not** used for
   fills: with biometrics unavailable it fails with
   `biometryNotAvailable` instead of offering a password path, which
   would silently kill the clamshell flow.
4. No custom Mac-password prompt is built anywhere — password entry for
   path C is rendered by macOS inside the LA sheet.
5. iOS uses the same single policy: Face ID with passcode fallback.
6. Card fills always force a fresh evaluation (no grace), action=`fill_card`.

### 6.5 Signed iPhone approval payload

Canonical TLV (§4.2), tags ascending:

| Tag | Field | Type | Notes |
|---|---|---|---|
| 0x01 | `proto` | u32 = 1 | |
| 0x02 | `request_id` | 16 B uuid | matches helper's pending request |
| 0x03 | `mac_device_id` | 16 B | must equal this Mac |
| 0x04 | `iphone_device_id` | 16 B | must be enrolled, not revoked |
| 0x05 | `origin` | string ≤ 253 | canonical origin (§9.4) |
| 0x06 | `action` | u8: 1 fill_password, 2 fill_card, 3 reveal, 4 unlock_vault, 5 update_password | |
| 0x07 | `credential_ref` | 16 B uuid or absent | required for actions 1, 2, 3, 5 (everything that acts on an existing credential); absent only for action 4 (unlock_vault) |
| 0x08 | `nonce` | 16 B | helper-generated, single-use |
| 0x09 | `iat` | u64 | issued-at |
| 0x0A | `exp` | u64 | iat + 120 s |
| 0x10 | `signature` | 64 B r‖s | over `SHA-256("ov0/approval/sign/v1" ‖ tlv(without 0x10))`, iPhone SE signing key |

### 6.6 Approval verification and edge behavior

Helper checks, in order, all must pass:
signature valid for `iphone_device_id` (registry, not revoked) →
`mac_device_id` == self → `request_id` pending → `origin`/`action`/`ref`
equal the pending request → `nonce` unconsumed → `now ≤ exp` → mark nonce
+ request consumed → release.

| Case | Result |
|---|---|
| Replayed approval | nonce consumed → reject `APPROVAL_REPLAY` |
| Duplicate delivery while pending | idempotent: same decision returned; no double release (fill response is generated once and cached against request_id until consumed) |
| Expired | reject `APPROVAL_EXPIRED`; extension shows "request expired — retry" |
| Wrong origin/ref/action | reject `APPROVAL_MISMATCH` — this is the phishing tripwire: phone UI showed exactly what it signed |
| Wrong Mac | reject `APPROVAL_MISMATCH` |
| Unknown/revoked iPhone | reject `DEVICE_NOT_AUTHORIZED` |
| User cancels on phone | phone returns unsigned `{request_id, cancelled:true}` (no signature needed to cancel) → helper cancels request |
| Phone unreachable / timeout (120 s) | helper cancels; extension offers §6.C fallback |
| Offline (no LAN, no push path) | same as unreachable → §6.C |

The phone signs an authorization statement only. Passwords, usernames,
VK, RK, MP, device private keys, and biometric data never appear in the
challenge, the approval, or any relay (v0.3 §15.2).

---

## 7. iOS / APNs approval architecture

### 7.1 Foreground path

Source iOS app open (or opened by tapping the alert) → connects to the Mac
over the existing pinned TLS channel (`core/mobile` server) → vault scope
authenticated by device challenge-response, not legacy bearer tokens:

```text
Phone → Mac: AUTH_HELLO {device_id, client_nonce}
Mac main relays to helper → helper: {server_nonce, registry seq it knows}
Phone signs SHA-256("ov0/session/v1" ‖ nonces ‖ device_id ‖ registry_seq)
Helper verifies against registry → session scoped to this device
```

Over that session the phone fetches pending approval challenges
(`GET /v1/vault/approvals/pending` semantics over the relay) and returns
signatures. All frames are opaque TLV blobs relayed by main; the mobile
server learns message sizes and timing only.

### 7.2 Background path (APNs)

- **Provider role:** the §11 backup service hosts a minimal push-relay
  endpoint (Phase G; the `/v2/push/*` module is reserved in v0.4) holding the APNs provider token (.p8). It is
  the only Source-infrastructure involvement.
- **Wake trigger:** main app (not helper — helper has no network) asks the
  provider to push. Provider authenticates the caller as a device of the
  vault via the §11.4 request signature (v0.4: a device-class
  `ProviderRequest`; operation codes 32–47 are reserved for push). **Phase
  F implements no push relay and no APNs code**; the provider only reserves
  the module and route boundary. Phase G owns the relay and APNs behavior.
- **Push payload (entire):**

  ```json
  {"aps": {"alert": {"title": "Source Vault",
                     "body": "Approval request — open Source to review"},
           "sound": "default"},
   "rid": "<uuid>"}
  ```

- **What Apple/APNs sees:** an opaque UUID, generic text, timing.
- **What the Source provider sees:** push token, opaque UUID, timing,
  vault_id (needed for routing).
- **What neither ever sees:** origin, action, credential ref, usernames,
  passwords, card data, VK/RK/MP, device private keys, biometric data.
  (v0.3 §15.2 clarification, restated as a hard requirement.)
- **No PushKit/VoIP** (App Store policy), **no silent pushes** (too
  unreliable for this UX) — alert pushes only, `apns-expiration` = 120 s
  aligned with challenge expiry.
- **App wake behavior:** user taps → app foregrounds → §7.1 direct fetch
  of the actual challenge. The push carries no actionable data, so a
  spoofed push can only open the app, never authorize anything.
- **Offline / unreachable fallback:** if the direct fetch fails (phone on
  cellular, Mac behind NAT), the request fails and the extension offers
  the §6.C macOS authentication fallback. v1 does not relay challenge
  contents through the provider; §21 keeps that as a possible future
  owner decision. UX
  copy must not promise phone approval when the phone is unreachable
  (v0.3 §15.2).

---

## 8. Credential record types

Record plaintext is a JSON object (serde, `deny_unknown_fields` off for
forward compatibility, `schema` tag discriminates). Encrypted per §2.6.

### 8.1 Website Login (`"login"`, schema 1)

```json
{
  "schema": "login@1",
  "title": "GitHub",
  "username": "alice@example.com",
  "password": "…",
  "urls": [
    {"host": "github.com", "match": "exact"},
    {"host": "www.github.com", "match": "exact"}
  ],
  "notes": "…",
  "created_at": 1789650000,
  "updated_at": 1789650000,
  "password_history": [
    {"password": "…", "changed_at": 1789650000}
  ]
}
```

| Field | Rules |
|---|---|
| `title` | required, ≤ 200 chars, NFC |
| `username` | ≤ 320 chars; may be empty (rare SSO cases) |
| `password` | ≤ 1024 chars; required for login kind |
| `urls[]` | 1–20 entries; `host` is the canonical registrable-or-exact host (§9.4), never a full URL with path/query; `match`: `exact` (default) or `domain` (registrable domain + all subdomains) |
| `notes` | ≤ 10 000 chars; inside `ct`, never in metadata |
| `password_history` | max 10 entries, oldest evicted; sealed inside `ct`; never returned by list ops |
| `created_at`/`updated_at` | duplicated outside as plaintext columns (§3.4) — values inside `ct` are authoritative for display |

**Domain matching policy (default-deny):** a fill candidate requires
`origin.host == urls[i].host` under `exact`, or
`registrable_domain(origin.host) == urls[i].host` under `domain`.
`domain` is opt-in per URL entry via the management UI with an explicit
confirmation explaining the risk. Scheme must be `https` unless the entry
carries `"allow_http": true` (imported only for `localhost`/RFC-1918
targets; settable only by direct record edit, never by the extension).

### 8.2 Payment / Debit Card (`"card"`, schema 1)

```json
{
  "schema": "card@1",
  "label": "Personal Visa",
  "number": "…",
  "expiry": "09/28",
  "cardholder": "A EXAMPLE",
  "billing_address": {"line1":"…","line2":"…","city":"…",
                      "region":"…","postal":"…","country":"…"},
  "notes": "…",
  "created_at": 0, "updated_at": 0
}
```

- **`cvv` is not in the schema.** The importer discards CVV columns; the
  add/update IPC ops reject unknown secret-class fields; there is no code
  path that stores one (v0.3 C20).
- Validation: Luhn check (UX warning, not a hard error — some valid cards
  fail Luhn-adjacent schemes; warn and allow with confirmation).
- Every fill requires fresh user presence with `action: fill_card`
  (§6.4.3); the approval dialog text says "card".

### 8.3 Explicit v1 exclusions

Not implemented in v1 (v0.3): passkeys, TOTP (Dashlane `otpSecret` is
import-reported as unsupported, not stored), shared vaults, teams,
enterprise admin, Source ID recovery, arbitrary API secrets, SSH keys,
developer secret management. The schema tag (`login@1`, `card@1`) and the
TLV/action registries leave numeric headroom so future types don't fork
the wire format — that is the only concession to them.

---

## 9. Chrome autofill extension protocol

Chrome/Chromium on macOS is v1 (v0.3 C19). Three components: MV3
extension, `com.racker.zero.nm-host` binary (spawned by Chrome per
browser session, connects to the helper socket as client class
`nm-host`), and the helper (all decisions).

### 9.1 Extension architecture and permissions

- Manifest V3. Permissions: `nativeMessaging`, `tabs`, `storage`. No
  `cookies`, no `<all_urls>` host permission — content scripts declare
  `matches: ["https://*/*", "http://*/*"]` (required for form access) but
  run at `document_idle` and do nothing until the background worker
  confirms vault availability.
- No remote code, no remote config, no telemetry. The extension's CSP is
  MV3 default (no remote scripts).
- Distribution: development via unpacked extension with a **pinned key**
  in `manifest.json` so the extension ID is stable across dev machines;
  production via Chrome Web Store (stable ID). The native host manifest
  pins the ID either way (§9.2).

### 9.2 Native messaging host

- Manifest at
  `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/com.racker.zero.vault.json`
  (+ Chromium/Edge/Arc/Brave paths), written by the main app at setup:
  ```json
  {"name": "com.racker.zero.vault",
   "description": "Source Vault native messaging host",
   "path": "/Applications/SOURCE.app/Contents/Library/vault-nm-host/source-vault-nm-host",
   "type": "stdio",
   "allowed_origins": ["chrome-extension://<pinned-id>/"]}
  ```
- **Authentication layers (strengthened, v0.2 finding 4).** The v0.1
  per-session `session_id` bound messages to a browser session but did
  not prove *Chrome* launched the broker — a same-user process could exec
  the signed host itself and drive it over its own pipes. The corrected
  stack:
  1. **Chrome `allowed_origins`** (above) — the browser only connects the
     pinned extension ID.
  2. **Caller-origin argument check:** Chrome passes the calling
     extension's origin as the host's first argv element. nm-host
     requires argv to contain exactly
     `chrome-extension://<production-id>/` (release build) or the
     explicitly compiled-in dev ID (debug builds only, set at build time,
     never accepted in release); missing/malformed/unexpected origin →
     exit nonzero before reading stdin. Necessary, not sufficient (a
     manual exec can forge argv).
  3. **Parent-process code-identity check:** nm-host verifies its parent
     process via pid → `SecCodeCopyGuestWithAttributes` →
     `SecCodeCheckValidityWithErrors` against a per-browser designated
     requirement (Chrome: `anchor apple generic and identifier
     "com.google.Chrome" and certificate leaf[subject.OU] = "EQHXZ8M8AV"`;
     other Chromium variants get their own reviewed requirement rows or
     are unsupported). A shell, script, or malware exec'ing the host has
     itself as parent and fails. **This check is specified but marked
     UNVERIFIED until the Phase H prototype confirms Chrome's actual
     spawn behavior on macOS (which process is the parent, whether the
     pid→SecCode mapping is race-free enough here); the spec's
     security claims below do not rely on it until the prototype reports
     (NM-05/NM-06).** If it proves unreliable, the honest residual
     boundary is layers 1+2+4 and the documented local-attacker
     limitation (§9.9).
  4. **Helper peer authentication** (§1.4) — the helper only serves
     peers signed as `com.racker.zero.nm-host`.
  5. The v0.1 `session_id` handshake remains as session hygiene, not as
     an authentication factor.
- Combined guarantee claimed: only the pinned extension, connected through
  a Chrome-controlled stdio channel, to the signed broker, to the
  authenticated helper. Not claimed: protection against a same-user
  attacker who executes the signed broker manually and speaks the
  protocol — that attacker reaches exactly the same IPC surface as the
  extension (per-origin, one-shot, presence-gated), which is why every
  release still requires user presence showing the claimed origin
  (§9.9); layer 3 narrows this further once verified.
- nm-host is a dumb broker: it validates frame shape (schema, lengths)
  and forwards to the helper. It holds no keys, performs no matching,
  never logs field values. It exits when stdin closes.

### 9.3 Message schemas (extension ↔ nm-host, JSON, ≤ 32 KiB)

```text
→ {"op":"hello","proto":1,"session_echo":"<session_id>"}
← {"ok":true,"vault_state":"unlocked"|"locked"|"uninitialized"}

→ {"op":"get_candidates","origin":"https://github.com","tab_url":"https://github.com/login"}
← {"ok":true,"request_id":"uuid","accounts":[{"ref":"uuid","title":"GitHub",
   "username":"alice@example.com"}]}
   or {"ok":true,"locked":true}

→ {"op":"authorize","request_id":"uuid","ref":"uuid","method":"local"}
→ {"op":"authorize","request_id":"uuid","ref":"uuid","method":"iphone"}
← {"ok":true,"fill":{"username":"…","password":"…"},"expires_in":30}
   or {"ok":false,"error":"USER_CANCELLED"|"APPROVAL_EXPIRED"|…}

→ {"op":"save_new","origin":"…","tab_url":"…","username":"…","password":"…","title":"…"}
← {"ok":true,"ref":"uuid"}

→ {"op":"save_update","ref":"uuid","password":"…"}
← {"ok":true}

→ {"op":"request_unlock"}      -- triggers helper LA prompt via app UI
← {"ok":true,"vault_state":"unlocked"}
```

### 9.4 Origin retrieval and canonicalization

- The **browser-reported origin** comes from the content script's
  `window.location.origin`; the **tab URL** comes from `chrome.tabs`
  (`tab.url`, requires `tabs` permission). Both travel in the request;
  the helper recomputes canonical origins from both and requires
  agreement (test BR-11). A compromised extension can lie about
  both — the mitigation is that the user-presence prompt and iPhone UI
  **display the claimed origin**, so a lying extension must show the user
  an origin they didn't intend (§9.9 assumptions).
- Canonicalization (identical implementation in helper (Rust) and test
  vectors in JS/Swift): IDN → UTS-46 punycode (`xn--` form); lowercase;
  strip trailing dot; strip default ports (443/80); scheme must be
  `http`/`https`; no userinfo; no trailing path. Result form:
  `scheme://host[:port]`.

### 9.5 Matching policy (helper-side, fail closed)

| Input | Decision |
|---|---|
| exact `scheme://host` match with `match:"exact"` | eligible |
| subdomain of stored host (`app.github.com` vs `github.com`) | eligible only if that URL entry has `match:"domain"`; else a "similar item exists" hint with no autofill |
| same registrable domain, different host | same rule; PSL decides the boundary (§9.7) |
| stored `https`, page `http` | refuse; exception: host is `localhost`/`127.0.0.1`/RFC-1918 **and** the record has `allow_http:true` |
| IDN/punycode host (`xn--…`) | refuse + warn banner "this site's address uses look-alike characters" unless the record was explicitly saved for that punycode host |
| confusable rendering | helper computes a confusable skeleton (bundled Unicode `confusables.txt` subset); skeleton collision with a *different* stored host → refuse + warn |
| host with userinfo, IP-literal trickery, dotless/decimal-IP forms | refuse |
| cross-origin iframe | fill only the frame whose own canonical origin matches; never the parent on a child's behalf |
| opaque origins (`data:`, `file:`, `about:`, sandboxed null origin) | refuse |
| multiple accounts | chooser UI; no silent pick |

### 9.6 One-shot fill token and replay protection

- `request_id` + `ref` + approved origin + nonce form one authorization.
- The fill response is delivered once; the helper marks it consumed when
  the response frame is written. Re-`authorize` with the same
  `request_id` → `APPROVAL_REPLAY`.
- Response lifetime 30 s: the extension fills immediately; the content
  script re-verifies `location.origin` at fill time and refuses if it no
  longer equals the approved origin (navigation races).
- The content script fills via direct DOM assignment + `input`/`change`
  events (React-safe); it never logs field values, never stores them in
  `chrome.storage` (session memory only, cleared on fill).

### 9.7 Public Suffix List

- Rust: `psl` crate with a **pinned, vendored** PSL snapshot
  (`public_suffix_list.dat` committed, versioned, updated deliberately).
- Extension: `tldts` with its bundled list; the vendored PSL snapshot
  date is recorded in both builds; shared test vectors (§16.4) pin
  boundary behavior (`github.io`, `co.uk`, `appspot.com` …) so the two
  implementations cannot drift silently.

### 9.8 Save / update flows

- **Save new:** on form submit with a password field, content script
  captures `{origin, username, password}` into session memory; after
  navigation success heuristic (URL change without error page), the
  popup badge offers "Save to Source Vault". Tapping sends `save_new`.
  Requires vault UNLOCKED; if locked, offer unlock first. Rationale: the
  user just typed this value into the page, so the extension learns
  nothing new; a compromised extension gains no extra capability.
  The helper merges duplicates by (canonical host, username): existing
  match → treated as an update, which changes the policy (next bullet).
- **Update (trusted confirmation required, v0.2 finding 9):** replacing
  an existing credential is never applied on the extension's say-so —
  that would let a compromised extension overwrite stored passwords.
  `save_update` is held pending by the helper until a confirmation
  **controlled outside the extension** completes:
  - local: helper-presented LA prompt
    ("Source Vault: update the saved password for alice@example.com on
    github.com?"), or
  - iPhone: signed approval, `action: update_password` (§6.5 tag 0x06 =
    5), bound to origin + credential ref + request_id + nonce + expiry.
  The confirmation UI names the origin and account and **never displays
  the new or old password**. Only after approval does the helper apply
  the update (old password → `password_history`) as a new revision.
  Replays of the approval fail per §6.6.
- Update decisions are logged in the vault's local audit view.

### 9.9 Extension compromise assumptions (documented, v0.3 §12.5)

A compromised extension can: request candidates for arbitrary origins
(getting only titles/usernames of matching items — a metadata leak
bounded by rate limiting: max 20 candidate queries/minute/session), and
trick the user into approving a fill for an origin displayed in the
prompt. It cannot: enumerate the vault, read passwords without a
per-fill user presence, obtain VK material, or replay a consumed fill.
The helper's origin display in every presence prompt is the load-bearing
mitigation — prompts are rendered by the OS/LA and the iOS app, not by
the extension.

---

## 10. Dashlane import

Deterministic, offline, synthetic-fixture-only until the §19 gate.
**The user's real Dashlane export is never requested, received, committed,
logged, pasted into AI context, or used in tests.**

### 10.1 Expected input formats

Dashlane's web export produces either a single `.csv` (newer) or a
`.zip` containing per-type CSVs (`credentials.csv`, `payments.csv`,
`securenotes.csv`, `ids.csv`, `personalinfo.csv`) (older). v1 supports:

- single CSV whose header matches the credentials or payments shape;
- the ZIP, reading `credentials.csv` and `payments.csv`;
  `securenotes.csv` handling is the §10.8 owner decision — until it is
  recorded as Choice A, the member is counted, explicitly reported as
  not imported, and never silently dropped; other members are reported
  as unsupported (not parsed).

Parser: Rust `csv` crate (RFC 4180 + lenient quoting mode), streaming
(`Reader::records()` iterator), one row at a time. No `eval`, no
deserialization frameworks beyond `csv`, no network, no AI involvement.

### 10.2 Schema detection and field mapping

Header row decides the dialect (case-insensitive, trimmed):

| Dashlane credentials.csv column | → login@1 field |
|---|---|
| `title` / `name` | `title` |
| `username` / `login` (+ `username2`, `username3` fallback) | `username` (first non-empty) |
| `password` | `password` |
| `url` / `website` | `urls[0]` after §9.4 canonicalization; `match:"exact"` |
| `note` | `notes` |
| `otpSecret` | **unsupported** — counted and reported "re-enter manually" |
| `category`, `totp`, others | ignored |

| payments.csv column | → card@1 field |
|---|---|
| `name`/`account_name` | `label` |
| `card_number`/`number` | `number` |
| `expiration`/`expiry` | `expiry` (normalised MM/YY) |
| `cardholder`/`name_on_card` | `cardholder` |
| `billing_*` | `billing_address` |
| `cvv`/`cvc`/`security_code` | **discarded on sight**; the row's report line notes "CVV not imported (by design)" |

Rows whose shape matches neither table → skipped, counted, reported by
row number only.

### 10.3 Row handling rules

- **Malformed rows** (bad quoting, column count mismatch): skip + count;
  never guess field positions.
- **Embedded commas/newlines/quotes:** standard RFC-4180 handling;
  Dashlane's historical inconsistent quoting is handled by lenient mode
  plus a strict-mode fallback retry per file (whole-file strategy chosen
  once from the first 100 rows — no per-row mode mixing).
- **Unicode:** NFC-normalize display fields; reject unpaired surrogates
  / invalid UTF-8 at read (lossy conversion forbidden for secrets).
- **Oversized:** field > 64 KiB or row > 256 KiB → skip + count.
- **Duplicates within the file / against the vault:** identity =
  (kind, canonical host, username) for logins, (kind, label, card
  number) for cards — compared only as HMAC fingerprints (below), never
  as persisted plaintext. Duplicate → keep existing vault item, count as
  duplicate.
- **Idempotent re-import (keyed construction, v0.2 finding 7):** the
  `import_log` table (§3.2) stores `fingerprint = HMAC-SHA256(
  import_fp_key, normalized_identity)` where

  ```text
  import_fp_key       = HKDF-SHA256(ikm=VK, salt=header.import_fp_salt,
                                    info="ov0/import-fingerprint/v1")
  normalized_identity = u8(kind) ‖ canonical_host ‖ username    (logins)
                      = u8(kind) ‖ label ‖ card_number          (cards)
  ```

  The key is derived from VK, never stored, and exists only while the
  vault is unlocked — without VK the log is a set of opaque HMACs with
  no feasible oracle (unlike v0.1's salted SHA-256 over password
  material, which would have been a fast offline guessing oracle to
  anyone holding the database + salt). **Passwords are not part of the
  identity:** idempotency is defined by *who/where*, not *what* — a
  re-imported row with the same identity but a changed password is a
  duplicate (the vault keeps its item; the change is reported in the
  counts as "duplicate"), matching the keep-existing duplicate rule.
  The plaintext identity string is additionally sealed into
  `identity_ct` under the meta key so that VK rotation can recompute all
  fingerprints under the new generation's key during re-encryption; a
  rotation that skips this recomputation would break idempotency, so it
  is part of the rotation transaction (§2.10).
- Re-importing the same file → all duplicates reported, zero new items.

### 10.4 Streaming encryption, no plaintext staging

```text
file → csv stream → row → map → JSON serialize → AEAD seal (record_key)
     → INSERT → zeroize row buffer
```

At no point is there a plaintext Vec of all records, a plaintext temp
file, or a plaintext "preview vault". The import UI shows counts and
titles only; titles come from the just-written `meta_ct` path like any
other list view. The file dialog path is handed to the helper; the helper
validates it is a regular file (not a symlink, not inside the vault
directory) and opens it itself.

### 10.5 Reporting

Import report (UI + savable text): counts imported / duplicates /
skipped-malformed / skipped-unsupported (with category names like
"otpSecret", never values), duration, and per-skip row numbers. No
secret values, usernames, or URLs in reports or logs. Report generation
runs with capture suppression active (§14).

### 10.6 Source-file deletion and honesty copy

Post-import the UI offers "Delete the export file" (move to Trash via
`tauri-plugin-dialog`+FS op by the main app) with copy that must say,
verbatim in spirit:

- the CSV is plaintext and readable by anyone on this Mac;
- it may already be in cloud-synced folders (iCloud Downloads/Desktop)
  and backups;
- delete it there too, and empty the Trash;
- SSD/APFS snapshots make guaranteed erasure impossible — Source does not
  claim secure deletion (v0.3 §13, C10 honesty rule).

### 10.7 Fuzzing and fixtures

- Synthetic fixtures under `src-tauri/vault-helper/tests/fixtures/dashlane/`:
  valid minimal, all-dialects, embedded commas/newlines/quotes, invalid
  UTF-8, oversized fields, duplicate storms, mixed ZIP, empty file,
  header-only, 10⁵-row stress. Every fixture is generated by a committed
  script with fixed seeds; none contain real-looking secrets (passwords
  are `fixture-…` tokens; the repository/CI check from v0.3 §14.1 item 6
  would flag real ones).
- `cargo fuzz` target `fuzz_dashlane_row`: arbitrary bytes → dialect
  sniff + row parse must never panic, never allocate > 2× input size,
  never emit a record with an empty password field from a non-empty
  password column, and round-trip escaping invariants hold.

### 10.8 Secure Notes — owner decision checkpoint (v0.3 finding 11)

The user's Dashlane data includes Secure Notes. v1 scope for them is an
**owner decision, not an engineering default**; this document records
the checkpoint and both choices without silently picking one. The
decision does not block Phases A–H. It must be resolved and recorded
**before Phase I importer implementation begins**, and before Source
Vault may be described as a complete Dashlane replacement.

**Choice A — include `secure_note@1` in v1.** Would require:

- encrypted note schema: `title`, `body`; the body lives only inside the
  record ciphertext, never in metadata;
- optional URL/category metadata only if separately justified;
- no plaintext note content anywhere outside the record ciphertext;
- Dashlane `securenotes.csv` importer mapping, fixtures, and fuzz rows;
- capture-safe add/edit/reveal UI on the §14 suppressed surfaces;
- no browser autofill behavior — notes never fill;
- CR/IM/CS test extensions and §8 record-type documentation.

**Choice B — defer Secure Notes.** Then the importer and UI must:

- explicitly report that Secure Notes were present and **not imported**,
  with an exact count;
- suppress the §10.6 delete-export offer whenever the export still
  contains unsupported secure-note data (the file must not be deleted
  automatically while it is the only copy of those notes);
- tell the user, in plain language, to retain and migrate those notes
  elsewhere.

Neither choice is made here. Recording the decision is §19 gate row 28.

---

## 11. Durable remote encrypted backup (v0.4)

The provider is untrusted for confidentiality and is **not a root of
trust** (v0.3 §11). It stores ciphertext and public verification data, and
it enforces structure as defense in depth; every device verifies everything
it accepts. Remote storage exists so that losing every device leaves a
recoverable ciphertext copy, and in Phase F it is also the path by which
Macs exchange state (multi-writer sync through the provider). Direct
Mac⇄iPhone peer sync and a full iPhone record client are Phase F.2 (§18);
in Phase F the iPhone only fetches its own envelope (§4.7).

### 11.1 Components and responsibilities

| Responsibility | Helper | Main process | Provider |
|---|---|---|---|
| Build, seal, index, sign the manifest, MAC the checkpoint | **yes** | no | — |
| Canonical `ProviderRequest` and its signature (§11.4) | **yes** | never supplies path, method, audience, vault, expected state, `t` or `n` | verifies |
| State-transition bodies | **builds** | transports verbatim | validates structurally |
| HTTP/TLS, retry, backoff, queueing | no | **yes** (`ProviderTransport`, `BackupCoordinator`) | — |
| Verify downloaded state (signatures, chain, checkpoint, rollback/fork, AEAD) | **root of trust** | no | defense in depth only |
| CAS, replay cache, throttling, handle claims, GC | — | — | **yes** |

- The main process moves ciphertext and public data only (§1.3, §1.5). It
  never sees plaintext records, MP/PK/RK, recovery-auth private keys or
  device private keys — and no reusable authentication secret exists.
- `ProviderTransport` has an in-process implementation (calling
  `vault-provider-core` over `FsStores`) for tests and rehearsals, and an
  HTTPS implementation for production. HTTPS uses standard platform /
  public-CA validation; **no certificate pinning**. TLS protects transport
  confidentiality and metadata; vault-state authenticity comes from
  signatures, checkpoints and manifests.
- **Production backend (owner decision):** a small stateless Rust service
  (`vault-provider`, §1.1) with **Amazon S3** holding immutable blobs
  (`If-None-Match: *`) and the per-vault state object (`If-Match` on the
  previous ETag). S3 ETags are concurrency tokens only, never content
  hashes. The hosting platform is not fixed; the service is a portable
  container. Horizontal scaling is safe because CAS, nonces, handle claims
  and rate-limit slots all live in S3.
- **Provider operational requirements:** the bucket has versioning on,
  Block Public Access, lifecycle rules (`v2/nonces/` and `v2/ratelimit/`
  2 days; incomplete multipart uploads 1 day; non-current versions 30
  days) and an IAM role scoped to it; the only provider-held secret is the
  locate pepper (§11.5), which carries no vault authority; logs record
  route, status, `vid` and `key_id` only, never request or response
  bodies.

### 11.2 Storage and state model

**Layout** (S3 keys; `FsStores` uses the same paths):

```text
# vault data (what clients verify)
v2/vaults/{vid}/state                     the only mutable vault object; If-Match CAS (create: If-None-Match: *)
v2/vaults/{vid}/blobs/{sha256}            immutable; If-None-Match: *; name = SHA-256(bytes), recomputed by the provider
# provider operational state (never a vault root of trust)
v2/handles/{handle_key}                   claim {vault_id, claim_id, created_at, status} (§11.3.1)
v2/nonces/{vid}/{key_id}/{n}              create-only; lifecycle 2 days (§11.4)
v2/ratelimit/{vid}/recovery-mp/{window}/{k}  create-only reservation (MP class only), deleted on success; lifecycle 2 days (§11.5)
```

Every stored artifact of a vault — record objects, header, registry,
wraps, **device envelopes**, index, checkpoints and manifests — is a blob.
No blob is ever overwritten: a second write of a name is rejected, and
same-name content is identical by construction. `state` and handle claims
change only by CAS on their ETag; nonces and rate-limit slots are
create-only. Concurrent publishers therefore cannot overwrite each other's
objects.

**Operational state is not a root of trust.** Handle claims, nonces,
rate-limit slots, GC bookkeeping and the provider-derived fields of
`state` exist for availability, idempotency and abuse control. A malicious
or buggy provider can lie about them — refuse service, report a handle as
taken, throttle, replay a stored result, GC early, serve fake locate data —
but it **cannot** produce a vault state a client accepts. Clients accept
only what verifies: the manifest signature under a registry they have
anchored (§4.4 rules and rollback floor, or the §4.8 checkpoint on a fresh
device), the registry chain, the checkpoint MAC under the VK they hold,
index and blob hashes, and AEAD.

**State object** (provider-internal JSON):

```json
{ "v": 2, "vault_id": "…",
  "generation": 7, "manifest_hash": "…", "checkpoint_hash": "…", "index_hash": "…",
  "registry_hash": "…", "registry_head": "…", "registry_seq": 12, "epoch": 0,
  "header_hash": "…", "vk_generation": 3,
  "active_devices": [{"device_id": "…", "sign_pub": "…"}],
  "recovery_auth": {"mp": {"pub": "…", "salt": "…"}, "rk": {"pub": "…", "salt": "…"}},
  "locate": {"kdf": {"alg":"argon2id","version":19,"m_kib":65536,"t":3,"p":1,"out_len":32,"salt":"…"},
             "auth_salt_mp":"…","auth_salt_rk":"…"},
  "handle_key": "…", "claim_id": "…",
  "state_commit": "…",
  "retained": [{"generation":6,"manifest_hash":"…","checkpoint_hash":"…","index_hash":"…"},
               {"generation":5, "…": "…"}],
  "recent": [{"body_sha256":"…","result":{"generation":7,"state_commit":"…"}}],
  "finalized": {"<expected generation>": "<body_sha256>"} }
```

`active_devices`, `locate` and `vk_generation` are **derived at commit**
from the committed registry and header blobs, never taken from a request.

**State commitment** — the client-visible compare-and-swap token:

```text
recovery_auth_digest = SHA-256("ov0/recovery-auth-set/v2" ‖ for c in [mp, rk] if present:
                                u8(class) ‖ pub(65) ‖ salt(16))
state_commit = SHA-256("ov0/vault-state/v2" ‖ TLV{
    0x01 vault_id(16)  0x02 generation(u64)  0x03 manifest_hash(32)
    0x04 checkpoint_hash(32)  0x05 recovery_auth_digest(32) })
```

The helper computes `state_commit` for the state it expects and the one it
proposes. Every input is either verified by the helper (manifest,
checkpoint) or public (`recovery_auth`), so a provider cannot swap state
behind an unchanged token. ETags never appear on the wire to clients.

**Object index v2** (the canonical bytes are the hashed bytes):

```text
#ov0-index v2
#generation <u64>
#items <u64>
<role> <logical-id> <blob-sha256-hex> <size> [<parents>]      # lines sorted bytewise
  header   -
  registry -
  wrap     mp|rk
  env      <device_id hex32>
  rev      <record_id hex32>/<revision_id hex64>   <parent revision_ids, comma-separated, sorted, or "-">
```

**Manifest v2 (`SignedManifest`, canonical TLV, signed by the publishing
device's signing key):**

| Tag | Field |
|---|---|
| 0x01 | `version` u32 = **2** (any other value is refused: `FORMAT_TOO_NEW` above 2, `FORMAT_INVALID` for the retired v1; §3.5) |
| 0x02 | `vault_id` 16 B |
| 0x03 | `generation` u64 (strictly increasing, +1 per state transition) |
| 0x04 | `created_at` u64 |
| 0x05 | `registry_head` 32 B |
| 0x06 | `vk_generation` u32 |
| 0x07 | `object_index_hash` 32 B = SHA-256(index v2 bytes) — also the index's blob address |
| 0x08 | `prev_manifest_hash` 32 B (all-zero for generation 1) |
| 0x09 | `signer_device_id` 16 B |
| 0x10 | `signature` 64 B, low-S, over `SHA-256("ov0/manifest/sign/v1" ‖ tlv(without 0x10))` |

```text
manifest_hash      = SHA-256(tlv(SignedManifest including 0x10))    # the manifest's blob address; bound by
                                                                     # §4.3 tag 0x0F, §4.5, state_commit 0x03
manifest_core_hash = SHA-256("ov0/manifest/core/v1" ‖ tlv(without 0x10))   # bound by the §4.8 checkpoint
```

Decoding is strict: canonical TLV, every field present with its exact
width, and re-encoding must reproduce the input. The §4.8 checkpoint
format is unchanged; it is its own blob, never listed in the index (no
recursion), and its hash is carried in `state` and returned by
`state_get`.

**Blob size caps (provider-enforced, `413` above):** index ≤ 8 MiB,
registry ≤ 4 MiB, every other blob (records, header, wraps, envelopes,
checkpoints, manifests) ≤ 1 MiB. A `create` transition's inline bootstrap
blobs (§11.3) total ≤ 1 MiB.

**Retention and GC (owner decision).** Devices have **no** delete API.
Reachable = the current and two `retained` states (their manifest,
checkpoint and index blobs), every blob those indexes list, and every blob
younger than 7 days. Provider-controlled GC deletes everything else on a
schedule. A commit that races GC fails with `412 BLOB_MISSING`; the client
re-uploads and retries.

### 11.3 State transitions (atomic vault-state CAS)

Every mutation of a vault's state — account creation, publication and
recovery finalize — is one **state transition**: blobs are uploaded first
(idempotent, content-addressed), then one `POST /v2/vaults/{vid}/state`
commits the whole change with a single CAS on the state object. An
interrupted transition leaves only unreferenced blobs, which GC collects.

**`StateTransition` body** (canonical TLV, §4.2 rules):

| Tag | Field |
|---|---|
| 0x01 | `proto` u32 = 2 |
| 0x02 | `vault_id` 16 B |
| 0x03 | `kind` u8: 1 `create`, 2 `publish`, 3 `finalize` |
| 0x04 | `expected_state` 32 B (`state_commit` being replaced; all-zero iff `create`) |
| 0x05 | `manifest` bytes (manifest v2) |
| 0x06 | `checkpoint` bytes (§4.8) |
| 0x07 | `recovery_auth_updates` bytes (optional): concatenation, sorted by class, of `u8 class ‖ pub(65) ‖ salt(16)` |
| 0x08 | `handle_key` 32 B (`create` only) |
| 0x09 | `bootstrap_blobs` bytes (`create` only, required): concatenation of `u32be(len) ‖ blob` for **every** blob the genesis state references (header, registry, wraps, the genesis device's envelope, index); total ≤ 1 MiB |

**Bootstrap (`create`).** Before a vault's state exists no signer can be
resolved, so `blob_put` is never accepted for a `{vid}` without state
(`401`). A new vault's state has no records, and `create` carries all of
its blobs inline in 0x09. The provider hashes each, requires one of them
to be the index (SHA-256 = `manifest.object_index_hash`) and the index to
reference exactly the others, writes them create-only (identical existing
blobs are fine) before the claim protocol, and only then creates the
state. Nothing is written for a `create` whose authentication or
validation fails. There is no unauthenticated write path into any
`{vid}` namespace.

**Provider algorithm.** Any failure means no mutation. Reads use S3 GET and
keep the ETag.

1. **Authenticate** the `ProviderRequest` (§11.4) and the kind/class
   pairing: `create` → signed by the genesis device of the supplied
   registry; `publish` → a device in the current `active_devices`;
   `finalize` → the recovery class registered in the current state.
2. **Idempotency:** if SHA-256(body) ∈ `recent`, return the stored `200`
   result. For `create`, idempotency is handled by §11.3.1 (which also
   completes an interrupted binding); step 2 never short-circuits a
   `create`.
3. **Precondition:** `create` → no state exists; otherwise
   `expected_state` must equal `state.state_commit`, else
   `409 STATE_MOVED {state_commit, generation}`.
4. **Manifest v2:** `vault_id` matches; `generation` = current + 1
   (`create`: 1); `prev_manifest_hash` = current `manifest_hash` (`create`:
   zeros); `signer_device_id` = the request signer (`publish`), the device
   installed by the appended `recovery_epoch` (`finalize`), or the genesis
   device (`create`).
5. **Index v2** (blob `object_index_hash` exists — for `create`, among the
   inline bootstrap blobs): parses; generation
   matches; every listed blob exists with the listed size; exactly one
   `header`, `registry` and `wrap mp`, at most one `wrap rk`; the `env`
   device set equals the active devices of the *new* registry; every `rev`
   line's parents are listed (ancestor closure).
6. **Registry blob:** parses; the current registry is an exact prefix by
   entry hash (`create`: exactly one valid genesis); verifies under the
   structural chain rules (§4.4 rules 1–5, 7, 8; recovery epochs
   structurally, as in §4.8); new head = `manifest.registry_head`.
   Appended entries by kind:
   - `publish`: `enroll`/`revoke` only, each signed by a device active at
     its seq. If any `revoke` is appended: `manifest.vk_generation` =
     current + 1 (mandatory rotation) and no `env` for the revoked id.
   - `finalize`: exactly one `recovery_epoch` (epoch = current + 1),
     immediately followed by one `revoke` per device in the current
     `active_devices`, in ascending `device_id` order, each with
     `authorizer` = the epoch's device and signed by it (§4.4 S-4); no
     other entries. The revoked set must equal the prior active set, else
     `422 REGISTRY_INVALID`. `manifest.vk_generation` = current + 1; no
     `env` for any prior device.
7. **Header blob:** parses as header v2; `vault_id` matches; `locate` is
   taken from it; its `kdf` block equals an allowlisted tuple (§2.3), else
   `422 MANIFEST_INVALID`. **Recovery-auth rules:** for each class c,
   `header.auth_salt_c` differs from the current one **iff** an update for
   c is present, and the update's salt equals the header's; additionally,
   for the MP class, an mp update is present **iff** `header.kdf.salt`
   changed, and the `password.wrap` blob's public parameter block (m, t,
   p, salt) equals `header.kdf` (the provider parses the wrap's public JSON
   fields). The `wrap mp` blob may change without an mp update — every VK
   rotation re-seals it under the same PK; otherwise
   `422 RECOVERY_AUTH_STALE`. `create` requires updates for both classes.
   A `publish` that appends a `revoke` must carry updates for **both**
   classes (§11.4 revocation), else `422 RECOVERY_AUTH_STALE`.
8. **Signatures and bindings:** the manifest signature verifies under the
   signer's `sign_pub` (for `finalize`, the `recovery_epoch` entry's); the
   checkpoint's structural binding matches (`vault_id`, registry head,
   `manifest_core_hash`, generation, `vk_generation`, epoch). The provider
   holds no VK and cannot verify the checkpoint MAC.
9. **Commit:** put the manifest blob (named `manifest_hash`) and the
   checkpoint blob (both create-only; identical existing blobs are fine);
   build the new state (derived fields; `retained ← [old
   current, old retained[0]]`; `recent` keeps the last 3; `finalized`; new
   `state_commit`). `create` → §11.3.1. Otherwise PUT state with
   `If-Match: <ETag from step 3>`; on `412`/`409` reload and repeat from
   step 2 (an idempotent hit returns `200`; a moved state returns `409
   STATE_MOVED`).
10. **Respond** `200 {generation, state_commit}`.

**Routes and responses:**

| Route | Auth | Returns / accepts |
|---|---|---|
| `GET /v2/vaults/{vid}/state` | device, recovery | `{generation, state_commit, manifest (b64), checkpoint (b64), vk_generation, recovery_auth: [{class, pub, salt}]}` — returned to authenticated callers only (device or recovery class), never to anonymous ones; the helper recomputes `state_commit` from the verified manifest/checkpoint hashes and `recovery_auth` before using it as `expected_state` |
| `GET /v2/vaults/{vid}/blobs/{sha}` | device, recovery | blob bytes |
| `PUT /v2/vaults/{vid}/blobs/{sha}` | device; recovery | creates (hash recomputed, `422` on mismatch; existing identical → `200`); size caps §11.2; never for a `{vid}` without state |
| `POST /v2/vaults/{vid}/state` | per transition kind | `StateTransition` → `200 {generation, state_commit}` |
| `POST /v2/recover/locate` | none | §11.5 |
| `/v2/push/*` | — | reserved module; Phase G only |

**Errors:**

| Status | Code | Client handling |
|---|---|---|
| 401 | `AUTH_INVALID` | generic: unknown vault, unknown key, a key no longer in `active_devices` (including a revoked device) and a bad signature are indistinguishable; never interpreted as revocation (§4.7) |
| 403 | `DEVICE_NOT_AUTHORIZED` | an authenticated signer attempting an operation its class may not perform (e.g. a recovery key posting a `publish`) |
| 409 | `BACKUP_REPLAY` | — |
| 409 | `STATE_MOVED` | merge, re-stage (≤ 3), then `BACKUP_CONFLICT` |
| 409 | `HANDLE_TAKEN` | ask for another handle |
| 412 | `BLOB_MISSING {count}` | re-upload, retry |
| 413 | — | too large |
| 422 | `MANIFEST_INVALID` / `INDEX_INVALID` / `REGISTRY_INVALID` / `CHECKPOINT_MISMATCH` / `RECOVERY_AUTH_STALE` / `FINALIZE_CONFLICT` | fail the transition |
| 429 | `RECOVERY_THROTTLED` / generic | §11.5 |
| 503 | — | `BACKUP_UNAVAILABLE` |

**Merging on `STATE_MOVED`** (helper, SYNCING): fetch and verify the new
state, merge per §3.2 (never a timestamp pick; the registry must extend;
divergence → §4.6 fork), rebuild and re-stage at the new generation, retry
at most 3 times, then surface `BACKUP_CONFLICT`. Conflicts replicate to
every Mac and are resolved once via `resolve_conflict`.

**Vault-wide singletons (normative merge rule).** `header.json`,
`password.wrap`, `recovery.wrap`, the device envelopes and `recovery_auth`
are vault-wide state, not per-record revisions. A device's pending
wrap-bearing change (MP change, RK replacement, revocation — anything
recorded in `pending_remote`) lives in its **local committed vault files**
and records its **base**: the `vk_generation`, `kdf.salt`, `auth_salt_mp`,
`auth_salt_rk` and `registry_head` of the remote state it was built on. On
merge:

1. **Base unchanged remotely** (the adopted state has the base's
   `vk_generation`, salts **and** `registry_head`): no conflicting
   singleton or registry change was committed. The device keeps its local
   singletons, merges records, and re-stages its pending transition.
2. **Base changed remotely** (another device committed a rotation, a
   change to the same singletons, **or any registry entry after the base**):
   the pending change is never re-staged automatically; it **cannot** be
   re-applied from stored material — its wraps seal a VK or a secret the
   committed state no longer uses, and `PK`/`RK_bytes` are not retained.
   The helper adopts the committed singletons (obtaining the committed VK
   from its own envelope in that state; no envelope and a valid revoke
   naming it → it is revoked), marks the pending operation
   `NEEDS_USER`, and asks the user to redo it on top of the adopted
   state: a revocation or RK replacement re-collects the master password
   (verified against the committed `password.wrap`, §11.4) and issues a
   new Recovery Key with a new acknowledged sheet (the previous new RK
   never reached the provider and is void); a pending MP change asks for
   the new MP again — or, **only when the adopted state actually changed
   `kdf.salt`**, lets the user keep the other device's MP. That choice is a
   **partial resolution**: it discards only the MP-change component of the
   `pending_remote` record. Any other component — a pending RK cutoff, a
   revocation, or a security warning that remains unresolved — stays
   pending with its status, priority and banner unchanged; the record is
   cleared only when no component remains. Security-driven operations keep
   their warning banner throughout.
   **Adoption is one §2.10 journaled commit.** If the pending change
   included a local VK rotation (RK replacement, revocation), that
   abandoned rotation's local copies of revisions present in the committed
   state are replaced by the committed blobs (same `revision_id`s); this
   device's own revisions absent from the committed state are re-sealed
   under the adopted VK (it still holds its own rotated VK until this
   commit); the pending operation's local-only registry entries are
   dropped (the redo re-appends them); and only then is the abandoned
   local VK zeroized. A crash leaves either the pre-adoption or the
   post-adoption vault. **Within this adoption path only**, the pending
   operation's own unpublished registry entries are dropped rather than
   treated as fork evidence against the committed registry — **except**
   that a committed registry entry after the pending operation's base that
   is signed by the target of this device's pending revocation remains
   fork evidence under §4.6 (§11.4). §3.2's duplicate rule compares only
   published copies.
3. The helper **never** stages a wrap or envelope whose payload
   `vk_generation` differs from the manifest it publishes.

Whenever a device adopts an MP-class change committed by another device
it tells the user: "Master password changed on <device name>".
Anything a now-revoked device planted in these singletons while it was
still active is overwritten by the revocation transition, which always
re-keys both recovery classes (§11.4).

**Records across a concurrent rotation.** A device that rotated locally
has zeroized the previous VK, so revisions other devices published under
that VK in the meantime cannot be re-sealed by it; it leaves them out of
its index. Their authors restore them: every device treats **its own
revisions that are absent from the latest committed state** as pending
local work and re-seals them under the new VK (same `revision_id`s and
parents, §2.10) once it adopts that VK. Revisions by revoked authors stay
refused (§3.2).

#### 11.3.1 `create`: handle claim without a cross-key transaction

**Claim object** `v2/handles/{handle_key}`: `{vault_id, claim_id (16 B
OsRng, chosen by the provider), created_at, status: "pending" | "bound"}`.
`state` records `claim_id` and `handle_key`.

A claim is **live** iff `status = bound`; or `status = pending` and
`now − created_at < G` (G = 24 h); or `status = pending` and
`state(claim.vault_id)` exists with `state.claim_id = claim.claim_id` (a
create that crashed after writing state — a created vault always keeps its
handle). Lookup (§11.5) resolves a handle only if it is `bound` and the
state's `claim_id` and `handle_key` match; anything else gets the fake
response.

| Step | Action | Outcomes |
|---|---|---|
| C1 | GET claim | absent → C2. Present with the same `vault_id` (our retry) → C3 with that `claim_id`. Present for another vault: live → `409 HANDLE_TAKEN`; not live → C2′ |
| C2 | PUT `{vault_id, new claim_id, now, pending}` with `If-None-Match: *` | `200` → C3; `412` → C1 |
| C2′ | reclaim: PUT the same body with `If-Match: <stale claim ETag>` | `200` → C3; `412` → C1 |
| C3 | PUT state (with `claim_id`, `handle_key`) with `If-None-Match: *` | `200` → C4. `412`: if `state.claim_id = claim_id` and SHA-256(body) ∈ `state.recent` → C4 (our retry); else `409` |
| C4 | bind: PUT `{…, status: bound}` with `If-Match: <claim ETag from C1/C2>` | `200` → respond `200`. `412` → re-GET: bound to us → `200`; now another vault's → roll back (DELETE our generation-1 state with `If-Match`) and `409 HANDLE_TAKEN` |

Consequences: a crash after C2 never burns the handle (our retry
continues; after G another vault may reclaim); a crash after C3 is
completed by our retry and protected from theft by the third liveness
clause; a crash after C4 is idempotent; of two concurrent creates exactly
one C2 succeeds; a reclaim racing the owner's late retry is decided by the
single `If-Match` on the claim, and the loser rolls back an unbound
generation-1 state. Rollback is a provider-internal compensation for a
failed `create`, not a client delete capability. The helper keeps the
staged `create` until `200`; on `409 HANDLE_TAKEN` it asks the user for
another handle.

#### 11.3.2 Publication and remote-completion status

**Publication flow.** `backup_prepare` (UNLOCKED → BACKING_UP) seals dirty
records and stages blobs plus the `publish` body; main uploads the blobs
(`blob_put`, each signed), then posts the transition; `backup_commit_result`
reports the outcome. A publication that was **fully staged** while unlocked
(blobs + body, including the VK-dependent checkpoint) may finish while
LOCKED. If the state moved and a merge (which needs the VK for a new
checkpoint) is required, completion waits for the next unlock.

**Remote-completion status (normative).** MP change (both modes), RK
replacement, revocation, enrollment and any trusted-device RK issuance are
tracked by an operation status persisted in `kv` in the **same local
commit** as the change:

```text
pending_remote { op: vault_create | mp_change | rk_replacement | revocation | enrollment,
                 security_driven: bool,   # true for a suspected-stolen RK (§12 scenario 7) and for revocation
                 local_committed_at: u64, staged_session: id | null,
                 recovery_auth_updates: [(class, pub, salt)],   # public; needed to re-stage after STATE_MOVED
                 base: { vk_generation, kdf_salt, auth_salt_mp, auth_salt_rk, registry_head },   # §11.3 singleton merge rule
                 needs_user: bool,        # base changed remotely; the user must redo the operation
                 attempts: u32, last_error: code }
```

The public recovery-auth updates plus the local committed wraps and
header are what a re-stage needs when the base is unchanged; `PK`,
`RK_bytes` and `sk_c` are **not** retained for it (§2.11). When the base
changed remotely the operation becomes `needs_user` (§11.3 singleton merge
rule). The local change and this record commit atomically: an MP change
uses the §2.10 journal (stage `password.wrap.next`, `header.json.next` and
the `kv` record, then the commit marker), exactly like a rotation, so a
crash leaves either the old MP state or the new one with its
`pending_remote` record — never a mix.

| Status | Meaning | True remotely |
|---|---|---|
| `LOCAL_COMMITTED` | the local vault changed (wrap renamed / journal committed) | nothing yet |
| `REMOTE_UPDATE_PENDING` | a transition is staged or being retried | the provider still holds the **previous** state: old wraps and old recovery-auth keys; for revocation, the revoked key still authenticates |
| `REMOTE_COMMITTED` | the provider returned `200` for a transition containing the change, or `state_get` shows a `state_commit` that includes it | the change is effective remotely |

`LOCAL_COMMITTED` → `REMOTE_UPDATE_PENDING` is immediate; a component
leaves `REMOTE_UPDATE_PENDING` only on `REMOTE_COMMITTED`, or — for an MP
change only — by the user's explicit "keep the other device's master
password" partial resolution (§11.3 singleton merge rule), which never
clears a pending RK cutoff, revocation or security warning. The status
survives lock, restart and crash (no timeout). It is an operation status reported by the
`remote_update` event, not a `VaultState`.

**Consequence while pending, stated to the user:** the old MP or old RK can
still authenticate a total-loss recovery at the provider and recover the
previous remote state. For a stolen RK this means someone holding it and
the public handle could complete a total-loss recovery, which (§4.4 S-4)
would revoke this Mac. The Mac cannot then read the provider (its key is no
longer active, so every request gets the generic `401`); it detects this as
**`BACKUP_ACCESS_LOST`**: its own signed `state_get` fails with `401` on
three consecutive attempts at least 5 minutes apart while the provider is
otherwise reachable (locate answers) **and** the local clock is within the
±300 s window of the provider's HTTP `Date` header (a skewed clock is
reported as a clock problem, not as lost access). The UI shows, distinctly from
`BACKUP_STALE`: "Backup access lost — this Mac may have been removed, or
your vault may have been recovered on another device. If you did not do
this, treat it as a security incident." It is never treated as revocation
and never deletes key material (§4.7); if a security-driven
`pending_remote` is open at that moment, the copy states that the pending
cutoff did not complete in time.

**UI rules:** "the old Recovery Key no longer works" (or any equivalent
claim of a remote cutoff) is **never** shown before `REMOTE_COMMITTED`.
While pending, MP and routine RK changes say the backup still accepts the
previous secret until this Mac reaches it; security-driven RK replacement
and revocation show a **persistent warning banner** ("not yet cut off at
your backup — keep this Mac online") until committed. After
`REMOTE_COMMITTED` the copy is: MP change — "Your backup now uses the new
master password."; RK replacement — "The previous Recovery Key no longer
works."; revocation — the banner clears.

**Retry and offline behavior:** security-driven operations get top queue
priority, backoff 1 → 5 → 15 min, plus immediate retries at launch,
unlock, network change and manual "retry now"; routine ones use the normal
queue. Offline, the status persists indefinitely. Provider failures count
toward `BACKUP_REVOCATION_FAILED` (after 3 attempts, for revocation) or
`BACKUP_STALE` (48 h) while the status stays pending. A second change while
one is pending stages a new full transition containing both;
`security_driven` is sticky.

### 11.4 Authentication, authorization and revocation

v0.4 replaces v0.3's symmetric credentials (Option A of v0.2 finding 1)
with **signatures**. The provider stores only public keys; nothing
reusable is ever registered, transported or stored, so a provider database
leak yields no directly usable authority. It does enable offline MP
guessing at Argon2id cost against `pk_mp` — exactly as the `password.wrap`
the provider already stores does (S-1 qualification below).

**Device class.** Requests are signed with the device's existing Secure
Enclave signing key (§2.7) through the bridge's `ov0_se_sign_digest`. The
provider authorizes a device against the `sign_pub` in the current state's
`active_devices`, which is derived from the committed registry: a key
becomes active when a transition commits a registry installing it
(genesis, enroll, recovery_epoch) and inactive when a transition commits a
registry revoking it. Signing never raises a presence prompt; LOCKED-state
reads are signable because they need no VK.

**Recovery class — derivation (normative; owner decision S-1).**

| Input | MP class | RK class |
|---|---|---|
| secret (never the raw password) | `PK` = Argon2id(MP, `header.kdf.salt`) per the frozen §2.3 tuple, 32 B | `RK_bytes` (§2.4), 32 B |
| class salt (public, 16 B, OsRng) | `header.auth_salt_mp` | `header.auth_salt_rk` |
| domain | `ov0/provider-recovery-auth/mp/v2` | `ov0/provider-recovery-auth/rk/v2` |

```text
ikm_c        = HKDF-SHA256(ikm = secret_c, salt = auth_salt_c, info = domain_c ‖ vault_id, L = 32)
(sk_c, pk_c) = DeriveKeyPair(ikm_c)       # RFC 9180 §7.1.3, DHKEM(P-256, HKDF-SHA256), KEM 0x0010, exactly:

suite_id = "KEM" ‖ I2OSP(0x0010, 2)
LabeledExtract(salt, label, ikm)   = HKDF-Extract(salt, "HPKE-v1" ‖ suite_id ‖ label ‖ ikm)
LabeledExpand(prk, label, info, L) = HKDF-Expand(prk, I2OSP(L,2) ‖ "HPKE-v1" ‖ suite_id ‖ label ‖ info, L)
dkp_prk = LabeledExtract("", "dkp_prk", ikm_c)
for counter in 0..=255:
    bytes = LabeledExpand(dkp_prk, "candidate", I2OSP(counter,1), 32); bytes[0] &= 0xFF
    sk = OS2IP(bytes); if 0 < sk < n: return (sk, sk·G)     # rejection sampling, unbiased
error DeriveKeyPairError

pk_c = 65-byte uncompressed X9.63;  key_id_c = SHA-256(pk_c)
signatures: ECDSA-P256-SHA256 over the request prehash, RFC 6979 deterministic nonce, low-S
```

**Security qualification (normative text):**

- RFC 9180 recommends `DeriveKeyPair` input carrying `Nsk` (32) bytes of
  entropy. `RK_bytes` satisfies this. An MP-derived `PK` in general does
  **not**: its security is bounded by the human master password. Argon2id
  makes each guess expensive; it does not create entropy.
- Therefore the MP recovery-auth key is in **the same password-guessing
  security class as `password.wrap`**. Nothing in this construction
  upgrades it to a 256-bit authentication factor, and no document may claim
  that it does. Whoever holds `pk_mp` can test MP guesses at one Argon2id
  evaluation each, exactly as against `password.wrap`, which the provider
  already stores.
- The HKDF domain separation (class, version, `vault_id`) prevents
  cross-protocol key reuse; it does **not** increase entropy. `ikm_c` is
  never used in any HPKE context; the RFC 9180 derivation is used only as
  a standardized, unbiased scalar derivation.
- `sk_c` and `ikm_c` exist only transiently in the helper and never cross
  IPC; `pk_c` and the salts are public. `pk_c` is served only to
  authenticated callers (device or recovery class), never to anonymous
  ones.
- Vectors are kept separately: RFC 9180 Appendix A.3 conformance for the
  primitive, and SOURCE composition vectors (XV-RECOVERY-AUTH, §16.8). No
  new crypto dependency is used.

**Recovery-auth registration and updates (atomic; owner decision D-11).**
Invariant: in every committed state, `recovery_auth.mp.pub` derives from
the PK that opens that state's `password.wrap`, and `recovery_auth.rk.pub`
from the RK that opens its `recovery.wrap`. Every recovery-key change
regenerates the class's `auth_salt_c`, so the provider can enforce the
§11.3 step 7 rule structurally.

| Event | Transition | Update carried |
|---|---|---|
| Setup | `create` | mp + rk |
| MP change (both modes, §12 scenario 5) | `publish` (new `kdf.salt`, new `auth_salt_mp`) | mp |
| RK replacement (§12 scenarios 6/7) | `publish` (new `auth_salt_rk`) | rk |
| Revocation (§12 scenarios 2, 8) | `publish` (new `kdf.salt` + `auth_salt_mp` with `password.wrap` re-sealed under the same MP; new RK with new `auth_salt_rk`) | **mp + rk** |
| Total loss via MP, RK kept | `finalize` | none |
| Total loss via MP, RK not kept (new RK issued) | `finalize` | rk |
| Total loss via RK (new MP required, §12 scenario 4) | `finalize` | mp |

Each is a remote-tracked operation (§11.3.2): the provider keeps the old,
mutually consistent pair until the transition commits.

**`ProviderRequest` (normative).** Canonical TLV (§4.2 rules); 0x08 only
for the device class; 0x0B only for `state_commit`.

| Tag | Field | Content |
|---|---|---|
| 0x01 | `proto` | u32 = 2 |
| 0x02 | `audience` | UTF-8 provider origin, lowercase `https://host[:port]`; see "Provider origin" below |
| 0x03 | `vault_id` | 16 B |
| 0x04 | `operation` | u16 (policy table) |
| 0x05 | `method` | UTF-8 |
| 0x06 | `path` | UTF-8 canonical path; no query string; lowercase hex segments |
| 0x07 | `signer_class` | u8: 1 device, 2 recovery-mp, 3 recovery-rk |
| 0x08 | `signer_device_id` | 16 B |
| 0x09 | `signer_key_id` | 32 B = SHA-256(signer public key, 65 B) |
| 0x0A | `body_sha256` | 32 B, SHA-256 of the exact body (SHA-256("") if empty) |
| 0x0B | `expected_state` | 32 B `state_commit` being replaced |
| 0x0C | `t` | u64 Unix seconds (helper clock) |
| 0x0D | `n` | 16 B OsRng |

`sig = ECDSA-P256(SHA-256("ov0/provider/request/v2" ‖ tlv))`, 64 B `r‖s`,
low-S. HTTP header: `Ov0-Auth: v2.<base64url(tlv)>.<base64url(sig)>`.

**Provider origin (normative).** The helper takes the provider origin
from a **compiled-in allowlist** in the signed helper build (v1: exactly
one production origin; debug builds may add a test origin). Setup records
the chosen origin in header v2 `provider`; the helper refuses a header or
recovery flow naming an origin outside its allowlist. The origin is never
taken from the main process, is shown in the helper's own recovery panel
before the MP/RK is entered, and is printed on the Recovery Key sheet
(§1.7) so the user can compare. A fresh device performing total-loss
recovery therefore signs recovery-class requests only for an allowlisted
origin, and a phishing or misconfigured origin cannot collect
recovery-class signatures (which would give it an offline Argon2id
verifier).

**Policy table:**

| u16 | operation | method + path | classes | helper states |
|---|---|---|---|---|
| 1 | `state_get` | GET `/v2/vaults/{vid}/state` | device, recovery | LOCKED, UNLOCKED, SYNCING, BACKING_UP, RECOVERING |
| 2 | `blob_get` | GET `/v2/vaults/{vid}/blobs/{sha}` | device, recovery | same |
| 3 | `blob_put` | PUT `/v2/vaults/{vid}/blobs/{sha}` | device; recovery in RECOVERING only | BACKING_UP, RECOVERING, LOCKED (fully staged publication only) |
| 4 | `state_commit` | POST `/v2/vaults/{vid}/state` | device (`create`, `publish`); recovery (`finalize`) | same as `blob_put` |
| 32–47 | reserved for Phase G push | — | — | — |

There is **no** delete, device-registration or revoke route. The
unauthenticated `POST /v2/recover/locate` is outside this table (§11.5).

**Helper checks before signing** (all → `SIGNING_REFUSED`, no signature):
the state × operation × class table; `blob_put` only for a SHA-256 in the
requesting session's blob set; `state_commit` only when `body_sha256` is
the helper's own staged body and 0x0B equals that body's
`expected_state`; clock sanity (`t` ≥ vault creation time, ≤ now + 60 s);
`audience` from the helper's own provider origin (below), never from main. The main
process supplies only the typed operation, typed parameters and the body
hash, so it cannot substitute a body, path, method, vault, provider or
target state.

**Provider verification** (any failure → no side effect): canonical parse,
`proto` = 2; `audience` = own origin; `method`/`path` = the actual request
line and `vault_id` = the path's `{vid}`; operation matches the route;
signer resolves (device: `signer_device_id` ∈ `active_devices` and
`signer_key_id` = SHA-256 of its `sign_pub`; recovery: `recovery_auth[class]`
key id matches; `create` resolves against the genesis entry of the supplied
registry); the low-S signature verifies; `|now − t| ≤ 300`; SHA-256 of the
received body = 0x0A; for `state_commit` 0x0B = the body's
`expected_state`; nonce fresh. Unknown vault, unknown key and bad signature
return the same `401 AUTH_INVALID`. Blob PUTs are recomputed and must hash
to `{sha}` (`422` otherwise; an existing identical blob returns `200`).

**Replay cache.** Mutating requests (`blob_put`, `state_commit`) record
`v2/nonces/{vid}/{key_id}/{n}` with `If-None-Match: *` (exists →
`409 BACKUP_REPLAY`), expiring after 2 days — durable across restarts and
instances. Reads use an in-memory, per-instance cache keyed by
`(vid, key_id, n)` (≥ 10 000 per key, LRU), best-effort: a replayed read
returns ciphertext the capturer already saw.

**Revocation (normative).** Revocation is the local journaled commit
(presence → MP → new RK acknowledged → registry `revoke` + VK rotation +
surviving envelopes, §12 scenario 8) followed by **one `publish` state
transition** carrying the new registry, the rotated state, the survivors'
envelopes and recovery-auth updates for **both** classes: the MP class is
re-keyed with a fresh `kdf.salt` and `auth_salt_mp` (`password.wrap`
re-sealed under the MP the flow collects), and the RK class with the new
RK. The entered MP is **verified against the committed `password.wrap`**
(after adopting any committed singletons). If it does not open it — for
example because the device being revoked changed the master password
while it was still trusted — the panel offers to set a new master password
(new + confirm, as `change_master_password {mode:"reset"}`); a typo can
never silently become the vault's master password. Re-keying both classes guarantees that no
recovery-auth key a revoked device may have registered while it was still
active survives the revocation. The provider verifies the
`revoke` against the registry in the same request, requires the rotation
and the absence of the target's envelope, and recomputes `active_devices`:
the revoked key's authentication ends at the same CAS that makes the
revocation current. Between the local commit and that CAS the provider
cannot know; the target can read only ciphertext it could already decrypt,
cannot delete anything, and a racing publish by it forces the revoker to
merge (its unseen revisions are refused, §3.2). A registry entry it
appends at the same seq is a fork (§4.6). The status is tracked as
`REMOTE_UPDATE_PENDING` with priority retry (§11.3.2);
`BACKUP_REVOCATION_FAILED` surfaces after 3 failed attempts.

### 11.5 Recovery lookup, download, restore, verification

**Recovery handle (owner decision; replaces the v0.3 email handle and
locators).** At setup the user chooses a non-secret recovery handle. Email
may be used as the handle if the user wishes, but SOURCE never requires an
email address. The handle is an identifier, **not** an authentication
factor: MP-only recovery needs the handle plus the MP and introduces no
second secret.

```text
h1 = NFKC(s) ; h2 = trim(h1) ; h3 = NFKC(to_lowercase(h2))      # Unicode default lowercase mapping
reject unless: 3 ≤ utf8_len(h3) ≤ 128, no Unicode White_Space, no Cc/Cf/Cs/Co/Cn code points,
               and NFKC(to_lowercase(h3)) == h3
handle_key = SHA-256("ov0/handle/v2" ‖ UTF-8(h3))
```

Confusables are not folded. Handles are unique per provider (§11.3.1) and
immutable in v1. The normalized handle is printed on the Recovery Key
sheet (§1.7).

**Privacy, stated honestly:** the handle is a deliberately **public
identifier**. `handle_key` is an unsalted deterministic hash, so a provider
or operator that sees the claim objects can dictionary-test predictable
handles (such as email addresses) and learn which have vaults. The pepper
below protects only the remote enumeration behavior for nonexistent
handles; it does not make stored handles opaque to the provider. The setup
UI says so. No OPRF or other privacy protocol is used.

**Locate (unauthenticated).** `POST /v2/recover/locate {handle_key}` (the
provider never receives the raw handle) returns exactly:

```json
{ "vault_id": "hex(16 B)",
  "kdf": {"alg":"argon2id","version":19,"m_kib":65536,"t":3,"p":1,"out_len":32,"salt":"hex(16 B)"},
  "auth_salt_mp": "hex(16 B)", "auth_salt_rk": "hex(16 B)" }
```

taken from the committed header. For a handle that does not resolve, the
provider returns the same shape with deterministic fake values
(`HMAC-SHA256(pepper, label ‖ handle_key)` truncated per field, the
allowlisted KDF tuple) and equalizes S3 reads best-effort; the next signed
recovery request then fails with the same generic `401` as a wrong MP/RK.
`pepper` is a 32 B provider-held secret with **no vault authority** —
enumeration mitigation only. Locate lookups are rate-limited per source IP
and per `handle_key` in memory per instance; they reveal only public
metadata.

**KDF-downgrade protection (normative).** Before any MP prompt or MP
derivation the helper:

1. parses the locate response strictly (exact keys, lowercase hex;
   `vault_id`, `kdf.salt`, `auth_salt_mp`, `auth_salt_rk` exactly 16 bytes);
2. requires `kdf` to equal an allowlisted tuple exactly (§2.3; in v1 only
   `argon2id`, version 0x13, `m_kib=65536`, `t=3`, `p=1`, `out_len=32`).
   Weaker, stronger, unknown or malformed values → `KDF_POLICY_VIOLATION`;
   recovery stops; nothing is derived and no MP prompt is shown. The
   helper uses its compiled-in parameters, never provider-supplied ones.
   RK recovery applies rule 1 to the fields it uses and applies rule 2 at
   the step where a new MP is set.

After authentication, once the fresh device has recovered the VK, verified
the checkpoint, anchored the registry and verified the manifest signature
(§4.8 order), it reads the **committed header blob** listed in the
verified index and requires `vault_id`, the entire `kdf` block and both
auth salts from the locate response to byte-equal it. A mismatch →
`RECOVERY_METADATA_MISMATCH`: recovery is aborted as provider tampering and
staging deleted; it is never silently accepted. Before the authenticated
header is obtained, a malicious provider can deny service but cannot make
the helper run a cheaper-than-policy MP derivation.

**Provider-wide recovery-authentication throttle (normative).** Adding
provider instances must not multiply the allowed MP guessing rate. The
shared throttle applies to the **MP class only** (owner decision on review
finding SEC-B3, amending S-5): an RK-derived key has 256-bit strength, so
throttling it protects nothing, and a shared RK pool would let anyone who
knows the public handle lock every total-loss recovery out indefinitely.
RK-class requests are subject only to the in-memory per-IP limits, so RK
recovery can never be blocked by a throttle. For every **MP-class**
request, before verifying its signature, the provider:

1. lists `v2/ratelimit/{vid}/recovery-mp/{window}/` (window =
   `floor(now / W)`, W = 3600 s; L = 10 slots; both tunable); if all L
   slots exist → `429 RECOVERY_THROTTLED` without verifying anything;
2. reserves the lowest free slot k with `PUT …/{k}` `If-None-Match: *`
   (`412` → try k+1; none free → `429`);
3. verifies the request: on success it deletes slot k (release); on failure
   the slot stays, counting one failed guess (generic `401`).

At most L MP-class verifications can fail per vault per window regardless
of the number of instances; a legitimate recovery consumes no slots; a crash
between reserve and release counts as one failure for that window;
fixed-window boundaries allow ≤ 2L across a boundary. Slots expire by
lifecycle after 2 days. MP-class requests naming a `vault_id` that
does not exist (including the fake ids of §11.5 locate responses) go
through the same slot mechanism under that id, so a full window's `429`
does not distinguish real vaults from fake ones.

**Residual denial of service (stated honestly):** an attacker who knows a
vault's public handle can keep refilling the MP-class window and so block
**MP-only** total-loss recovery for as long as the attack continues; RK
recovery is unaffected. The client reports "too many recovery attempts —
try again later, or recover with your Recovery Key", and provider
operators can intervene.

**Download and verification:**

1. `state_get` → verify the manifest signature under the anchored registry;
   generation must be ≥ this device's last-seen (lower → `MANIFEST_ROLLBACK`;
   equal generation with a different `manifest_hash` → fork evidence, §4.6;
   equal and identical → nothing to do); the registry must extend the
   accepted chain (§4.4).
   A device with no prior state follows the §4.8 order (recover VK → verify
   the current-VK checkpoint → require it to bind the served registry head
   and manifest → trust that registry), then the header cross-check above.
2. Fetch the index blob and every needed blob; verify each SHA-256 against
   its name and index line; hand the blobs to the helper through §1.3
   streams. The helper verifies AEAD lazily at decrypt time; corruption
   surfaces per record.
3. Missing/corrupt blob → retry ×3 → `BACKUP_OBJECT_MISSING`; a restore can
   proceed partially with the user told exactly how many records are
   unavailable (count only).
4. **Rollback:** generation regression → refuse and surface.
5. **Fork:** a manifest whose `prev_manifest_hash` is not the one this
   device accepted at that generation, or two validly signed manifests for
   the same generation → surface both (device names, generations,
   `created_at`, record counts); the user picks; nothing is deleted
   silently.
6. **Stale snapshot:** the backup is a floor, not a ceiling; a device's own
   newer local state is never replaced by an older remote one.

### 11.6 Metadata leakage and DoS assumptions (explicit)

The provider may learn: `vault_id`, the handle key (and by dictionary test
a predictable handle), device public keys and names (registry), object
counts and sizes, `record_id`s, `revision_id`s and the revision graph
(index), timing, IP addresses, generation cadence, nonce and throttle
activity. Mitigations: none claimed in v1 beyond TLS transport. The
provider may also delete or withhold everything: availability is out of
scope cryptographically; the product monitors backup success and warns
after 48 h without a successful publication (`BACKUP_STALE`). Devices keep
full local copies; a provider outage never blocks local unlock or fill.

### 11.7 Freshness: two honest classes (v0.2 finding 13)

- **Existing device with remembered state** (any enrolled device that has
  accepted a generation before): rollback is *detected and rejected* —
  the helper's persisted last-seen `(generation, state commitment)` refuses
  anything lower (§4.6). This guarantee is solid.
- **Fresh-device recovery after total device loss:** a brand-new device
  holds no remembered state. A malicious provider can serve an **older
  but still validly signed** state — every signature verifies, the
  registry chains, and nothing cryptographic distinguishes "latest" from
  "older valid". v1 does **not** claim rollback *detection* in this
  class. What v1 does instead:
  1. The recovery UI always displays the recovered state's
     `generation`, `created_at`, and item count: "This backup is from
     <date> and contains N items" — the user is the freshness oracle for
     whether that matches their memory.
  2. The printed recovery sheet (§1.7) carries `vault_id`, the manifest
     `generation`, an 8-hex prefix of the registry head hash as of
     printing/last RK rotation, the normalized handle and the provider
     origin. At recovery the user can compare: a served state *older than
     the sheet* is detectable; a state *newer than the sheet* is normal
     (backups advance); a mismatched `vault_id` or head prefix on an
     allegedly-old state is fork evidence.
  3. A recovered epoch transition binds the manifest it recovered from
     (§4.5), so a stale-state recovery is permanently visible in the
     registry to any device that later sees a newer history.
- Explicitly **not** added: a centralized Source freshness anchor, a
  transparency log, or hosted checkpoint infrastructure. Such a
  mechanism would be a separate owner-level design decision with its own
  trust analysis; it is not silently introduced here.

### 11.8 Total-loss recovery finalize (normative; `StateTransition` kind 3)

Total-loss recovery atomically replaces the provider's current state with a
new-epoch state built by a device the old registry does not contain. It
happens through exactly one transition kind; implementations must not
invent variants.

**Transport:** `POST /v2/vaults/{vid}/state` with a `StateTransition` of
`kind = 3` (§11.3), signed as a `ProviderRequest` by the recovery-auth key
of the class (`recovery-mp` or `recovery-rk`) registered in the state being
replaced. This is the **only** mutation a recovery-class key may authorize
besides `blob_put` of the re-encrypted state's blobs in RECOVERING.

**Body content:** `expected_state` = the current `state_commit`; `manifest`
= the new manifest (generation = old + 1, `vk_generation` = old + 1, every
referenced record, wrap and envelope already sealed under the fresh VK);
`checkpoint` = the §4.8 checkpoint for the state being installed, MAC'd
under the fresh VK; `recovery_auth_updates` for any class whose key
changed (§11.4 table). The new registry blob (listed in the index) is the
old registry + exactly one `recovery_epoch` + one named `revoke` per prior
active device, signed by the epoch's device (§4.4 S-4). v0.3's
`FinalizeBody` TLV and its tag `0x0B new_device_backup_credential` are
retired; no credential is carried.

**Provider validation** is §11.3 steps 1–10 with the `finalize` rules. The
provider cannot verify `recovery_proof` (its key derives from the old VK):
clients always verify proofs or the checkpoint themselves (§4.5, §4.8);
the provider's checks keep third parties and accidents from corrupting the
account.

**Atomicity.** The new manifest, registry, checkpoint, recovery-auth keys,
derived `active_devices` (exactly the new device) and the `finalized`
record commit in **one** CAS on the state object: all or nothing. A failed
finalize leaves no advanced registry, no half-installed device and no
changed recovery authority; orphaned pre-uploaded blobs are GC'd normally.

**Replay / idempotency.** `finalized[expected_old_generation]` stores the
SHA-256 of the committed body: a byte-identical replay → `200` with the
stored result; a **different** finalize body for an already-passed
generation → `422 FINALIZE_CONFLICT`. The nonce cache independently
rejects verbatim request replays.

**After success:** the recovery-auth keys revert to read + future-finalize
rights; the new device publishes under the device class; every prior
device is revoked (registry and provider); the epoch transition is
permanent registry history.

**Client side:** in RECOVERING the helper applies the §11.5 KDF policy
before prompting, recovers the old VK, verifies the checkpoint and header
cross-check, builds the `recovery_epoch` entry and the prior-device
revokes, generates a fresh VK, re-encrypts the vault under it (§2.10
re-seal rules, including `import_log` fingerprints), writes new MP/RK
wraps, and has the main process upload the new blobs. Only then does it
build the finalize transition, sign it, and hand it to main for transport.
On success the helper transitions RECOVERING → UNLOCKED (§13). The old VK
is zeroized once the re-encryption completes and is never used after
finalize. **There is no post-finalize VK rotation.** Other devices — all of
them now revoked — cannot read the provider any more; a Mac surfaces
`BACKUP_ACCESS_LOST` (§11.3.2), and a phone learns its revocation through
the §4.7 refresh once it is re-paired with a Mac of the recovered vault (the §4.7 route serves only already-paired devices).

---


## 12. Recovery

All scenarios share primitives: backup download (§11.5), registry epoch
rules (§4.5), VK rotation (§2.10), enrollment (§5). "Publish" = a §11.3
`publish` state transition; every security-relevant change is tracked by
the §11.3.2 remote-completion status and is not claimed effective remotely
before it commits.

### Scenario 1 — Mac lost, iPhone retained

**Requires a Mac until Phase F.2 (v0.4).** The steps below need an iPhone
vault engine (record store, rotation, re-encryption, publication), which
is Phase F.2 (§18). Until then, a user whose only surviving device is an
iPhone uses total-loss recovery (scenarios 3/4) on a new Mac, which revokes
every prior device, then re-enrolls the iPhone. (Revoking from another Mac
requires Mac-to-Mac enrollment, which is not specified in v0.4; Phase F
tests this path only with simulated devices, RC-01M.)

1. iPhone: Settings → Trusted Devices → Mac → Revoke (Face ID presence).
2. Helper-equivalent on iPhone appends signed `revoke` entry; VK rotation
   runs on iPhone (new VK, re-encrypt all records, new wraps for MP/RK/
   remaining devices, new state generation).
3. One `publish` transition commits the revocation; the provider
   deactivates the lost Mac's signing key at that CAS (§11.4), and rotation
   guarantees it cannot decrypt new state.
4. Replacement Mac: §5 enrollment, iPhone authorizes; new device envelope
   under the *new* VK.
5. Old Mac's envelope blob is GC'd per §11.2 retention.

### Scenario 2 — iPhone lost, Mac retained

Symmetric, on the Mac: revocation + rotation committed locally (both
recovery classes re-keyed, a new RK issued), then one `publish` transition
(§11.4); the iPhone's signing key is rejected by the registry and by the
provider from that commit (generic `401`, §11.4).

### Scenario 3 — all devices lost, master password retained

1. New supported device → Source installed → "Recover vault" → the user
   enters the **recovery handle** (non-secret; printed on the Recovery Key
   sheet, §1.7).
2. **Locate** (§11.5): `POST /v2/recover/locate {handle_key}` returns
   `vault_id`, the KDF block and the auth salts. The helper enforces the
   exact KDF allowlist **before** prompting for the MP
   (`KDF_POLICY_VIOLATION` otherwise), then the panel collects the MP,
   derives PK and the MP recovery-auth key (§11.4). A wrong MP (or a
   nonexistent handle) yields a generic `401` on the first signed request
   — never a decryption result — and counts against the provider-wide
   throttle (§11.5).
3. Download the state and blobs with recovery-class signed requests →
   unwrap VK locally → verify the current-VK checkpoint, anchor the
   registry, verify the manifest (§4.8 order) → require the locate
   metadata to equal the committed header (`RECOVERY_METADATA_MISMATCH`
   otherwise) → the recovery UI shows the state's generation, date and
   item count and offers the recovery-sheet comparison (§11.7) —
   freshness here is user-checked, not assumed.
4. Create a new device identity (SE keys) → build the `recovery_epoch`
   entry with `recovery_proof` (§4.5, keyed by the recovered old VK)
   binding the downloaded manifest hash — the entry itself installs the
   new device (§4.4 rule 6) — followed by one named `revoke` for every
   prior active device, signed by the new device (§4.4, S-4).
5. Generate a fresh VK (`vk_generation` = old + 1) → re-encrypt the
   current vault under it (§2.10 re-seal rules) → rebuild both wraps →
   zeroize the old VK. The MP wrap is re-sealed under the same MP (its
   PK is in hand). **The RK wrap can only be re-sealed by a party holding
   `RK_bytes`** (§2.5): the recovery UI offers to enter the existing
   Recovery Key — entered, it is kept; not entered, the helper **issues a
   new Recovery Key** (new `auth_salt_rk`), shows it in the §1.7 window,
   and the finalize transition carries the RK recovery-auth update. A
   vault is never left with a `recovery.wrap` of a retired VK.
6. Upload the new blobs, then commit the §11.8 finalize transition, which
   atomically installs the rotated manifest, the extended registry
   (epoch + revokes), the checkpoint and any recovery-auth updates in one
   CAS → enter UNLOCKED. Required order: KDF policy → recover old VK →
   create `recovery_epoch` and revokes → generate fresh VK → re-encrypt →
   upload blobs → finalize → UNLOCKED. Finalize never installs old-VK
   state, and nothing rotates after it.
7. The helper stores the normalized handle the user entered in `kv` (for
   later sheet reprints). Enroll the user's other replacement devices per
   §5; each becomes active at the provider when the authorizer's publish
   commits.

### Scenario 4 — all devices lost, Recovery Key retained

Identical to scenario 3 with the RK class. The 24-word RK is entered on the
new device; the checksum validates before any network call; the handle
comes from the sheet. Symmetrically, the MP wrap cannot be re-sealed
without PK, so this path **requires the user to set a new master password**
(new `kdf.salt` and `auth_salt_mp`, KDF allowlist applied); the finalize
transition carries the MP recovery-auth update. The entered RK is kept.

### Scenario 5 — MP forgotten, trusted device retained

1. Trusted device: fresh LA presence → helper decrypts nothing bulk; it
   re-wraps resident VK under PK′ = Argon2id(MP′, new salt).
2. `password.wrap`, `header.json` and the `pending_remote` record replaced
   in one §2.10 journaled commit (§11.3.2); header `kdf`
   salt and `auth_salt_mp` regenerated; a `publish` transition carrying
   the MP recovery-auth update is staged (§11.4).
3. Locally the old password wrap is destroyed in the same transaction;
   there is no local "both wraps work" window. **Remotely** the old MP
   still recovers the previous backup state until the publish commits
   (`REMOTE_UPDATE_PENDING`, §11.3.2); the UI says so.

### Scenario 6 — RK lost (no theft suspicion), trusted device retained

1. Fresh LA presence → the helper panel collects the **current MP**
   (the MP wrap must be re-sealed under the new VK and only PK can do
   that) → generate RK′ → show RK′ and require acknowledgement (§1.7) →
   **rotate VK** (v0.3 C12: re-wrap alone is insufficient) → re-encrypt
   records → new wraps for MP, RK′, all devices → new `auth_salt_rk` →
   `publish` carrying the RK recovery-auth update → print new recovery
   sheet (print path never renders RK words to the screen longer than the
   print dialog requires; the words are shown once, in a
   capture-suppressed window, §14).
2. The old RK stops working for the current state **locally at once and
   remotely only when the publish commits**; until then it can still
   recover the previous backup state, and the UI does not claim otherwise
   (§11.3.2).

### Scenario 7 — RK suspected stolen

Identical mechanics to scenario 6, plus: treated as a security incident and
**security-driven** for §11.3.2 — top-priority retry and a persistent
warning ("not yet cut off at your backup") until the remote cutoff commits.
UI banners on all enrolled Macs at their next sync, and on the iPhone at its next §4.7 refresh ("Recovery Key was replaced
on <date>; if this wasn't you…"), the backup retains the pre-rotation state
for 2 generations (§11.2), and the audit view lists the rotation event.
While the remote update is pending, whoever holds the old RK and the public
handle could run a total-loss recovery that revokes this device (§11.3.2);
this is inherent to RK possession until the cutoff commits.

**Retained limitation (must appear in product copy):** an attacker who
previously copied old ciphertext plus the old recovery.wrap can still
decrypt that historical snapshot with the old RK. Rotation protects the
current and future states; it cannot erase already-exfiltrated copies
(v0.3 §10.4, C12).

### Scenario 8 — device compromised while unlocked

1. Revoke the device from any surviving trusted **Mac** (iPhone-initiated
   revocation is Phase F.2) → VK rotation → both recovery classes
   re-keyed (§11.4) → one `publish` transition that deactivates the device
   at the provider and replaces any recovery-auth key it could have
   registered. Unseen revisions it authored are refused (§3.2).
2. The helper on the revoking device computes the **exposure set**:
   records whose plaintext was served to that device in the last N days
   (the helper keeps a local, non-secret audit log of `record_id` +
   timestamp + action for exactly this; default N = 30, log sealed under
   meta_key).
3. Product requires the user through a per-credential rotation checklist
   (change passwords at the account providers) for the exposure set, and
   recommends it for everything else. This is breach response, not
   recovery (v0.3 §9 honest limits apply).

---

## 13. Lock and runtime state machine

### 13.1 States

```text
UNINITIALIZED ──setup──▶ LOCKED ◀─────────────┐
                            │ unlock flow      │ lock / timeout /
                            ▼                  │ crash / sleep
                        UNLOCKING ──fail──▶ LOCKED
                            │ ok
                            ▼
                        UNLOCKED ◀────────────┐
                          │  ▲ per-op         │ op complete
                          ▼  └────────────────┘
                      AUTHORIZING (substate; VK resident)
UNLOCKED ──▶ SYNCING ──▶ UNLOCKED      (helper-side merge)
UNLOCKED ──▶ BACKING_UP ──▶ UNLOCKED   (helper seals; main uploads)
LOCKED ──▶ BACKING_UP ──▶ LOCKED       (finish a publication fully staged while unlocked)
UNLOCKED ──▶ ROTATING_KEYS ──▶ UNLOCKED (internal; reported by rotation_progress)
UNINITIALIZED/LOCKED ──▶ RECOVERING ──▶ UNLOCKED (fresh-VK re-encryption happens
                                      inside RECOVERING, before finalize)
any ──fatal──▶ ERROR ──retry/restore──▶ LOCKED
registry fork / equivocation / confirmed vault tamper ──▶ COMPROMISED
                                      (writes frozen; exit not specified in v0.4, §4.6)
```

**v0.4 (Phase F):** BACKING_UP, SYNCING, RECOVERING and COMPROMISED are
real `VaultState` values with the transitions above; long-running provider
work is not hidden inside the other states. **ROTATING_KEYS is internal**
(owner decision O-2): it is a real state for op gating, reported to the UI
by a `rotation_progress` status event rather than presented as a vault
state. The §11.3.2 remote-completion status is an operation status, not a
state: it outlives lock and restart and coexists with every state.

### 13.2 State table

| State | VK | MP/RK | Decrypted records | IPC ops allowed | Network (main) | UI |
|---|---|---|---|---|---|---|
| UNINITIALIZED | none | none | none | `setup_vault`(panel), `recovery_begin`, `begin_enrollment`(first-device) | recovery locate | setup wizard only |
| LOCKED | none | none | none | `unlock*`, `get_state`, `fill_candidates`(→`locked`), `approval_result`, `sign_provider_request` (`state_get`/`blob_get`; `blob_put`/`state_commit` only for a fully staged publication, §11.4 table), `recovery_begin` | backup poll; staged publication | unlock prompt |
| UNLOCKING | transient (unwrap in flight) | transient during entry | none | only the in-flight op | — | spinner, cancel |
| UNLOCKED | resident (helper only) | never resident | never resident as a set; per-record transient during an op | all §1.5 ops | sync/backup ok | full vault UI |
| AUTHORIZING | resident | never | the one approved record, transient, post-approval | the in-flight authorize op only for that request_id | approval relay | presence prompt / phone sheet |
| SYNCING | resident | never | none (ciphertext merge; decrypt only for duplicate-id comparison, §3.2) | reads blocked ≤ 5 s, presence ops continue; `backup_state_offer`/stream/`backup_apply` | backup download | sync badge |
| BACKING_UP | resident while staging; not needed once fully staged | never | none (seal only) | all (the staged state is consistent) plus the publication ops | upload + state transition | backup badge |
| ROTATING_KEYS (internal) | old+new transient, old zeroized at flip | never | per-record transient re-seal | fill ops queue ≤ 30 s; high-risk ops refused | publish staged at end | `rotation_progress` status |
| RECOVERING | old VK transient post-unwrap; fresh VK generated before finalize; old zeroized once re-encryption completes | MP/RK themselves only during entry; derived material held until named points: `PK` (MP path) or `RK_bytes` (RK path) until the new wraps are sealed; the recovery-auth key `sk_c` until the finalize transition commits or the session aborts; a kept RK's `RK_bytes` until `recovery.wrap` is re-sealed. None is persisted; all are zeroized on commit, abort, lock or timeout | verify + per-record transient re-seal | recovery ops only (`recovery_*`, streams, `sign_provider_request` for recovery classes and finalize, §11.4 table) | download, then upload of re-encrypted blobs and the finalize transition | recovery wizard |
| ERROR | none (zeroized on entry) | none | none | `get_state`, `lock`, restore ops | restore download | error + restore path |
| COMPROMISED | unchanged but writes frozen | none | reads allowed, writes frozen | read ops, `get_state`, `lock` | reads only | both tips surfaced; exit procedure not specified in v0.4 (§4.6) |

### 13.3 Global rules

- **Unlock ≠ authorization.** UNLOCKED means VK residency only. Every
  credential release additionally passes through AUTHORIZING with one-shot
  user presence (§6). There is no grace window in v1 (v0.3 §6.5 default).
- Transition triggers: as drawn above; any unexpected IPC op for the
  current state → `BAD_STATE` error, no side effects.
- Timeout behavior: UNLOCKING/RECOVERING ops abort after 120 s →
  LOCKED/ERROR; AUTHORIZING expires with the challenge (120 s);
  ROTATING_KEYS, and the fresh-VK re-encryption phase inside RECOVERING,
  have no timeout but are resumable-idempotent after crash (§2.10).
- Crash behavior: process death in any state → LOCKED on next start
  (except interrupted rotation, which resumes; interrupted recovery,
  which restarts from downloaded state re-verification with a newly
  generated fresh VK, with blobs uploaded by the abandoned attempt
  unreferenced and GC'd; a persisted `pending_remote` publication, which is
  resumed at launch with priority, §11.3.2; and COMPROMISED, which is
  re-entered at open).
- Lock during BACKING_UP or SYNCING: a publication still being built is
  aborted and its staging deleted; a fully staged one continues without
  the VK; a sync is aborted (its apply is one DB transaction, so nothing is
  half-applied). Lock during ROTATING_KEYS is deferred until the journal
  commits or rolls back.
- Zeroization on every transition into LOCKED/ERROR and at the end of
  every transient use (§2.11).

---

## 14. Source capture isolation

Built on the mechanisms already landed in this repository (hardening
phase, commits fd659c3/d96d51c and the smoke harness in 47f6e3d). This
section defines how vault surfaces use them; it does not re-specify the
mechanisms.

### 14.1 Mechanism inventory (existing)

| Mechanism | Location | Behavior |
|---|---|---|
| Exclusion registry | `core/capture_exclusions.rs` | `register_excluded_window_title` / `unregister_excluded_window_title` / `is_excluded_window_title` |
| Sensitive-surface counter | same | `sensitive_surface_shown` / `sensitive_surface_hidden` / `sensitive_surface_visible` |
| Frame suppression | `core/screen_recorder/lifecycle.rs` | recording loop drops frames and one-shot `capture_frame` refuses while `screen_capture_suppressed()` |
| Keyboard suppression | `platform/input/keyboard_macos.rs` | `should_record_keystroke` choke point + `IsSecureEventInputEnabled` FFI gate |
| Vault path exclusion | `tauri.conf.json` + `core/asset_scope_guard.rs` + `core/vault_dir.rs` | asset scope `recordings/**` + explicit `!vault/**` deny; dir `0700` + `.metadata_never_index`; runtime probe in `src/dev/smoke.ts` |
| Tauri commands | `app/commands/capture_exclusions.rs` | `register_capture_excluded_window`, `unregister_capture_excluded_window`, `sensitive_capture_surface_changed`, `capture_suppression_active` |

### 14.2 Vault surface obligations

| Surface | Registration duty |
|---|---|
| Vault management window (unlock, item list, add/edit, reveal, recovery sheet) | window title registered as excluded at creation, unregistered at destroy; during reveal/edit/print steps the surface counter is additionally incremented |
| Dashlane import wizard (all steps) | window registered + counter incremented for the whole wizard |
| Setup wizard (MP creation, RK display/print) | same as import wizard |
| Helper secure panel (§1.7: MP/RK entry, confirm, print) | counter incremented for the full panel lifetime via the `secure_panel_visible` event; panel window title registered at creation; all MP/RK fields are native secure text fields; close decrements the counter and zeroizes inputs |
| iPhone approval sheet on Mac | registered window |
| Chrome extension popup | not a Source window — protection comes from never rendering secrets in-page (passwords go into masked fields only); the fill itself happens in Chrome's window, which Source's recorder can see — accepted: filled passwords render as dots; reveal-in-page is not offered |
| Keyboard: all vault password/MP/RK fields | `secureInput` class → `IsSecureEventInputEnabled` suppression + password-field heuristics; RK/MP entry uses native secure text fields |

### 14.3 Interaction rules

- **OCR / indexing / timeline:** suppressed frames never reach OCR, so
  indexing and timeline exclusion follows from §14.1 frame dropping.
  Additionally the vault directory never appears in any search/export
  root (storage-layer exclusion, v0.3 §7).
- **One-shot screenshot APIs:** `capture_frame` refuses while any
  sensitive surface is visible (already implemented); vault UI calls
  `sensitive_capture_surface_changed(true)` before rendering secret
  material.
- **Secure event input:** when macOS reports secure input enabled (any
  app), keystroke recording suppresses; when a Source vault field has
  focus the surface counter is also up, so both signals suppress.
- **ScreenCaptureKit migration:** when the recorder migrates from
  `CGDisplay.image` to SCK per-window filters, vault windows must
  additionally be excluded by window ID in the content filter; until then
  global suppression is the normative control and this spec's fail-closed
  rule applies to it.

### 14.4 Fail-closed behavior (normative)

Before any **display-class** secret release (`reveal`, RK display,
import report, add/edit form prefill), the helper queries the main app:
`sensitive_capture_surface_changed` state must show the requesting
surface registered and visible, i.e. `capture_suppression_active()` =
true. If the check fails or times out (500 ms), the helper returns
`CAPTURE_UNSAFE`; the UI shows "Screen-capture exclusion is unavailable,
so Source won't display this right now." Fill-class releases (password
into Chrome's masked field) do not require the Source-side check — the
secret never renders in a Source window — but the extension still never
reveals in-page (§14.2). This is a code path, not a comment: no display
without the check, and the check's failure default is refusal.

---

## 15. Error handling

Universal rules: fail closed; errors carry a stable code + human-safe
context; no secret values, usernames, URLs (beyond the origin a user is
actively approving), VK/RK/MP fragments, or serialized secret objects in
any log, panic message, telemetry, or error string. `Debug` is not
implemented for secret-bearing types (§2.11); the helper's log filter
allowlists module paths rather than denylisting content.

| Condition | Code | Behavior | User sees |
|---|---|---|---|
| Wrong master password | `WRONG_CREDENTIAL` | AEAD failure on unwrap; 500 ms delay ×2^attempts backoff, capped 30 s; no counter oracle beyond attempts | "Incorrect password" |
| Wrong Recovery Key | `RECOVERY_KEY_INVALID` (checksum) / `WRONG_CREDENTIAL` (AEAD) | checksum checked offline first | "Not a valid recovery key" |
| Corrupted record | `RECORD_CORRUPT` | quarantine record, continue; restore from backup offered (peer copy from Phase F.2) | "One item is damaged and can be restored" |
| Malformed object / format | `FORMAT_INVALID` | refuse; never a best-effort parse (§3.7, SY-07) | generic failure copy |
| Local records ≠ manifest, or served state inconsistent | `MANIFEST_MISMATCH` | refuse; force the restore/verify path (§3.5, §11.5) | "Vault data failed verification" |
| Backup blob missing or corrupt after retries | `BACKUP_OBJECT_MISSING` | partial restore with an exact count (§11.5) | "N items could not be restored" |
| Corrupted wrap | `WRAP_CORRUPT` | that wrap path disabled; other wraps unaffected | "This unlock method is damaged — use another" |
| Corrupted database | `DB_CORRUPT` | ERROR state; never auto-delete; restore flow offered | restore wizard |
| Unknown/future format (object magic, wrap kdf_version, TLV entry_version) | `FORMAT_TOO_NEW` | refuse; never a best-effort parse (§3.5, §3.7) | "This data was written by a newer version of Source — update the app" |
| Display-class release with capture suppression unverifiable | `CAPTURE_UNSAFE` | §14.4 refusal; nothing rendered | "Screen-capture exclusion is unavailable, so Source won't display this right now" |
| Invalid AEAD tag (any) | `INTEGRITY_FAILURE` | refuse the operation; counts toward tamper signal | generic failure copy |
| Invalid signature | `SIGNATURE_INVALID` | reject object; if registry/manifest → COMPROMISED flow | "Vault data failed verification" |
| Registry fork | `REGISTRY_FORK` | COMPROMISED; writes frozen | fork resolution UI (§4.6) |
| Truncated registry | `REGISTRY_TRUNCATED` | reject; fetch full chain | sync error copy |
| Manifest rollback | `MANIFEST_ROLLBACK` | reject; keep local | "Backup is older than this Mac's vault" |
| Stale backup | `BACKUP_STALE` | warn after 48 h without publish | settings warning |
| Provider state moved (another device committed first) | `STATE_MOVED` | provider `409`; helper syncs, merges and re-stages (§11.3), ≤ 3 attempts | none |
| Conflicting device states | `BACKUP_CONFLICT` | surfaced after 3 failed `STATE_MOVED` merges (§11.3) | "Two devices changed the vault — review" |
| Helper unavailable / crash | `HELPER_UNAVAILABLE` | §1.6 restart policy | "Vault is restarting…" |
| Phone unreachable | `PHONE_UNREACHABLE` | offer §6.C fallback | "iPhone unavailable — use Mac password" |
| Phone approval timeout | `APPROVAL_EXPIRED` | cancel request | "Approval expired" |
| Invalid phone signature | `SIGNATURE_INVALID` | reject; log device_id (not secret) | "Approval couldn't be verified" |
| Expired approval delivery | `APPROVAL_EXPIRED` | reject | retry affordance |
| Extension disconnected | `EXTENSION_LOST` | pending nm requests cancelled; fill not delivered | none (page-side timeout copy) |
| Backup provider unavailable | `BACKUP_UNAVAILABLE` | queue publication, backoff 1→5→15 min, warn at 48 h | settings badge only |
| Provider request replay (reused nonce) | `BACKUP_REPLAY` | request refused (a timestamp outside ±300 s is `AUTH_INVALID`); counts toward tamper signal | none (diagnostics only) |
| Revocation not yet committed at the provider | `BACKUP_REVOCATION_FAILED` | the revocation publish (§11.4) failed 3 times; retries continue with priority; status stays `REMOTE_UPDATE_PENDING` (§11.3.2) — revocation is security-boundary, not best-effort | "Not yet cut off at your backup: <device name> — keep this Mac online" |
| Secure panel dismissed/cancelled | `PANEL_CANCELLED` | operation aborts; panel inputs zeroized; no partial state | panel closes, no error copy |
| Record sync conflict pending | `CONFLICT_PENDING` | both revisions kept; `tip_rev` is NULL so the record is excluded from fill candidates until resolved (fail closed, no silent pick); `resolve_conflict` writes a merge revision | "Needs review" badge on the item |
| Provider-request signing refused (state/class/operation/body-hash violation) | `SIGNING_REFUSED` | no signature produced; attempt pattern logged as tamper signal | none (caller treats as terminal) |
| Recovery-finalize conflict (different body for a passed generation) | `FINALIZE_CONFLICT` | nothing mutates; idempotent success only for a byte-identical replay of the committed finalize (§11.8) | recovery wizard retries with a fresh state |
| Keychain / Secure Enclave unavailable for background signing | `KEYCHAIN_UNAVAILABLE` | queue and retry; never prompt; never change ACLs (§1.6) | none (status only) |
| Stream transfer violation or abort | `TRANSFER_INVALID` / `TRANSFER_ABORTED` | partial data deleted; the session op fails; retried by the coordinator (§1.3) | none |
| Recovery handle already claimed | `HANDLE_TAKEN` | provider `409` at `create` (§11.3.1) | "That recovery handle is taken — choose another" |
| Locate KDF parameters outside the allowlist | `KDF_POLICY_VIOLATION` | recovery stops before any MP prompt or derivation (§11.5) | "The backup service returned unsupported settings — recovery stopped" |
| Locate metadata ≠ committed header | `RECOVERY_METADATA_MISMATCH` | recovery aborted as provider tampering; staging deleted (§11.5) | "The backup service returned inconsistent data — recovery stopped" |
| Recovery throttled | `RECOVERY_THROTTLED` | provider `429` from the shared MP-class throttle (§11.5) | "Too many recovery attempts — try again later, or recover with your Recovery Key" |
| Recovery-auth update inconsistent with header | `RECOVERY_AUTH_STALE` | provider `422`; transition refused (§11.3 step 7); indicates a client bug | none (diagnostics) |
| Counter regression in an incoming revision | `COUNTER_REGRESSION` | revision rejected, counted as tamper evidence (§3.2) | none (tamper count only) |
| Unseen revision from a revoked author | `REVOKED_AUTHOR_REFUSED` | refused and counted (§3.2) | "N changes from removed device X were not accepted" |
| This device's own provider key no longer accepted | `BACKUP_ACCESS_LOST` | three consecutive `401`s on its own `state_get` ≥ 5 min apart while locate answers (§11.3.2); never treated as revocation; nothing deleted | "Backup access lost — this Mac may have been removed, or your vault may have been recovered on another device" |
| Failed VK rotation | `ROTATION_FAILED` | §2.10 resume-or-rollback to old manifest; never a half-live mix | "Security update didn't finish — retrying" |
| Interrupted recovery | `RECOVERY_INCOMPLETE` | re-verify downloaded state on retry; idempotent | recovery wizard resumes |
| Partially completed import | `IMPORT_PARTIAL` | committed rows stay; report shows exact counts; re-import is idempotent | import report |

**User-facing vs internal:** the user sees the copy column; internal
diagnostics record code + state + counters. Crash reports from the helper
are disabled entirely (no crash reporter is linked); the main app's
diagnostics never include helper frames.

---

## 16. Security test plan

Every test below has a binary pass/fail. IDs are stable; the §19 gate
references them. Rust tests live in the helper crate; Swift tests in the
iOS repo; shared vectors in `src-tauri/vault-helper/tests/vectors/`.

### 16.1 Cryptography

| ID | Test | Expected |
|---|---|---|
| CR-01 | MP wrap → unwrap round trip | RecoveryWrapPayload recovered byte-identical |
| CR-02 | RK wrap → unwrap round trip | same |
| CR-03 | Wrong MP (1-bit flip) | AEAD failure, `WRONG_CREDENTIAL`, no partial state |
| CR-04 | Wrong RK (valid checksum, wrong entropy) | same |
| CR-05 | Record ciphertext 1-byte tamper | `RECORD_CORRUPT`, no plaintext |
| CR-06 | AAD tamper (record_id, generation, schema swapped) | decryption fails |
| CR-07 | Nonce strategy audit: 2²⁰ seals, no nonce/key pair repeats | unique (statistical guard; construction uses random 192-bit) |
| CR-08 | VK rotation: full vault re-seal | old VK fails on every new record/wrap (`INTEGRITY_FAILURE`); new VK opens all |
| CR-09 | KDF downgrade attempt (header kdf_version-1) | refused |
| CR-10 | Wrap header tamper (salt/params swapped between files) | AEAD failure |
| CR-11 | Zeroization spot test: after lock, helper heap sampled for VK/RK sentinel patterns | not found (test build with canary secrets) |
| CR-12 | (v0.4) no payload of any kind carries backup/provider credential material, before and after rotation | `DeviceEnvelopePayload` v2 and both `RecoveryWrapPayload`s decode to exactly {vk, wrapped_at, vk_generation}; tag 0x04 rejected; no `creds.bin` exists |
| CR-13 | (v0.4) recovery-auth key derivation (§11.4) | (a) RFC 9180 Appendix A.3 `ikmE→skEm/pkEm`, `ikmR→skRm/pkRm` reproduced by our `DeriveKeyPair`; (b) XV-RECOVERY-AUTH composition vectors byte-exact for MP and RK; (c) the rejection loop exercised with an injected candidate source; (d) `sk_c`/`ikm_c` never persisted or sent over IPC (storage scan + transcript scan) |

### 16.2 Registry

| ID | Test | Expected |
|---|---|---|
| RG-01 | Unsigned enroll entry appended | rejected, chain unchanged |
| RG-02 | Forged enroll (attacker key) | `SIGNATURE_INVALID` |
| RG-03 | Forged revocation of a live device | rejected (authorizer not enrolled/unauthorized) |
| RG-04 | Truncated chain (last 2 entries removed) | `REGISTRY_TRUNCATED` |
| RG-05 | Fork (two valid tips, same prev_hash) | `REGISTRY_FORK`, writes frozen, both tips surfaced |
| RG-06 | Rollback (older head presented) | rejected via persisted head |
| RG-07 | Wrong authorizer (device signs for another) | rejected |
| RG-08 | recovery_epoch: bad proof (wrong VK) **or** any bound field altered — vault_id, prev_hash, manifest_hash, prior_epoch, epoch, new device_id/keys, recovery_nonce | rejected in every case |
| RG-09 | recovery_epoch binding stale manifest after devices saw newer | rejected on those devices; recovery UI shows bound generation |
| RG-10 | TLV canonicalization: reordered tags, padded integers, non-NFC strings | decode rejects or normalizes deterministically — byte-identical re-encode required |
| RG-11 | any self-signed enroll other than genesis | rejected (v2 has no epoch-start enroll; §4.4 rule 6) |
| RG-12 | 33-byte or off-curve device keys in any entry | rejected (§4.4 rule 8) |
| RG-13 | recovery_epoch with substituted `sign_pub`, same `device_id` | proof invalid → rejected |
| RG-14 | recovery_epoch with substituted `agree_pub`, same `device_id` | proof invalid → rejected |
| RG-15 | recovery_epoch with altered `platform`/`device_name` | proof invalid → rejected |
| RG-16 | replayed old recovery transition (superseded epoch/manifest_hash) | rejected (epoch + manifest binding; §4.6 rollback) |
| RG-17 | valid recovery_epoch | installs the device; the next registry entry verifies under the bound `sign_pub` |
| RG-18 | (v0.4, S-4) total-loss finalize registry: epoch followed by revokes of every prior active device, signed by the epoch device | accepted only with the complete set in ascending order; missing/extra/unsigned revoke → `REGISTRY_INVALID`; afterwards the only active device is the new one |

### 16.3 Device approval

| ID | Test | Expected |
|---|---|---|
| DA-01 | Correct iPhone signature, all fields match | one fill released |
| DA-02 | Wrong iPhone (enrolled other / unenrolled) | `DEVICE_NOT_AUTHORIZED` |
| DA-03 | Approval naming a different Mac | `APPROVAL_MISMATCH` |
| DA-04 | Approval origin ≠ pending origin | `APPROVAL_MISMATCH` |
| DA-05 | Expired challenge (t > exp) | `APPROVAL_EXPIRED` |
| DA-06 | Replayed approval (same nonce) | `APPROVAL_REPLAY` |
| DA-07 | Duplicate delivery while pending | idempotent single release |
| DA-08 | Modified challenge (1-bit flip pre-signature) | `SIGNATURE_INVALID` |
| DA-09 | iPhone revoked mid-request | reject |
| DA-10 | Offline phone → timeout | fallback offered; no fill |
| DA-11 | Card fill with yesterday's password-fill presence | fresh presence demanded (`fill_card` action) |
| DA-12 | `update_password` approval without `credential_ref` | rejected at decode/verify (§6.5 presence rule); nothing applied |

### 16.4 Browser security

| ID | Test | Expected |
|---|---|---|
| BR-01 | exact `https://github.com` | candidate listed |
| BR-02 | `app.github.com` vs `github.com` (exact entry) | no autofill; "similar item" hint |
| BR-03 | same with `match:"domain"` entry | eligible |
| BR-04 | `github.com.evil.example` | no match (PSL boundary) |
| BR-05 | `xn--80ak6aa92e.com` (аррӏе-style lookalike) vs `apple.com` | refuse + warning |
| BR-06 | confusable skeleton collision (`g00gle.example`) | refuse + warning |
| BR-07 | stored https, page http | refuse (except localhost+`allow_http`) |
| BR-08 | cross-origin iframe fill attempt | only matching frame filled |
| BR-09 | opaque origin (`data:` URL tab) | refuse |
| BR-10 | replay consumed `request_id` | `APPROVAL_REPLAY` |
| BR-11 | origin/tab_url disagreement (extension lies about one) | reject |
| BR-12 | candidate query rate > 20/min | rate-limited |
| BR-13 | request for `ref` belonging to another origin | `APPROVAL_MISMATCH` (origin bound at authorize time) |
| BR-14 | locked vault | `{locked:true}`, no candidates |
| BR-15 | PSL drift: `github.io`, `co.uk`, `appspot.com` corpus | identical decisions Rust vs JS |
| BR-16 | `save_update` without trusted confirmation | held pending; stored record unchanged |
| BR-17 | update approved via helper LA sheet naming origin + account | applied as new revision; old password in `password_history`; sheet displayed no password |
| BR-18 | update approval with mismatched origin/ref (iPhone action 5) | `APPROVAL_MISMATCH` |
| BR-19 | replayed update approval | `APPROVAL_REPLAY` |

### 16.5 Capture safety

| ID | Test | Expected |
|---|---|---|
| CS-01 | vault window visible during recording | frames dropped (lifecycle log shows suppression) |
| CS-02 | vault content never appears in OCR corpus/index/search/timeline | corpus grep for canary strings: absent |
| CS-03 | typing into vault secure fields with keyboard recorder on | zero events |
| CS-04 | typing into Chrome/1Password/Safari password fields (secure input on) | zero events |
| CS-05 | ambiguous focus context | suppresses (fail toward not recording) |
| CS-06 | reveal requested with suppression unverifiable | `CAPTURE_UNSAFE`, nothing displayed |
| CS-07 | import wizard start → end | counter up for entire wizard; report view still suppressed |
| CS-08 | asset protocol: vault path fetch at runtime | rejected (existing smoke probe, wired into CI) |

### 16.6 Backup

| ID | Test | Expected |
|---|---|---|
| BK-01 | full publish → fresh-device restore | byte-identical vault after unwrap |
| BK-02 | object bit-flip in store | hash mismatch → `BACKUP_OBJECT_MISSING`, partial-restore path with exact count |
| BK-03 | object deleted | same |
| BK-04 | manifest generation -1 presented | `MANIFEST_ROLLBACK` |
| BK-05 | forked manifests (prev-hash mismatch against the accepted chain, or two signed manifests for one generation) | both surfaced; user picks; nothing deleted silently |
| BK-06 | stale remote state + fresher local state | local state kept; backup is a floor only |
| BK-07 | provider down at publish | queued, backoff, local vault fully usable; `REMOTE_UPDATE_PENDING` where applicable |
| BK-08 | interrupted upload before the state transition | orphan blobs GC'd after the 7-day grace; no committed state references a missing blob |
| BK-09 | CAS race (two publishers) | loser gets `STATE_MOVED`, merges and republishes at the next generation |
| BK-10 | historical snapshot: old RK + old wrap + old object bytes after rotation | old snapshot still decrypts (documents the §12 scenario 7 limitation as tested behavior); **new** state refuses old RK |
| BK-11 | provider attempts manifest signed by non-enrolled key | devices reject regardless of server acceptance |
| BK-12 | captured signed mutating request replayed verbatim (same nonce, in-window), including against a second provider instance | `BACKUP_REPLAY` |
| BK-13 | a revoked device's key used after the revoking transition commits | refused before any operation executes with the generic `401 AUTH_INVALID`; the client does not treat it as revocation; three such failures surface `BACKUP_ACCESS_LOST` |
| BK-14 | recovery-class key posts a `publish` (`state_commit` kind ≠ 3) | provider `403 DEVICE_NOT_AUTHORIZED`. (Recovery-class `blob_put` is permitted by the provider and policed by the helper's state check only — PR-05 — bounded by GC and the size caps) |
| BK-15 | finalize activates the epoch device's `sign_pub`; its later publishes pass; the recovery key's publish is refused | as specified |
| BK-16 | MP change / RK replacement carry the recovery-auth update in the same transition; the salt rule (§11.3 step 7) is enforced | old recovery key fails and new succeeds only after the commit; a mismatched update → `RECOVERY_AUTH_STALE` |
| BK-17 | concurrent publishers | no blob or state overwrite; exactly one commit per generation; loser merges |
| BK-18 | with canary MP, PK, RK, VK, `ikm_c` and `sk_c` values planted in a synthetic flow, dump provider state, blobs, nonces, handles and rate-limit slots | none of the canaries appears; only public keys, ciphertext and public metadata |
| BK-19 | a publish appending `revoke` without the `vk_generation` bump, or with the target's `env` | `422` |
| BK-20 | byte-identical replay of a committed transition (lost response) | `200` with the stored result; no second commit |
| BK-21 | GC never deletes a reachable blob; a GC race | `BLOB_MISSING` → re-upload → commit |
| BK-22 | fake locate responses for unknown handles | same shape and allowlisted KDF tuple as real ones; next step fails with the same `401` |
| BK-23 | `create` bootstrap: `blob_put` for a `{vid}` without state; bootstrap set ≠ index references; a `create` failing authentication or validation | `401`; `422`; nothing written in either failure case |
| BK-24 | a revoking `publish` missing either recovery-class update, or re-sealing `password.wrap` with a `kdf` block ≠ the header's | `422 RECOVERY_AUTH_STALE`; a rotation re-sealing `password.wrap` under the same PK without an mp update is accepted |
| BK-25 | a header or recovery flow naming a provider origin outside the helper's compiled-in allowlist | refused before any request is signed |
| BK-26 | singleton merge: pending MP change vs a committed MP change; pending MP change vs a committed RK-only change; pending revocation vs a committed rotation; pending RK replacement vs a committed revocation | base unchanged → re-staged; base changed → `needs_user`, re-collection required, and no wrap of a retired VK or of a superseded secret is ever published; "keep the other device's MP" offered only when `kdf.salt` moved; after adoption nothing is frozen, no false fork is raised, and this device's unpublished revisions survive — including after a crash mid-adoption; "Master password changed on <device>" shown on adoption |
| BK-27 | records across a concurrent rotation | revisions published under the retired VK are left out by the rotator and re-sealed and re-published by their author; nothing is lost |
| BK-28 | `HANDLE_TAKEN` at first `create` (and a second `HANDLE_TAKEN` on retry) | `setup_retry_handle` collects the MP, issues a new RK and sheet, rotates the VK in one journaled commit (crash at any point leaves the old or the new state, never a mix), re-stages; the new handle resolves; the first attempt's provider-stored `recovery.wrap` opened with the old RK yields only the retired VK, which opens nothing in the committed state; local records survive |

### 16.7 Recovery scenario tests

RC-02…RC-08 execute §12 scenarios 2–8 end-to-end against
`vault-provider-core` over `FsStores` (via the in-process transport) with
simulated Mac devices, asserting: final vault contents equal, registry
chains valid, revoked/old material fails everywhere it must (BK-10
pattern), S-4 revokes present after RC-03/RC-04, exposure-set log correct
in RC-08, remote-completion copy never claims a cutoff before commit, and
all user-facing copy steps occur in order. **RC-01 (scenario 1,
iPhone-initiated) is deferred to Phase F.2** (§18); Phase F runs a Mac
variant, **RC-01M** (Mac A lost; simulated Mac B revokes it, rotates and
publishes), and RC-01 must be green before Phase J.

**Phase F additions (v0.4):**

| ID | Test | Expected |
|---|---|---|
| HC-01 | crash after handle claim (C2), before state create | the same vault's retry completes; after G another vault can reclaim; within G it gets `409` |
| HC-02 | two concurrent `create`s for one handle across two provider instances | exactly one claim; the other `409 HANDLE_TAKEN` |
| HC-03 | lost `create` response | retry → `200`, same `state_commit`, claim `bound`, no second state |
| HC-04 | crash after C3, before bind | retry binds; another vault's reclaim is refused (claim live) |
| HC-05 | reclaim racing the owner's late retry | exactly one wins; the loser's generation-1 state is rolled back; it gets `409` |
| RU-01 | MP change while the provider is unavailable | `REMOTE_UPDATE_PENDING` persists across lock/restart; old MP still authenticates remotely; copy never claims the cutoff; after commit the old MP is refused |
| RU-02 | security-driven RK replacement while offline | persistent warning; priority retry; old RK recovers remotely until commit, then fails |
| RU-03 | crash between local commit and publish | status restored at launch; staged transition resent; idempotent |
| RU-04 | provider `5xx` ×3 during a pending revocation | `BACKUP_REVOCATION_FAILED` surfaced; retries continue; status stays pending |
| RU-05 | pending transition needs a merge while LOCKED | waits for unlock; warning stays |
| KD-01 | locate returns weaker Argon2 parameters (m, t, p, version, out_len) or wrong-length salts/ids | `KDF_POLICY_VIOLATION`; no MP prompt and no derivation (asserted by panel and KDF call counters) |
| KD-02 | locate returns stronger or unknown parameters | refused (allowlist only) |
| KD-03 | locate metadata differs from the authenticated committed header | `RECOVERY_METADATA_MISMATCH`; recovery aborted; staging deleted |
| RL-01 | MP-class recovery-auth failures spread across two provider instances sharing one store | combined failures per vault per window ≤ L; the next MP attempt → `429` without verification |
| RL-02 | a legitimate recovery with many successful recovery-class requests | consumes no slots; completes |
| RL-03 | crash between slot reserve and release | counts as one failure for that window only |
| RL-04 | an MP-class window is full (attacker refilling it) | RK-class recovery for the same vault still completes; RK requests consume no slots |
| EV-01 | published envelope set | equals the active device set in every state |
| EV-02 | Mac offline during a rotation | fetches and opens its new envelope from the provider |
| EV-03 | iPhone envelope catch-up (§4.7), **on a physical A15+ iPhone** (SE required); the gate checks these tests ran by name | steps 1–8 pass in the §4.8 order; the phone verifies the checkpoint binding to the manifest core hash; a provider-served revoke naming the phone is not acted on destructively; floors raised |
| EV-04 | revoked device | gets nothing openable; provider `401`; no key material deleted on `401`/`403` |
| EV-05 | envelope missing without a revoke entry | "unable to verify"; nothing deleted |
| TR-01…TR-08 | §1.3 streams: cross-session read refused; write off the need list refused; out-of-order chunk; SHA-256 mismatch; oversize; cancel cleanup; disconnect cleanup; restart sweep | `TRANSFER_INVALID`/cleanup as specified |
| TR-09 | enrollment bundle larger than 64 KiB | delivered via a stream session |
| ST-01…ST-05 | §13 transitions for BACKING_UP, SYNCING, ROTATING_KEYS, RECOVERING, COMPROMISED, including lock during each | as §13.3 |

### 16.8 Cross-language vectors

Generated by a Rust CLI (`vault-helper/src/bin/gen_vectors.rs`) into
JSON+hex files; consumed by Rust tests and Swift XCTest:

| Vector family | Contents |
|---|---|
| XV-ECDSA | P-256 verify of **randomized** signatures produced by Apple stacks (never byte-equality); producer low-S normalization; verifier high-S rejection; 65-byte uncompressed key parsing (exact RFC 9180 serialization) |
| XV-ECDH/HPKE | envelope seal/open both directions (Rust-seal→Swift-open and vice versa) |
| XV-HPKE-SE | Rust-seal→Apple-SE-open, Apple-seal→Rust-open, Apple-seal→Apple-SE-open; exact suite bytes (KEM 0x0010 / KDF 0x0001 / AEAD 0x0003) cross-checked against RFC 9180 vectors; Path B adapter vectors added only if the PoC triggers Path B (PoC artifact, Phase E pre-gate) |
| XV-HKDF | all §2.9 info strings |
| XV-TLV | registry entries (all kinds, incl. v2 recovery_epoch with the §4.5 binding fields), approval payloads, manifests: canonical bytes + expected hashes + signatures/proofs |
| XV-SAS | enrollment transcripts → 8-char SAS |
| XV-BIP39 | RK entropy ↔ 24 words (official reference vectors + ours) |
| XV-ORIGIN | canonicalization + matching corpus (shared with JS for BR-15) |
| XV-RECOVERY-EPOCH | VK + manifest + new-device keys + nonce → recovery_proof (values regenerated for manifest v2) |
| XV-OBJ (v0.4) | `OV0OBJ02` objects: canonical bytes, `blob_hash`, parse rejections |
| XV-RECORD-AAD (v0.4) | `graph_digest`, record and meta AAD v2 |
| XV-INDEX (v0.4) | index v2 canonical text and `object_index_hash` |
| XV-STATE (v0.4) | `StateTransition` bodies, `recovery_auth_digest`, `state_commit` |
| XV-REQSIG (v0.4) | `ProviderRequest` TLV + prehash; recovery class byte-exact (RFC 6979); device class verify-only (SE is randomized) |
| XV-RECOVERY-AUTH (v0.4) | `(PK \| RK_bytes, auth_salt, vault_id, class) → ikm_c → sk_c/pk_c → key_id_c → signature` over a fixed request; plus RFC 9180 A.3 DeriveKeyPair conformance, RFC 6979 A.2.5 (P-256/SHA-256) signing conformance, and an **independent** cross-check of `DeriveKeyPair(ikm_c)` against the PoC workspace's `hpke` crate (dev/test only; never linked into the helper), so the composition vectors are not checked only against the implementation that produced them |
| XV-HANDLE (v0.4) | handle normalization and rejection cases, `handle_key` |
| XV-ENROLL (v0.4) | envelope v2 (`ov0/envelope/v2`, no credential); Rust and Swift |

Any wire/crypto change bumps versions and regenerates vectors in the same
commit.

### 16.9 Sync revision graph

| ID | Test | Expected |
|---|---|---|
| SY-01 | single-author linear edits across sync | fast-forward; no conflict rows |
| SY-02 | two devices edit one record concurrently | conflict marked; both revisions retained; **no** timestamp pick; record excluded from fills until resolved |
| SY-03 | edit concurrent with delete | conflict (never silent delete-wins) |
| SY-04 | edit claiming causal ancestry after a tombstone | conflict per §3.2 (conservative, never resurrect) |
| SY-05 | same author equivocates (two children, same counter) | fork evidence surfaced; both surfaced; writes on that record freeze |
| SY-06 | counter regression from one author | rejected |
| SY-07 | parent_count > 8, trailing bytes, unknown object magic | rejected (`FORMAT_TOO_NEW` / `FORMAT_INVALID`) |
| SY-08 | `resolve_conflict` revision syncs | all devices converge to identical tips |
| SY-09 | (v0.4) any topologically valid permutation / out-of-order delivery of the same revision set | identical heads, conflicts and freezes on every device (property test) |
| SY-10 | (v0.4) VK rotation | `revision_id`s and parents unchanged; blob hashes change; a device with pending local work re-seals it with the same ids |
| SY-11 | (v0.4) unseen revisions from a revoked author (§3.2) | refused and counted; `Admit(D)` kept; trusted descendants re-authored only by their author |
| SY-12 | (v0.4) duplicate `revision_id` with different blob at the same `vk_generation` | benign if identical content, else freeze |
| SY-13 | (v0.4) local rollback (vault older than Keychain last-seen) | authoring refused until synced |

### 16.10 Native-messaging host

| ID | Test | Expected |
|---|---|---|
| NM-01 | host exec'd by a non-Chrome parent with correct argv | parent SecCode check fails; exit nonzero before stdin read |
| NM-02 | missing argv origin | exit nonzero |
| NM-03 | unexpected origin (other extension ID) | exit nonzero |
| NM-04 | dev extension ID offered to a release build | refused (dev ID compiled into debug builds only) |
| NM-05 | Phase H prototype: observe Chrome's actual spawn behavior (parent identity, argv format) | confirms the §9.2 layer-3 requirement or fails the gate |
| NM-06 | Phase H prototype: pid→SecCode mapping under rapid spawn/exit | race verdict recorded; if unreliable, §9.2 claims are reduced to layers 1+2+4 and re-reviewed |

### 16.11 Import fingerprints

| ID | Test | Expected |
|---|---|---|
| IM-01 | exfiltrated `import_log` + salt, vault locked | no fingerprint oracle: helper exposes no op that computes fingerprints while locked; construction is HMAC under a VK-derived key (code assertion + XV-HKDF vectors) |
| IM-02 | re-import identical file | 0 new items, all duplicates |
| IM-03 | same identity, changed password | duplicate; existing item kept; counted in report |
| IM-04 | VK rotation then re-import of pre-rotation file | fingerprints recomputed under the new generation key inside the rotation transaction; dedupe still works |
| IM-05 | `identity_ct` tampered | AEAD failure; row quarantined; no fingerprint consulted for it |
| IM-06 | VK rotation leaves `import_fp_salt` unchanged | header diff across rotation: salt identical; fingerprints recomputed under the new key in the same transaction (§2.10) |

### 16.12 Fresh-device freshness UX

| ID | Test | Expected |
|---|---|---|
| FR-01 | fresh-device recovery flow | UI shows served generation, date, and item count **before** completion |
| FR-02 | recovery-sheet checkpoint comparison | vault_id, manifest generation, registry head prefix, normalized handle and provider origin rendered on the printed sheet; comparison affordance present in the recovery UI |
| FR-03 | stale-but-valid manifest served at recovery | recovery completes with the freshness display (no claimed detection); a device that later sees newer history surfaces the stale binding from the registry (§11.7) |

### 16.13 Helper secure panel

| ID | Test | Expected |
|---|---|---|
| UI-01 | panel opens | `secure_panel_visible` emitted; capture suppression counter up for the whole lifetime |
| UI-02 | MP/RK fields are native secure fields | secure input active while focused; keystroke recorder sees zero events (CS-03 analog) |
| UI-03 | panel close/cancel (`PANEL_CANCELLED`) | counter decrements; input buffers zeroized (canary absent from helper heap) |
| UI-04 | recovery-sheet print | sheet bytes reach NSPrintOperation only; no WebView copy exists (webview snapshot assertion); counter stays up through the print dialog |
| UI-05 | IPC surface audit | no §1.5 op schema contains an MP/RK/VK-bearing field (schema lint test, run in CI) |

### 16.14 Provider request signing (PR; replaces v0.3 BA)

| ID | Test | Expected |
|---|---|---|
| PR-01 | canary recovery-auth `ikm_c`/`sk_c` and PK planted; full IPC transcript and helper log recorded across all flows | canary bytes never appear (also covered by UI-05 / SC-01) |
| PR-02 | main requests a signature over caller-supplied raw bytes / a field not in the schema | refused — the op has no raw-bytes field; the helper signs only canonical requests it built |
| PR-03 | XV-REQSIG | recovery-class signatures byte-exact; device-class signatures verify |
| PR-04 | body, path, method, vault, audience, operation or expected state substituted after signing | provider rejects (all are bound; body hash recomputed) |
| PR-05 | recovery class asked to sign `publish`, or a `blob_put` outside RECOVERING | `SIGNING_REFUSED` at the helper (the provider independently rejects the `publish`, BK-14) |
| PR-06 | wrong helper state for any operation; LOCKED `blob_put`/`state_commit` without a fully staged publication | `SIGNING_REFUSED` (policy table, §11.4) |

### 16.15 Recovery finalize (RF)

| ID | Test | Expected |
|---|---|---|
| RF-01 | valid total-loss finalize | atomic success: state advanced, epoch device active, every prior device revoked, manifest resolvable, registry extended, recovery-auth updates applied — one commit |
| RF-02 | stale `expected_state` | `STATE_MOVED`; state unchanged; new device not activated |
| RF-03 | finalize failing structural validation after the precondition passes | nothing mutates; new device inactive; old state authoritative |
| RF-04 | manifest signer ≠ `sign_pub` inside the supplied recovery_epoch entry | rejected |
| RF-05 | byte-identical replay of committed finalize → idempotent success; different body for a passed generation → `FINALIZE_CONFLICT` | no duplicate state either way |
| RF-06 | provider crash/failure injected mid-transaction | old head remains authoritative; no half-applied finalize observable |
| RF-07 | immediately after success: publish with the new device's key; attempt a publish with the recovery key | publish succeeds; recovery key refused |
| RF-08 | recovery ordering: inspect the finalize body and post-finalize state | `new_manifest.vk_generation` = old + 1; every referenced record/wrap opens only under the fresh VK (old VK → `INTEGRITY_FAILURE`); helper enters UNLOCKED directly from RECOVERING; no ROTATING_KEYS transition or second rotation follows finalize |

### 16.17 Registry checkpoint (CP, v0.3.1)

| ID | Test | Expected |
|---|---|---|
| CP-01 | second and third total-loss recovery on fresh devices | each succeeds; one rotation and one generation per recovery; contents preserved; all epochs present as audit history |
| CP-02 | provider substitutes a different registry (its own device, its own manifest signature) | refused: the served checkpoint cannot bind the substituted head |
| CP-03 | provider alters historical `recovery_epoch` bytes | refused: object hash fails against the index; rebuilt index/manifest still fails the checkpoint binding |
| CP-04 | checkpoint for the wrong registry head | refused |
| CP-05 | checkpoint for the wrong manifest generation / wrong epoch | refused |
| CP-06 | checkpoint MAC'd under an old VK after rotation | refused; the freshly published checkpoint verifies under the new VK only |
| CP-07 | stale-but-valid complete state served to a fresh device | still recovers (the §11.7 limitation is unchanged); sheet comparison classifies it as older |
| CP-08 | existing device with the relevant VK | still verifies epoch proofs; rollback and fork detection unchanged |

### 16.16 Schema consistency lints (SC, CI)

| ID | Test | Expected |
|---|---|---|
| SC-01 | no main-process IPC op schema contains an MP/RK/VK/credential-bearing field | lint green (companion to UI-05) |
| SC-02 | approval TLV decoder: action 5 with absent `credential_ref` | decode/verify rejection |
| SC-03 | header v2 schema: `import_fp_salt`, `auth_salt_mp`, `auth_salt_rk` (16 B each), `provider`, and the exact §2.3 `kdf` block | lint green; malformed length or extra/missing keys rejected |
| SC-04 | `authorizer`/`signature` tags present on a kind=4 registry entry | decode rejection (v2 semantics, §4.3) |

---

## 17. Dependency plan

Existing usable: `sha2`, `rand` (OsRng), `serde`/`serde_json`, `zeroize`-adjacent patterns are absent — see below. `sqlx` (with the rsa-advisory transitive, RUSTSEC-2023-0071) stays **out of the helper entirely** (the helper uses rusqlite).

### 17.1 New Rust crates — vault-helper

| Crate | Version | Purpose | Maintenance / audit status | Why existing deps can't | In helper | Processes secrets |
|---|---|---|---|---|---|---|
| `argon2` | 0.5.x | MP KDF | RustCrypto, widely reviewed; no known advisories | none present | yes | yes (MP, PK) |
| `chacha20poly1305` | 0.10.x | record/wrap AEAD | RustCrypto; no known advisories | none present | yes | yes |
| `hkdf` | 0.12.x | subkeys, KEKs | RustCrypto | sha2 alone is not HKDF | yes | yes |
| `p256` | 0.13.x (`ecdsa`, `ecdh`) | device signatures/verification | RustCrypto; constant-time via `elliptic-curve` | none present | yes | pubkeys/signatures only; private ops happen in SE |
| `hpke` | 0.12.x | device envelopes (RFC 9180): **seal** on the Rust side; software open only in tests/vectors | small crate (rozbb); matches RFC 9180 vectors; **flagged for manual pre-merge review** (§17.4); production decapsulation uses Apple CryptoKit HPKE via the bridge below (§2.12 Path A) | composing ECDH+HKDF+AEAD by hand is the worse alternative; CryptoKit interop requires exact RFC 9180 | yes | yes (envelope payloads) |
| — | in-house Swift | `vault-apple-crypto` bridge: CryptoKit HPKE Sender/Recipient + SE key load, C ABI, ≤ 200 lines | not a crate; the only Swift linked into the helper; reviewed as our own code (§19) | CryptoKit is not callable from Rust directly; the bridge avoids any reimplementation of HPKE | yes (linked) | yes |
| — | in-house Rust, **contingency only** | `hpke_se` adapter (§2.12 Path B): RFC 9180 KEM ExtractAndExpand + base-mode key schedule with externally supplied DH | built **only** if the PoC documents Path A as impossible; ≤ 150 lines over `hkdf`+`chacha20poly1305`; RFC 9180 vectors + independent review required (§2.12, §19) | SE keys are non-exportable; fallback if Apple's HPKE cannot interop | only if triggered | yes |
| `zeroize` | 1.x | secret erasure | de facto standard, audited transitively everywhere | none present | yes | yes |
| `secrecy` | 0.10.x | SecretBox wrappers | widely used, tiny | complements zeroize with type-level guards | yes | yes |
| `subtle` | 2.x | constant-time compares | RustCrypto | — | yes | tokens/secrets |
| `rusqlite` | 0.32.x, `bundled` | vault.db | mainstream; bundled SQLite avoids system-lib variance; helper keeps sqlx out | repo's sqlx would drag the async+rsa-advisory graph into the helper | yes | encrypted blobs only |
| `csv` | 1.x | Dashlane parsing | BurntSushi crate, stable, no history of soundness issues | none present | yes | yes (import rows) |
| `objc2`, `objc2-foundation` | 0.5/0.2 | LA + Security framework FFI glue | obj-c2 project, actively maintained | existing `objc` 0.2 in main graph is the legacy crate; helper uses the maintained successor | yes | no (prompt plumbing) |
| `security-framework` | 3.x | Keychain items, SecCode checks | rust-mobile; **known SE key-agreement FFI gap** (v0.3 review) → thin in-crate FFI shim for `SecKeyCopyKeyExchangeResult`, reviewed as our own unsafe | none present | yes | handles wrapped blobs only |
| `base32` | 0.5.x | enroll secret encoding | tiny, stable | base64 present but QR favors base32 | yes | enrollment secret (transient) |
| `getrandom` | (via rand) | CSPRNG | — | already present | yes | yes |

Total new helper crates: 14 (plus transitive). Target: helper tree
< 120 crates total (vs ~500 in main). Measured and reported in Phase B.

**Note (v0.4):** the table above is the v0.3 plan. The shipped helper
differs in recorded, reviewed ways — no `hpke`, `security-framework` or
`base32` crates (HPKE via the §2.12 bridge; hand-rolled Security.framework
FFI), and newer RustCrypto lines (e.g. `argon2` 0.6); see the Phase B–E
verification reports and `vault-helper/Cargo.toml`, which are
authoritative for the current graph.

### 17.1a Phase F provider crates (v0.4)

- `vault-proto` adds **no** new crate: it reuses the helper's already
  vetted `sha2`, `hkdf`, `hmac`, `p256`, `serde`/`serde_json` and
  `unicode-normalization`. Moving code into it must not change the helper
  dependency count (gate: < 120; the Phase E count was 96).
- The recovery-auth derivation (§11.4) uses only `hkdf` and `p256`; no new
  cryptographic dependency or feature flag is permitted for it.
- `vault-provider-core` / `vault-provider` form a **separate supply-chain
  scope**: HTTP server (the repo already vets `axum`/`axum-server`/rustls),
  and a lean S3 client (SigV4 + conditional headers) preferred over the
  full AWS SDK tree. Each addition follows §17.4 and is reported
  separately; nothing from this scope may enter the helper's graph.
- The iPhone app still adds no third-party package (§17.3); envelope
  catch-up uses CryptoKit and URLSession only.

### 17.2 nm-host crate dependencies

`serde`, `serde_json` only (plus std). No crypto, no secrets logic beyond
pass-through + zeroize of the fill response (`zeroize` allowed, cheap).

### 17.3 Swift (iOS app) — zero new third-party packages

CryptoKit (HPKE `Sender`/`Recipient` directly, including with
`SecureEnclave.P256.KeyAgreement.PrivateKey` per §2.12 Path A; P-256,
HKDF), LocalAuthentication, Security, Network/
URLSession with SPKI pinning (existing pattern in the iOS repo, for the paired Mac channels only; the Phase F provider client uses standard public-CA validation with **no** pinning, §11.1). BIP-39
wordlist asset + in-house codec (§2.4). This keeps the iOS supply chain
at Apple-only. The macOS helper additionally links the in-house
`vault-apple-crypto` Swift bridge (§2.12/§17.1) — same CryptoKit API,
same review bar.

### 17.4 Vetting policy for new security dependencies

- The existing 759-crate `cargo vet` exemption baseline **must not** grow
  to absorb these. Each new helper crate gets either (a) an imported
  audit (`cargo vet import` from Mozilla/Google/bytecodealliance where
  available) or (b) a recorded manual review note in
  `src-tauri/supply-chain/audits.toml` covering: unsafe usage, FFI
  surface, panic behavior on adversarial input, and maintenance pulse.
- HPKE policy (corrected, v0.3 finding 4) — ordered, none skippable:
  1. review the chosen `hpke` crate (Rust seal path + test open path)
     before Phase E code lands;
  2. run the §2.12 PoC **Path A first**: Apple native HPKE with
     SE-resident keys via `HPKEDiffieHellmanPrivateKey`, exact RFC 9180
     suite, both directions, before Phase E is authorized;
  3. only if the PoC documents Path A as impossible (authoritative
     evidence + failing vector): build the §2.12 Path B adapter, held to
     RFC 9180 vectors and independent review;
  4. Path C (any broader hand-composition) is prohibited absent
     documented A and B failure, independent review, and an owner
     decision;
  5. independent review of the envelope path — crate usage plus bridge
     or adapter, whichever ships — is a §19 release-gate item.
  "Hand-compose HPKE" is never an automatic fallback.
- CI gate: `cargo tree -p source-vault-helper | grep -E "^.* rsa "` must
  be empty; **RUSTSEC-2023-0071 remains outside all vault/helper build
  graphs** and any change that pulls it in is a merge blocker
  (`scripts/audit-deps.sh` gains this check in Phase A).

### 17.5 npm

Extension (separate `extension/` package, not the app frontend): `tldts`
(PSL matching; processes origins, never secrets), `vite`+`typescript`
build only. No new dependencies in the app's `package.json`.

---

## 18. Implementation order

Each phase is independently committable, tested, and stops at its gate.
The 350-line file cap applies: vault code is organized as module
directories (`vault-helper/src/{crypto,storage,registry,ipc,ops,import}/…`).

### Phase A — helper skeleton
Workspace split; `SourceVaultHelper.app` bundle plumbing; signed-build
scripting; socket + framing + peer auth (both directions); lifecycle
(launch/idle/exit/crash-restart); `hello`/`get_state`/`lock`; CI checks:
helper dep count, rsa-absence, designated-requirement verification.
**No vault, no keys — synthetic echo service behind the real boundary.**
Gate: peer-auth rejects an unsigned clone binary; helper restart leaves
state machine at LOCKED; `cargo vet` passes with new audits recorded.

### Phase B — cryptographic core
VK/wraps/records/subkeys/zeroization; BIP-39 codec; TLV codec;
gen_vectors CLI; CR-01…CR-13, RG/RG-10, XV-Rust-side.
Gate: all green; `cargo fuzz` smoke run clean (10 min); **Argon2id
calibration benchmark table committed (§2.3) with the chosen tuple
justified per device class**.

### Phase C — local Mac vault
rusqlite storage; header/manifest; record CRUD with synthetic records;
UNLOCKED lifecycle + auto-lock; LA presence per op; **helper-owned secure
panel (§1.7) for unlock/change-MP flows**; capture-safe management UI
(§14 wiring incl. fail-closed reveal); CS-01…CS-07, UI-01…UI-05.
Gate: synthetic vault usable end-to-end in the app; capture and panel
tests green.

### Phase D — recovery
MP set/change; RK generate/print/enter (panel + print path, §1.7);
rotation engine; recovery epoch; RC-03…RC-07 vs FsBackupStore; BK-10
historical-snapshot test; FR-01…FR-03 freshness UX.
Gate: scenarios 3–7 rehearse green on synthetic data.

### Phase E — device identity + enrollment
**Pre-gate (authorization to start envelope code): the §2.12 HPKE-SE
PoC is green, Path A first** — Rust-seal→Apple-SE-open,
Apple-seal→Rust-open, and Apple-seal→Apple-SE-open demonstrated with
real SE keys at the exact RFC 9180 suite; XV-HPKE-SE vectors committed;
if Path A fails, the PoC report documents the precise Apple limitation
and Path B is evaluated under §17.4 before any envelope code. Then: SE
keygen (both roles, both platforms); §5 protocol + ephemeral server;
SAS; envelopes; registry live; revocation + auto-rotation (RC-01/02);
DA vectors; iOS app vault screens.
Gate: two real devices enroll/revoke/rotate green on synthetic vaults;
backup credential issued and registered at enrollment (§5.2) — a v0.3.1
gate item; v0.4 removes the credential, and provider activation happens at
the enrollment publish (§5.2, §11.4).

### Phase F — remote encrypted backup and Mac multi-writer protocol (v0.4)
Scope: `vault-proto`, `vault-provider-core` (with `FsStores`) and the
`vault-provider` S3 service (§1.1, §11); the in-process and HTTPS
transports and the backup coordinator; signature-based provider
authentication and recovery-auth keys (§11.4); state transitions,
handle claims, remote-completion status, shared MP-class recovery throttle, KDF
policy, GC (§11.2–§11.5); total-loss finalize with S-4 revokes (§11.8);
v2 formats (§3.7, §11.2, §2.2); stable revision ids, heads merge, counter
algorithm, revoked-author refusal (§3.2); chunked IPC streams, including
the enrollment bundle (§1.3); the new states (§13); iPhone **envelope
catch-up only** (§4.7). Not in scope: iPhone record store/sync/backup
client and direct peer sync (Phase F.2), push relay and APNs (Phase G).
**Pre-gate test infrastructure:** unattended Keychain gates — a random
per-run test namespace instead of PIDs, teardown deletion of synthetic
Keychain items, a fail-fast guard against interactive prompts, a gate
pre-flight rejecting leftover `ov0*` items, and a distinct `test.` tag
prefix for test Secure Enclave keys — without changing production ACLs.
**Multi-writer scope (clarification).** Phase F implements and tests the
multi-writer protocol (concurrent publishers, merge, revocation of one
writer by another) with **simulated Mac devices** — additional helper
instances with test identities driven through `vault-provider-core`. §5
enrollment is Mac ↔ iPhone only; **Mac-to-Mac enrollment is not specified
and is out of Phase F scope**, so a product vault in Phase F has one Mac
writer. Adding a second physical Mac is a separate owner decision.
Tests: BK-01…BK-28, SY-01…SY-13, PR-01…PR-06, RF-01…RF-08, RG-01…RG-18,
CP-01…CP-08, FR-01…FR-03, RC-01M (below) and RC-02…RC-08 re-run on v2
formats, HC/RU/KD/RL/EV/TR/ST families, CR-12/CR-13 and the v0.4 vectors
(§16). **RC-01M** runs scenario 1's Mac variant with simulated devices:
Mac A lost, Mac B (simulated) revokes it, rotates and publishes. RC-01
itself (iPhone-initiated) requires Phase F.2 and must be green before
Phase J.
**Pre-gate prerequisites:** the Keychain test-infrastructure changes
above; the U-4 experiment (does the fail-fast guard suppress legacy ACL
dialogs; does deleting a foreign-ACL item prompt) run and recorded; and
the one-time owner-run cleanup of the stale `ov0*` test Keychain items
completed after a reviewed dry run.
Gate (`scripts/phase-f-gate.sh`, nesting `phase-e-gate.sh`): all of the
above green; the nested Phase E gate's `device_backup_cred` transcript
grep is replaced by the PR-01 canary scan (a grep for a removed field can
never fail); **EV-03 runs on a physical A15+ iPhone** (the simulator has
no Secure Enclave) and the gate asserts the EV-03 tests ran **by name**
rather than by a nonzero total count; Secure Enclave signing latency
measured and recorded (it decides whether blob uploads need batch
signing); helper dependency count unchanged; provider supply-chain
report; and a **kill-both-devices rehearsal in a fresh environment** with
synthetic data — a new macOS user account on the physical Mac with no
access to the original user's vault files or Keychain state, creating a
new Secure Enclave identity, and recovering only from remote provider
state plus the permitted recovery material (handle + MP, and separately
handle + RK). The rehearsal log records: the FR-01 display, the sheet
comparison, the KDF-policy check and header cross-check, the S-4 revokes
of every prior device, and a successful publish by the new device. The
report calls this a fresh environment, not proof of physical-hardware
loss; a second physical Mac is optional extra evidence.

### Phase F.2 — iPhone vault client and direct peer sync (not yet authorized)
iPhone record store, merge (§3.2), rotation and publication; iPhone-
initiated revocation (§12 scenario 1, RC-01); direct Mac⇄iPhone peer sync
over the pinned channel. Requires separate owner authorization and its own
design review.

### Phase G — iPhone remote approval
§6.5/§6.6 + §7.1 foreground + §7.2 APNs alert path; DA-01…DA-11;
fallback UX; APNs provider side on the Phase F service.
Gate: clamshell Mac + iPhone approval demo on synthetic vault; offline
fallback demonstrated.

### Phase H — Chrome extension
extension + nm-host + manifest install; canonicalization + PSL + policy
engine; fill/save/update with trusted update confirmation (§9.8);
BR-01…BR-19 with synthetic sites (local test server with IDN/punycode/
iframe fixtures); **nm-host caller-verification prototype NM-05/NM-06
runs first and its verdict is recorded before relying on §9.2 layer 3**.
Gate: synthetic fills green; extension cannot enumerate (attempts logged
+ refused); NM-01…NM-06 closed with the prototype report committed.

### Phase I — Dashlane importer
**Pre-gate: the §10.8 Secure Notes owner decision is recorded** (Choice A
or B with its listed consequences). §10 end to end on synthetic
fixtures; fuzz corpus + `cargo fuzz` CI budget; import UI with capture
suppression + honesty copy; IM-01…IM-06.
Gate: 10⁵-row synthetic import < 60 s, zero panics, report clean;
Choice-B honesty behavior (count, report, no auto-delete) demonstrated
on a fixture containing secure notes if B was chosen.

### Phase J — first real credential gate
Run §19. Nothing proceeds to real data on "seems to work".

---

## 19. First real credential release gate

Every item must be verifiably green, with the named evidence:

| # | Gate item | Evidence |
|---|---|---|
| 1 | helper-process isolation working | Phase A gate; `codesign` DR checks in CI |
| 2 | IPC peer authentication working | unsigned-clone rejection test |
| 3 | strict CSP still green | smoke harness: zero `CSP VIOLATION` in dev + bundle runs |
| 4 | vault excluded from asset protocol | asset_scope_guard tests + runtime probe (CS-08) |
| 5 | vault excluded from screen capture | CS-01/02/06 |
| 6 | vault excluded from OCR/indexing | CS-02 |
| 7 | password input suppression working | CS-03/04/05 |
| 8 | cryptographic tests green | CR-01…CR-13 |
| 9 | tamper tests green | CR-05/06/10, RG-01/02, BK-02 |
| 10 | registry tests green | RG-01…RG-18, CP-01…CP-08, XV-TLV v2 vectors |
| 11 | key rotation tests green | CR-08/12, RC-01 (Phase F.2), RC-01M, RC-02/06/07, SY-10 |
| 12 | recovery tests green | RC-01…RC-08 (RC-01 via Phase F.2) + RC-01M, FR-01…FR-03, RF-01…RF-08, HC-01…HC-05, RU-01…RU-05, KD-01…KD-03, RL-01…RL-04, EV-01…EV-05, BK-23, BK-26…BK-28 |
| 13 | remote backup restore tested | BK-01…BK-22 + the Phase F fresh-environment rehearsal log |
| 14 | no bulk-secret API | IPC catalog audit vs §1.5 — any new op reviewed against the never-list; SC-01…SC-04 lints green |
| 15 | no agent vault API | route/command audit: `/v1/agent`, Tauri commands, nm ops |
| 16 | dependency security review complete | §17.4 audits recorded; helper dep count reported; rsa-absence CI green; `cargo vet` clean |
| 17 | security logs verified clean | log scrape during full test run: no secret-class strings (canary sweep) |
| 18 | synthetic Dashlane import fuzzed | Phase I gate + corpus in repo + IM-01…IM-06 |
| 19 | Chrome origin-matching tests green | BR-01…BR-19 |
| 20 | iPhone approval replay tests green | DA-06/07/08/12 |
| 21 | production build signing/hardened runtime verified | `codesign --verify --deep --strict`, runtime flag 0x10000, entitlement audit vs `macos-signing-and-hardening.md`; helper carries no `disable-library-validation` |
| 22 | HPKE-SE path proven | §2.12 PoC report: Path A green both directions on real SE keys at the exact suite — or documented impossibility + Path B adapter with its independent review; XV-HPKE-SE vectors green |
| 23 | sync merge semantics proven | SY-01…SY-13 (no timestamp pick anywhere), TR-01…TR-09, ST-01…ST-05 |
| 24 | provider request authentication and revocation proven (signatures; no symmetric credentials exist) | BK-12…BK-19, BK-24, BK-25, PR-01…PR-06, RL-01…RL-04, KD-01…KD-03 |
| 25 | nm-host caller verification dispositioned | NM-01…NM-06 closed; prototype report committed; if layer 3 proved unreliable, §9.2 claims were reduced and re-reviewed |
| 26 | helper panel isolation proven | UI-01…UI-05; MP/RK never observable in the WebView (CS-03 analog + webview snapshot) |
| 27 | independent envelope-path review | §17.4 step 5 report attached (covers `hpke` usage + the shipped Path A bridge or Path B adapter) |
| 28 | Secure Notes decision recorded | §10.8 choice on file; if Choice B: importer report/count/no-auto-delete behavior verified on fixtures |
| 29 | recovery-sheet printing exercised on a configured printer | one real print from the §1.7 window on a Mac with a printer set up: sheet legible, capture bracket up for the whole interaction, no file written by the helper; the standard dialog's PDF menu and spool behavior documented as-is (v0.3.1) |
| 30 | Argon2id tuple frozen with cross-device evidence | **Closed by option (b), owner decision confirmed 2026-09-21.** The v1 support floor was narrowed to **A15-class or newer** (§2.3, §21 OQ-3) rather than measuring an A12: none had been tested, and an untested support claim was not acceptable to ship. The slowest supported device is then the iPhone 13 mini measured at median 89 ms / worst 124 ms, inside the §2.3 budget, so the tuple is frozen at `m=64 MiB, t=3, p=1`. The tuple was **not** weakened; the device set was narrowed. **Remaining before the first real credential: enforce the floor at runtime** — today it is documentation, and the app would run on hardware nobody has measured |


Only then may the first real credential be imported.

---

## 20. Source ID future integration

v1: Source ID is not required, not a recovery dependency, and its failure
cannot destroy vault access (v0.3 §10.5, C13). Reserved extension points,
designed now so integration later is additive:

1. **Registry authorizer namespace:** `authorizer` field (0x0C) is present
   on genesis/enroll/revoke and absent on recovery_epoch (v2 semantics —
   recovery is authorized by `recovery_proof`, never by a zero-byte or
   implicit authorizer); a future entry kind 5 (`source_id_attest`) can
   bind a Source ID root signature as an *additional* authorizer without
   changing existing entries or weakening the recovery proof model.
2. **Approval action space:** §6.5 `action` u8 has headroom; Source ID
   would add an approval path alongside — never replacing — LA and iPhone
   approval.
3. **Wrap slots:** `wraps/` gains a `sourceid.wrap` at most as *one more
   independent wrap of VK*, never the only wrap; MP and RK wraps are
   never removed by Source ID integration.

Hard separation invariant (must survive integration review): the Source
ID identity root never encrypts the vault, never appears in a wrap
derivation path, and Source-controlled infrastructure gains no ability to
unwrap VK or mint devices. Any future proposal violating this is a
backdoor and must be called one in review. The full Source ID recovery
protocol is out of scope here by instruction.

---

## 21. Formerly open questions — resolved (v0.2, finding 14)

Settled product decisions remain settled (v0.3 §19). The three v0.1 open
questions are now closed; the locked choices below are binding for
implementation. Anything genuinely new that surfaces during
implementation must be escalated to the owner, not decided silently.

### OQ-1 → locked: Secure-Enclave-capable Macs required

Device identity requires an SE on both platforms: all Apple Silicon Macs
and T2 Intel Macs (subject to the §2.12 PoC confirming the required
`SecKeyCreateRandomKey`/key-agreement APIs behave identically there).
Vault setup refuses on hardware without an SE, with a plain explanation.
No software-Keychain identity tier ships in v1: a downgrade tier would
be a steering target at setup time, and a silent one would be dishonest.
This trades old-hardware support for one identity-security level
everywhere.

### OQ-2 → locked: direct reachability in v1; relay is a future decision

v1 phone approval requires the APNs wake (§7.2) plus the phone reaching
the Mac directly; otherwise the §6.C fallback path serves. No provider
mailbox/relay ships in v1 — the provider stays a dumb ciphertext store
(v0.4: plus the structural checks and operational state of §11, none of
which is a vault root of trust). Push is Phase G.
The §6.5 approval TLV is already opaque and relay-safe by construction,
so a future E2E relay (provider learns envelopes + timing only) can be
adopted as an owner decision without redesigning the payload.

### OQ-3 → locked: iOS 17+ for v1

CryptoKit's HPKE API floor is iOS 17 (macOS 14), and current Apple
documentation confirms `SecureEnclave.P256.KeyAgreement.PrivateKey`
works with `HPKE.Recipient` at that floor (§2.12 Path A); supporting
older iOS would mean a second crypto path in Swift, which v0.3 §18
forbids without re-review. iOS 17+ / macOS 14+ is therefore the v1
floor, and the §2.12 PoC verifies the exact CryptoKit calls on the
oldest supported versions. Revisit older OSes only with measured user
need and a dedicated review.

**Hardware floor added (owner decision, confirmed 2026-09-21).** The OS
floor is no longer the binding constraint: v1 additionally requires an
**A15-class or newer** iPhone (§2.3). iOS 17 runs on A12 devices, but
none was measured against the production Argon2id parameters, and the
owner declined to ship an untested support claim. The constraint is the
chip, not the model year, so the SE 3rd generation is supported. Older
hardware is addable later by measuring it; the memory cost is not
reduced to accommodate it.

---

*End of specification (v0.4). Implementation of any phase requires explicit
owner authorization; release to other users additionally requires
independent security/crypto review (v0.3 §21).*
